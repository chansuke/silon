//! The actions a [`Store`](crate::Store) has dispatched since introspection
//! was attached, kept so that agents can read and replay them.

use std::collections::VecDeque;

use serde_json::{Value, json};
use silon_core::Feature;

/// Number of actions a store keeps before folding the oldest into the base.
pub const HISTORY_CAPACITY: usize = 256;

/// A ring buffer of dispatched actions on top of a base state.
///
/// Invariant: applying `entries` to `base` with [`Feature::update`] gives the
/// store's current state. Effects are not part of the history: their
/// follow-up actions are recorded like any other action.
pub(crate) struct History<F: Feature> {
    base: F::State,
    entries: VecDeque<Value>,
    /// Sequence number of the next recorded action.
    next_seq: u64,
    capacity: usize,
}

impl<F: Feature> History<F> {
    pub(crate) fn new(base: F::State, capacity: usize) -> Self {
        Self {
            base,
            entries: VecDeque::with_capacity(capacity),
            next_seq: 0,
            capacity,
        }
    }

    /// Sequence number of the oldest listed entry, which is also the number of
    /// actions no longer listed.
    fn first_seq(&self) -> u64 {
        self.next_seq - self.entries.len() as u64
    }

    /// Records `action`, serialized before it was applied, given the state
    /// `current` right after it was applied.
    ///
    /// If an action cannot be serialized or read back, the history restarts
    /// from `current`, which keeps the invariant at the cost of the entries.
    pub(crate) fn record(&mut self, action: serde_json::Result<Value>, current: &F::State) {
        self.next_seq += 1;
        let Ok(action) = action else {
            return self.restart_at(current);
        };
        self.entries.push_back(action);
        if self.entries.len() > self.capacity {
            let oldest = self.entries.pop_front().map(serde_json::from_value);
            match oldest {
                // The oldest action's effects already ran; only its state change is kept.
                Some(Ok(oldest)) => {
                    let _ = F::update(&mut self.base, oldest);
                }
                _ => self.restart_at(current),
            }
        }
    }

    fn restart_at(&mut self, current: &F::State) {
        self.base = current.clone();
        self.entries.clear();
    }

    /// `{"base": state, "dropped": n, "entries": [{"seq": n, "action": action}]}`.
    pub(crate) fn to_json(&self) -> Value {
        let entries: Vec<Value> = (self.first_seq()..)
            .zip(&self.entries)
            .map(|(seq, action)| json!({ "seq": seq, "action": action }))
            .collect();
        json!({
            "base": serde_json::to_value(&self.base).unwrap_or(Value::Null),
            "dropped": self.first_seq(),
            "entries": entries,
        })
    }

    /// Returns the state right after entry `through`, or after every entry for
    /// `None`, computed from a copy of the base.
    pub(crate) fn replay(&self, through: Option<u64>) -> Result<F::State, String> {
        let count = match through {
            None => self.entries.len(),
            Some(seq) if seq < self.first_seq() => {
                return Err(format!(
                    "entry {seq} was dropped; the oldest entry is {}",
                    self.first_seq()
                ));
            }
            Some(seq) if seq >= self.next_seq => {
                return Err(format!("entry {seq} has not been recorded"));
            }
            Some(seq) => (seq - self.first_seq() + 1) as usize,
        };
        let mut state = self.base.clone();
        for action in self.entries.iter().take(count) {
            let action = serde_json::from_value(action.clone())
                .map_err(|err| format!("cannot read recorded action {action}: {err}"))?;
            // Replay recomputes states; it never runs effects again.
            let _ = F::update(&mut state, action);
        }
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use silon_core::Command;

    /// Appends each digit, so that the order of the actions shows in the state.
    struct Digits;

    #[derive(Debug, Serialize, Deserialize, JsonSchema)]
    struct Push(u8);

    impl Feature for Digits {
        type State = String;
        type Action = Push;
        type Effect = ();

        fn update(state: &mut String, Push(digit): Push) -> Command<()> {
            state.push(char::from(b'0' + digit));
            Command::perform(())
        }
    }

    fn history_of(digits: &[u8], capacity: usize) -> (History<Digits>, String) {
        let mut history = History::new(String::new(), capacity);
        let mut state = String::new();
        for &digit in digits {
            let action = serde_json::to_value(Push(digit));
            Digits::update(&mut state, Push(digit));
            history.record(action, &state);
        }
        (history, state)
    }

    #[test]
    fn lists_actions_in_order_with_their_seq() {
        let (history, _) = history_of(&[1, 2], 4);

        assert_eq!(
            history.to_json(),
            json!({
                "base": "",
                "dropped": 0,
                "entries": [{ "seq": 0, "action": 1 }, { "seq": 1, "action": 2 }],
            })
        );
    }

    #[test]
    fn folds_the_oldest_entries_into_the_base_at_capacity() {
        let (history, state) = history_of(&[1, 2, 3, 4, 5], 2);

        assert_eq!(
            history.to_json(),
            json!({
                "base": "123",
                "dropped": 3,
                "entries": [{ "seq": 3, "action": 4 }, { "seq": 4, "action": 5 }],
            })
        );
        assert_eq!(history.replay(None), Ok(state));
    }

    #[test]
    fn replays_through_a_seq_without_changing_the_history() {
        let (history, _) = history_of(&[1, 2, 3, 4, 5], 3);

        assert_eq!(history.replay(Some(2)), Ok("123".to_string()));
        assert_eq!(history.replay(Some(3)), Ok("1234".to_string()));
        assert_eq!(history.replay(None), Ok("12345".to_string()));
        assert_eq!(history.to_json()["dropped"], json!(2));
    }

    #[test]
    fn rejects_dropped_and_future_seqs() {
        let (history, _) = history_of(&[1, 2, 3], 2);

        assert_eq!(
            history.replay(Some(0)),
            Err("entry 0 was dropped; the oldest entry is 1".to_string())
        );
        assert_eq!(
            history.replay(Some(3)),
            Err("entry 3 has not been recorded".to_string())
        );
    }

    #[test]
    fn an_unserializable_action_restarts_the_history_at_the_current_state() {
        let (mut history, _) = history_of(&[1, 2], 4);
        let failure = Err(serde::ser::Error::custom("not serializable"));

        history.record(failure, &"129".to_string());

        assert_eq!(
            history.to_json(),
            json!({ "base": "129", "dropped": 3, "entries": [] })
        );
        assert_eq!(history.replay(None), Ok("129".to_string()));
    }
}
