//! `FeatureRoot` renders the view from the store's state without copying it.

use std::sync::atomic::{AtomicUsize, Ordering};

use gpui::{AppContext, TestAppContext, VisualTestContext, WindowOptions, div};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use silon_core::{Command, Feature, NoEffects};
use silon_gpui::{Dispatcher, FeatureRoot, FeatureView, Store};

static CLONES: AtomicUsize = AtomicUsize::new(0);
static RENDERS: AtomicUsize = AtomicUsize::new(0);

/// A state that counts its copies, like a large screen would cost them.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Counted(u32);

impl Clone for Counted {
    fn clone(&self) -> Self {
        CLONES.fetch_add(1, Ordering::SeqCst);
        Counted(self.0)
    }
}

struct Bump;

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct Up;

impl Feature for Bump {
    type State = Counted;
    type Action = Up;
    type Effect = std::convert::Infallible;
    type Subscription = std::convert::Infallible;

    fn update(state: &mut Counted, Up: Up) -> Command<Self::Effect> {
        state.0 += 1;
        Command::none()
    }
}

struct BumpView;

impl FeatureView<Bump> for BumpView {
    fn view(
        _state: &Counted,
        _dispatcher: Dispatcher<Up>,
        _window: &mut gpui::Window,
        _cx: &mut gpui::App,
    ) -> impl gpui::IntoElement + use<> {
        RENDERS.fetch_add(1, Ordering::SeqCst);
        div()
    }
}

#[gpui::test]
fn rendering_does_not_copy_the_state(cx: &mut TestAppContext) {
    let store = cx.new(|cx| Store::<Bump, NoEffects>::new(Counted::default(), NoEffects, cx));
    let window = cx.update(|cx| {
        let store = store.clone();
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|cx| FeatureRoot::<_, _, BumpView>::new(store, cx))
        })
        .unwrap()
    });
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.run_until_parked();
    for _ in 0..3 {
        store.update(cx, |store, cx| store.dispatch(Up, cx));
        cx.run_until_parked();
    }

    assert!(
        RENDERS.load(Ordering::SeqCst) >= 4,
        "the view did not re-render"
    );
    assert_eq!(CLONES.load(Ordering::SeqCst), 0);
}
