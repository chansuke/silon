use gpui::{App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div};

use crate::{Space, Theme, Tone};

/// A small label on a tinted pill, such as a count or a status.
#[derive(IntoElement)]
pub struct Badge {
    label: SharedString,
    tone: Tone,
}

impl Badge {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            tone: Tone::Neutral,
        }
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }
}

impl RenderOnce for Badge {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let color = theme.tone(self.tone);
        div()
            .px(theme.space(Space::Sm))
            .py(theme.space(Space::Xs) / 2.)
            .rounded_full()
            .border_1()
            .border_color(color.opacity(0.6))
            .bg(color.opacity(0.15))
            .text_color(color)
            .text_size(theme.text_caption)
            .child(self.label)
    }
}
