use gpui::{
    AnyElement, App, ElementId, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    MouseButton, ParentElement, RenderOnce, Styled, Window, div,
};
use silon_gpui::Dispatcher;

use crate::{Space, Theme};

/// One key press, as plain data a view can turn into an action.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyPress {
    /// The key's name: `a`, `enter`, `up`, `f1`, ...
    pub key: String,
    /// The text the press types, with Shift and the layout applied.
    pub text: Option<String>,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// Cmd on macOS, the Windows or Super key elsewhere.
    pub platform: bool,
}

impl From<&gpui::Keystroke> for KeyPress {
    fn from(keystroke: &gpui::Keystroke) -> Self {
        let modifiers = keystroke.modifiers;
        Self {
            key: keystroke.key.clone(),
            text: keystroke.key_char.clone(),
            ctrl: modifiers.control,
            alt: modifiers.alt,
            shift: modifiers.shift,
            platform: modifiers.platform,
        }
    }
}

/// An area that takes keyboard focus when clicked and reports each key
/// pressed while focused. The focus handle is the element's own ephemeral
/// state; the view only sees [`KeyPress`]es.
#[derive(IntoElement)]
pub struct KeyCapture {
    id: ElementId,
    on_key: Option<Dispatcher<KeyPress>>,
    children: Vec<AnyElement>,
}

impl KeyCapture {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            on_key: None,
            children: Vec::new(),
        }
    }

    /// Dispatches every key pressed while the area has focus.
    pub fn on_key(mut self, dispatcher: Dispatcher<KeyPress>) -> Self {
        self.on_key = Some(dispatcher);
        self
    }
}

impl ParentElement for KeyCapture {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for KeyCapture {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let theme = Theme::of(cx);
        let border = if focus.is_focused(window) {
            theme.colors.accent
        } else {
            theme.colors.border
        };
        let on_key = self.on_key;
        let clicked: FocusHandle = focus.clone();

        div()
            .id(self.id)
            .track_focus(&focus)
            .on_mouse_down(MouseButton::Left, move |_, window, _| {
                window.focus(&clicked);
            })
            .on_key_down(move |event: &KeyDownEvent, _, cx| {
                if let Some(on_key) = &on_key {
                    on_key.dispatch(KeyPress::from(&event.keystroke), cx);
                    cx.stop_propagation();
                }
            })
            .p(theme.space(Space::Md))
            .bg(theme.colors.background)
            .border_1()
            .border_color(border)
            .rounded(theme.radius_md)
            .children(self.children)
    }
}
