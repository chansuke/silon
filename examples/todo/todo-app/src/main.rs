mod view;

use gpui::{App, Application, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};
use silon_core::NoEffects;
use silon_gpui::{FeatureRoot, Store};
use silon_ui::components;
use todo_core::{TodoFeature, TodoState};

use view::TodoView;

type TodoStore = Store<TodoFeature, NoEffects>;

fn main() {
    Application::new().run(|cx: &mut App| {
        silon_ui::init(cx);
        components::init(cx);

        let store = cx.new(|_| TodoStore::new(TodoState::default(), NoEffects));

        #[cfg(feature = "introspect")]
        start_introspection(&store, cx);

        let bounds = Bounds::centered(None, size(px(520.), px(560.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| FeatureRoot::<_, _, TodoView>::new(store, cx));
                components::window_root(view, window, cx)
            },
        )
        .expect("failed to open the todo window");

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
fn start_introspection(store: &gpui::Entity<TodoStore>, cx: &mut App) {
    let registry = silon_introspect::IntrospectionRegistry::default();
    registry.register(store.update(cx, |store, cx| store.attach_introspection("todo", cx)));

    let addr = silon_introspect::DEFAULT_ADDR;
    match silon_introspect::spawn_server(registry, addr) {
        Ok(_) => eprintln!("introspection server listening on http://{addr}"),
        Err(err) => eprintln!("introspection server failed to start on {addr}: {err}"),
    }
}
