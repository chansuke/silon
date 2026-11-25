//! `TextInput` reports user edits and submits, and writes values that change
//! in state into the field without reporting them back.

use gpui::{
    AppContext, Entity, Modifiers, TestAppContext, VisualTestContext, WindowOptions, point, px,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use silon_core::{Command, Feature, NoEffects};
use silon_gpui::{Dispatcher, FeatureRoot, FeatureView, Store};
use silon_ui::components::{self, TextInput};

struct Note;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct NoteState {
    draft: String,
    /// Every action received, in order.
    log: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
enum NoteAction {
    Draft(String),
    Submit(String),
}

impl Feature for Note {
    type State = NoteState;
    type Action = NoteAction;
    type Effect = std::convert::Infallible;

    fn update(state: &mut NoteState, action: NoteAction) -> Command<Self::Effect> {
        state.log.push(format!("{action:?}"));
        match action {
            NoteAction::Draft(text) => state.draft = text,
            NoteAction::Submit(_) => state.draft.clear(),
        }
        Command::none()
    }
}

struct NoteView;

impl FeatureView<Note> for NoteView {
    fn view(
        state: &NoteState,
        dispatcher: Dispatcher<NoteAction>,
        _window: &mut gpui::Window,
        _cx: &mut gpui::App,
    ) -> impl gpui::IntoElement + use<> {
        TextInput::new("draft", state.draft.clone())
            .on_change(dispatcher.map(NoteAction::Draft))
            .on_submit(dispatcher.map(NoteAction::Submit))
    }
}

type NoteStore = Store<Note, NoEffects>;

fn open(cx: &mut TestAppContext) -> (Entity<NoteStore>, &mut VisualTestContext) {
    let store = cx.new(|_| NoteStore::new(NoteState::default(), NoEffects));
    let window = cx.update(|cx| {
        silon_ui::init(cx);
        components::init(cx);
        let store = store.clone();
        cx.open_window(WindowOptions::default(), |window, cx| {
            let view = cx.new(|cx| FeatureRoot::<_, _, NoteView>::new(store, cx));
            components::window_root(view, window, cx)
        })
        .unwrap()
    });
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.run_until_parked();
    (store, cx)
}

/// Focuses the field by clicking it; it is the only element in the window.
fn focus_field(cx: &mut VisualTestContext) {
    cx.simulate_click(point(px(40.), px(12.)), Modifiers::none());
    let focused = cx.update(|window, cx| window.focused(cx).is_some());
    assert!(focused, "the text field did not take focus");
}

fn state(store: &Entity<NoteStore>, cx: &mut VisualTestContext) -> NoteState {
    store.read_with(cx, |store, _| store.state().clone())
}

#[gpui::test]
fn edits_and_submits_are_dispatched(cx: &mut TestAppContext) {
    let (store, cx) = open(cx);

    focus_field(cx);
    cx.simulate_input("hi");
    // A simulated "enter" keystroke also types "\n" after the single-line
    // input lets it propagate, which only the test platform does. Dispatch
    // the action the "enter" binding maps to instead.
    cx.dispatch_action(gpui_component::input::Enter { secondary: false });
    cx.run_until_parked();

    let state = state(&store, cx);
    assert_eq!(
        state.log,
        [r#"Draft("h")"#, r#"Draft("hi")"#, r#"Submit("hi")"#]
    );
    assert_eq!(state.draft, "");
}

#[gpui::test]
fn values_from_state_are_not_reported_back(cx: &mut TestAppContext) {
    let (store, cx) = open(cx);

    store.update(cx, |store, cx| {
        store.dispatch(NoteAction::Draft("from agent".into()), cx)
    });
    cx.run_until_parked();

    // The field now holds the value, so typing at its end appends to it.
    focus_field(cx);
    cx.simulate_keystrokes("end");
    cx.simulate_input("!");
    cx.run_until_parked();

    let state = state(&store, cx);
    assert_eq!(
        state.log,
        [r#"Draft("from agent")"#, r#"Draft("from agent!")"#]
    );
}
