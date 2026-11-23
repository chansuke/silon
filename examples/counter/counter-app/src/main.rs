mod effects;
mod theme;
mod view;

use counter_core::{CounterFeature, CounterState};
use gpui::{App, Application, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};
use silon_gpui::{FeatureRoot, Store};

use effects::CounterEffectRunner;
use view::CounterView;

type CounterStore = Store<CounterFeature, CounterEffectRunner>;

fn main() {
    Application::new().run(|cx: &mut App| {
        let store = cx.new(|_| CounterStore::new(CounterState::default(), CounterEffectRunner));

        let bounds = Bounds::centered(None, size(px(420.), px(320.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|cx| FeatureRoot::<_, _, CounterView>::new(store, cx)),
        )
        .expect("failed to open the counter window");

        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
