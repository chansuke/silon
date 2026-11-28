use gpui::{App, ElementId, IntoElement, ParentElement, SharedString, Window};
use herdboard_core::{
    HerdAction, HerdState, Herdboard, Key, Link, Notice, Screen, Status, Terminal,
};
use silon_gpui::{Dispatcher, FeatureView};
use silon_ui::components::TextInput;
use silon_ui::{
    Align, Badge, Button, ButtonVariant, KeyCapture, KeyPress, Panel, Row, Space, Stack, Text,
    TextSize, Tone,
};

pub struct HerdView;

impl FeatureView<Herdboard> for HerdView {
    fn view(
        state: &HerdState,
        dispatcher: Dispatcher<HerdAction>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> impl IntoElement + use<> {
        Stack::screen()
            .padding(Space::Xl)
            .gap(Space::Lg)
            .child(header(state))
            .child(
                Row::new()
                    .gap(Space::Lg)
                    .align(Align::Start)
                    .child(list(state, &dispatcher))
                    .child(Stack::new().grow().child(detail(state, &dispatcher))),
            )
    }
}

fn header(state: &HerdState) -> Row {
    let (link, tone) = match &state.link {
        Link::Connecting => ("connecting".to_owned(), Tone::Muted),
        Link::Live => ("live".to_owned(), Tone::Success),
        Link::Waiting { error, delay_ms } => (
            format!("{error}; retrying in {} s", delay_ms.div_ceil(1000)),
            Tone::Danger,
        ),
    };
    Row::new()
        .gap(Space::Md)
        .child(Text::new("herdr").size(TextSize::Title))
        .child(Badge::new(link).tone(tone))
        .child(
            Text::new(state.socket.clone())
                .size(TextSize::Caption)
                .tone(Tone::Muted),
        )
}

fn status_badge(status: Option<&Status>) -> Option<Badge> {
    let status = status?;
    let tone = match status.state.as_str() {
        "working" => Tone::Accent,
        "blocked" | "error" => Tone::Danger,
        "done" => Tone::Success,
        _ => Tone::Neutral,
    };
    Some(Badge::new(status.state.clone()).tone(tone))
}

fn label(terminal: &Terminal) -> SharedString {
    match &terminal.name {
        Some(name) => format!("{} {name}", terminal.address).into(),
        None => terminal.address.clone().into(),
    }
}

fn list(state: &HerdState, dispatcher: &Dispatcher<HerdAction>) -> Panel {
    let panel = Panel::new().gap(Space::Sm);
    if state.terminals.is_empty() {
        return panel.child(Text::new("No terminals.").tone(Tone::Muted));
    }
    panel.children(state.terminals.iter().enumerate().map(|(i, terminal)| {
        let selected = state.selected.as_deref() == Some(terminal.address.as_str());
        Row::new()
            .gap(Space::Sm)
            .child(
                Button::new(
                    ElementId::NamedInteger("select".into(), i as u64),
                    label(terminal),
                )
                .variant(if selected {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Ghost
                })
                .on_press(dispatcher, HerdAction::Select(terminal.address.clone())),
            )
            .children(status_badge(terminal.status.as_ref()))
            .children((terminal.state == "finished").then(|| Badge::new("finished")))
    }))
}

fn detail(state: &HerdState, dispatcher: &Dispatcher<HerdAction>) -> Panel {
    let Some(terminal) = state.selected_terminal() else {
        return Panel::new().child(Text::new("Select a terminal.").tone(Tone::Muted));
    };
    let caption = |text: String| Text::new(text).size(TextSize::Caption).tone(Tone::Muted);
    let running = match (&terminal.foreground, &terminal.reason) {
        (Some(fg), _) => format!("{} in {}", fg.name, fg.cwd),
        (None, Some(reason)) => match terminal.exit_code {
            Some(code) => format!("{reason}, exit code {code}"),
            None => reason.clone(),
        },
        (None, None) => terminal.argv.join(" "),
    };
    let status = terminal
        .status
        .as_ref()
        .map(|s| {
            if s.msg.is_empty() {
                s.state.clone()
            } else {
                format!("{}: {}", s.state, s.msg)
            }
        })
        .unwrap_or_else(|| "no status reported".into());

    Panel::new()
        .child(Text::new(label(terminal)).size(TextSize::Title))
        .children((!terminal.title.is_empty()).then(|| Text::new(terminal.title.clone())))
        .child(caption(running))
        .child(caption(format!("{status}, turn {}", terminal.turn)))
        .child(
            Row::new()
                .gap(Space::Sm)
                .child(
                    Stack::new().grow().child(
                        TextInput::new("draft", state.draft.clone())
                            .placeholder("Text to send, then Enter")
                            .on_change(dispatcher.map(HerdAction::DraftChanged))
                            .on_submit(dispatcher.map(|_: String| HerdAction::Send)),
                    ),
                )
                .child(
                    Button::new("send", "Send")
                        .variant(ButtonVariant::Primary)
                        .disabled(!state.can_send())
                        .on_press(dispatcher, HerdAction::Send),
                ),
        )
        .children(
            state
                .notice
                .as_ref()
                .map(|notice| caption(describe(notice))),
        )
        .child(screen(state, dispatcher))
}

/// The attached screen, which takes the keyboard when clicked.
fn screen(state: &HerdState, dispatcher: &Dispatcher<HerdAction>) -> Stack {
    let caption = |text: String| Text::new(text).size(TextSize::Caption).tone(Tone::Muted);
    if let Some(reason) = &state.attachment.ended {
        return Stack::new()
            .gap(Space::Sm)
            .child(caption(format!("detached: {reason}")))
            .child(
                Button::new("reattach", "Attach again").on_press(dispatcher, HerdAction::Reattach),
            );
    }
    let attached = state.attach_stream().is_some();
    let Some(screen) = &state.attachment.screen else {
        let waiting = attached.then(|| caption("attaching".into()));
        return Stack::new().children(waiting);
    };
    if !attached {
        // A finished terminal keeps its last screen, without the keyboard.
        return Stack::new().children(lines(screen));
    }
    Stack::new().child(
        KeyCapture::new("screen")
            .on_key(dispatcher.map(|press: KeyPress| {
                HerdAction::Key(Key {
                    key: press.key,
                    text: press.text,
                    ctrl: press.ctrl,
                    alt: press.alt,
                })
            }))
            .children(lines(screen)),
    )
}

fn line(text: impl Into<SharedString>) -> Text {
    Text::new(text).mono().size(TextSize::Caption)
}

/// One row of text per screen row, with a block where the cursor is.
fn lines(screen: &Screen) -> Vec<Row> {
    let (cursor_row, cursor_col) = screen.cursor;
    screen
        .lines
        .iter()
        .enumerate()
        .map(|(row, text)| {
            if row != usize::from(cursor_row) {
                // An empty text has no height; a space keeps the row.
                let text = if text.is_empty() { " " } else { text };
                return Row::new().child(line(text.to_owned()));
            }
            let mut chars: Vec<char> = text.chars().collect();
            let col = usize::from(cursor_col);
            if chars.len() <= col {
                chars.resize(col + 1, ' ');
            }
            let before: String = chars[..col].iter().collect();
            let after: String = chars[col + 1..].iter().collect();
            Row::new()
                .child(line(before))
                .child(line("\u{2588}").tone(Tone::Accent))
                .child(line(after))
        })
        .collect()
}

fn describe(notice: &Notice) -> String {
    match notice {
        Notice::Sending { terminal } => format!("sending to {terminal}"),
        Notice::Sent { terminal, turn } => format!("sent to {terminal} at turn {turn}"),
        Notice::Failed { terminal, error } => format!("could not send to {terminal}: {error}"),
    }
}
