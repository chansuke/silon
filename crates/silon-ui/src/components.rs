//! Adapters around gpui-component's stateful widgets.
//!
//! The widgets keep their ephemeral state (cursor, selection, IME
//! composition) to themselves and report only meaningful transitions through
//! a [`Dispatcher`]. Windows that show these widgets must use
//! [`window_root`] as their root view.

use gpui::{
    App, AppContext, ElementId, Entity, IntoElement, Render, RenderOnce, SharedString,
    Subscription, Window,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{Root, ThemeMode};
use silon_gpui::Dispatcher;

use crate::Theme;

/// Installs gpui-component and aligns its colors with the Silon [`Theme`].
/// Call after [`crate::init`].
pub fn init(cx: &mut App) {
    gpui_component::init(cx);
    gpui_component::Theme::change(ThemeMode::Dark, None, cx);

    let tokens = Theme::of(cx).clone();
    let colors = &tokens.colors;
    let theme = gpui_component::Theme::global_mut(cx);
    theme.font_size = tokens.text_body;
    theme.radius = tokens.radius_md;
    theme.colors.background = colors.background;
    theme.colors.foreground = colors.text;
    theme.colors.muted_foreground = colors.text_muted;
    theme.colors.border = colors.border;
    theme.colors.input = colors.border;
    theme.colors.ring = colors.accent;
    theme.colors.caret = colors.accent;
    theme.colors.primary = colors.accent;
    theme.colors.selection = colors.accent.opacity(0.35);
}

/// Wraps `view` in the root that gpui-component's widgets require.
pub fn window_root<V: Render>(view: Entity<V>, window: &mut Window, cx: &mut App) -> Entity<Root> {
    cx.new(|cx| Root::new(view, window, cx))
}

/// An IME-aware single-line text input bound to a `String` in feature state.
///
/// The view passes the current value on every render. Edits are reported
/// through `on_change`; a value that changes in state (for example, a draft
/// cleared after submit) is written into the field without being reported
/// back.
#[derive(IntoElement)]
pub struct TextInput {
    id: ElementId,
    value: SharedString,
    placeholder: SharedString,
    on_change: Option<Dispatcher<String>>,
    on_submit: Option<Dispatcher<String>>,
    on_blur: Option<Dispatcher<String>>,
}

impl TextInput {
    /// `id` must be unique within the window and stable across renders; the
    /// widget's state lives under it.
    pub fn new(id: impl Into<ElementId>, value: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            value: value.into(),
            placeholder: SharedString::default(),
            on_change: None,
            on_submit: None,
            on_blur: None,
        }
    }

    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Receives the full text after each edit.
    pub fn on_change(mut self, dispatcher: Dispatcher<String>) -> Self {
        self.on_change = Some(dispatcher);
        self
    }

    /// Receives the text when Enter is pressed.
    pub fn on_submit(mut self, dispatcher: Dispatcher<String>) -> Self {
        self.on_submit = Some(dispatcher);
        self
    }

    /// Receives the text when the field loses focus.
    pub fn on_blur(mut self, dispatcher: Dispatcher<String>) -> Self {
        self.on_blur = Some(dispatcher);
        self
    }
}

/// Per-widget state kept across renders.
struct Binding {
    input: Entity<InputState>,
    /// Text written from state whose `Change` event has not arrived yet.
    pending_echo: Option<SharedString>,
    on_change: Option<Dispatcher<String>>,
    on_submit: Option<Dispatcher<String>>,
    on_blur: Option<Dispatcher<String>>,
    _events: Subscription,
}

impl Binding {
    fn on_input_event(
        &mut self,
        input: Entity<InputState>,
        event: &InputEvent,
        cx: &mut gpui::Context<Self>,
    ) {
        let text = input.read(cx).value();
        let target = match event {
            InputEvent::Change => {
                if self.pending_echo.as_ref() == Some(&text) {
                    self.pending_echo = None;
                    return;
                }
                &self.on_change
            }
            InputEvent::PressEnter { secondary: false } => &self.on_submit,
            InputEvent::Blur => &self.on_blur,
            InputEvent::PressEnter { secondary: true } | InputEvent::Focus => return,
        };
        if let Some(dispatcher) = target.clone() {
            dispatcher.dispatch(text.to_string(), cx);
        }
    }
}

impl RenderOnce for TextInput {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let placeholder = self.placeholder.clone();
        let binding = window.use_keyed_state(self.id, cx, |window, cx| {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
            let events = cx.subscribe(&input, Binding::on_input_event);
            Binding {
                input,
                pending_echo: None,
                on_change: None,
                on_submit: None,
                on_blur: None,
                _events: events,
            }
        });

        let input = binding.update(cx, |binding, cx| {
            binding.on_change = self.on_change;
            binding.on_submit = self.on_submit;
            binding.on_blur = self.on_blur;
            if binding.input.read(cx).value() != self.value {
                binding.pending_echo = Some(self.value.clone());
                binding
                    .input
                    .update(cx, |input, cx| input.set_value(self.value, window, cx));
            }
            binding.input.clone()
        });

        Input::new(&input)
    }
}
