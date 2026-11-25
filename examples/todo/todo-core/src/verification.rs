//! Kani proof harnesses for the todo list. Run with `cargo kani -p todo-core`.

use silon_core::Feature;

use crate::{TodoAction, TodoFeature, TodoItem, TodoState};

/// Items are bounded so that every loop unwinds within `kani::unwind`.
const MAX_ITEMS: usize = 2;

/// Ids are unique and below `next_id`, which every reachable state keeps.
fn ids_are_valid(state: &TodoState) -> bool {
    state.items.iter().enumerate().all(|(index, item)| {
        item.id < state.next_id && state.items[..index].iter().all(|other| other.id != item.id)
    })
}

fn any_state() -> TodoState {
    let len: usize = kani::any_where(|len| *len <= MAX_ITEMS);
    let items = (0..len)
        .map(|_| TodoItem {
            id: kani::any(),
            title: "a".into(),
            done: kani::any(),
        })
        .collect();
    let state = TodoState {
        draft: any_draft(),
        items,
        next_id: kani::any(),
    };
    kani::assume(ids_are_valid(&state));
    state
}

/// Blank, padded and plain drafts.
fn any_draft() -> String {
    match kani::any::<u8>() % 4 {
        0 => String::new(),
        1 => "  ".into(),
        2 => " x ".into(),
        _ => "y".into(),
    }
}

fn any_action() -> TodoAction {
    match kani::any::<u8>() % 5 {
        0 => TodoAction::DraftChanged(any_draft()),
        1 => TodoAction::Submit,
        2 => TodoAction::Toggle(kani::any()),
        3 => TodoAction::Remove(kani::any()),
        _ => TodoAction::ClearCompleted,
    }
}

/// From any state with valid ids and for any action, `update` does not
/// panic, keeps the ids valid, and changes exactly what the action names.
#[kani::proof]
#[kani::unwind(4)]
fn update_keeps_ids_unique_and_changes_only_the_named_item() {
    let before = any_state();
    let action = any_action();
    let mut after = before.clone();

    let effects = TodoFeature::update(&mut after, action.clone()).into_effects();

    assert!(effects.is_empty());
    assert!(ids_are_valid(&after));
    assert_eq!(
        after.remaining(),
        after.items.iter().filter(|item| !item.done).count()
    );

    match action {
        TodoAction::DraftChanged(text) => {
            assert_eq!(after.draft, text);
            assert_eq!(after.items, before.items);
        }
        TodoAction::Submit => {
            let title = before.draft.trim();
            if title.is_empty() || before.next_id == u64::MAX {
                assert_eq!(after, before);
            } else {
                assert_eq!(after.items.len(), before.items.len() + 1);
                assert_eq!(after.items[..before.items.len()], before.items[..]);
                let added = &after.items[before.items.len()];
                assert_eq!(
                    (added.id, added.title.as_str(), added.done),
                    (before.next_id, title, false)
                );
                assert!(after.draft.is_empty());
            }
        }
        TodoAction::Toggle(id) => {
            assert_eq!(after.items.len(), before.items.len());
            for (old, new) in before.items.iter().zip(&after.items) {
                assert_eq!(new.done, old.done != (old.id == id));
                assert_eq!((new.id, &new.title), (old.id, &old.title));
            }
        }
        TodoAction::Remove(id) => {
            let kept: Vec<_> = before
                .items
                .iter()
                .filter(|item| item.id != id)
                .cloned()
                .collect();
            assert_eq!(after.items, kept);
        }
        TodoAction::ClearCompleted => {
            let kept: Vec<_> = before
                .items
                .iter()
                .filter(|item| !item.done)
                .cloned()
                .collect();
            assert_eq!(after.items, kept);
        }
    }

    kani::cover!(
        after.items.len() == MAX_ITEMS + 1,
        "submitted onto a full list"
    );
    kani::cover!(
        after.items.len() + 1 == before.items.len(),
        "removed an item"
    );
    kani::cover!(
        before.next_id == u64::MAX && !before.draft.trim().is_empty(),
        "submitted after the ids ran out"
    );
}
