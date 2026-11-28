//! `TextGrid` reports its size once, wheel scrolling in lines, and where
//! the mouse presses and drags.

use gpui::{AppContext, Entity, TestAppContext, VisualTestContext, WindowOptions};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use silon_core::{Command, Feature, NoEffects};
use silon_gpui::{Dispatcher, FeatureRoot, FeatureView, Store};
use silon_ui::{GridPoint, GridPointer, GridSize, Span, TextGrid};

struct Grid;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct GridState {
    size: GridSize,
    reports: u32,
    scrolled: Vec<i32>,
    pointer: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
enum GridAction {
    Fits(u16, u16),
    Scrolled(i32),
    Pointer(String),
}

impl Feature for Grid {
    type State = GridState;
    type Action = GridAction;
    type Effect = std::convert::Infallible;
    type Subscription = std::convert::Infallible;

    fn update(state: &mut GridState, action: GridAction) -> Command<Self::Effect> {
        match action {
            GridAction::Fits(cols, rows) => {
                state.size = (cols, rows);
                state.reports += 1;
            }
            GridAction::Scrolled(lines) => state.scrolled.push(lines),
            GridAction::Pointer(event) => state.pointer.push(event),
        }
        Command::none()
    }
}

struct GridView;

impl FeatureView<Grid> for GridView {
    fn view(
        state: &GridState,
        dispatcher: Dispatcher<GridAction>,
        _window: &mut gpui::Window,
        _cx: &mut gpui::App,
    ) -> impl gpui::IntoElement + use<> {
        let line = |text: &str| {
            vec![Span {
                text: text.into(),
                ..Span::default()
            }]
        };
        TextGrid::new(vec![line("hello"), line("日本")])
            .fit(
                state.size,
                dispatcher.map(|(cols, rows): GridSize| GridAction::Fits(cols, rows)),
            )
            .on_scroll("grid", dispatcher.map(GridAction::Scrolled))
            .on_pointer(dispatcher.map(|pointer| {
                let (kind, GridPoint { row, col }) = match pointer {
                    GridPointer::Press(point) => ("press", point),
                    GridPointer::Drag(point) => ("drag", point),
                };
                GridAction::Pointer(format!("{kind} {row}:{col}"))
            }))
    }
}

type GridStore = Store<Grid, NoEffects>;

fn open(cx: &mut TestAppContext) -> (Entity<GridStore>, &mut VisualTestContext) {
    let store = cx.new(|cx| GridStore::new(GridState::default(), NoEffects, cx));
    let window = cx.update(|cx| {
        silon_ui::init(cx);
        let store = store.clone();
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|cx| FeatureRoot::<_, _, GridView>::new(store, cx))
        })
        .unwrap()
    });
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.run_until_parked();
    (store, cx)
}

#[gpui::test]
fn the_size_that_fits_is_reported_once(cx: &mut TestAppContext) {
    let (store, cx) = open(cx);
    // Draw again with the reported size; nothing new should be reported.
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();

    let state = store.read_with(cx, |store, _| store.state().clone());
    let (cols, rows) = state.size;
    assert!(cols > 1 && rows > 1, "reported {:?}", state.size);
    assert_eq!(state.reports, 1);
}

#[gpui::test]
fn wheel_events_become_whole_lines(cx: &mut TestAppContext) {
    let (store, cx) = open(cx);
    let wheel = |y: f32| gpui::ScrollWheelEvent {
        position: gpui::point(gpui::px(20.), gpui::px(20.)),
        delta: gpui::ScrollDelta::Lines(gpui::point(0., y)),
        ..Default::default()
    };

    cx.simulate_event(wheel(2.));
    cx.simulate_event(wheel(-0.5));
    cx.simulate_event(wheel(-0.5));
    cx.run_until_parked();

    let scrolled = store.read_with(cx, |store, _| store.state().scrolled.clone());
    assert_eq!(scrolled, [-2, 1]);
}

#[gpui::test]
fn presses_and_drags_land_between_characters(cx: &mut TestAppContext) {
    let (store, cx) = open(cx);
    // Rows are 18px high at the default 14px text.
    let row = |n: f32| gpui::px(18. * n + 9.);

    cx.simulate_mouse_down(
        gpui::point(gpui::px(1.), row(0.)),
        gpui::MouseButton::Left,
        gpui::Modifiers::none(),
    );
    cx.simulate_mouse_move(
        gpui::point(gpui::px(500.), row(1.)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::none(),
    );
    cx.simulate_mouse_move(
        gpui::point(gpui::px(1.), row(0.)),
        None,
        gpui::Modifiers::none(),
    );
    cx.run_until_parked();

    let pointer = store.read_with(cx, |store, _| store.state().pointer.clone());
    assert_eq!(pointer, ["press 0:0", "drag 1:2"]);
}

#[gpui::test]
fn a_press_lands_on_the_nearer_side_of_a_character(cx: &mut TestAppContext) {
    let (store, cx) = open(cx);
    // The test font advances 8px per character.
    for x in [3., 5., 11., 13.] {
        let at = gpui::point(gpui::px(x), gpui::px(9.));
        cx.simulate_mouse_down(at, gpui::MouseButton::Left, gpui::Modifiers::none());
        cx.simulate_mouse_up(at, gpui::MouseButton::Left, gpui::Modifiers::none());
    }
    cx.run_until_parked();

    let pointer = store.read_with(cx, |store, _| store.state().pointer.clone());
    assert_eq!(
        pointer,
        ["press 0:0", "press 0:1", "press 0:1", "press 0:2"]
    );
}
