use std::ops::Range;

use gpui::{
    App, Bounds, ElementId, FontStyle, FontWeight, Hsla, InteractiveElement, IntoElement,
    MouseButton, MouseDownEvent, MouseMoveEvent, ParentElement, Pixels, Point, RenderOnce, Rgba,
    ScrollDelta, ScrollWheelEvent, SharedString, Styled, StyledText, TextLayout, TextRun,
    UnderlineStyle, Window, canvas, div, font, prelude::FluentBuilder, px,
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
    scroll: Option<(ElementId, Dispatcher<i32>)>,
    selection: Option<(GridPoint, GridPoint)>,
    on_pointer: Option<Dispatcher<GridPointer>>,
}

/// Columns and rows of a grid.
pub type GridSize = (u16, u16);

/// A place between characters: before the character at `col` on `row`,
/// both zero-based.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct GridPoint {
    pub row: usize,
    pub col: usize,
}

/// The left mouse button over the grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridPointer {
    /// Pressed here.
    Press(GridPoint),
    /// Moved here while held.
    Drag(GridPoint),
}

struct Fit {
    current: GridSize,
    on_fit: Dispatcher<GridSize>,
}

/// Line height as a multiple of the font size.
const LINE_HEIGHT: f32 = 1.3;

/// The whole lines a wheel event scrolls, up when negative, and the
/// fraction left to carry to the next one.
fn wheel_lines(delta: ScrollDelta, line_height: Pixels, carry: f32) -> (i32, f32) {
    // GPUI's deltas are positive when the content should move down, which
    // is scrolling up.
    let lines = match delta {
        ScrollDelta::Lines(lines) => -lines.y,
        ScrollDelta::Pixels(pixels) => -f32::from(pixels.y) / f32::from(line_height).max(1.),
    };
    let total = carry + lines;
    let whole = total.trunc();
    // Saturates; a wheel never scrolls that far.
    (whole as i32, total - whole)
}

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
            scroll: None,
            selection: None,
            on_pointer: None,
        }
    }

    /// Highlights the characters between `from` and `to`, in either order.
    pub fn selection(mut self, from: GridPoint, to: GridPoint) -> Self {
        self.selection = Some((from.min(to), from.max(to)));
        self
    }

    /// Dispatches where the left mouse button is pressed and dragged, at
    /// the nearest place between characters. A wide character counts as
    /// one, as it does for the cursor.
    pub fn on_pointer(mut self, on_pointer: Dispatcher<GridPointer>) -> Self {
        self.on_pointer = Some(on_pointer);
        self
    }

    /// Dispatches whole lines scrolled with the wheel or touchpad, up when
    /// negative. Fractions of a line carry over to the next event, which
    /// `id` keys.
    pub fn on_scroll(mut self, id: impl Into<ElementId>, on_scroll: Dispatcher<i32>) -> Self {
        self.scroll = Some((id.into(), on_scroll));
        self
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

/// How a piece of a line is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mark {
    Plain,
    Selected,
    Cursor,
}

/// Splits `line` into pieces that each draw one way: the cursor cell at
/// `cursor`, the characters in `selected`, and the rest. A cursor past the
/// end pads the line with spaces.
fn mark(line: &[Span], cursor: Option<usize>, selected: Range<usize>) -> Vec<(Span, Mark)> {
    let mark_of = |col: usize| {
        if cursor == Some(col) {
            Mark::Cursor
        } else if selected.contains(&col) {
            Mark::Selected
        } else {
            Mark::Plain
        }
    };
    let len: usize = line.iter().map(|span| span.text.chars().count()).sum();
    let pad = cursor.filter(|&col| col >= len).map(|col| Span {
        text: " ".repeat(col + 1 - len),
        ..Span::default()
    });
    let mut out = Vec::with_capacity(line.len() + 2);
    let mut col = 0;
    for span in line.iter().chain(pad.as_ref()) {
        let mut piece = String::new();
        let mut piece_mark = mark_of(col);
        for ch in span.text.chars() {
            let here = mark_of(col);
            if here != piece_mark && !piece.is_empty() {
                let text = std::mem::take(&mut piece);
                out.push((
                    Span {
                        text,
                        ..span.clone()
                    },
                    piece_mark,
                ));
            }
            piece_mark = here;
            piece.push(ch);
            col += 1;
        }
        if !piece.is_empty() {
            out.push((
                Span {
                    text: piece,
                    ..span.clone()
                },
                piece_mark,
            ));
        }
    }
    out
}

/// The columns of `row` that `selection` covers.
fn selected_in(selection: Option<(GridPoint, GridPoint)>, row: usize) -> Range<usize> {
    match selection {
        Some((from, to)) if (from.row..=to.row).contains(&row) => {
            let start = if row == from.row { from.col } else { 0 };
            let end = if row == to.row { to.col } else { usize::MAX };
            start..end
        }
        _ => 0..0,
    }
}

/// The colors each mark draws with, besides the span's own.
#[derive(Clone, Copy)]
struct Palette {
    text: Hsla,
    cursor_fg: Hsla,
    cursor_bg: Hsla,
    selection: Hsla,
}

/// The place between characters nearest to `position`, in a line laid
/// out from `content`.
fn boundary(layout: &TextLayout, content: &str, position: Point<Pixels>) -> usize {
    let index = layout
        .index_for_position(position)
        .unwrap_or_else(|nearest| nearest)
        .min(content.len());
    let next = content[index..]
        .chars()
        .next()
        .map_or(index, |ch| index + ch.len_utf8());
    let past_middle = match (
        layout.position_for_index(index),
        layout.position_for_index(next),
    ) {
        (Some(start), Some(end)) => position.x > (start.x + end.x) / 2.,
        _ => false,
    };
    let at = if past_middle { next } else { index };
    content[..at].chars().count()
}

fn render_line(
    pieces: &[(Span, Mark)],
    family: &SharedString,
    palette: Palette,
) -> (SharedString, Vec<TextRun>) {
    let mut content = String::new();
    let mut runs = Vec::with_capacity(pieces.len());
    for (span, mark) in pieces {
        content.push_str(&span.text);
        let mut run_font = font(family.clone());
        if span.bold {
            run_font.weight = FontWeight::BOLD;
        }
        if span.italic {
            run_font.style = FontStyle::Italic;
        }
        let fg = span.fg.map_or(palette.text, Hsla::from);
        let (color, background) = match mark {
            Mark::Cursor => (palette.cursor_fg, Some(palette.cursor_bg)),
            Mark::Selected => (fg, Some(palette.selection)),
            Mark::Plain => (fg, span.bg.map(Hsla::from)),
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
            color: palette.text,
            background_color: None,
            underline: None,
            strikethrough: None,
        });
    }
    (content.into(), runs)
}

impl RenderOnce for TextGrid {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let scroll = self.scroll.map(|(id, on_scroll)| {
            let carry = window.use_keyed_state(id, cx, |_, _| 0f32);
            (on_scroll, carry)
        });
        let theme = Theme::of(cx);
        let family = theme.font_mono.clone();
        let size = theme.text_body;
        let line_height = px((f32::from(size) * LINE_HEIGHT).round());
        let palette = Palette {
            text: theme.colors.text,
            cursor_fg: theme.colors.background,
            cursor_bg: theme.colors.accent,
            selection: theme.colors.selection,
        };
        let selection = self.selection;
        let on_pointer = self.on_pointer;
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
            .when_some(scroll, |d, (on_scroll, carry)| {
                d.on_scroll_wheel(move |event: &ScrollWheelEvent, _, cx| {
                    let (lines, rest) = wheel_lines(event.delta, line_height, *carry.read(cx));
                    carry.update(cx, |carry, _| *carry = rest);
                    if lines != 0 {
                        on_scroll.dispatch(lines, cx);
                    }
                })
            })
            .children(self.lines.iter().enumerate().map(|(row, spans)| {
                let col = self.cursor.and_then(|(r, c)| (r == row).then_some(c));
                let pieces = mark(spans, col, selected_in(selection, row));
                let (content, runs) = render_line(&pieces, &family, palette);
                let text = StyledText::new(content.clone()).with_runs(runs);
                let layout = text.layout().clone();
                div()
                    .h(line_height)
                    .whitespace_nowrap()
                    .when_some(on_pointer.clone(), |d, on_pointer| {
                        let point = move |position| GridPoint {
                            row,
                            col: boundary(&layout, &content, position),
                        };
                        let on_press = on_pointer.clone();
                        let press_at = point.clone();
                        d.on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, _, cx| {
                            on_press.dispatch(GridPointer::Press(press_at(event.position)), cx);
                        })
                        .on_mouse_move(
                            move |event: &MouseMoveEvent, _, cx| {
                                if event.pressed_button == Some(MouseButton::Left) {
                                    on_pointer
                                        .dispatch(GridPointer::Drag(point(event.position)), cx);
                                }
                            },
                        )
                    })
                    .child(text)
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

    fn texts(pieces: &[(Span, Mark)]) -> Vec<&str> {
        pieces.iter().map(|(s, _)| s.text.as_str()).collect()
    }

    /// The cursor cell becomes a piece of its own; returns the pieces and
    /// the cursor's index among them.
    fn isolate(line: &[Span], col: usize) -> (Vec<(Span, Mark)>, usize) {
        let pieces = mark(line, Some(col), 0..0);
        let index = pieces.iter().position(|(_, m)| *m == Mark::Cursor).unwrap();
        (pieces, index)
    }

    #[test]
    fn the_cursor_cell_becomes_a_span_of_its_own() {
        let bold = Span {
            bold: true,
            ..span("cd")
        };
        let (pieces, index) = isolate(&[span("ab"), bold.clone(), span("ef")], 3);

        assert_eq!(texts(&pieces), ["ab", "c", "d", "ef"]);
        assert_eq!(index, 2);
        assert!(pieces[1].0.bold && pieces[2].0.bold);
    }

    #[test]
    fn a_cursor_at_the_start_or_end_of_a_span_splits_once() {
        let (pieces, index) = isolate(&[span("ab"), span("cd")], 2);
        assert_eq!((texts(&pieces), index), (vec!["ab", "c", "d"], 1));

        let (pieces, index) = isolate(&[span("ab")], 1);
        assert_eq!((texts(&pieces), index), (vec!["a", "b"], 1));
    }

    #[test]
    fn a_cursor_past_the_end_pads_the_line() {
        let (pieces, index) = isolate(&[span("ab")], 4);
        assert_eq!((texts(&pieces), index), (vec!["ab", "  ", " "], 2));

        let (pieces, index) = isolate(&[span("ab")], 2);
        assert_eq!((texts(&pieces), index), (vec!["ab", " "], 1));

        let (pieces, index) = isolate(&[], 0);
        assert_eq!((texts(&pieces), index), (vec![" "], 0));
    }

    #[test]
    fn characters_count_as_one_column_each() {
        let (pieces, index) = isolate(&[span("日本語")], 1);
        assert_eq!((texts(&pieces), index), (vec!["日", "本", "語"], 1));
    }

    #[test]
    fn a_selection_splits_spans_and_gives_way_to_the_cursor() {
        let marks = |pieces: &[(Span, Mark)]| pieces.iter().map(|(_, m)| *m).collect::<Vec<_>>();

        let pieces = mark(&[span("abc"), span("def")], None, 1..4);
        assert_eq!(texts(&pieces), ["a", "bc", "d", "ef"]);
        assert_eq!(
            marks(&pieces),
            [Mark::Plain, Mark::Selected, Mark::Selected, Mark::Plain]
        );

        let pieces = mark(&[span("abcd")], Some(2), 0..usize::MAX);
        assert_eq!(texts(&pieces), ["ab", "c", "d"]);
        assert_eq!(
            marks(&pieces),
            [Mark::Selected, Mark::Cursor, Mark::Selected]
        );

        let pieces = mark(&[span("ab")], None, 0..0);
        assert_eq!(marks(&pieces), [Mark::Plain]);
    }

    #[test]
    fn a_selection_covers_whole_rows_between_its_ends() {
        let at = |row, col| GridPoint { row, col };
        let selection = Some((at(1, 3), at(3, 2)));

        assert_eq!(selected_in(selection, 0), 0..0);
        assert_eq!(selected_in(selection, 1), 3..usize::MAX);
        assert_eq!(selected_in(selection, 2), 0..usize::MAX);
        assert_eq!(selected_in(selection, 3), 0..2);
        assert_eq!(selected_in(selection, 4), 0..0);
        assert_eq!(selected_in(Some((at(2, 1), at(2, 4))), 2), 1..4);
        assert_eq!(selected_in(None, 0), 0..0);
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
    fn wheel_lines_scroll_up_for_positive_deltas_and_carry_fractions() {
        let line = px(20.);
        let lines = |y: f32| ScrollDelta::Lines(gpui::point(0., y));
        let pixels = |y: f32| ScrollDelta::Pixels(gpui::point(px(0.), px(y)));

        assert_eq!(wheel_lines(lines(3.), line, 0.), (-3, 0.));
        assert_eq!(wheel_lines(lines(-1.), line, 0.), (1, 0.));
        assert_eq!(wheel_lines(pixels(-50.), line, 0.), (2, 0.5));
        assert_eq!(wheel_lines(pixels(-10.), line, 0.5), (1, 0.));
        assert_eq!(wheel_lines(pixels(5.), line, 0.), (0, -0.25));
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
