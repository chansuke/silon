//! With `on_text`, typed text goes through the platform's text input, where
//! input methods compose it, and other keys still come as key presses.

use gpui::{
    AppContext, Entity, Modifiers, ParentElement, TestAppContext, VisualTestContext, WindowOptions,
    point, px,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use silon_core::{Command, Feature, NoEffects};
use silon_gpui::{Dispatcher, FeatureRoot, FeatureView, Store};
use silon_ui::{KeyCapture, KeyPress, Text};

struct Typing;

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
enum Typed {
    Text(String),
    Key(String),
}

impl Feature for Typing {
    type State = Vec<String>;
    type Action = Typed;
    type Effect = std::convert::Infallible;
    type Subscription = std::convert::Infallible;

    fn update(log: &mut Vec<String>, typed: Typed) -> Command<Self::Effect> {
        log.push(format!("{typed:?}"));
        Command::none()
    }
}

struct TypingView;

impl FeatureView<Typing> for TypingView {
    fn view(
        _log: &Vec<String>,
        dispatcher: Dispatcher<Typed>,
        _window: &mut gpui::Window,
        _cx: &mut gpui::App,
    ) -> impl gpui::IntoElement + use<> {
        KeyCapture::new("typing")
            .on_key(dispatcher.map(|press: KeyPress| {
                let ctrl = if press.ctrl { "ctrl-" } else { "" };
                Typed::Key(format!("{ctrl}{}", press.key))
            }))
            .on_text(dispatcher.map(Typed::Text))
            .child(Text::new("type here"))
    }
}

fn open(cx: &mut TestAppContext) -> (Entity<Store<Typing, NoEffects>>, &mut VisualTestContext) {
    let store = cx.new(|cx| Store::new(Vec::new(), NoEffects, cx));
    let window = cx.update(|cx| {
        silon_ui::init(cx);
        let store = store.clone();
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|cx| FeatureRoot::<_, _, TypingView>::new(store, cx))
        })
        .unwrap()
    });
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.run_until_parked();
    (store, cx)
}

#[gpui::test]
fn text_comes_through_text_input_and_other_keys_as_presses(cx: &mut TestAppContext) {
    let (store, cx) = open(cx);
    cx.simulate_click(point(px(20.), px(10.)), Modifiers::none());

    cx.simulate_input("aあ");
    cx.simulate_keystrokes("shift-b enter ctrl-c");
    cx.run_until_parked();

    let log = store.read_with(cx, |store, _| store.state().clone());
    assert_eq!(
        log,
        [
            r#"Text("a")"#,
            r#"Text("あ")"#,
            r#"Text("B")"#,
            r#"Key("enter")"#,
            r#"Key("ctrl-c")"#,
        ]
    );
}
