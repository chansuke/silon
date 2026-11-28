//! Effects reach a subscription's mailbox, even one the same update opens.

use futures::StreamExt;
use gpui::{AppContext, TestAppContext};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use silon_core::{BoxFuture, BoxStream, Command, EffectRunner, Feature, Mailboxes};
use silon_gpui::Store;

/// Opens a line and says something on it in the same update.
struct Line;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct LineState {
    open: bool,
    heard: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
enum LineAction {
    OpenAndSay(String),
    Say(String),
    Heard(String),
}

impl Feature for Line {
    type State = LineState;
    type Action = LineAction;
    type Effect = String;
    type Subscription = ();

    fn update(state: &mut LineState, action: LineAction) -> Command<String> {
        match action {
            LineAction::OpenAndSay(text) => {
                state.open = true;
                Command::perform(text)
            }
            LineAction::Say(text) => Command::perform(text),
            LineAction::Heard(text) => {
                state.heard.push(text);
                Command::none()
            }
        }
    }

    fn subscriptions(state: &LineState) -> Vec<()> {
        if state.open { vec![()] } else { Vec::new() }
    }
}

/// Echoes what is posted to the open line back as `Heard`.
#[derive(Default)]
struct Echo(Mailboxes<(), String>);

impl EffectRunner<Line> for Echo {
    fn run(&self, text: String) -> BoxFuture<'static, Option<LineAction>> {
        let posted = self.0.post(&(), text);
        Box::pin(async move {
            posted
                .err()
                .map(|text| LineAction::Heard(format!("lost {text}")))
        })
    }

    fn subscribe(&self, (): ()) -> BoxStream<'static, LineAction> {
        Box::pin(self.0.open(()).map(LineAction::Heard))
    }
}

#[gpui::test]
fn an_effect_reaches_the_subscription_its_update_opens(cx: &mut TestAppContext) {
    let store = cx.new(|cx| Store::new(LineState::default(), Echo::default(), cx));

    store.update(cx, |store, cx| {
        store.dispatch(LineAction::OpenAndSay("hello".into()), cx);
        store.dispatch(LineAction::Say("again".into()), cx);
    });
    cx.run_until_parked();

    let heard = store.read_with(cx, |store, _| store.state().heard.clone());
    assert_eq!(heard, ["hello", "again"]);
}
