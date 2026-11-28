//! `TextGrid::fit` reports the grid that fits once, then stays quiet.

use gpui::{AppContext, Entity, TestAppContext, VisualTestContext, WindowOptions};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use silon_core::{Command, Feature, NoEffects};
use silon_gpui::{Dispatcher, FeatureRoot, FeatureView, Store};
use silon_ui::{GridSize, TextGrid};

struct Grid;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct GridState {
    size: GridSize,
    reports: u32,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct Fits(u16, u16);

impl Feature for Grid {
    type State = GridState;
    type Action = Fits;
    type Effect = std::convert::Infallible;
    type Subscription = std::convert::Infallible;

    fn update(state: &mut GridState, Fits(cols, rows): Fits) -> Command<Self::Effect> {
        state.size = (cols, rows);
        state.reports += 1;
        Command::none()
    }
}

struct GridView;

impl FeatureView<Grid> for GridView {
    fn view(
        state: &GridState,
        dispatcher: Dispatcher<Fits>,
        _window: &mut gpui::Window,
        _cx: &mut gpui::App,
    ) -> impl gpui::IntoElement + use<> {
        TextGrid::new(Vec::new()).fit(
            state.size,
            dispatcher.map(|(cols, rows): GridSize| Fits(cols, rows)),
        )
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
