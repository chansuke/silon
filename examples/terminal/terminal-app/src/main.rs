mod engine;
mod pty;
mod runner;
mod session;
mod view;

use gpui::{App, Application, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};
use silon_gpui::{FeatureRoot, Store};
use terminal_core::{TermAction, TermState, Terminal};

use runner::TermRunner;
use view::TermView;

type TermStore = Store<Terminal, TermRunner>;

/// The user's login shell, or `/bin/sh`.
fn shell() -> Vec<String> {
    vec![std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())]
}

fn main() {
    Application::new().run(|cx: &mut App| {
        silon_ui::init(cx);

        let store = cx.new(|cx| TermStore::new(TermState::new(shell()), TermRunner::new(), cx));
        store.update(cx, |store, cx| store.dispatch(TermAction::NewSession, cx));

        #[cfg(feature = "introspect")]
        start_introspection(&store, cx);

        let bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|cx| FeatureRoot::<_, _, TermView>::new(store, cx)),
        )
        .expect("failed to open the terminal window");

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
fn start_introspection(store: &gpui::Entity<TermStore>, cx: &mut App) {
    let registry = silon_introspect::IntrospectionRegistry::default();
    registry.register(store.update(cx, |store, cx| store.attach_introspection("terminal", cx)));

    let addr = silon_introspect::DEFAULT_ADDR;
    match silon_introspect::spawn_server(registry, addr) {
        Ok(_) => eprintln!("introspection server listening on http://{addr}"),
        Err(err) => eprintln!("introspection server failed to start on {addr}: {err}"),
    }
}
