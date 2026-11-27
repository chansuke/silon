//! The actions a [`Store`](crate::Store) has dispatched since introspection
//! was attached, stored as JSON so that agents can read and replay them.

use serde_json::{Value, json};
use silon_core::{Feature, History};

/// Number of actions a store keeps before folding the oldest into the base.
pub const HISTORY_CAPACITY: usize = 256;

/// A [`History`] of actions serialized as JSON. Effects are not part of it:
/// their follow-up actions are recorded like any other action.
pub(crate) struct ActionHistory<F: Feature>(History<F::State, Value>);

/// Applies an action stored as JSON. Its effects are discarded: they already
/// ran when the action was dispatched.
fn apply<F: Feature>(state: &mut F::State, action: &Value) -> Result<(), String> {
    let action = serde_json::from_value(action.clone())
        .map_err(|err| format!("cannot read recorded action {action}: {err}"))?;
    let _ = F::update(state, action);
    Ok(())
}

impl<F: Feature> ActionHistory<F> {
    pub(crate) fn new(base: F::State) -> Self {
        Self(History::new(base, HISTORY_CAPACITY))
    }

    /// Records `action`, serialized before it was applied, given the state
    /// `current` right after it was applied.
    pub(crate) fn record(&mut self, action: serde_json::Result<Value>, current: &F::State) {
        self.0.record(action.ok(), current, apply::<F>);
    }

    /// `{"base": state, "dropped": n, "entries": [{"seq": n, "action": action}]}`.
    pub(crate) fn to_json(&self) -> Value {
        let entries: Vec<Value> = self
            .0
            .entries()
            .map(|(seq, action)| json!({ "seq": seq, "action": action }))
            .collect();
        json!({
            "base": serde_json::to_value(self.0.base()).unwrap_or(Value::Null),
            "dropped": self.0.first_seq(),
            "entries": entries,
        })
    }

    /// The state right after entry `through`, or after every entry for `None`.
    pub(crate) fn replay(&self, through: Option<u64>) -> Result<F::State, String> {
        self.0
            .replay(through, apply::<F>)
            .map_err(|err| err.to_string())
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
        type Subscription = std::convert::Infallible;

        fn update(state: &mut String, Push(digit): Push) -> Command<()> {
            state.push(char::from(b'0' + digit));
            Command::perform(())
        }
    }

    fn history_of(digits: &[u8]) -> (ActionHistory<Digits>, String) {
        let mut history = ActionHistory::new(String::new());
        let mut state = String::new();
        for &digit in digits {
            let action = serde_json::to_value(Push(digit));
            let _ = Digits::update(&mut state, Push(digit));
            history.record(action, &state);
        }
        (history, state)
    }

    #[test]
    fn lists_actions_as_json_with_their_seq() {
        let (history, state) = history_of(&[1, 2]);

        assert_eq!(
            history.to_json(),
            json!({
                "base": "",
                "dropped": 0,
                "entries": [{ "seq": 0, "action": 1 }, { "seq": 1, "action": 2 }],
            })
        );
        assert_eq!(history.replay(None), Ok(state));
        assert_eq!(history.replay(Some(0)), Ok("1".to_string()));
        assert_eq!(
            history.replay(Some(2)),
            Err("entry 2 has not been recorded".to_string())
        );
    }

    #[test]
    fn an_unserializable_action_restarts_the_history_at_the_current_state() {
        let (mut history, _) = history_of(&[1, 2]);

        history.record(
            Err(serde::ser::Error::custom("not serializable")),
            &"129".to_string(),
        );

        assert_eq!(
            history.to_json(),
            json!({ "base": "129", "dropped": 3, "entries": [] })
        );
    }

    #[test]
    fn an_action_that_does_not_deserialize_fails_the_replay() {
        let mut history = ActionHistory::<Digits>::new(String::new());
        history.record(Ok(json!("not a digit")), &"?".to_string());

        let err = history.replay(None).unwrap_err();
        assert!(
            err.starts_with(r#"cannot read recorded action "not a digit""#),
            "{err}"
        );
    }
}
