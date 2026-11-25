use gpui::{
    App, ClickEvent, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px,
};
use silon_gpui::Dispatcher;

use crate::Theme;

type ToggleHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A square that shows a boolean and reports clicks. It holds no state: the
/// view passes `checked` from the feature state.
#[derive(IntoElement)]
pub struct Checkbox {
    id: ElementId,
    checked: bool,
    on_toggle: Option<ToggleHandler>,
}

impl Checkbox {
    pub fn new(id: impl Into<ElementId>, checked: bool) -> Self {
        Self {
            id: id.into(),
            checked,
            on_toggle: None,
        }
    }

    /// Dispatches a copy of `action` on every click.
    pub fn on_toggle<A: Clone + 'static>(mut self, dispatcher: &Dispatcher<A>, action: A) -> Self {
        self.on_toggle = Some(Box::new(dispatcher.on(action)));
        self
    }
}

impl RenderOnce for Checkbox {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let colors = &theme.colors;

        div()
            .id(self.id)
            .flex_none()
            .size(px(18.))
            .flex()
            .justify_center()
            .items_center()
            .rounded(theme.radius_sm)
            .border_1()
            .cursor_pointer()
            .map(|d| {
                if self.checked {
                    d.bg(colors.accent)
                        .border_color(colors.accent)
                        .child(div().size(px(8.)).rounded(px(2.)).bg(colors.on_accent))
                } else {
                    d.border_color(colors.text_muted)
                        .hover(|style| style.border_color(colors.accent))
                }
            })
            .when_some(self.on_toggle, |d, handler| d.on_click(handler))
    }
}
