//! Design tokens and view building blocks for Silon applications.
//!
//! Views build their screens from the stateless primitives here ([`Stack`],
//! [`Row`], [`Panel`], [`Text`], [`Button`], [`Badge`], [`Checkbox`]), which
//! take every color, space and size from the active [`Theme`]. The
//! `components` feature adds adapters around gpui-component's stateful
//! widgets.

mod badge;
mod button;
mod checkbox;
#[cfg(feature = "components")]
pub mod components;
mod layout;
mod text;
mod theme;

pub use badge::Badge;
pub use button::{Button, ButtonVariant};
pub use checkbox::Checkbox;
pub use layout::{Align, Flex, Justify, Panel, Row, Stack};
pub use text::{Text, TextSize};
pub use theme::{Colors, Space, Theme, Tone};

/// Installs the default theme. Call once at startup, before opening windows.
pub fn init(cx: &mut gpui::App) {
    cx.set_global(Theme::dark());
}
