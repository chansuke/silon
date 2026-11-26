//! Kani proof harnesses for the counter. Run with `cargo kani -p counter-core`.

use silon_core::{Command, Feature};

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
///
/// `Command<E>` is recursive via `Batch(Vec<Command<E>>)`, so its `Drop` impl
/// is recursive in MIR even though `update` only returns `None` or `Perform`.
#[kani::proof]
#[kani::unwind(2)]
fn update_is_total_and_starts_one_delayed_add_at_a_time() {
    let before = CounterState {
        count: kani::any(),
        loading: kani::any(),
    };
    let action = any_action();
    let mut after = before.clone();

    let cmd = CounterFeature::update(&mut after, action.clone());

    match action {
        CounterAction::Increment => {
            assert_eq!(after.count, before.count.saturating_add(1));
            assert_eq!(after.loading, before.loading);
            assert!(matches!(cmd, Command::None));
        }
        CounterAction::Decrement => {
            assert_eq!(after.count, before.count.saturating_sub(1));
            assert_eq!(after.loading, before.loading);
            assert!(matches!(cmd, Command::None));
        }
        CounterAction::RequestAsyncAdd(delta) => {
            assert_eq!(after.count, before.count);
            assert!(after.loading);
            if before.loading {
                assert!(matches!(cmd, Command::None));
            } else {
                let delay_ms = ASYNC_ADD_DELAY_MS;
                assert!(matches!(
                    cmd,
                    Command::Perform(CounterEffect::DelayedAdd {
                        delta: d,
                        delay_ms: ms,
                    }) if d == delta && ms == delay_ms
                ));
            }
        }
        CounterAction::AsyncAddCompleted(delta) => {
            assert_eq!(after.count, before.count.saturating_add(delta));
            assert!(!after.loading);
            assert!(matches!(cmd, Command::None));
        }
    }

    kani::cover!(
        matches!(action, CounterAction::Increment)
            && before.count == i32::MAX
            && after.count == i32::MAX,
        "increment saturated at MAX"
    );
    kani::cover!(
        matches!(action, CounterAction::Decrement)
            && before.count == i32::MIN
            && after.count == i32::MIN,
        "decrement saturated at MIN"
    );
    kani::cover!(
        matches!(action, CounterAction::RequestAsyncAdd(_)) && !before.loading && after.loading,
        "started a delayed add"
    );
    kani::cover!(
        matches!(action, CounterAction::RequestAsyncAdd(_)) && before.loading,
        "ignored delayed add while loading"
    );
    kani::cover!(
        matches!(action, CounterAction::AsyncAddCompleted(_)) && before.loading && !after.loading,
        "completed a delayed add"
    );
}
