use gpui::{
    AnyElement, App, IntoElement, ParentElement, RenderOnce, Styled, Window, div,
    prelude::FluentBuilder,
};

use crate::{Space, Theme};

/// Cross-axis alignment of children.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
    #[default]
    Stretch,
}

/// Main-axis distribution of children.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Justify {
    #[default]
    Start,
    Center,
    End,
    Between,
}

/// A flex container. Use it through [`Stack`] (vertical) or [`Row`]
/// (horizontal).
#[derive(IntoElement)]
pub struct Flex<const ROW: bool> {
    gap: Option<Space>,
    padding: Option<Space>,
    align: Align,
    justify: Justify,
    fill: bool,
    grow: bool,
    screen: bool,
    children: Vec<AnyElement>,
}

/// Lays children out top to bottom.
pub type Stack = Flex<false>;
/// Lays children out left to right.
pub type Row = Flex<true>;

impl<const ROW: bool> Flex<ROW> {
    pub fn new() -> Self {
        Self {
            gap: None,
            padding: None,
            align: if ROW { Align::Center } else { Align::Stretch },
            justify: Justify::Start,
            fill: false,
            grow: false,
            screen: false,
            children: Vec::new(),
        }
    }

    /// A container for a whole window: it fills the window and paints the
    /// theme's background and default text color.
    pub fn screen() -> Self {
        let mut flex = Self::new().fill();
        flex.screen = true;
        flex
    }

    pub fn gap(mut self, gap: Space) -> Self {
        self.gap = Some(gap);
        self
    }

    pub fn padding(mut self, padding: Space) -> Self {
        self.padding = Some(padding);
        self
    }

    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    pub fn justify(mut self, justify: Justify) -> Self {
        self.justify = justify;
        self
    }

    /// Takes the full size of the parent.
    pub fn fill(mut self) -> Self {
        self.fill = true;
        self
    }

    /// Takes the remaining space along the parent's main axis.
    pub fn grow(mut self) -> Self {
        self.grow = true;
        self
    }
}

impl<const ROW: bool> Default for Flex<ROW> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const ROW: bool> ParentElement for Flex<ROW> {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl<const ROW: bool> RenderOnce for Flex<ROW> {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let base = div()
            .flex()
            .when(ROW, |d| d.flex_row())
            .when(!ROW, |d| d.flex_col());
        let base = match self.align {
            Align::Start => base.items_start(),
            Align::Center => base.items_center(),
            Align::End => base.items_end(),
            Align::Stretch => base,
        };
        let base = match self.justify {
            Justify::Start => base.justify_start(),
            Justify::Center => base.justify_center(),
            Justify::End => base.justify_end(),
            Justify::Between => base.justify_between(),
        };
        base.when_some(self.gap, |d, gap| d.gap(theme.space(gap)))
            .when_some(self.padding, |d, padding| d.p(theme.space(padding)))
            .when(self.fill, |d| d.size_full())
            .when(self.grow, |d| d.flex_1().min_w_0())
            .when(self.screen, |d| {
                d.bg(theme.colors.background)
                    .text_color(theme.colors.text)
                    .text_size(theme.text_body)
            })
            .children(self.children)
    }
}

/// A raised, bordered area holding a vertical group of children.
#[derive(IntoElement)]
pub struct Panel {
    gap: Space,
    padding: Space,
    children: Vec<AnyElement>,
}

impl Panel {
    pub fn new() -> Self {
        Self {
            gap: Space::Md,
            padding: Space::Lg,
            children: Vec::new(),
        }
    }

    pub fn gap(mut self, gap: Space) -> Self {
        self.gap = gap;
        self
    }

    pub fn padding(mut self, padding: Space) -> Self {
        self.padding = padding;
        self
    }
}

impl Default for Panel {
    fn default() -> Self {
        Self::new()
    }
}

impl ParentElement for Panel {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Panel {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .flex()
            .flex_col()
            .gap(theme.space(self.gap))
            .p(theme.space(self.padding))
            .bg(theme.colors.surface)
            .border_1()
            .border_color(theme.colors.border)
            .rounded(theme.radius_md)
            .children(self.children)
    }
}
