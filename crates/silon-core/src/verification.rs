//! Kani proof harnesses for the pure core. Run with `cargo kani` in this crate.
//!
//! `Command::into_effects` has no harness: with its recursion over `Vec`s,
//! CBMC ran past 8 minutes even for three leaves. Its tests and mutants cover it.
//!
//! Each harness uses concrete record counts so that `VecDeque`'s length stays
//! concrete in CBMC (avoiding symbolic reallocation blowup during SSA
//! conversion) while keeping `base`, all entries, and replay `through` queries
//! fully symbolic.

use crate::{History, ReplayError};

/// An order-sensitive way to apply entries: swapping two entries changes the
/// result, so a replay in the wrong order fails the assertions.
fn step(state: u32, entry: u8) -> u32 {
    state.wrapping_mul(3).wrapping_add(u32::from(entry))
}

fn apply(state: &mut u32, entry: &u8) -> Result<(), String> {
    *state = step(*state, *entry);
    Ok(())
}

/// Sentinel entry whose `fallible_apply` fails, simulating a stored entry that
/// cannot be deserialized or re-applied on fold or replay.
const BAD_ENTRY: u8 = u8::MAX;

fn fallible_apply(state: &mut u32, entry: &u8) -> Result<(), String> {
    if *entry == BAD_ENTRY {
        return Err(String::new());
    }
    apply(state, entry)
}

/// Recording four storable entries into a capacity-2 history folds the oldest
/// two into `base`, keeps the exact last two `(seq, entry)` pairs in order,
/// replays `None` to the current state, replays `Some(seq)` for any listed seq
/// to the state right after that action, and rejects dropped/unrecorded seqs.
#[kani::proof]
#[kani::unwind(6)]
fn replay_folds_entries_into_base_at_capacity_two() {
    const CAPACITY: usize = 2;
    const RECORDS: usize = 4;

    let base: u32 = kani::any();
    let mut history = History::new(base, CAPACITY);
    let mut entries = [0u8; RECORDS];
    let mut states = [base; RECORDS + 1];
    let mut state = base;

    for index in 0..RECORDS {
        let entry: u8 = kani::any();
        entries[index] = entry;
        state = step(state, entry);
        states[index + 1] = state;
        history.record(Some(entry), &state, apply);
    }

    let first_seq = history.first_seq();
    assert_eq!(first_seq, (RECORDS - CAPACITY) as u64);

    let mut listed = 0usize;
    for (offset, (entry_seq, entry_val)) in history.entries().enumerate() {
        assert_eq!(entry_seq, first_seq + offset as u64);
        assert_eq!(*entry_val, entries[entry_seq as usize]);
        listed += 1;
    }
    assert_eq!(listed, CAPACITY);

    let through: Option<u64> = if kani::any() {
        None
    } else {
        Some(kani::any_where(|seq| *seq <= RECORDS as u64))
    };

    match history.replay(through, apply) {
        Ok(replayed) => match through {
            None => assert_eq!(replayed, state),
            Some(seq) => {
                assert!(first_seq <= seq && seq < RECORDS as u64);
                assert_eq!(replayed, states[seq as usize + 1]);
            }
        },
        Err(ReplayError::Dropped { seq, oldest }) => {
            assert_eq!(through, Some(seq));
            assert!(seq < oldest && oldest == first_seq);
        }
        Err(ReplayError::NotRecorded { seq }) => {
            assert_eq!(through, Some(seq));
            assert!(seq >= RECORDS as u64);
        }
        Err(ReplayError::Apply(_)) => panic!("apply never fails here"),
    }

    kani::cover!(through.is_none(), "replayed all entries after folding");
    kani::cover!(
        matches!(through, Some(seq) if seq >= first_seq && seq < RECORDS as u64),
        "replayed through a listed seq"
    );
    kani::cover!(
        matches!(through, Some(seq) if seq < first_seq),
        "rejected dropped seq"
    );
    kani::cover!(
        matches!(through, Some(seq) if seq >= RECORDS as u64),
        "rejected unrecorded seq"
    );
}

/// Recording an unstorable (`None`) entry restarts `base` at the current state
/// and drops earlier entries; entries recorded afterwards are listed with
/// advancing seqs and replay accurately.
#[kani::proof]
#[kani::unwind(5)]
fn restart_clears_buffer_and_replays_subsequent_entries() {
    const CAPACITY: usize = 2;
    const RECORDS: usize = 3;

    let base: u32 = kani::any();
    let e0: u8 = kani::any();
    let e1: u8 = kani::any();
    let e2: u8 = kani::any();

    let mut history = History::new(base, CAPACITY);
    let s1 = step(base, e0);
    history.record(Some(e0), &s1, apply);
    let s2 = step(s1, e1);
    history.record(None, &s2, apply);
    let s3 = step(s2, e2);
    history.record(Some(e2), &s3, apply);

    let first_seq = history.first_seq();
    assert_eq!(first_seq, 2);
    assert_eq!(*history.base(), s2);

    let mut listed = 0usize;
    for (entry_seq, entry_val) in history.entries() {
        assert_eq!(entry_seq, 2);
        assert_eq!(*entry_val, e2);
        listed += 1;
    }
    assert_eq!(listed, 1);

    let through: Option<u64> = if kani::any() {
        None
    } else {
        Some(kani::any_where(|seq| *seq <= RECORDS as u64))
    };

    match history.replay(through, apply) {
        Ok(replayed) => match through {
            None => assert_eq!(replayed, s3),
            Some(seq) => {
                assert_eq!(seq, 2);
                assert_eq!(replayed, s3);
            }
        },
        Err(ReplayError::Dropped { seq, oldest }) => {
            assert_eq!(through, Some(seq));
            assert_eq!(oldest, 2);
            assert!(seq < 2);
        }
        Err(ReplayError::NotRecorded { seq }) => {
            assert_eq!(through, Some(seq));
            assert!(seq >= RECORDS as u64);
        }
        Err(ReplayError::Apply(_)) => panic!("apply never fails here"),
    }

    kani::cover!(through.is_none(), "replayed after restart");
    kani::cover!(through == Some(2), "replayed post-restart seq");
    kani::cover!(
        matches!(through, Some(seq) if seq < 2),
        "rejected pre-restart dropped seq"
    );
    kani::cover!(
        matches!(through, Some(seq) if seq >= RECORDS as u64),
        "rejected unrecorded seq after restart"
    );
}

/// A zero-capacity history keeps no entries, advances `base` to the latest
/// state, replays `None` to that state, and rejects every `Some(seq)`.
#[kani::proof]
#[kani::unwind(4)]
fn zero_capacity_history_keeps_only_the_base() {
    const RECORDS: usize = 2;

    let base: u32 = kani::any();
    let e0: u8 = kani::any();
    let e1: u8 = kani::any();

    let mut history = History::new(base, 0);
    let s1 = step(base, e0);
    history.record(Some(e0), &s1, apply);
    let s2 = step(s1, e1);
    history.record(None, &s2, apply);

    assert!(history.entries().next().is_none());
    assert_eq!(history.first_seq(), RECORDS as u64);
    assert_eq!(*history.base(), s2);

    let through: Option<u64> = if kani::any() {
        None
    } else {
        Some(kani::any_where(|seq| *seq <= RECORDS as u64))
    };

    match history.replay(through, apply) {
        Ok(replayed) => {
            assert_eq!(through, None);
            assert_eq!(replayed, s2);
        }
        Err(ReplayError::Dropped { seq, oldest }) => {
            assert_eq!(through, Some(seq));
            assert_eq!(oldest, RECORDS as u64);
            assert!(seq < RECORDS as u64);
        }
        Err(ReplayError::NotRecorded { seq }) => {
            assert_eq!(through, Some(seq));
            assert!(seq >= RECORDS as u64);
        }
        Err(ReplayError::Apply(_)) => panic!("apply never fails here"),
    }

    kani::cover!(through.is_none(), "replayed zero-capacity history");
    kani::cover!(
        matches!(through, Some(seq) if seq < RECORDS as u64),
        "rejected dropped seq at zero capacity"
    );
    kani::cover!(
        matches!(through, Some(seq) if seq >= RECORDS as u64),
        "rejected unrecorded seq at zero capacity"
    );
}

/// When a failing entry is still in the buffer, `replay` reports
/// `ReplayError::Apply`; once a subsequent record folds that entry into `base`,
/// `record` restarts at the current state so later replays succeed.
#[kani::proof]
#[kani::unwind(4)]
fn failing_apply_restarts_on_fold_and_errors_on_replay() {
    let base: u32 = kani::any();
    let good: u8 = kani::any_where(|entry| *entry != BAD_ENTRY);

    let mut history = History::new(base, 1);
    let s1 = step(base, BAD_ENTRY);
    history.record(Some(BAD_ENTRY), &s1, fallible_apply);

    // While BAD_ENTRY is buffered, replay surfaces ReplayError::Apply.
    let mid_replay = history.replay(None, fallible_apply);
    assert!(matches!(mid_replay, Err(ReplayError::Apply(_))));

    // Recording another entry attempts to fold BAD_ENTRY, fails, and restarts
    // at `s2` with an empty buffer.
    let s2 = step(s1, good);
    history.record(Some(good), &s2, fallible_apply);
    assert!(history.entries().next().is_none());
    assert_eq!(history.first_seq(), 2);
    assert_eq!(*history.base(), s2);
    assert_eq!(history.replay(None, fallible_apply), Ok(s2));

    // Subsequent valid entries record and replay cleanly.
    let s3 = step(s2, good);
    history.record(Some(good), &s3, fallible_apply);
    assert_eq!(history.first_seq(), 2);
    assert_eq!(history.replay(Some(2), fallible_apply), Ok(s3));
    assert_eq!(history.replay(None, fallible_apply), Ok(s3));

    kani::cover!(
        matches!(mid_replay, Err(ReplayError::Apply(_))),
        "replay failed on bad entry"
    );
    kani::cover!(
        history.replay(None, fallible_apply) == Ok(s3),
        "replayed cleanly after fold-failure restart"
    );
}
