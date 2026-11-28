//! Pure counter feature: no GPUI, no async runtime, no IO.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use silon_core::{Command, Feature};

/// Delay requested for the asynchronous add.
pub const ASYNC_ADD_DELAY_MS: u64 = 1_000;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CounterState {
    pub count: i32,
    pub loading: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CounterAction {
    Increment,
    Decrement,
    /// Asks for `delta` to be added after a delay. Ignored while another
    /// asynchronous add is in flight.
    RequestAsyncAdd(i32),
    /// Delivered by the effect runner when the delayed add finishes.
    AsyncAddCompleted(i32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CounterEffect {
    DelayedAdd { delta: i32, delay_ms: u64 },
}

pub struct CounterFeature;

impl Feature for CounterFeature {
    type State = CounterState;
    type Action = CounterAction;
    type Effect = CounterEffect;
    type Subscription = std::convert::Infallible;

    fn update(state: &mut CounterState, action: CounterAction) -> Command<CounterEffect> {
        match action {
            CounterAction::Increment => {
                state.count = state.count.saturating_add(1);
                Command::none()
            }
            CounterAction::Decrement => {
                state.count = state.count.saturating_sub(1);
                Command::none()
            }
            CounterAction::RequestAsyncAdd(_) if state.loading => Command::none(),
            CounterAction::RequestAsyncAdd(delta) => {
                state.loading = true;
                Command::perform(CounterEffect::DelayedAdd {
                    delta,
                    delay_ms: ASYNC_ADD_DELAY_MS,
                })
            }
            CounterAction::AsyncAddCompleted(delta) => {
                state.count = state.count.saturating_add(delta);
                state.loading = false;
                Command::none()
            }
        }
    }
}

#[cfg(kani)]
mod verification;

#[cfg(test)]
mod tests {
    use super::*;
    use silon_core::TestHarness;

    fn harness() -> TestHarness<CounterFeature> {
        TestHarness::new(CounterState::default())
    }

    #[test]
    fn increment_and_decrement_change_count_without_effects() {
        let mut h = harness();
        h.send(CounterAction::Increment)
            .send(CounterAction::Increment)
            .send(CounterAction::Decrement);

        assert_eq!(
            h.state,
            CounterState {
                count: 1,
                loading: false
            }
        );
        assert!(h.emitted_effects.is_empty());
    }

    #[test]
    fn decrement_below_zero_goes_negative() {
        let mut h = harness();
        h.send(CounterAction::Decrement);

        assert_eq!(h.state.count, -1);
        assert!(h.emitted_effects.is_empty());
    }

    #[test]
    fn count_saturates_at_bounds() {
        let mut h = TestHarness::<CounterFeature>::new(CounterState {
            count: i32::MAX,
            loading: false,
        });
        h.send(CounterAction::Increment);
        assert_eq!(h.state.count, i32::MAX);

        h.state.count = i32::MIN;
        h.send(CounterAction::Decrement);
        assert_eq!(h.state.count, i32::MIN);
    }

    #[test]
    fn request_async_add_sets_loading_and_emits_delayed_add() {
        let mut h = harness();
        h.send(CounterAction::RequestAsyncAdd(5));

        assert_eq!(
            h.state,
            CounterState {
                count: 0,
                loading: true
            }
        );
        assert_eq!(
            h.take_effects(),
            vec![CounterEffect::DelayedAdd {
                delta: 5,
                delay_ms: ASYNC_ADD_DELAY_MS,
            }]
        );
    }

    #[test]
    fn request_async_add_is_ignored_while_loading() {
        let mut h = harness();
        h.send(CounterAction::RequestAsyncAdd(5));
        h.take_effects();

        h.send(CounterAction::RequestAsyncAdd(3));

        assert_eq!(
            h.state,
            CounterState {
                count: 0,
                loading: true
            }
        );
        assert!(h.emitted_effects.is_empty());
    }

    #[test]
    fn async_add_completed_applies_delta_and_clears_loading() {
        let mut h = harness();
        h.send(CounterAction::Increment)
            .send(CounterAction::RequestAsyncAdd(5));
        h.take_effects();

        h.send(CounterAction::AsyncAddCompleted(5));

        assert_eq!(
            h.state,
            CounterState {
                count: 6,
                loading: false
            }
        );
        assert!(h.emitted_effects.is_empty());
    }

    #[test]
    fn async_add_completed_saturates() {
        let mut h = TestHarness::<CounterFeature>::new(CounterState {
            count: i32::MAX - 1,
            loading: true,
        });
        h.send(CounterAction::AsyncAddCompleted(5));

        assert_eq!(
            h.state,
            CounterState {
                count: i32::MAX,
                loading: false
            }
        );
    }

    #[test]
    fn state_and_actions_round_trip_through_json() {
        assert_eq!(
            serde_json::to_string(&CounterState::default()).unwrap(),
            r#"{"count":0,"loading":false}"#
        );
        assert_eq!(
            serde_json::from_str::<CounterAction>(r#""Increment""#).unwrap(),
            CounterAction::Increment
        );
        assert_eq!(
            serde_json::from_str::<CounterAction>(r#"{"RequestAsyncAdd":5}"#).unwrap(),
            CounterAction::RequestAsyncAdd(5)
        );
    }
}
