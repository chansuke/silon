//! Pure dashboard for the terminals of a herdr-core server: no GPUI, no
//! async runtime, no IO. The app opens the socket; this crate decides what
//! to ask for and what each answer does to the state.

pub mod wire;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use silon_core::{Command, Feature};

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

/// Where the event stream stands.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Link {
    #[default]
    Connecting,
    Live,
    /// The last stream ended with this error; the next attempt is on its way.
    Down(String),
}

/// What became of the last text sent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Notice {
    Sending { terminal: String },
    Sent { terminal: String, turn: u64 },
    Failed { terminal: String, error: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HerdState {
    /// Path of the herdr-core socket.
    pub socket: String,
    /// Bumped each time the event stream ends, which opens a new one.
    pub attempt: u32,
    pub link: Link,
    /// The last event applied. Events at or below it are old news.
    pub seq: u64,
    pub terminals: Vec<Terminal>,
    pub selected: Option<String>,
    pub draft: String,
    pub notice: Option<Notice>,
}

impl HerdState {
    pub fn new(socket: impl Into<String>) -> Self {
        Self {
            socket: socket.into(),
            attempt: 0,
            link: Link::Connecting,
            seq: 0,
            terminals: Vec::new(),
            selected: None,
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

    /// Whether `Send` would send the draft now.
    pub fn can_send(&self) -> bool {
        self.link == Link::Live
            && !self.draft.trim().is_empty()
            && self.selected_terminal().is_some_and(|t| t.state == "live")
    }

    /// Keeps the selection on a terminal that exists, falling back to the first.
    fn fix_selection(&mut self) {
        if self.selected_terminal().is_none() {
            self.selected = self.terminals.first().map(|t| t.address.clone());
        }
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
    /// The whole list, from the stream of `attempt`: its first reply or a
    /// `missed` notification after it fell behind.
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
    /// The stream of `attempt` ended.
    Disconnected {
        attempt: u32,
        error: String,
    },
    Select(String),
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
}

/// The event stream of one connection attempt.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Events {
    pub socket: String,
    pub attempt: u32,
}

pub struct Herdboard;

impl Feature for Herdboard {
    type State = HerdState;
    type Action = HerdAction;
    type Effect = HerdEffect;
    type Subscription = Events;

    fn update(state: &mut HerdState, action: HerdAction) -> Command<HerdEffect> {
        match action {
            HerdAction::Snapshot {
                attempt,
                seq,
                terminals,
            } => {
                if attempt == state.attempt {
                    state.link = Link::Live;
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
                if attempt == state.attempt {
                    state.attempt += 1;
                    state.link = Link::Down(error);
                }
            }
            HerdAction::Select(address) => {
                if state.terminal(&address).is_some() {
                    state.selected = Some(address);
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

    fn subscriptions(state: &HerdState) -> Vec<Events> {
        vec![Events {
            socket: state.socket.clone(),
            attempt: state.attempt,
        }]
    }
}

#[cfg(test)]
mod tests;
