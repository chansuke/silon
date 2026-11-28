use gpui::{
    App, Bounds, FontStyle, FontWeight, Hsla, IntoElement, ParentElement, Pixels, RenderOnce, Rgba,
    Styled, StyledText, TextRun, UnderlineStyle, Window, canvas, div, font, prelude::FluentBuilder,
    px,
};
use silon_gpui::Dispatcher;

use crate::Theme;

/// A color that comes with the content, such as a terminal's SGR colors,
/// rather than from the theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl From<Rgb> for Hsla {
    fn from(Rgb { r, g, b }: Rgb) -> Self {
        Rgba {
            r: f32::from(r) / 255.,
            g: f32::from(g) / 255.,
            b: f32::from(b) / 255.,
            a: 1.,
        }
        .into()
    }
}

/// A run of text with one style. Colors left out take the theme's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub fg: Option<Rgb>,
    pub bg: Option<Rgb>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

/// Lines of styled monospaced text, one column per character, with an
/// optional block cursor. For content whose colors are data, such as a
/// terminal screen.
#[derive(IntoElement)]
pub struct TextGrid {
    lines: Vec<Vec<Span>>,
    cursor: Option<(usize, usize)>,
    fit: Option<Fit>,
}

/// Columns and rows of a grid.
pub type GridSize = (u16, u16);

struct Fit {
    current: GridSize,
    on_fit: Dispatcher<GridSize>,
}

/// Line height as a multiple of the font size.
const LINE_HEIGHT: f32 = 1.3;

/// The columns and rows of `cell` that fit in `bounds`, at least one each.
fn cells_in(bounds: Bounds<Pixels>, cell: gpui::Size<Pixels>) -> GridSize {
    let count = |space: Pixels, step: Pixels| {
        let n = (f32::from(space) / f32::from(step).max(1.)).floor();
        // Saturates: NaN becomes 0, huge values u16::MAX.
        (n as u16).max(1)
    };
    (
        count(bounds.size.width, cell.width),
        count(bounds.size.height, cell.height),
    )
}

impl TextGrid {
    pub fn new(lines: Vec<Vec<Span>>) -> Self {
        Self {
            lines,
            cursor: None,
            fit: None,
        }
    }

    /// Fills the space it is given and dispatches the columns and rows that
    /// fit there whenever they differ from `current`, the size the content
    /// was made for. A view learns its size this way, as an action after
    /// layout, instead of reading the window while it renders.
    pub fn fit(mut self, current: GridSize, on_fit: Dispatcher<GridSize>) -> Self {
        self.fit = Some(Fit { current, on_fit });
        self
    }

    /// Draws a block cursor on `row`, `col` (zero-based, in characters).
    pub fn cursor(mut self, row: usize, col: usize) -> Self {
        self.cursor = Some((row, col));
        self
    }
}

/// Where the cursor cell gets drawn: its text and its default colors.
#[derive(Clone, Copy)]
struct CursorColors {
    fg: Hsla,
    bg: Hsla,
}

/// Splits `line` so the character at `col` is a span of its own, padding
/// with spaces when the line is shorter, and returns that span's index.
fn isolate(line: &[Span], col: usize) -> (Vec<Span>, usize) {
    let mut out = Vec::with_capacity(line.len() + 2);
    let mut start = 0;
    let mut found = None;
    for span in line {
        let len = span.text.chars().count();
        if found.is_none() && col < start + len {
            let at = col - start;
            let mut chars = span.text.chars();
            let before: String = chars.by_ref().take(at).collect();
            let cell: String = chars.by_ref().take(1).collect();
            let after: String = chars.collect();
            for (text, is_cell) in [(before, false), (cell, true), (after, false)] {
                if text.is_empty() {
                    continue;
                }
                if is_cell {
                    found = Some(out.len());
                }
                out.push(Span {
                    text,
                    ..span.clone()
                });
            }
        } else {
            out.push(span.clone());
        }
        start += len;
    }
    let index = found.unwrap_or_else(|| {
        let pad = col - start;
        if pad > 0 {
            out.push(Span {
                text: " ".repeat(pad),
                ..Span::default()
            });
        }
        out.push(Span {
            text: " ".into(),
            ..Span::default()
        });
        out.len() - 1
    });
    (out, index)
}

fn render_line(
    spans: &[Span],
    cursor: Option<usize>,
    family: &gpui::SharedString,
    text: Hsla,
    cursor_colors: CursorColors,
) -> StyledText {
    let (spans, cursor_index) = match cursor {
        Some(col) => {
            let (spans, index) = isolate(spans, col);
            (spans, Some(index))
        }
        None => (spans.to_vec(), None),
    };
    let mut content = String::new();
    let mut runs = Vec::with_capacity(spans.len());
    for (i, span) in spans.iter().enumerate() {
        content.push_str(&span.text);
        let mut run_font = font(family.clone());
        if span.bold {
            run_font.weight = FontWeight::BOLD;
        }
        if span.italic {
            run_font.style = FontStyle::Italic;
        }
        let (color, background) = if cursor_index == Some(i) {
            (cursor_colors.fg, Some(cursor_colors.bg))
        } else {
            (span.fg.map_or(text, Hsla::from), span.bg.map(Hsla::from))
        };
        runs.push(TextRun {
            len: span.text.len(),
            font: run_font,
            color,
            background_color: background,
            underline: span.underline.then_some(UnderlineStyle {
                thickness: px(1.),
                color: None,
                wavy: false,
            }),
            strikethrough: None,
        });
    }
    if content.is_empty() {
        // An empty line has no height; a space keeps the row.
        content.push(' ');
        runs.push(TextRun {
            len: 1,
            font: font(family.clone()),
            color: text,
            background_color: None,
            underline: None,
            strikethrough: None,
        });
    }
    StyledText::new(content).with_runs(runs)
}

impl RenderOnce for TextGrid {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let family = theme.font_mono.clone();
        let size = theme.text_body;
        let line_height = px((f32::from(size) * LINE_HEIGHT).round());
        let text = theme.colors.text;
        let cursor_colors = CursorColors {
            fg: theme.colors.background,
            bg: theme.colors.accent,
        };
        let measure = self.fit.map(|Fit { current, on_fit }| {
            let family = family.clone();
            canvas(
                move |bounds, window, cx| {
                    let text_system = window.text_system();
                    let font_id = text_system.resolve_font(&font(family));
                    let Ok(advance) = text_system.advance(font_id, size, 'm') else {
                        return;
                    };
                    let cell = gpui::size(advance.width, line_height);
                    let fits = cells_in(bounds, cell);
                    if fits != current {
                        cx.defer(move |cx| on_fit.dispatch(fits, cx));
                    }
                },
                |_, (), _, _| {},
            )
            .absolute()
            .size_full()
        });
        div()
            .relative()
            .flex()
            .flex_col()
            .when(measure.is_some(), |d| d.size_full().overflow_hidden())
            .font_family(family.clone())
            .text_size(size)
            .line_height(line_height)
            .children(measure)
            .children(self.lines.iter().enumerate().map(|(row, spans)| {
                let col = self.cursor.and_then(|(r, c)| (r == row).then_some(c));
                div().h(line_height).whitespace_nowrap().child(render_line(
                    spans,
                    col,
                    &family,
                    text,
                    cursor_colors,
                ))
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(text: &str) -> Span {
        Span {
            text: text.into(),
            ..Span::default()
        }
    }

    fn texts(spans: &[Span]) -> Vec<&str> {
        spans.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn the_cursor_cell_becomes_a_span_of_its_own() {
        let bold = Span {
            bold: true,
            ..span("cd")
        };
        let (spans, index) = isolate(&[span("ab"), bold.clone(), span("ef")], 3);

        assert_eq!(texts(&spans), ["ab", "c", "d", "ef"]);
        assert_eq!(index, 2);
        assert!(spans[1].bold && spans[2].bold);
    }

    #[test]
    fn a_cursor_at_the_start_or_end_of_a_span_splits_once() {
        let (spans, index) = isolate(&[span("ab"), span("cd")], 2);
        assert_eq!((texts(&spans), index), (vec!["ab", "c", "d"], 1));

        let (spans, index) = isolate(&[span("ab")], 1);
        assert_eq!((texts(&spans), index), (vec!["a", "b"], 1));
    }

    #[test]
    fn a_cursor_past_the_end_pads_the_line() {
        let (spans, index) = isolate(&[span("ab")], 4);
        assert_eq!((texts(&spans), index), (vec!["ab", "  ", " "], 2));

        let (spans, index) = isolate(&[span("ab")], 2);
        assert_eq!((texts(&spans), index), (vec!["ab", " "], 1));

        let (spans, index) = isolate(&[], 0);
        assert_eq!((texts(&spans), index), (vec![" "], 0));
    }

    #[test]
    fn characters_count_as_one_column_each() {
        let (spans, index) = isolate(&[span("日本語")], 1);
        assert_eq!((texts(&spans), index), (vec!["日", "本", "語"], 1));
    }

    #[test]
    fn the_cells_that_fit_round_down_and_never_reach_zero() {
        let bounds = |w: f32, h: f32| Bounds {
            origin: gpui::point(px(0.), px(0.)),
            size: gpui::size(px(w), px(h)),
        };
        let cell = gpui::size(px(8.), px(18.));

        assert_eq!(cells_in(bounds(800., 540.), cell), (100, 30));
        assert_eq!(cells_in(bounds(807.9, 557.), cell), (100, 30));
        assert_eq!(cells_in(bounds(3., 0.), cell), (1, 1));
        assert_eq!(
            cells_in(bounds(800., 540.), gpui::size(px(0.), px(0.))),
            (800, 540)
        );
    }

    #[test]
    fn rgb_converts_to_the_same_color() {
        let color: Hsla = Rgb { r: 255, g: 0, b: 0 }.into();
        assert_eq!(
            Rgba::from(color),
            Rgba {
                r: 1.,
                g: 0.,
                b: 0.,
                a: 1.
            }
        );
    }
}
