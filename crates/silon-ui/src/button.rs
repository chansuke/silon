use gpui::{
    App, ClickEvent, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder,
};
use silon_gpui::Dispatcher;

use crate::{Space, Theme};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    Primary,
    #[default]
    Secondary,
    /// No fill until hovered.
    Ghost,
    Danger,
}

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A clickable label.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    variant: ButtonVariant,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            variant: ButtonVariant::Secondary,
            disabled: false,
            on_click: None,
        }
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// A disabled button is dimmed and ignores clicks.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Dispatches a copy of `action` on every click.
    pub fn on_press<A: Clone + 'static>(self, dispatcher: &Dispatcher<A>, action: A) -> Self {
        self.on_click(dispatcher.on(action))
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let colors = &theme.colors;
        let (fill, hover, text) = match self.variant {
            ButtonVariant::Primary => (colors.accent, colors.accent_hover, colors.on_accent),
            ButtonVariant::Secondary => (colors.neutral, colors.neutral_hover, colors.text),
            ButtonVariant::Ghost => (colors.background.opacity(0.), colors.neutral, colors.text),
            ButtonVariant::Danger => (colors.danger, colors.danger_hover, colors.on_accent),
        };
        let enabled = !self.disabled;

        div()
            .id(self.id)
            .flex()
            .justify_center()
            .items_center()
            .min_w(theme.space(Space::Xxl) + theme.space(Space::Lg))
            .px(theme.space(Space::Lg))
            .py(theme.space(Space::Sm))
            .rounded(theme.radius_md)
            .bg(fill)
            .text_color(text)
            .text_size(theme.text_body)
            .child(self.label)
            .when(!enabled, |d| d.opacity(0.5))
            .when(enabled, |d| {
                d.cursor_pointer().hover(move |style| style.bg(hover))
            })
            .when_some(self.on_click.filter(|_| enabled), |d, handler| {
                d.on_click(handler)
            })
    }
}
