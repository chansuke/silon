mod runner;
mod view;

use std::env;
use std::path::PathBuf;

use gpui::{App, Application, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};
use herdboard_core::{HerdState, Herdboard};
use silon_gpui::{FeatureRoot, Store};
use silon_ui::components;

use runner::HerdRunner;
use view::HerdView;

type HerdStore = Store<Herdboard, HerdRunner>;

/// The socket of the herdr-core session to show: the first argument, then
/// `HC_SOCKET`, then the default path of the `HC_SESSION` session. The socket
/// setting in hc's config and the `/run/user` fallback are not read.
fn socket() -> PathBuf {
    if let Some(path) = env::args_os().nth(1) {
        return path.into();
    }
    if let Some(path) = env::var_os("HC_SOCKET") {
        return path.into();
    }
    let session = env::var("HC_SESSION").unwrap_or_else(|_| "default".into());
    let file = format!("{session}.sock");
    if let Some(runtime) = env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime).join("herdr-core").join(file);
    }
    let state = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::home_dir().map(|home| home.join(".local/state")))
        .unwrap_or_default();
    state.join("herdr-core").join(&session).join(file)
}

fn main() {
    let socket = socket().to_string_lossy().into_owned();
    Application::new().run(move |cx: &mut App| {
        silon_ui::init(cx);
        components::init(cx);

        let store = cx.new(|cx| HerdStore::new(HerdState::new(socket), HerdRunner, cx));

        #[cfg(feature = "introspect")]
        start_introspection(&store, cx);

        let bounds = Bounds::centered(None, size(px(880.), px(560.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| FeatureRoot::<_, _, HerdView>::new(store, cx));
                components::window_root(view, window, cx)
            },
        )
        .expect("failed to open the herdboard window");

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
fn start_introspection(store: &gpui::Entity<HerdStore>, cx: &mut App) {
    let registry = silon_introspect::IntrospectionRegistry::default();
    registry.register(store.update(cx, |store, cx| store.attach_introspection("herdboard", cx)));

    let addr = silon_introspect::DEFAULT_ADDR;
    match silon_introspect::spawn_server(registry, addr) {
        Ok(_) => eprintln!("introspection server listening on http://{addr}"),
        Err(err) => eprintln!("introspection server failed to start on {addr}: {err}"),
    }
}
