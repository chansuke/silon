use gpui::{App, ElementId, IntoElement, ParentElement, Window};
use silon_gpui::{Dispatcher, FeatureView};
use silon_ui::{
    Align, Badge, Button, ButtonVariant, GridPoint, GridPointer, GridSize, KeyCapture, KeyPress,
    Panel, Rgb, Row, Space, Span, Stack, Text, TextGrid, TextSize, Tone,
};
use terminal_core::{Key, Point, Screen, Session, Size, TermAction, TermState, Terminal};

pub struct TermView;

impl FeatureView<Terminal> for TermView {
    fn view(
        state: &TermState,
        dispatcher: Dispatcher<TermAction>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> impl IntoElement + use<> {
        Row::screen()
            .padding(Space::Md)
            .gap(Space::Md)
            .align(Align::Stretch)
            .child(sidebar(state, &dispatcher))
            .child(screen(state, &dispatcher))
    }
}

fn sidebar(state: &TermState, dispatcher: &Dispatcher<TermAction>) -> Panel {
    Panel::new()
        .gap(Space::Sm)
        .child(
            Button::new("new", "New session")
                .variant(ButtonVariant::Primary)
                .on_press(dispatcher, TermAction::NewSession),
        )
        .children(
            state
                .sessions
                .iter()
                .map(|session| tab(session, state.active == Some(session.id), dispatcher)),
        )
}

fn tab(session: &Session, active: bool, dispatcher: &Dispatcher<TermAction>) -> Stack {
    let id = u64::from(session.id);
    let place = session.cwd.as_deref().map(|cwd| {
        cwd.rsplit('/')
            .find(|part| !part.is_empty())
            .unwrap_or("/")
            .to_owned()
    });
    Stack::new()
        .gap(Space::Xs)
        .child(
            Row::new()
                .gap(Space::Xs)
                .child(
                    Button::new(
                        ElementId::NamedInteger("tab".into(), id),
                        session.title.clone(),
                    )
                    .variant(if active {
                        ButtonVariant::Secondary
                    } else {
                        ButtonVariant::Ghost
                    })
                    .on_press(dispatcher, TermAction::Activate(session.id)),
                )
                .children(
                    (session.bells > 0)
                        .then(|| Badge::new(format!("{} bell", session.bells)).tone(Tone::Accent)),
                )
                .child(
                    Button::new(ElementId::NamedInteger("close".into(), id), "×")
                        .variant(ButtonVariant::Ghost)
                        .on_press(dispatcher, TermAction::Close(session.id)),
                ),
        )
        .children(place.map(|place| Text::new(place).size(TextSize::Caption).tone(Tone::Muted)))
}

fn content_color(terminal_core::Rgb(r, g, b): terminal_core::Rgb) -> Rgb {
    Rgb { r, g, b }
}

fn lines(screen: &Screen) -> Vec<Vec<Span>> {
    screen
        .lines
        .iter()
        .map(|spans| {
            spans
                .iter()
                .map(|span| Span {
                    text: span.text.clone(),
                    fg: span.fg.map(content_color),
                    bg: span.bg.map(content_color),
                    bold: span.bold,
                    italic: span.italic,
                    underline: span.underline,
                })
                .collect()
        })
        .collect()
}

fn key(press: KeyPress) -> TermAction {
    TermAction::Key(Key {
        key: press.key,
        text: press.text,
        ctrl: press.ctrl,
        alt: press.alt,
        shift: press.shift,
    })
}

fn grid_point(Point { row, col }: Point) -> GridPoint {
    GridPoint {
        row: usize::from(row),
        col: usize::from(col),
    }
}

fn screen_point(GridPoint { row, col }: GridPoint) -> Point {
    let clamp = |n: usize| u16::try_from(n).unwrap_or(u16::MAX);
    Point {
        row: clamp(row),
        col: clamp(col),
    }
}

fn pointer(pointer: GridPointer) -> TermAction {
    match pointer {
        GridPointer::Press(at) => TermAction::SelectFrom(screen_point(at)),
        GridPointer::Drag(at) => TermAction::SelectTo(screen_point(at)),
    }
}

fn screen(state: &TermState, dispatcher: &Dispatcher<TermAction>) -> KeyCapture {
    let mut grid = TextGrid::new(
        state
            .active_session()
            .map(|session| lines(&session.screen))
            .unwrap_or_default(),
    )
    .fit(
        (state.size.cols, state.size.rows),
        dispatcher.map(|(cols, rows): GridSize| TermAction::Resized(Size { cols, rows })),
    )
    .on_scroll("screen-scroll", dispatcher.map(TermAction::Scroll))
    .on_pointer(dispatcher.map(pointer));
    if let Some((row, col)) = state.active_session().and_then(|s| s.screen.cursor) {
        grid = grid.cursor(usize::from(row), usize::from(col));
    }
    if let Some(selection) = state.active_session().and_then(|s| s.selection) {
        grid = grid.selection(grid_point(selection.anchor), grid_point(selection.head));
    }
    let capture = KeyCapture::new("screen")
        .grow()
        .autofocus()
        .on_key(dispatcher.map(key))
        .on_text(dispatcher.map(TermAction::Text))
        .on_paste(dispatcher.map(TermAction::Paste))
        .on_copy(dispatcher.map(|()| TermAction::Copy));
    if state.sessions.is_empty() {
        return capture
            .child(Text::new("No sessions. Start one with New session.").tone(Tone::Muted));
    }
    let scrolled_back = state
        .active_session()
        .is_some_and(|session| session.screen.scrolled_back);
    capture
        .children(scrolled_back.then(|| {
            Text::new("Showing history. Type or press Shift+PageDown to return.")
                .size(TextSize::Caption)
                .tone(Tone::Accent)
        }))
        .child(grid)
}
