//! The conversations the dashboard has with herdr-core, without the IO.
//! The app runs each over a socket with `silon_core::converse`.

use std::convert::Infallible;
use std::io;
use std::num::NonZeroU16;

use silon_core::{Exchange, Protocol};

use crate::wire::{
    Frame, Kind, attach_request, close_reason, decode_attach_reply, decode_send_reply,
    decode_stream_line, hello_request, reply_error, send_request, subscribe_request, take_lines,
};
use crate::{HerdAction, SIZE, Screen};

/// Follows every terminal: the snapshot, then each change, in plain mode.
pub struct Events {
    attempt: u32,
    pending: Vec<u8>,
}

impl Events {
    pub fn new(attempt: u32) -> Self {
        Self {
            attempt,
            pending: Vec::new(),
        }
    }
}

impl Protocol for Events {
    type Action = HerdAction;
    type Input = Infallible;

    fn open(&mut self, exchange: &mut Exchange<HerdAction>) {
        exchange.write(subscribe_request().as_bytes());
    }

    fn receive(&mut self, bytes: &[u8], exchange: &mut Exchange<HerdAction>) {
        self.pending.extend_from_slice(bytes);
        for line in take_lines(&mut self.pending) {
            if let Some(action) = decode_stream_line(self.attempt, &line) {
                let ended = matches!(action, HerdAction::Disconnected { .. });
                exchange.emit(action);
                if ended {
                    exchange.finish();
                    return;
                }
            }
        }
    }

    fn input(&mut self, input: Infallible, _: &mut Exchange<HerdAction>) {
        match input {}
    }

    fn closed(self, error: io::Error) -> Option<HerdAction> {
        Some(HerdAction::Disconnected {
            attempt: self.attempt,
            error: error.to_string(),
        })
    }
}

/// Pastes text into a terminal and reads the one reply.
pub struct SendText {
    terminal: String,
    text: String,
    pending: Vec<u8>,
}

impl SendText {
    pub fn new(terminal: String, text: String) -> Self {
        Self {
            terminal,
            text,
            pending: Vec::new(),
        }
    }
}

impl Protocol for SendText {
    type Action = HerdAction;
    type Input = Infallible;

    fn open(&mut self, exchange: &mut Exchange<HerdAction>) {
        exchange.write(send_request(&self.terminal, &self.text).as_bytes());
    }

    fn receive(&mut self, bytes: &[u8], exchange: &mut Exchange<HerdAction>) {
        self.pending.extend_from_slice(bytes);
        if let Some(line) = take_lines(&mut self.pending).first() {
            exchange.emit(decode_send_reply(&self.terminal, line));
            exchange.finish();
        }
    }

    fn input(&mut self, input: Infallible, _: &mut Exchange<HerdAction>) {
        match input {}
    }

    fn closed(self, error: io::Error) -> Option<HerdAction> {
        Some(HerdAction::Sent {
            terminal: self.terminal,
            result: Err(error.to_string()),
        })
    }
}

enum Phase {
    /// Waiting for the reply to `hello`, still in plain mode.
    Hello,
    /// Framed; waiting for the reply to `terminal.attach`.
    Attaching,
    Attached {
        channel: u32,
    },
}

/// Shows one terminal: its screen through a VT parser, and its keyboard.
/// Inputs are bytes typed into the terminal.
pub struct Attach {
    terminal: String,
    attempt: u32,
    phase: Phase,
    pending: Vec<u8>,
    /// Keys typed before the attach reply named the channel.
    typed_early: Vec<u8>,
    parser: vt100::Parser,
}

fn dimension(n: u16) -> NonZeroU16 {
    NonZeroU16::new(n).unwrap_or(NonZeroU16::MIN)
}

impl Attach {
    pub fn new(terminal: String, attempt: u32) -> Self {
        let (cols, rows) = SIZE;
        Self {
            terminal,
            attempt,
            phase: Phase::Hello,
            pending: Vec::new(),
            typed_early: Vec::new(),
            parser: vt100::Parser::new(dimension(rows), dimension(cols), 0),
        }
    }

    fn detached(&self, reason: String) -> HerdAction {
        HerdAction::Detached {
            terminal: self.terminal.clone(),
            attempt: self.attempt,
            reason,
        }
    }

    fn end(&self, reason: String, exchange: &mut Exchange<HerdAction>) {
        exchange.emit(self.detached(reason));
        exchange.finish();
    }

    fn screen(&self) -> Screen {
        let screen = self.parser.screen();
        let (_, cols) = screen.size();
        Screen {
            lines: screen.rows(0, cols.get()).collect(),
            cursor: screen.cursor_position(),
        }
    }
}

impl Protocol for Attach {
    type Action = HerdAction;
    type Input = Vec<u8>;

    fn open(&mut self, exchange: &mut Exchange<HerdAction>) {
        exchange.write(hello_request().as_bytes());
    }

    fn receive(&mut self, bytes: &[u8], exchange: &mut Exchange<HerdAction>) {
        self.pending.extend_from_slice(bytes);

        if let Phase::Hello = self.phase {
            let Some(end) = self.pending.iter().position(|&b| b == b'\n') else {
                return;
            };
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            if let Some(error) = reply_error(&String::from_utf8_lossy(&line)) {
                return self.end(error, exchange);
            }
            let request = attach_request(&self.terminal, SIZE);
            exchange.write(&Frame::new(0, Kind::Data, request).encode());
            self.phase = Phase::Attaching;
        }

        let mut consumed = 0;
        while let Some(frame) = Frame::take(&mut self.pending) {
            match self.phase {
                Phase::Attaching if frame.channel == 0 && frame.is(Kind::Data) => {
                    match decode_attach_reply(&frame.payload) {
                        None => {}
                        Some(Err(error)) => return self.end(error, exchange),
                        Some(Ok(channel)) => {
                            self.phase = Phase::Attached { channel };
                            let typed = std::mem::take(&mut self.typed_early);
                            if !typed.is_empty() {
                                exchange.write(&Frame::new(channel, Kind::Data, typed).encode());
                            }
                        }
                    }
                }
                Phase::Attached { channel } if frame.channel == channel => {
                    if frame.is(Kind::Data) {
                        self.parser.process(&frame.payload);
                        consumed += frame.payload.len();
                    } else if frame.is(Kind::Close) {
                        return self.end(close_reason(&frame.payload), exchange);
                    }
                }
                Phase::Hello | Phase::Attaching | Phase::Attached { .. } => {}
            }
        }

        if let (Phase::Attached { channel }, true) = (&self.phase, consumed > 0) {
            let credit = u32::try_from(consumed).unwrap_or(u32::MAX);
            let frame = Frame::new(*channel, Kind::Credit, credit.to_le_bytes().to_vec());
            exchange.write(&frame.encode());
            exchange.emit(HerdAction::Painted {
                terminal: self.terminal.clone(),
                attempt: self.attempt,
                screen: self.screen(),
            });
        }
    }

    fn input(&mut self, bytes: Vec<u8>, exchange: &mut Exchange<HerdAction>) {
        match self.phase {
            Phase::Attached { channel } => {
                exchange.write(&Frame::new(channel, Kind::Data, bytes).encode());
            }
            Phase::Hello | Phase::Attaching => self.typed_early.extend(bytes),
        }
    }

    fn closed(self, error: io::Error) -> Option<HerdAction> {
        Some(self.detached(error.to_string()))
    }
}
