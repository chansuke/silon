//! A bounded record of the actions applied to a state, which replays them.

use std::collections::VecDeque;
use std::fmt;

/// A ring buffer of recorded entries on top of a base state.
///
/// Invariant: applying the entries to [`base`](Self::base) in order gives the
/// state passed to the last [`record`](Self::record). When the buffer is full,
/// the oldest entry is applied to the base and dropped, so the invariant holds
/// with a bounded number of entries.
///
/// `E` is how an entry is stored (for example an action serialized as JSON);
/// the caller passes the function that applies an entry to a state.
#[derive(Clone, Debug)]
pub struct History<S, E> {
    base: S,
    entries: VecDeque<E>,
    next_seq: u64,
    capacity: usize,
}

/// Why [`History::replay`] could not produce a state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayError {
    /// The entry was folded into the base or discarded by a restart.
    Dropped { seq: u64, oldest: u64 },
    /// The entry has not been recorded yet.
    NotRecorded { seq: u64 },
    /// Applying a recorded entry failed.
    Apply(String),
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dropped { seq, oldest } => {
                write!(f, "entry {seq} was dropped; the oldest entry is {oldest}")
            }
            Self::NotRecorded { seq } => write!(f, "entry {seq} has not been recorded"),
            Self::Apply(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ReplayError {}

impl<S: Clone, E> History<S, E> {
    /// Starts an empty history at `base` that lists at most `capacity` entries.
    pub fn new(base: S, capacity: usize) -> Self {
        Self {
            base,
            entries: VecDeque::with_capacity(capacity),
            next_seq: 0,
            capacity,
        }
    }

    /// The state before the oldest listed entry.
    pub fn base(&self) -> &S {
        &self.base
    }

    /// Sequence number of the oldest listed entry, which is also the number of
    /// entries no longer listed.
    pub fn first_seq(&self) -> u64 {
        self.next_seq - self.entries.len() as u64
    }

    /// The listed entries with their sequence numbers, oldest first.
    pub fn entries(&self) -> impl Iterator<Item = (u64, &E)> {
        (self.first_seq()..).zip(&self.entries)
    }

    /// Records the entry for an action that turned the previous state into
    /// `current`. `None` stands for an action that cannot be stored.
    ///
    /// If the entry is `None`, or the oldest entry cannot be applied when it is
    /// folded into the base, the history restarts at `current` without
    /// entries. That keeps the invariant at the cost of the listed entries.
    pub fn record(
        &mut self,
        entry: Option<E>,
        current: &S,
        mut apply: impl FnMut(&mut S, &E) -> Result<(), String>,
    ) {
        self.next_seq += 1;
        let Some(entry) = entry else {
            return self.restart_at(current);
        };
        if self.capacity == 0 {
            return self.restart_at(current);
        }
        if self.entries.len() == self.capacity
            && let Some(oldest) = self.entries.pop_front()
            && apply(&mut self.base, &oldest).is_err()
        {
            return self.restart_at(current);
        }
        self.entries.push_back(entry);
    }

    fn restart_at(&mut self, current: &S) {
        self.base = current.clone();
        self.entries.clear();
    }

    /// Returns the state right after entry `through`, or after every listed
    /// entry for `None`, computed from a copy of the base.
    pub fn replay(
        &self,
        through: Option<u64>,
        mut apply: impl FnMut(&mut S, &E) -> Result<(), String>,
    ) -> Result<S, ReplayError> {
        let oldest = self.first_seq();
        let count = match through {
            None => self.entries.len(),
            Some(seq) if seq < oldest => return Err(ReplayError::Dropped { seq, oldest }),
            Some(seq) if seq >= self.next_seq => return Err(ReplayError::NotRecorded { seq }),
            Some(seq) => (seq - oldest + 1) as usize,
        };
        let mut state = self.base.clone();
        for entry in self.entries.iter().take(count) {
            apply(&mut state, entry).map_err(ReplayError::Apply)?;
        }
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Appends each digit, so that the order of the entries shows in the state.
    fn push(state: &mut String, digit: &u8) -> Result<(), String> {
        state.push(char::from(b'0' + digit));
        Ok(())
    }

    /// Fails on 9, like an entry that cannot be read back.
    fn push_except_nine(state: &mut String, digit: &u8) -> Result<(), String> {
        if *digit == 9 {
            return Err("cannot apply 9".into());
        }
        push(state, digit)
    }

    fn history_of(digits: &[u8], capacity: usize) -> (History<String, u8>, String) {
        let mut history = History::new(String::new(), capacity);
        let mut state = String::new();
        for digit in digits {
            push(&mut state, digit).unwrap();
            history.record(Some(*digit), &state, push);
        }
        (history, state)
    }

    fn listed(history: &History<String, u8>) -> Vec<(u64, u8)> {
        history
            .entries()
            .map(|(seq, digit)| (seq, *digit))
            .collect()
    }

    #[test]
    fn lists_entries_in_order_with_their_seq() {
        let (history, state) = history_of(&[1, 2], 4);

        assert_eq!(history.base(), "");
        assert_eq!(history.first_seq(), 0);
        assert_eq!(listed(&history), [(0, 1), (1, 2)]);
        assert_eq!(history.replay(None, push), Ok(state));
    }

    #[test]
    fn keeps_exactly_capacity_entries() {
        let (history, _) = history_of(&[1, 2, 3], 3);
        assert_eq!(history.base(), "");
        assert_eq!(listed(&history), [(0, 1), (1, 2), (2, 3)]);
    }

    #[test]
    fn folds_the_oldest_entries_into_the_base_at_capacity() {
        let (history, state) = history_of(&[1, 2, 3, 4, 5], 2);

        assert_eq!(history.base(), "123");
        assert_eq!(history.first_seq(), 3);
        assert_eq!(listed(&history), [(3, 4), (4, 5)]);
        assert_eq!(history.replay(None, push), Ok(state));
    }

    #[test]
    fn a_zero_capacity_history_keeps_only_the_base() {
        let (history, state) = history_of(&[1, 2], 0);

        assert_eq!(history.base(), &state);
        assert_eq!(listed(&history), []);
        assert_eq!(history.first_seq(), 2);
    }

    #[test]
    fn replays_through_a_seq_without_changing_the_history() {
        let (history, _) = history_of(&[1, 2, 3, 4, 5], 3);

        assert_eq!(history.replay(Some(2), push), Ok("123".to_string()));
        assert_eq!(history.replay(Some(3), push), Ok("1234".to_string()));
        assert_eq!(history.replay(Some(4), push), Ok("12345".to_string()));
        assert_eq!(history.replay(None, push), Ok("12345".to_string()));
        assert_eq!(history.first_seq(), 2);
    }

    #[test]
    fn rejects_dropped_and_future_seqs() {
        let (history, _) = history_of(&[1, 2, 3], 2);

        let dropped = history.replay(Some(0), push).unwrap_err();
        assert_eq!(dropped, ReplayError::Dropped { seq: 0, oldest: 1 });
        assert_eq!(
            dropped.to_string(),
            "entry 0 was dropped; the oldest entry is 1"
        );

        let future = history.replay(Some(3), push).unwrap_err();
        assert_eq!(future, ReplayError::NotRecorded { seq: 3 });
        assert_eq!(future.to_string(), "entry 3 has not been recorded");
    }

    #[test]
    fn an_unstorable_entry_restarts_at_the_current_state() {
        let (mut history, _) = history_of(&[1, 2], 4);

        history.record(None, &"129".to_string(), push);

        assert_eq!(history.base(), "129");
        assert_eq!(history.first_seq(), 3);
        assert_eq!(listed(&history), []);
    }

    #[test]
    fn an_entry_that_cannot_be_folded_restarts_at_the_current_state() {
        let mut history = History::new(String::new(), 1);
        history.record(Some(9), &"9".to_string(), push_except_nine);
        history.record(Some(1), &"91".to_string(), push_except_nine);

        assert_eq!(history.base(), "91");
        assert_eq!(listed(&history), []);
    }

    #[test]
    fn a_failing_entry_fails_the_replay() {
        let (history, _) = history_of(&[1, 9], 4);

        assert_eq!(
            history.replay(None, push_except_nine),
            Err(ReplayError::Apply("cannot apply 9".into()))
        );
        assert_eq!(history.replay(Some(0), push_except_nine), Ok("1".into()));
    }
}
