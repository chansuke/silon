//! Pure terminal workspace: sessions, the active one, and what each shows.
//! No GPUI, no PTY, no VT parser. The app runs each session's shell and
//! VT emulator as a subscription and reports screens back as actions.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use silon_core::{Command, Feature};

/// A color the running program chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// A run of cells with one style. Colors left out are the defaults.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Span {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fg: Option<Rgb>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg: Option<Rgb>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub underline: bool,
}

/// What a session shows. Columns count characters, so a wide character
/// takes one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Screen {
    pub lines: Vec<Vec<Span>>,
    /// Row and column of the cursor, when it is shown.
    pub cursor: Option<(u16, u16)>,
    /// Whether the view shows history instead of the latest output.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub scrolled_back: bool,
}

impl Screen {
    /// The text of each line, without styles.
    pub fn text(&self) -> Vec<String> {
        self.lines
            .iter()
            .map(|spans| spans.iter().map(|s| s.text.as_str()).collect())
            .collect()
    }

    /// The text between `from` and `to`, one line per row with trailing
    /// blanks dropped, as a terminal copies it.
    pub fn text_between(&self, from: Point, to: Point) -> String {
        let lines = self.text();
        let last = usize::from(to.row).min(lines.len().saturating_sub(1));
        let rows = lines
            .iter()
            .enumerate()
            .take(last + 1)
            .skip(usize::from(from.row));
        rows.map(|(row, line)| {
            let start = if row == usize::from(from.row) {
                usize::from(from.col)
            } else {
                0
            };
            let end = if row == usize::from(to.row) {
                usize::from(to.col)
            } else {
                usize::MAX
            };
            let text: String = line.chars().take(end).skip(start).collect();
            text.trim_end().to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
    }
}

/// A place between characters on the screen: before the character at
/// `col` on `row`.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct Point {
    pub row: u16,
    pub col: u16,
}

/// Text marked with the mouse, from where the button went down to where
/// it is now. Rows count on the screen as it shows now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Selection {
    pub anchor: Point,
    pub head: Point,
}

impl Selection {
    /// The two ends in reading order.
    pub fn ordered(self) -> (Point, Point) {
        (self.anchor.min(self.head), self.anchor.max(self.head))
    }
}

/// Columns and rows of the terminal grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
}

impl Default for Size {
    fn default() -> Self {
        Self {
            cols: 100,
            rows: 30,
        }
    }
}

/// One key press, as plain data. The session encodes it for the program,
/// which depends on the modes the program set.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Key {
    /// The key's name: `a`, `enter`, `up`, `f1`, ...
    pub key: String,
    /// The text the press types, with Shift and the layout applied.
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub shift: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: u32,
    /// What the program called the session (OSC 0/2), or its shell.
    pub title: String,
    /// The working directory the shell reported (OSC 7), if any.
    pub cwd: Option<String>,
    pub screen: Screen,
    /// Bells rung since the session was last active.
    pub bells: u32,
    #[serde(default)]
    pub selection: Option<Selection>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TermState {
    /// The program each new session runs.
    pub shell: Vec<String>,
    pub size: Size,
    pub sessions: Vec<Session>,
    pub active: Option<u32>,
    pub next_id: u32,
}

impl TermState {
    pub fn new(shell: Vec<String>) -> Self {
        Self {
            shell,
            size: Size::default(),
            sessions: Vec::new(),
            active: None,
            next_id: 0,
        }
    }

    pub fn session(&self, id: u32) -> Option<&Session> {
        self.sessions.iter().find(|s| s.id == id)
    }

    fn session_mut(&mut self, id: u32) -> Option<&mut Session> {
        self.sessions.iter_mut().find(|s| s.id == id)
    }

    pub fn active_session(&self) -> Option<&Session> {
        self.active.and_then(|id| self.session(id))
    }

    fn pty(&self, id: u32) -> Pty {
        Pty {
            id,
            shell: self.shell.clone(),
        }
    }

    fn type_into(&self, id: u32, input: Input) -> TermCommand {
        Command::perform(TermEffect::Input {
            to: self.pty(id),
            input,
        })
    }

    /// Sends `input` to the active session, dropping its selection: the
    /// screen it was made on is about to change.
    fn type_into_active(&mut self, input: Input) -> TermCommand {
        match self.active {
            Some(id) => {
                self.select(None);
                self.type_into(id, input)
            }
            None => Command::none(),
        }
    }

    fn active_mut(&mut self) -> Option<&mut Session> {
        let id = self.active?;
        self.session_mut(id)
    }

    fn select(&mut self, selection: Option<Selection>) {
        if let Some(session) = self.active_mut() {
            session.selection = selection;
        }
    }

    /// The text the active session has selected, if any.
    fn selected_text(&self) -> Option<String> {
        let session = self.active_session()?;
        let (from, to) = session.selection?.ordered();
        let text = session.screen.text_between(from, to);
        (!text.is_empty()).then_some(text)
    }

    fn activate(&mut self, id: u32) {
        if let Some(session) = self.session_mut(id) {
            session.bells = 0;
            self.active = Some(id);
        }
    }

    /// Removes session `id`, moving to its neighbour if it was active.
    fn remove(&mut self, id: u32) {
        let Some(index) = self.sessions.iter().position(|s| s.id == id) else {
            return;
        };
        self.sessions.remove(index);
        if self.active == Some(id) {
            self.active = None;
            let neighbour = index.min(self.sessions.len().saturating_sub(1));
            if let Some(next) = self.sessions.get(neighbour).map(|s| s.id) {
                self.activate(next);
            }
        }
    }
}

type TermCommand = Command<TermEffect>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum TermAction {
    /// Opens a session running the shell and makes it active.
    NewSession,
    /// Ends session `id` and its program.
    Close(u32),
    Activate(u32),
    /// The window fits `size` now; every session follows.
    Resized(Size),
    /// A key pressed in the active session.
    Key(Key),
    /// Text typed into the active session, as an input method committed it.
    Text(String),
    /// Text pasted into the active session.
    Paste(String),
    /// Scrolls the active session by this many lines, up when negative.
    Scroll(i32),
    /// The mouse went down here in the active session, starting a selection.
    SelectFrom(Point),
    /// The mouse moved here while held, ending the selection here.
    SelectTo(Point),
    /// Copies the active session's selection to the clipboard.
    Copy,
    /// Session `id` shows `screen`.
    Painted {
        id: u32,
        screen: Screen,
    },
    Titled {
        id: u32,
        title: String,
    },
    MovedTo {
        id: u32,
        cwd: String,
    },
    Rang {
        id: u32,
    },
    /// The program of session `id` ended.
    Exited {
        id: u32,
        code: Option<i32>,
    },
}

/// What the app sends into a running session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    Key(Key),
    /// Typed text, sent as it is.
    Text(String),
    Paste(String),
    Resize(Size),
    /// Lines to scroll, up when negative: through history, or as arrow
    /// keys when a full-screen program is running.
    Scroll(i32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermEffect {
    Input {
        to: Pty,
        input: Input,
    },
    /// Puts this text on the clipboard.
    Copy(String),
}

/// The program of one session, in its PTY, behind its VT emulator.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Pty {
    pub id: u32,
    pub shell: Vec<String>,
}

pub struct Terminal;

impl Feature for Terminal {
    type State = TermState;
    type Action = TermAction;
    type Effect = TermEffect;
    type Subscription = Pty;

    fn update(state: &mut TermState, action: TermAction) -> TermCommand {
        match action {
            TermAction::NewSession => {
                let Some(next_id) = state.next_id.checked_add(1) else {
                    return Command::none();
                };
                let id = state.next_id;
                state.next_id = next_id;
                state.sessions.push(Session {
                    id,
                    title: state.shell.first().cloned().unwrap_or_default(),
                    cwd: None,
                    screen: Screen::default(),
                    bells: 0,
                    selection: None,
                });
                state.activate(id);
                // The store opens the session's stream before it runs this,
                // so the size reaches the new session.
                return state.type_into(id, Input::Resize(state.size));
            }
            TermAction::Close(id) => state.remove(id),
            TermAction::Activate(id) => state.activate(id),
            TermAction::Resized(size) => {
                if size != state.size && size.cols > 0 && size.rows > 0 {
                    state.size = size;
                    for session in &mut state.sessions {
                        session.selection = None;
                    }
                    let ids: Vec<u32> = state.sessions.iter().map(|s| s.id).collect();
                    return Command::batch(
                        ids.into_iter()
                            .map(|id| state.type_into(id, Input::Resize(size))),
                    );
                }
            }
            TermAction::Key(key) => {
                // Shift with Page Up or Down pages through history, as in
                // most terminals; the program does not see it.
                let page = i32::from(state.size.rows.saturating_sub(1).max(1));
                let paged = match (key.shift, key.key.as_str()) {
                    (true, "pageup") => Some(-page),
                    (true, "pagedown") => Some(page),
                    _ => None,
                };
                return match paged {
                    Some(lines) => state.type_into_active(Input::Scroll(lines)),
                    None => state.type_into_active(Input::Key(key)),
                };
            }
            TermAction::Scroll(lines) => {
                if lines != 0 {
                    return state.type_into_active(Input::Scroll(lines));
                }
            }
            TermAction::Text(text) => {
                if !text.is_empty() {
                    return state.type_into_active(Input::Text(text));
                }
            }
            TermAction::Paste(text) => {
                if !text.is_empty() {
                    return state.type_into_active(Input::Paste(text));
                }
            }
            TermAction::SelectFrom(at) => state.select(Some(Selection {
                anchor: at,
                head: at,
            })),
            TermAction::SelectTo(at) => {
                if let Some(selection) = state.active_mut().and_then(|s| s.selection.as_mut()) {
                    selection.head = at;
                }
            }
            TermAction::Copy => {
                if let Some(text) = state.selected_text() {
                    return Command::perform(TermEffect::Copy(text));
                }
            }
            TermAction::Painted { id, screen } => {
                if let Some(session) = state.session_mut(id) {
                    session.screen = screen;
                }
            }
            TermAction::Titled { id, title } => {
                if let Some(session) = state.session_mut(id) {
                    session.title = title;
                }
            }
            TermAction::MovedTo { id, cwd } => {
                if let Some(session) = state.session_mut(id) {
                    session.cwd = Some(cwd);
                }
            }
            TermAction::Rang { id } => {
                let active = state.active;
                if let Some(session) = state.session_mut(id)
                    && active != Some(id)
                {
                    session.bells += 1;
                }
            }
            TermAction::Exited { id, .. } => state.remove(id),
        }
        Command::none()
    }

    fn subscriptions(state: &TermState) -> Vec<Pty> {
        state.sessions.iter().map(|s| state.pty(s.id)).collect()
    }
}

#[cfg(test)]
mod tests;
