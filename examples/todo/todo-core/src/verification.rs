//! Kani proof harnesses for the todo list. Run with `cargo kani -p todo-core`.
//!
//! Each harness builds a concrete-length `Vec<TodoItem>` (0, 1, or 2 items) and
//! forgets the verified `Command::None` return value so CBMC does not unwind
//! `Command<E>`'s recursive `Drop` or symbolic `Vec` reallocations.

use silon_core::{Command, Feature};

use crate::{TodoAction, TodoFeature, TodoItem, TodoState};

/// Maximum items in the initial state of each harness.
const MAX_ITEMS: usize = 2;

/// Ids are unique and below `next_id`, which every reachable state keeps.
fn ids_are_valid(state: &TodoState) -> bool {
    state.items.iter().enumerate().all(|(index, item)| {
        item.id < state.next_id && state.items[..index].iter().all(|other| other.id != item.id)
    })
}

/// Asserts that `cmd` is `Command::None` and skips its recursive `Drop` glue
/// (which is a no-op for `Command::None`).
fn assert_no_effects(cmd: Command<std::convert::Infallible>) {
    assert!(matches!(cmd, Command::None));
    std::mem::forget(cmd);
}

fn any_item(title: &str) -> TodoItem {
    TodoItem {
        id: kani::any(),
        title: title.into(),
        done: kani::any(),
    }
}

/// Builds a valid `TodoState` with 2 distinct-titled items (or 0 items when
/// `empty` is true) without symbolic `Vec::with_capacity` reallocations.
fn two_item_state(draft: String) -> TodoState {
    let state = TodoState {
        draft,
        items: vec![any_item("a"), any_item("b")],
        next_id: kani::any(),
    };
    kani::assume(ids_are_valid(&state));
    state
}

/// A 2-byte ASCII draft drawn from `' '` and `'x'`, allocated as a single
/// concrete 2-byte `String` so CBMC explores blank (`"  "`), leading-padded
/// (`" x"`), trailing-padded (`"x "`), and plain (`"xx"`) drafts without
/// symbolic allocation-size branching.
fn any_draft() -> String {
    let b0 = if kani::any() { b' ' } else { b'x' };
    let b1 = if kani::any() { b' ' } else { b'x' };
    String::from_utf8(vec![b0, b1]).unwrap()
}

#[kani::proof]
#[kani::unwind(4)]
fn draft_changed_replaces_only_the_draft() {
    let before = two_item_state("d".into());
    let text = any_draft();
    let mut after = before.clone();

    let cmd = TodoFeature::update(&mut after, TodoAction::DraftChanged(text.clone()));

    assert_no_effects(cmd);
    assert!(ids_are_valid(&after));
    assert_eq!(after.draft, text);
    assert_eq!(after.items, before.items);
    assert_eq!(after.next_id, before.next_id);

    kani::cover!(after.draft == "  ", "changed draft to blank");
    kani::cover!(after.draft == " x", "changed draft to padded text");
}

#[kani::proof]
#[kani::unwind(4)]
fn submit_appends_trimmed_draft_or_keeps_state() {
    let before = two_item_state(" x ".into());
    let mut after = TodoState {
        draft: before.draft.clone(),
        items: {
            let mut items = Vec::with_capacity(MAX_ITEMS + 1);
            items.extend(before.items.iter().cloned());
            items
        },
        next_id: before.next_id,
    };

    let cmd = TodoFeature::update(&mut after, TodoAction::Submit);

    assert_no_effects(cmd);
    assert!(ids_are_valid(&after));
    assert_eq!(
        after.remaining(),
        after.items.iter().filter(|item| !item.done).count()
    );

    if before.next_id == u64::MAX {
        assert_eq!(after, before);
    } else {
        assert_eq!(after.items.len(), MAX_ITEMS + 1);
        assert_eq!(after.items[..MAX_ITEMS], before.items[..]);
        let added = &after.items[MAX_ITEMS];
        assert_eq!(
            (added.id, added.title.as_str(), added.done),
            (before.next_id, "x", false)
        );
        assert_eq!(after.next_id, before.next_id + 1);
        assert!(after.draft.is_empty());
    }

    kani::cover!(
        after.items.len() == MAX_ITEMS + 1,
        "submitted onto a full list"
    );
    kani::cover!(
        before.next_id == u64::MAX,
        "submitted after the ids ran out"
    );
}

#[kani::proof]
#[kani::unwind(4)]
fn submit_ignores_blank_draft() {
    let before = two_item_state("  ".into());
    let mut after = before.clone();

    let cmd = TodoFeature::update(&mut after, TodoAction::Submit);

    assert_no_effects(cmd);
    assert!(ids_are_valid(&after));
    assert_eq!(after, before);

    kani::cover!(after.draft == "  ", "kept blank draft unchanged");
}

#[kani::proof]
#[kani::unwind(4)]
fn toggle_flips_only_the_named_item() {
    let before = two_item_state("d".into());
    let id: u64 = kani::any();
    let mut after = before.clone();

    let cmd = TodoFeature::update(&mut after, TodoAction::Toggle(id));

    assert_no_effects(cmd);
    assert!(ids_are_valid(&after));
    assert_eq!(after.draft, before.draft);
    assert_eq!(after.next_id, before.next_id);
    assert_eq!(after.items.len(), before.items.len());
    for (old, new) in before.items.iter().zip(&after.items) {
        assert_eq!(new.done, old.done != (old.id == id));
        assert_eq!((new.id, &new.title), (old.id, &old.title));
    }

    let matched = before.items.iter().any(|item| item.id == id);
    kani::cover!(matched, "toggled an existing item");
    kani::cover!(!matched, "ignored an unknown id");
}

#[kani::proof]
#[kani::unwind(4)]
fn remove_deletes_only_the_named_item() {
    let before = two_item_state("d".into());
    let id: u64 = kani::any();
    let mut after = before.clone();

    let cmd = TodoFeature::update(&mut after, TodoAction::Remove(id));

    assert_no_effects(cmd);
    assert!(ids_are_valid(&after));
    assert_eq!(after.draft, before.draft);
    assert_eq!(after.next_id, before.next_id);

    let mut kept = 0usize;
    for item in &before.items {
        if item.id != id {
            assert!(kept < after.items.len());
            assert_eq!(after.items[kept], *item);
            kept += 1;
        }
    }
    assert_eq!(kept, after.items.len());

    kani::cover!(
        after.items.len() + 1 == before.items.len(),
        "removed an existing item"
    );
    kani::cover!(
        after.items.len() == before.items.len(),
        "ignored an unknown id"
    );
}

#[kani::proof]
#[kani::unwind(4)]
fn clear_completed_keeps_open_items_in_order() {
    let before = two_item_state("d".into());
    let mut after = before.clone();

    let cmd = TodoFeature::update(&mut after, TodoAction::ClearCompleted);

    assert_no_effects(cmd);
    assert!(ids_are_valid(&after));
    assert_eq!(after.draft, before.draft);
    assert_eq!(after.next_id, before.next_id);
    assert_eq!(after.remaining(), after.items.len());

    let mut kept = 0usize;
    for item in &before.items {
        if !item.done {
            assert!(kept < after.items.len());
            assert_eq!(after.items[kept], *item);
            kept += 1;
        }
    }
    assert_eq!(kept, after.items.len());

    kani::cover!(
        after.items.len() == 1,
        "cleared one completed item while keeping one open"
    );
    kani::cover!(after.items.is_empty(), "cleared all completed items");
    kani::cover!(
        after.items.len() == before.items.len(),
        "kept all open items"
    );
}
