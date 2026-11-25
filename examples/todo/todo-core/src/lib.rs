//! Pure todo-list feature: no GPUI, no async runtime, no IO.

use std::convert::Infallible;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use silon_core::{Command, Feature};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: u64,
    pub title: String,
    pub done: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoState {
    /// Text of the input field.
    pub draft: String,
    pub items: Vec<TodoItem>,
    /// Id given to the next item, so ids stay unique after removals.
    pub next_id: u64,
}

impl TodoState {
    pub fn remaining(&self) -> usize {
        self.items.iter().filter(|item| !item.done).count()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum TodoAction {
    /// The input text changed.
    DraftChanged(String),
    /// Adds the trimmed draft as an item and clears the draft. Blank drafts
    /// are ignored, and so is every draft once item ids run out.
    Submit,
    /// Flips `done` of the item with this id.
    Toggle(u64),
    /// Removes the item with this id.
    Remove(u64),
    /// Removes every done item.
    ClearCompleted,
}

pub struct TodoFeature;

impl Feature for TodoFeature {
    type State = TodoState;
    type Action = TodoAction;
    type Effect = Infallible;

    fn update(state: &mut TodoState, action: TodoAction) -> Command<Infallible> {
        match action {
            TodoAction::DraftChanged(text) => state.draft = text,
            TodoAction::Submit => {
                let title = state.draft.trim();
                // Once ids run out, which takes 2^64 submits, the draft stays.
                let next_id = state.next_id.checked_add(1);
                if let (false, Some(next_id)) = (title.is_empty(), next_id) {
                    state.items.push(TodoItem {
                        id: state.next_id,
                        title: title.to_owned(),
                        done: false,
                    });
                    state.next_id = next_id;
                    state.draft.clear();
                }
            }
            TodoAction::Toggle(id) => {
                if let Some(item) = state.items.iter_mut().find(|item| item.id == id) {
                    item.done = !item.done;
                }
            }
            TodoAction::Remove(id) => state.items.retain(|item| item.id != id),
            TodoAction::ClearCompleted => state.items.retain(|item| !item.done),
        }
        Command::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use silon_core::TestHarness;

    fn item(id: u64, title: &str, done: bool) -> TodoItem {
        TodoItem {
            id,
            title: title.to_owned(),
            done,
        }
    }

    fn with_items(items: Vec<TodoItem>) -> TestHarness<TodoFeature> {
        let next_id = items.iter().map(|item| item.id + 1).max().unwrap_or(0);
        TestHarness::new(TodoState {
            draft: String::new(),
            items,
            next_id,
        })
    }

    #[test]
    fn draft_changed_replaces_the_draft() {
        let mut h = with_items(vec![]);
        h.send(TodoAction::DraftChanged("milk".into()))
            .send(TodoAction::DraftChanged("milk and eggs".into()));

        assert_eq!(h.state.draft, "milk and eggs");
        assert!(h.state.items.is_empty());
        assert!(h.emitted_effects.is_empty());
    }

    #[test]
    fn submit_appends_the_trimmed_draft_and_clears_it() {
        let mut h = with_items(vec![]);
        h.send(TodoAction::DraftChanged("  牛乳を買う ".into()))
            .send(TodoAction::Submit)
            .send(TodoAction::DraftChanged("eggs".into()))
            .send(TodoAction::Submit);

        assert_eq!(
            h.state,
            TodoState {
                draft: String::new(),
                items: vec![item(0, "牛乳を買う", false), item(1, "eggs", false)],
                next_id: 2,
            }
        );
        assert!(h.emitted_effects.is_empty());
    }

    #[test]
    fn submit_keeps_the_draft_once_ids_run_out() {
        let full = TodoState {
            draft: "one more".into(),
            items: vec![item(u64::MAX - 1, "last", false)],
            next_id: u64::MAX,
        };
        let mut harness = TestHarness::<TodoFeature>::new(full.clone());

        harness.send(TodoAction::Submit);

        assert_eq!(harness.state, full);
        assert!(harness.take_effects().is_empty());
    }

    #[test]
    fn submit_ignores_a_blank_draft_and_keeps_it() {
        let mut h = with_items(vec![]);
        h.send(TodoAction::DraftChanged("   ".into()))
            .send(TodoAction::Submit);

        assert_eq!(h.state.draft, "   ");
        assert!(h.state.items.is_empty());
        assert_eq!(h.state.next_id, 0);
    }

    #[test]
    fn ids_stay_unique_after_removal() {
        let mut h = with_items(vec![item(0, "a", false), item(1, "b", false)]);
        h.send(TodoAction::Remove(1))
            .send(TodoAction::DraftChanged("c".into()))
            .send(TodoAction::Submit);

        assert_eq!(
            h.state.items,
            vec![item(0, "a", false), item(2, "c", false)]
        );
    }

    #[test]
    fn toggle_flips_only_the_matching_item() {
        let mut h = with_items(vec![item(0, "a", false), item(1, "b", false)]);
        h.send(TodoAction::Toggle(1));
        assert_eq!(h.state.items, vec![item(0, "a", false), item(1, "b", true)]);

        h.send(TodoAction::Toggle(1));
        assert_eq!(
            h.state.items,
            vec![item(0, "a", false), item(1, "b", false)]
        );
        assert!(h.emitted_effects.is_empty());
    }

    #[test]
    fn toggle_and_remove_ignore_unknown_ids() {
        let mut h = with_items(vec![item(0, "a", false)]);
        h.send(TodoAction::Toggle(9)).send(TodoAction::Remove(9));

        assert_eq!(h.state.items, vec![item(0, "a", false)]);
    }

    #[test]
    fn remove_deletes_the_matching_item() {
        let mut h = with_items(vec![item(0, "a", false), item(1, "b", true)]);
        h.send(TodoAction::Remove(0));

        assert_eq!(h.state.items, vec![item(1, "b", true)]);
        assert!(h.emitted_effects.is_empty());
    }

    #[test]
    fn clear_completed_keeps_open_items_in_order() {
        let mut h = with_items(vec![
            item(0, "a", true),
            item(1, "b", false),
            item(2, "c", true),
            item(3, "d", false),
        ]);
        h.send(TodoAction::ClearCompleted);

        assert_eq!(
            h.state.items,
            vec![item(1, "b", false), item(3, "d", false)]
        );
        assert_eq!(h.state.remaining(), 2);
        assert!(h.emitted_effects.is_empty());
    }

    #[test]
    fn actions_deserialize_from_agent_json() {
        let parse = |json: &str| serde_json::from_str::<TodoAction>(json).unwrap();

        assert_eq!(
            parse(r#"{"DraftChanged":"x"}"#),
            TodoAction::DraftChanged("x".into())
        );
        assert_eq!(parse(r#""Submit""#), TodoAction::Submit);
        assert_eq!(parse(r#"{"Toggle":3}"#), TodoAction::Toggle(3));
        assert_eq!(
            serde_json::to_value(TodoState::default()).unwrap(),
            serde_json::json!({ "draft": "", "items": [], "next_id": 0 })
        );
    }
}
