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
enum Pressed {
    Key {
        key: String,
        text: Option<String>,
        ctrl: bool,
    },
    Paste(String),
}

impl Feature for Keys {
    type State = Vec<String>;
    type Action = Pressed;
    type Effect = std::convert::Infallible;
    type Subscription = std::convert::Infallible;

    fn update(log: &mut Vec<String>, pressed: Pressed) -> Command<Self::Effect> {
        log.push(match pressed {
            Pressed::Key { key, text, ctrl } => {
                let ctrl = if ctrl { "ctrl-" } else { "" };
                format!("{ctrl}{key} {text:?}")
            }
            Pressed::Paste(text) => format!("paste {text:?}"),
        });
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
            .on_key(dispatcher.map(|press: KeyPress| Pressed::Key {
                key: press.key,
                text: press.text,
                ctrl: press.ctrl,
            }))
            .on_paste(dispatcher.map(Pressed::Paste))
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

#[gpui::test]
fn the_paste_shortcut_dispatches_the_clipboard_text(cx: &mut TestAppContext) {
    let (store, cx) = open(cx);
    cx.write_to_clipboard(gpui::ClipboardItem::new_string("ls -l".into()));

    cx.simulate_click(point(px(20.), px(10.)), Modifiers::none());
    let paste = if cfg!(target_os = "macos") {
        "cmd-v"
    } else {
        "ctrl-shift-v"
    };
    cx.simulate_keystrokes(paste);
    cx.simulate_keystrokes("shift-insert");
    cx.run_until_parked();

    let expected = if cfg!(target_os = "macos") {
        vec![r#"paste "ls -l""#, r#"insert None"#]
    } else {
        vec![r#"paste "ls -l""#, r#"paste "ls -l""#]
    };
    assert_eq!(log(&store, cx), expected);
}

#[test]
fn only_the_platform_shortcut_is_a_paste() {
    let press = |key: &str, ctrl, shift, platform| KeyPress {
        key: key.into(),
        ctrl,
        shift,
        platform,
        ..KeyPress::default()
    };
    let ctrl_v = press("v", true, false, false);
    assert!(!ctrl_v.is_paste(), "ctrl-v belongs to the program");
    let ctrl_alt_shift_v = KeyPress {
        alt: true,
        ..press("v", true, true, false)
    };
    assert!(!ctrl_alt_shift_v.is_paste());
    if cfg!(target_os = "macos") {
        assert!(press("v", false, false, true).is_paste());
    } else {
        assert!(press("v", true, true, false).is_paste());
        assert!(press("V", true, true, false).is_paste());
        assert!(press("insert", false, true, false).is_paste());
        assert!(!press("insert", true, true, false).is_paste());
    }
}
