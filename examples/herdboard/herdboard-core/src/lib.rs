//! Pure dashboard and viewer for the terminals of a herdr-core server: no
//! GPUI, no async runtime, no IO. The app opens the socket; this crate
//! decides what to ask for, speaks the protocols, and decides what each
//! answer does to the state.

pub mod keys;
pub mod protocols;
pub mod wire;

use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use silon_core::{Command, Feature};

pub use keys::Key;

/// One terminal as herdr-core reports it. Fields the dashboard does not
/// show are left out; unknown fields on the wire are ignored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Terminal {
    /// `t7`, or `machine/t7` for a terminal on another machine.
    pub address: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub argv: Vec<String>,
    #[serde(default)]
    pub cwd: String,
    /// `live` or `finished`.
    pub state: String,
    /// What the program said it is doing through OSC 7501.
    #[serde(default)]
    pub status: Option<Status>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub turn: u64,
    #[serde(default)]
    pub foreground: Option<Foreground>,
    /// Why the program ended, once it has.
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub exit_code: Option<i32>,
}

/// One OSC 7501 report. The words are the program's own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Status {
    /// idle, working, done, blocked or error.
    pub state: String,
    #[serde(default)]
    pub msg: String,
}

/// The process in the foreground of a terminal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Foreground {
    pub name: String,
    pub cwd: String,
}

/// A change to one terminal, as an `event` notification carries it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Change {
    Created(Box<Terminal>),
    /// A kept terminal got a new shell; the record replaces the old one.
    Restarted(Box<Terminal>),
    Removed {},
    Status {
        status: Option<Status>,
    },
    Title {
        title: String,
    },
    Turn {
        turn: u64,
    },
    Finished {
        reason: Option<String>,
        exit_code: Option<i32>,
    },
    /// The program of a kept terminal died; it stays live without one.
    Lost {
        reason: Option<String>,
        exit_code: Option<i32>,
    },
    Foreground(Option<Foreground>),
}

/// The visible text of an attached terminal.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Screen {
    /// One string per row, without trailing blanks.
    pub lines: Vec<String>,
    /// Row and column of the cursor.
    pub cursor: (u16, u16),
}

/// Where the event stream stands.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Link {
    #[default]
    Connecting,
    Live,
    /// The last stream ended with `error`; the next attempt starts after
    /// `delay_ms`.
    Waiting {
        error: String,
        delay_ms: u64,
    },
}

/// The selected terminal's screen, while the view is attached to it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    /// Bumped on every new selection or reattach, which opens a new stream.
    pub attempt: u32,
    pub screen: Option<Screen>,
    /// Why the last stream ended. No stream runs until a reattach.
    pub ended: Option<String>,
}

/// What became of the last text sent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Notice {
    Sending { terminal: String },
    Sent { terminal: String, turn: u64 },
    Failed { terminal: String, error: String },
}

/// Columns and rows the viewer asks for.
pub const SIZE: (u16, u16) = (100, 24);

/// Wait before the first reconnect; it doubles with each failure in a row.
const FIRST_DELAY_MS: u64 = 500;
const MAX_DELAY_MS: u64 = 30_000;

/// How long to wait after `failures` failed attempts in a row.
pub fn backoff(failures: u32) -> Duration {
    let doublings = failures.saturating_sub(1).min(16);
    Duration::from_millis((FIRST_DELAY_MS << doublings).min(MAX_DELAY_MS))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HerdState {
    /// Path of the herdr-core socket.
    pub socket: String,
    /// Bumped each time a reconnect starts, which opens a new event stream.
    pub attempt: u32,
    pub link: Link,
    /// Event streams that ended in a row without a snapshot.
    pub failures: u32,
    /// The last event applied. Events at or below it are old news.
    pub seq: u64,
    pub terminals: Vec<Terminal>,
    pub selected: Option<String>,
    pub attachment: Attachment,
    pub draft: String,
    pub notice: Option<Notice>,
}

impl HerdState {
    pub fn new(socket: impl Into<String>) -> Self {
        Self {
            socket: socket.into(),
            attempt: 0,
            link: Link::Connecting,
            failures: 0,
            seq: 0,
            terminals: Vec::new(),
            selected: None,
            attachment: Attachment::default(),
            draft: String::new(),
            notice: None,
        }
    }

    pub fn terminal(&self, address: &str) -> Option<&Terminal> {
        self.terminals.iter().find(|t| t.address == address)
    }

    pub fn selected_terminal(&self) -> Option<&Terminal> {
        self.selected.as_deref().and_then(|a| self.terminal(a))
    }

    fn selected_is_live(&self) -> bool {
        self.selected_terminal().is_some_and(|t| t.state == "live")
    }

    fn is_waiting(&self) -> bool {
        matches!(self.link, Link::Waiting { .. })
    }

    /// Whether `Send` would send the draft now.
    pub fn can_send(&self) -> bool {
        self.link == Link::Live && !self.draft.trim().is_empty() && self.selected_is_live()
    }

    /// The attach stream that should run, if any.
    pub fn attach_stream(&self) -> Option<Stream> {
        let live = self.link == Link::Live && self.selected_is_live();
        (live && self.attachment.ended.is_none()).then(|| Stream::Attach {
            socket: self.socket.clone(),
            terminal: self.selected.clone().unwrap_or_default(),
            attempt: self.attachment.attempt,
        })
    }

    /// Starts a new attachment, which opens a new attach stream.
    fn reattach(&mut self) {
        self.attachment = Attachment {
            attempt: self.attachment.attempt + 1,
            screen: None,
            ended: None,
        };
    }

    /// Selects `address`. A new selection starts a new attachment.
    fn select(&mut self, address: Option<String>) {
        if self.selected != address {
            self.selected = address;
            self.reattach();
        }
    }

    /// Keeps the selection on a terminal that exists, falling back to the first.
    fn fix_selection(&mut self) {
        if self.selected_terminal().is_none() {
            self.select(self.terminals.first().map(|t| t.address.clone()));
        }
    }

    /// Whether `terminal` and `attempt` name the current attachment.
    fn is_attachment(&self, terminal: &str, attempt: u32) -> bool {
        self.selected.as_deref() == Some(terminal) && self.attachment.attempt == attempt
    }

    /// Edits the terminal at `index`; news about one this view never had
    /// changes nothing.
    fn edit(&mut self, index: Option<usize>, edit: impl FnOnce(&mut Terminal)) {
        if let Some(i) = index {
            edit(&mut self.terminals[i]);
        }
    }

    fn apply(&mut self, address: &str, change: Change) {
        let index = self.terminals.iter().position(|t| t.address == address);
        match change {
            Change::Created(record) | Change::Restarted(record) => match index {
                Some(i) => self.terminals[i] = *record,
                None => self.terminals.push(*record),
            },
            Change::Removed {} => {
                if let Some(i) = index {
                    self.terminals.remove(i);
                }
            }
            Change::Status { status } => self.edit(index, |t| t.status = status),
            Change::Title { title } => self.edit(index, |t| t.title = title),
            Change::Turn { turn } => self.edit(index, |t| t.turn = turn),
            Change::Finished { reason, exit_code } => self.edit(index, |t| {
                t.state = "finished".into();
                t.reason = reason;
                t.exit_code = exit_code;
                t.foreground = None;
            }),
            Change::Lost { reason, exit_code } => self.edit(index, |t| {
                t.reason = reason;
                t.exit_code = exit_code;
                t.foreground = None;
            }),
            Change::Foreground(foreground) => self.edit(index, |t| t.foreground = foreground),
        }
        self.fix_selection();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum HerdAction {
    /// The whole list, from the event stream of `attempt`: its first reply
    /// or a `missed` notification after it fell behind.
    Snapshot {
        attempt: u32,
        seq: u64,
        terminals: Vec<Terminal>,
    },
    /// Event `seq` changed `terminal`.
    Changed {
        seq: u64,
        terminal: String,
        change: Change,
    },
    /// The event stream of `attempt` ended.
    Disconnected {
        attempt: u32,
        error: String,
    },
    /// The wait after the event stream of `attempt` ended is over.
    Retry {
        attempt: u32,
    },
    Select(String),
    /// Opens a new attach stream after the last one ended.
    Reattach,
    /// The attach stream `attempt` of `terminal` shows `screen`.
    Painted {
        terminal: String,
        attempt: u32,
        screen: Screen,
    },
    /// The attach stream `attempt` of `terminal` ended.
    Detached {
        terminal: String,
        attempt: u32,
        reason: String,
    },
    /// A key pressed on the attached screen.
    Key(Key),
    DraftChanged(String),
    /// Sends the draft to the selected terminal, then clears it.
    Send,
    /// How sending to `terminal` went: the turn it went in at, or the error.
    Sent {
        terminal: String,
        result: Result<u64, String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HerdEffect {
    /// Pastes `text` into `terminal` and presses Enter.
    Send {
        socket: String,
        terminal: String,
        text: String,
    },
    /// Answers `Retry { attempt }` after `delay`.
    Wait { attempt: u32, delay: Duration },
    /// Writes `bytes` to the open attach stream `to`.
    Type { to: Stream, bytes: Vec<u8> },
}

/// The streams the dashboard keeps open.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Stream {
    /// Every terminal and every change to them.
    Events { socket: String, attempt: u32 },
    /// The screen of one terminal, and its keyboard.
    Attach {
        socket: String,
        terminal: String,
        attempt: u32,
    },
}

pub struct Herdboard;

impl Feature for Herdboard {
    type State = HerdState;
    type Action = HerdAction;
    type Effect = HerdEffect;
    type Subscription = Stream;

    fn update(state: &mut HerdState, action: HerdAction) -> Command<HerdEffect> {
        match action {
            HerdAction::Snapshot {
                attempt,
                seq,
                terminals,
            } => {
                if attempt == state.attempt && !state.is_waiting() {
                    state.link = Link::Live;
                    state.failures = 0;
                    state.seq = seq;
                    state.terminals = terminals;
                    state.fix_selection();
                }
            }
            HerdAction::Changed {
                seq,
                terminal,
                change,
            } => {
                if state.link == Link::Live && seq > state.seq {
                    state.seq = seq;
                    state.apply(&terminal, change);
                }
            }
            HerdAction::Disconnected { attempt, error } => {
                if attempt == state.attempt && !state.is_waiting() {
                    state.failures += 1;
                    let delay = backoff(state.failures);
                    state.link = Link::Waiting {
                        error,
                        delay_ms: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                    };
                    return Command::perform(HerdEffect::Wait { attempt, delay });
                }
            }
            HerdAction::Retry { attempt } => {
                if attempt == state.attempt && state.is_waiting() {
                    state.attempt += 1;
                    state.link = Link::Connecting;
                }
            }
            HerdAction::Select(address) => {
                if state.terminal(&address).is_some() {
                    state.select(Some(address));
                }
            }
            HerdAction::Reattach => {
                if state.attachment.ended.is_some() {
                    state.reattach();
                }
            }
            HerdAction::Painted {
                terminal,
                attempt,
                screen,
            } => {
                if state.is_attachment(&terminal, attempt) {
                    state.attachment.screen = Some(screen);
                }
            }
            HerdAction::Detached {
                terminal,
                attempt,
                reason,
            } => {
                if state.is_attachment(&terminal, attempt) {
                    state.attachment.ended = Some(reason);
                }
            }
            HerdAction::Key(key) => {
                let bytes = key.encode();
                if let (Some(to), false) = (state.attach_stream(), bytes.is_empty()) {
                    return Command::perform(HerdEffect::Type { to, bytes });
                }
            }
            HerdAction::DraftChanged(text) => state.draft = text,
            HerdAction::Send => {
                if state.can_send() {
                    let terminal = state.selected.clone().unwrap_or_default();
                    let text = std::mem::take(&mut state.draft);
                    state.notice = Some(Notice::Sending {
                        terminal: terminal.clone(),
                    });
                    return Command::perform(HerdEffect::Send {
                        socket: state.socket.clone(),
                        terminal,
                        text,
                    });
                }
            }
            HerdAction::Sent { terminal, result } => {
                state.notice = Some(match result {
                    Ok(turn) => Notice::Sent { terminal, turn },
                    Err(error) => Notice::Failed { terminal, error },
                });
            }
        }
        Command::none()
    }

    fn subscriptions(state: &HerdState) -> Vec<Stream> {
        let events = (!state.is_waiting()).then(|| Stream::Events {
            socket: state.socket.clone(),
            attempt: state.attempt,
        });
        events.into_iter().chain(state.attach_stream()).collect()
    }
}

#[cfg(test)]
mod tests;
