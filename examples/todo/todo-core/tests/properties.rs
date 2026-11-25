//! Property tests: random action sequences from the initial state keep the
//! list consistent.

use proptest::prelude::*;
use silon_core::Feature;
use todo_core::{TodoAction, TodoFeature, TodoState};

fn action() -> impl Strategy<Value = TodoAction> {
    // Few ids, so that Toggle and Remove often hit an existing item.
    prop_oneof![
        "[ a-c]{0,3}".prop_map(TodoAction::DraftChanged),
        Just(TodoAction::Submit),
        (0_u64..6).prop_map(TodoAction::Toggle),
        (0_u64..6).prop_map(TodoAction::Remove),
        Just(TodoAction::ClearCompleted),
    ]
}

proptest! {
    #[test]
    fn items_stay_ordered_unique_and_trimmed(actions in prop::collection::vec(action(), 0..60)) {
        let mut state = TodoState::default();
        for action in actions {
            let effects = TodoFeature::update(&mut state, action).into_effects();
            prop_assert!(effects.is_empty());

            // Items keep the order they were added in, so ids increase.
            prop_assert!(state.items.windows(2).all(|pair| pair[0].id < pair[1].id));
            prop_assert!(state.items.iter().all(|item| item.id < state.next_id));
            let trimmed = |title: &str| !title.is_empty() && title.trim() == title;
            prop_assert!(state.items.iter().all(|item| trimmed(&item.title)));
            let open = state.items.iter().filter(|item| !item.done).count();
            prop_assert_eq!(state.remaining(), open);
        }
    }
}
