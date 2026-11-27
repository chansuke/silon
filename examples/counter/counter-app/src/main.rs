mod effects;
mod view;

use counter_core::{CounterFeature, CounterState};
use gpui::{App, Application, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};
use silon_gpui::{FeatureRoot, Store};

use effects::CounterEffectRunner;
use view::CounterView;

type CounterStore = Store<CounterFeature, CounterEffectRunner>;

fn main() {
    Application::new().run(|cx: &mut App| {
        silon_ui::init(cx);
        let store =
            cx.new(|cx| CounterStore::new(CounterState::default(), CounterEffectRunner, cx));

        #[cfg(feature = "introspect")]
        start_introspection(&store, cx);

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

#[cfg(feature = "introspect")]
fn start_introspection(store: &gpui::Entity<CounterStore>, cx: &mut App) {
    let registry = silon_introspect::IntrospectionRegistry::default();
    registry.register(store.update(cx, |store, cx| store.attach_introspection("counter", cx)));

    let addr = silon_introspect::DEFAULT_ADDR;
    match silon_introspect::spawn_server(registry, addr) {
        Ok(_) => eprintln!("introspection server listening on http://{addr}"),
        Err(err) => eprintln!("introspection server failed to start on {addr}: {err}"),
    }
}
