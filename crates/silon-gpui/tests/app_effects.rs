//! Effects that need the app run on the main thread; the rest still reach
//! the runner.

use gpui::{AppContext, ClipboardItem, TestAppContext};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use silon_core::{BoxFuture, BoxStream, Command, EffectRunner, Feature};
use silon_gpui::Store;

struct Notes;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct NotesState {
    log: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
enum NotesAction {
    CopyAndSave(String),
    Done(String),
}

#[derive(Debug)]
enum NotesEffect {
    Copy(String),
    Save(String),
}

impl Feature for Notes {
    type State = NotesState;
    type Action = NotesAction;
    type Effect = NotesEffect;
    type Subscription = std::convert::Infallible;

    fn update(state: &mut NotesState, action: NotesAction) -> Command<NotesEffect> {
        match action {
            NotesAction::CopyAndSave(text) => Command::batch([
                Command::perform(NotesEffect::Copy(text.clone())),
                Command::perform(NotesEffect::Save(text)),
            ]),
            NotesAction::Done(what) => {
                state.log.push(what);
                Command::none()
            }
        }
    }
}

/// Saves in the background; copying never reaches it.
struct Saver;

impl EffectRunner<Notes> for Saver {
    fn run(&self, effect: NotesEffect) -> BoxFuture<'static, Option<NotesAction>> {
        let done = match effect {
            NotesEffect::Save(text) => format!("saved {text}"),
            NotesEffect::Copy(text) => format!("runner copied {text}"),
        };
        Box::pin(async move { Some(NotesAction::Done(done)) })
    }

    fn subscribe(&self, subscription: std::convert::Infallible) -> BoxStream<'static, NotesAction> {
        match subscription {}
    }
}

#[gpui::test]
fn the_app_runs_the_effects_it_takes_and_hands_back_the_rest(cx: &mut TestAppContext) {
    let store = cx.new(|cx| {
        Store::new(NotesState::default(), Saver, cx).on_app(|effect, cx| match effect {
            NotesEffect::Copy(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                Ok(Some(NotesAction::Done(format!("copied {text}"))))
            }
            other => Err(other),
        })
    });

    store.update(cx, |store, cx| {
        store.dispatch(NotesAction::CopyAndSave("hi".into()), cx);
        // The follow-up waits for a later turn, like a background one.
        assert!(store.state().log.is_empty());
    });
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("hi".into())
    );
    cx.run_until_parked();

    let mut log = store.read_with(cx, |store, _| store.state().log.clone());
    log.sort();
    assert_eq!(log, ["copied hi", "saved hi"]);
}
