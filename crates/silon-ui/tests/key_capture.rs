//! `KeyCapture` takes focus when clicked and reports keys only while focused.

use gpui::{
    AppContext, Entity, Modifiers, ParentElement, TestAppContext, VisualTestContext, WindowOptions,
    point, px,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use silon_core::{Command, Feature, NoEffects};
use silon_gpui::{Dispatcher, FeatureRoot, FeatureView, Store};
use silon_ui::{KeyCapture, KeyPress, Text};

struct Keys;

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct Pressed {
    key: String,
    text: Option<String>,
    ctrl: bool,
}

impl Feature for Keys {
    type State = Vec<String>;
    type Action = Pressed;
    type Effect = std::convert::Infallible;
    type Subscription = std::convert::Infallible;

    fn update(log: &mut Vec<String>, pressed: Pressed) -> Command<Self::Effect> {
        let ctrl = if pressed.ctrl { "ctrl-" } else { "" };
        log.push(format!("{ctrl}{} {:?}", pressed.key, pressed.text));
        Command::none()
    }
}

struct KeysView;

impl FeatureView<Keys> for KeysView {
    fn view(
        _log: &Vec<String>,
        dispatcher: Dispatcher<Pressed>,
        _window: &mut gpui::Window,
        _cx: &mut gpui::App,
    ) -> impl gpui::IntoElement + use<> {
        KeyCapture::new("keys")
            .on_key(dispatcher.map(|press: KeyPress| Pressed {
                key: press.key,
                text: press.text,
                ctrl: press.ctrl,
            }))
            .child(Text::new("type here"))
    }
}

type KeysStore = Store<Keys, NoEffects>;

fn open(cx: &mut TestAppContext) -> (Entity<KeysStore>, &mut VisualTestContext) {
    let store = cx.new(|cx| KeysStore::new(Vec::new(), NoEffects, cx));
    let window = cx.update(|cx| {
        silon_ui::init(cx);
        let store = store.clone();
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|cx| FeatureRoot::<_, _, KeysView>::new(store, cx))
        })
        .unwrap()
    });
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.run_until_parked();
    (store, cx)
}

fn log(store: &Entity<KeysStore>, cx: &mut VisualTestContext) -> Vec<String> {
    store.read_with(cx, |store, _| store.state().clone())
}

#[gpui::test]
fn keys_are_reported_once_the_area_is_clicked(cx: &mut TestAppContext) {
    let (store, cx) = open(cx);

    cx.simulate_keystrokes("x");
    cx.run_until_parked();
    assert!(log(&store, cx).is_empty(), "reported before it had focus");

    cx.simulate_click(point(px(20.), px(10.)), Modifiers::none());
    cx.simulate_keystrokes("a ctrl-c enter");
    cx.run_until_parked();

    // GPUI gives Enter the text "\n"; the press is passed on as it comes.
    assert_eq!(
        log(&store, cx),
        [r#"a Some("a")"#, "ctrl-c None", r#"enter Some("\n")"#]
    );
}
