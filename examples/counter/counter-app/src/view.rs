use counter_core::{CounterAction, CounterFeature, CounterState};
use gpui::{App, ElementId, IntoElement, ParentElement, Rgba, Window, div, prelude::*, px};
use silon_gpui::{Dispatcher, FeatureView};

use crate::theme;

/// Amount added by the asynchronous button.
const ASYNC_DELTA: i32 = 5;

pub struct CounterView;

impl FeatureView<CounterFeature> for CounterView {
    fn view(
        state: &CounterState,
        dispatcher: Dispatcher<CounterAction>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> impl IntoElement + use<> {
        let status = if state.loading { "Adding…" } else { "Idle" };

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_4()
            .justify_center()
            .items_center()
            .bg(theme::surface())
            .text_color(theme::text_primary())
            .child(div().id("count").text_3xl().child(state.count.to_string()))
            .child(
                div()
                    .text_sm()
                    .text_color(theme::text_muted())
                    .child(status),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(button(
                        "decrement",
                        "-",
                        theme::button(),
                        &dispatcher,
                        CounterAction::Decrement,
                    ))
                    .child(button(
                        "increment",
                        "+",
                        theme::button(),
                        &dispatcher,
                        CounterAction::Increment,
                    ))
                    .child(
                        button(
                            "async-add",
                            "+5 (Async)",
                            theme::accent(),
                            &dispatcher,
                            CounterAction::RequestAsyncAdd(ASYNC_DELTA),
                        )
                        .text_color(theme::on_accent()),
                    ),
            )
    }
}

fn button(
    id: impl Into<ElementId>,
    label: &'static str,
    background: Rgba,
    dispatcher: &Dispatcher<CounterAction>,
    action: CounterAction,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px_4()
        .py_2()
        .min_w(px(48.))
        .flex()
        .justify_center()
        .rounded_md()
        .bg(background)
        .cursor_pointer()
        .hover(|style| style.bg(theme::button_hover()))
        .child(label)
        .on_click(dispatcher.on(action))
}
