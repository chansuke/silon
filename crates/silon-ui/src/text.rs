use gpui::{
    App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div,
    prelude::FluentBuilder,
};

use crate::{Theme, Tone};

/// Steps of the type scale.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextSize {
    Caption,
    #[default]
    Body,
    Title,
    Display,
}

/// A run of text drawn with theme sizes and tones.
#[derive(IntoElement)]
pub struct Text {
    content: SharedString,
    size: TextSize,
    tone: Tone,
    strikethrough: bool,
    mono: bool,
}

impl Text {
    pub fn new(content: impl Into<SharedString>) -> Self {
        Self {
            content: content.into(),
            size: TextSize::Body,
            tone: Tone::Neutral,
            strikethrough: false,
            mono: false,
        }
    }

    pub fn size(mut self, size: TextSize) -> Self {
        self.size = size;
        self
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    pub fn strikethrough(mut self, strikethrough: bool) -> Self {
        self.strikethrough = strikethrough;
        self
    }

    /// Draws in the theme's monospaced font, so columns line up.
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }
}

impl RenderOnce for Text {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let size = match self.size {
            TextSize::Caption => theme.text_caption,
            TextSize::Body => theme.text_body,
            TextSize::Title => theme.text_title,
            TextSize::Display => theme.text_display,
        };
        div()
            .text_size(size)
            .text_color(theme.tone(self.tone))
            .when(self.strikethrough, |d| d.line_through())
            .when(self.mono, |d| d.font_family(theme.font_mono.clone()))
            .child(self.content)
    }
}
