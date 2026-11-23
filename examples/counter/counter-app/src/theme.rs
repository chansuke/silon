//! Semantic colors for the counter views. Placeholder until `silon-ui`
//! provides design tokens.

use gpui::{Rgba, rgb};

pub fn surface() -> Rgba {
    rgb(0x1e1e2e)
}

pub fn text_primary() -> Rgba {
    rgb(0xcdd6f4)
}

pub fn text_muted() -> Rgba {
    rgb(0x9399b2)
}

pub fn button() -> Rgba {
    rgb(0x45475a)
}

pub fn button_hover() -> Rgba {
    rgb(0x585b70)
}

pub fn accent() -> Rgba {
    rgb(0x89b4fa)
}

pub fn on_accent() -> Rgba {
    rgb(0x11111b)
}
