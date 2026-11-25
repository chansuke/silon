use counter_core::{CounterAction, CounterFeature, CounterState};
use gpui::{App, IntoElement, ParentElement, Window};
use silon_gpui::{Dispatcher, FeatureView};
use silon_ui::{
    Align, Badge, Button, ButtonVariant, Justify, Row, Space, Stack, Text, TextSize, Tone,
};

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
        let status = if state.loading {
            Badge::new("Adding…").tone(Tone::Accent)
        } else {
            Badge::new("Idle").tone(Tone::Muted)
        };

        Stack::screen()
            .gap(Space::Lg)
            .align(Align::Center)
            .justify(Justify::Center)
            .child(Text::new(state.count.to_string()).size(TextSize::Display))
            .child(status)
            .child(
                Row::new()
                    .gap(Space::Sm)
                    .child(
                        Button::new("decrement", "-")
                            .on_press(&dispatcher, CounterAction::Decrement),
                    )
                    .child(
                        Button::new("increment", "+")
                            .on_press(&dispatcher, CounterAction::Increment),
                    )
                    .child(
                        Button::new("async-add", "+5 (Async)")
                            .variant(ButtonVariant::Primary)
                            .disabled(state.loading)
                            .on_press(&dispatcher, CounterAction::RequestAsyncAdd(ASYNC_DELTA)),
                    ),
            )
    }
}
