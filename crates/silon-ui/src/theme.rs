use std::sync::LazyLock;

use gpui::{App, Global, Hsla, Pixels, SharedString, px, rgb};

/// Semantic colors. Views refer to roles, never to concrete colors.
#[derive(Clone, Debug)]
pub struct Colors {
    /// Window background.
    pub background: Hsla,
    /// Raised areas such as panels.
    pub surface: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    /// Fill of secondary controls.
    pub neutral: Hsla,
    pub neutral_hover: Hsla,
    pub accent: Hsla,
    pub accent_hover: Hsla,
    /// Text and marks drawn on `accent`, `success` or `danger`.
    pub on_accent: Hsla,
    pub success: Hsla,
    pub danger: Hsla,
    pub danger_hover: Hsla,
}

/// Steps of the spacing scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    Xs,
    Sm,
    Md,
    Lg,
    Xl,
    Xxl,
}

/// Meaning of a piece of text or a badge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Neutral,
    Muted,
    Accent,
    Success,
    Danger,
}

/// Every token the primitives draw with.
#[derive(Clone, Debug)]
pub struct Theme {
    pub colors: Colors,
    /// Pixel sizes of [`Space::Xs`] through [`Space::Xxl`].
    pub space_scale: [Pixels; 6],
    pub radius_sm: Pixels,
    pub radius_md: Pixels,
    pub text_caption: Pixels,
    pub text_body: Pixels,
    pub text_title: Pixels,
    pub text_display: Pixels,
    /// Font family for monospaced text, such as a terminal screen.
    pub font_mono: SharedString,
}

impl Global for Theme {}

static FALLBACK: LazyLock<Theme> = LazyLock::new(Theme::dark);

impl Theme {
    /// The installed theme, or the default dark theme if none is installed.
    pub fn of(cx: &App) -> &Theme {
        cx.try_global::<Theme>().unwrap_or(&FALLBACK)
    }

    pub fn space(&self, space: Space) -> Pixels {
        let index = match space {
            Space::Xs => 0,
            Space::Sm => 1,
            Space::Md => 2,
            Space::Lg => 3,
            Space::Xl => 4,
            Space::Xxl => 5,
        };
        self.space_scale[index]
    }

    pub fn tone(&self, tone: Tone) -> Hsla {
        match tone {
            Tone::Neutral => self.colors.text,
            Tone::Muted => self.colors.text_muted,
            Tone::Accent => self.colors.accent,
            Tone::Success => self.colors.success,
            Tone::Danger => self.colors.danger,
        }
    }

    pub fn dark() -> Self {
        Self::with_colors(Colors {
            background: rgb(0x1e1e2e).into(),
            surface: rgb(0x262637).into(),
            border: rgb(0x3a3a4f).into(),
            text: rgb(0xcdd6f4).into(),
            text_muted: rgb(0x9399b2).into(),
            neutral: rgb(0x45475a).into(),
            neutral_hover: rgb(0x585b70).into(),
            accent: rgb(0x89b4fa).into(),
            accent_hover: rgb(0xa4c4fb).into(),
            on_accent: rgb(0x11111b).into(),
            success: rgb(0xa6e3a1).into(),
            danger: rgb(0xf38ba8).into(),
            danger_hover: rgb(0xf5a3ba).into(),
        })
    }

    pub fn light() -> Self {
        Self::with_colors(Colors {
            background: rgb(0xeff1f5).into(),
            surface: rgb(0xffffff).into(),
            border: rgb(0xccd0da).into(),
            text: rgb(0x4c4f69).into(),
            text_muted: rgb(0x6c6f85).into(),
            neutral: rgb(0xdce0e8).into(),
            neutral_hover: rgb(0xccd0da).into(),
            accent: rgb(0x1e66f5).into(),
            accent_hover: rgb(0x4682f7).into(),
            on_accent: rgb(0xffffff).into(),
            success: rgb(0x40a02b).into(),
            danger: rgb(0xd20f39).into(),
            danger_hover: rgb(0xe03e61).into(),
        })
    }

    fn with_colors(colors: Colors) -> Self {
        Self {
            colors,
            space_scale: [px(4.), px(8.), px(12.), px(16.), px(24.), px(32.)],
            radius_sm: px(4.),
            radius_md: px(6.),
            text_caption: px(12.),
            text_body: px(14.),
            text_title: px(20.),
            text_display: px(32.),
            font_mono: if cfg!(target_os = "macos") {
                "Menlo".into()
            } else if cfg!(target_os = "windows") {
                "Consolas".into()
            } else {
                "DejaVu Sans Mono".into()
            },
        }
    }
}
