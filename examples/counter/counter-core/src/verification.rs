//! Kani proof harnesses for the counter. Run with `cargo kani -p counter-core`.

use silon_core::Feature;

use crate::{ASYNC_ADD_DELAY_MS, CounterAction, CounterEffect, CounterFeature, CounterState};

fn any_action() -> CounterAction {
    match kani::any::<u8>() % 4 {
        0 => CounterAction::Increment,
        1 => CounterAction::Decrement,
        2 => CounterAction::RequestAsyncAdd(kani::any()),
        _ => CounterAction::AsyncAddCompleted(kani::any()),
    }
}

/// From any state and for any action, `update` does not panic, changes the
/// count by exactly the saturated amount, and starts a delayed add only when
/// none is in flight, which is then reported by `loading`.
#[kani::proof]
fn update_is_total_and_starts_one_delayed_add_at_a_time() {
    let before = CounterState {
        count: kani::any(),
        loading: kani::any(),
    };
    let action = any_action();
    let mut after = before.clone();

    let effects = CounterFeature::update(&mut after, action.clone()).into_effects();

    match action {
        CounterAction::Increment => {
            assert_eq!(after.count, before.count.saturating_add(1));
            assert_eq!(after.loading, before.loading);
            assert!(effects.is_empty());
        }
        CounterAction::Decrement => {
            assert_eq!(after.count, before.count.saturating_sub(1));
            assert_eq!(after.loading, before.loading);
            assert!(effects.is_empty());
        }
        CounterAction::RequestAsyncAdd(delta) => {
            assert_eq!(after.count, before.count);
            assert!(after.loading);
            if before.loading {
                assert!(effects.is_empty());
            } else {
                let delay_ms = ASYNC_ADD_DELAY_MS;
                assert_eq!(effects, [CounterEffect::DelayedAdd { delta, delay_ms }]);
            }
        }
        CounterAction::AsyncAddCompleted(delta) => {
            assert_eq!(after.count, before.count.saturating_add(delta));
            assert!(!after.loading);
            assert!(effects.is_empty());
        }
    }

    kani::cover!(
        after.count == i32::MAX && before.count == i32::MAX,
        "saturated"
    );
    kani::cover!(effects.len() == 1, "started a delayed add");
}
