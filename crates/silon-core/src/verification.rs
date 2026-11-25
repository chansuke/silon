//! Kani proof harnesses for the pure core. Run with `cargo kani` in this crate.
//!
//! `Command::into_effects` has no harness: with its recursion over `Vec`s,
//! CBMC ran past 8 minutes even for three leaves. Its tests and mutants cover it.

use crate::{History, ReplayError};

/// Records are bounded so that every loop unwinds within `kani::unwind`.
const MAX_RECORDS: usize = 4;

/// An order-sensitive way to apply entries: swapping two entries changes the
/// result, so a replay in the wrong order fails the assertions.
fn apply(state: &mut u32, entry: &u8) -> Result<(), String> {
    *state = state.wrapping_mul(3).wrapping_add(u32::from(*entry));
    Ok(())
}

/// For any capacity up to 2 and up to four actions, some of which cannot be
/// stored: replaying everything gives the current state, replaying through a
/// listed seq gives the state right after that action, and other seqs are
/// rejected with the right error.
#[kani::proof]
#[kani::unwind(6)]
fn replay_reproduces_every_recorded_state() {
    let capacity: usize = kani::any_where(|capacity| *capacity <= 2);
    let count: usize = kani::any_where(|count| *count <= MAX_RECORDS);
    let base: u32 = kani::any();

    let mut history = History::new(base, capacity);
    let mut states = [base; MAX_RECORDS + 1];
    let mut state = base;
    let mut restarted = false;
    for (index, after) in states.iter_mut().skip(1).enumerate() {
        if index < count {
            let entry: u8 = kani::any();
            let storable: bool = kani::any();
            restarted |= !storable;
            let _ = apply(&mut state, &entry);
            history.record(storable.then_some(entry), &state, apply);
            *after = state;
        }
    }

    assert_eq!(history.replay(None, apply), Ok(state));
    let listed = history.entries().count();
    assert!(listed <= capacity);
    assert_eq!(history.first_seq() + listed as u64, count as u64);

    let seq: u64 = kani::any_where(|seq| *seq <= MAX_RECORDS as u64);
    match history.replay(Some(seq), apply) {
        Ok(replayed) => {
            assert!(history.first_seq() <= seq && seq < count as u64);
            assert_eq!(replayed, states[seq as usize + 1]);
        }
        Err(ReplayError::Dropped { oldest, .. }) => {
            assert!(seq < oldest && oldest == history.first_seq());
        }
        Err(ReplayError::NotRecorded { .. }) => assert!(seq >= count as u64),
        Err(ReplayError::Apply(_)) => panic!("apply never fails here"),
    }

    kani::cover!(
        listed == capacity && history.first_seq() > 0,
        "folded into the base"
    );
    kani::cover!(restarted && listed > 0, "recorded again after a restart");
    kani::cover!(
        seq >= history.first_seq() && seq < count as u64,
        "replayed through a listed seq"
    );
}
