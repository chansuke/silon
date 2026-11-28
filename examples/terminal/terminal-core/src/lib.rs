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

    fn type_into_active(&self, input: Input) -> TermCommand {
        match self.active {
            Some(id) => self.type_into(id, input),
            None => Command::none(),
        }
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
    /// Text pasted into the active session.
    Paste(String),
    /// Scrolls the active session by this many lines, up when negative.
    Scroll(i32),
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
    Paste(String),
    Resize(Size),
    /// Lines to scroll, up when negative: through history, or as arrow
    /// keys when a full-screen program is running.
    Scroll(i32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermEffect {
    Input { to: Pty, input: Input },
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
            TermAction::Paste(text) => {
                if !text.is_empty() {
                    return state.type_into_active(Input::Paste(text));
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
