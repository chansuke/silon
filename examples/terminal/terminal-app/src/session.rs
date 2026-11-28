//! A session as a subscription stream: a thread of its own that owns the
//! PTY and the VT engine, since libghostty-vt handles cannot leave the
//! thread that made them.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::task::{Context, Poll, Wake, Waker};
use std::thread;
use std::time::Duration;

use futures::Stream;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use silon_core::{BoxStream, Mailbox};
use terminal_core::{Input, Pty as PtyKey, TermAction};

use crate::engine::{Engine, Report};
use crate::pty::Pty;

/// What wakes the session thread.
enum Wakeup {
    /// The program wrote this.
    Output(Vec<u8>),
    /// The program's side of the terminal closed.
    Ended,
    /// Something was posted to the mailbox.
    Posted,
    /// Nobody listens any more.
    Stop,
}

struct PostedWaker(Sender<Wakeup>);

impl Wake for PostedWaker {
    fn wake(self: Arc<Self>) {
        let _ = self.0.send(Wakeup::Posted);
    }
}

/// The actions of one session. Dropping it stops the session, which hangs
/// up its program.
struct SessionStream {
    actions: UnboundedReceiver<TermAction>,
    stop: Sender<Wakeup>,
}

impl Stream for SessionStream {
    type Item = TermAction;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<TermAction>> {
        Pin::new(&mut self.actions).poll_next(cx)
    }
}

impl Drop for SessionStream {
    fn drop(&mut self) {
        let _ = self.stop.send(Wakeup::Stop);
    }
}

pub type Inputs = Mailbox<PtyKey, Input>;

/// Starts session `key`. It runs its shell once the first size arrives in
/// `inputs`, which the feature posts when it opens the session.
pub fn start(
    key: PtyKey,
    inputs: Inputs,
    terminfo: Option<PathBuf>,
) -> BoxStream<'static, TermAction> {
    let (actions, receiver) = unbounded();
    let (wake, wakeups) = mpsc::channel();
    let stop = wake.clone();
    let id = key.id;
    let spawned = thread::Builder::new()
        .name(format!("session-{id}"))
        .spawn(move || {
            let mut session = Session {
                key,
                inputs,
                wake,
                wakeups,
                actions,
                terminfo,
            };
            let code = session.run().unwrap_or_else(|err| {
                eprintln!("session {id}: {err}");
                None
            });
            let _ = session
                .actions
                .unbounded_send(TermAction::Exited { id, code });
        });
    if let Err(err) = spawned {
        eprintln!("session {id}: cannot start a thread: {err}");
    }
    Box::pin(SessionStream {
        actions: receiver,
        stop,
    })
}

struct Session {
    key: PtyKey,
    inputs: Inputs,
    wake: Sender<Wakeup>,
    wakeups: Receiver<Wakeup>,
    actions: UnboundedSender<TermAction>,
    terminfo: Option<PathBuf>,
}

/// The running parts, which exist once the size is known.
struct Running {
    pty: Pty,
    writer: File,
    engine: Engine,
}

fn io_error(err: ghostty_vt::Error) -> io::Error {
    io::Error::other(err.to_string())
}

impl Session {
    fn emit(&self, action: TermAction) -> bool {
        self.actions.unbounded_send(action).is_ok()
    }

    /// Takes what has been posted so far.
    fn posted(&mut self) -> Vec<Input> {
        let waker = Waker::from(Arc::new(PostedWaker(self.wake.clone())));
        let mut cx = Context::from_waker(&waker);
        let mut inputs = Vec::new();
        while let Poll::Ready(Some(input)) = Pin::new(&mut self.inputs).poll_next(&mut cx) {
            inputs.push(input);
        }
        inputs
    }

    /// Runs the session until its program ends or nobody listens, and
    /// returns the program's exit code if it ended.
    fn run(&mut self) -> io::Result<Option<i32>> {
        let mut early = Vec::new();
        let mut running = loop {
            let inputs = self.posted();
            let first_size = inputs
                .iter()
                .enumerate()
                .find_map(|(i, input)| match input {
                    Input::Resize(size) => Some((i, *size)),
                    _ => None,
                });
            if let Some((index, size)) = first_size {
                early.extend(
                    inputs
                        .into_iter()
                        .enumerate()
                        .filter_map(|(i, input)| (i != index).then_some(input)),
                );
                let pty = Pty::spawn(&self.key.shell, size, self.terminfo.as_deref())?;
                let engine = Engine::new(size).map_err(io_error)?;
                let writer = pty.writer()?;
                spawn_reader(pty.reader()?, self.wake.clone())?;
                break Running {
                    pty,
                    writer,
                    engine,
                };
            }
            early.extend(inputs);
            // Only posts and stops can come before the program runs.
            if let Ok(Wakeup::Stop) | Err(_) = self.wakeups.recv() {
                return Ok(None);
            }
        };
        for input in early {
            self.apply(&mut running, input)?;
        }

        loop {
            let mut painted = false;
            match self.wakeups.recv() {
                Ok(Wakeup::Posted) => {
                    for input in self.posted() {
                        painted |= self.apply(&mut running, input)?;
                    }
                }
                Ok(Wakeup::Output(bytes)) => {
                    self.feed(&mut running, &bytes)?;
                    // Draw once for everything that is already waiting.
                    loop {
                        match self.wakeups.try_recv() {
                            Ok(Wakeup::Output(bytes)) => self.feed(&mut running, &bytes)?,
                            Ok(Wakeup::Posted) => {
                                for input in self.posted() {
                                    self.apply(&mut running, input)?;
                                }
                            }
                            Ok(Wakeup::Ended) => return Ok(self.paint_and_wait(&mut running)),
                            Ok(Wakeup::Stop) => return Ok(None),
                            Err(_) => break,
                        }
                    }
                    painted = true;
                }
                Ok(Wakeup::Ended) => return Ok(self.paint_and_wait(&mut running)),
                Ok(Wakeup::Stop) | Err(_) => return Ok(None),
            }
            if painted && !self.paint(&mut running) {
                return Ok(None);
            }
        }
    }

    /// Feeds output to the engine and acts on what it reports.
    fn feed(&self, running: &mut Running, bytes: &[u8]) -> io::Result<()> {
        let id = self.key.id;
        for report in running.engine.feed(bytes) {
            match report {
                Report::Reply(bytes) => running.writer.write_all(&bytes)?,
                Report::Title(title) => {
                    self.emit(TermAction::Titled { id, title });
                }
                Report::Cwd(uri) => {
                    self.emit(TermAction::MovedTo {
                        id,
                        cwd: path_of(&uri),
                    });
                }
                Report::Bell => {
                    self.emit(TermAction::Rang { id });
                }
            }
        }
        Ok(())
    }

    /// Applies one input; returns whether the screen needs a new paint.
    fn apply(&self, running: &mut Running, input: Input) -> io::Result<bool> {
        let bytes = match input {
            Input::Key(key) => running.engine.key(&key).map_err(io_error)?,
            Input::Text(text) => running.engine.text(&text),
            Input::Paste(text) => running.engine.paste(&text).map_err(io_error)?,
            Input::Resize(size) => {
                running.engine.resize(size).map_err(io_error)?;
                running.pty.resize(size)?;
                return Ok(true);
            }
            Input::Scroll(lines) => {
                let bytes = running.engine.scroll(lines).map_err(io_error)?;
                running.writer.write_all(&bytes)?;
                return Ok(true);
            }
        };
        running.writer.write_all(&bytes)?;
        // Typing returns the view to the latest output.
        Ok(true)
    }

    /// Reports the screen; returns false when nobody listens any more.
    fn paint(&self, running: &mut Running) -> bool {
        match running.engine.screen() {
            Ok(screen) => self.emit(TermAction::Painted {
                id: self.key.id,
                screen,
            }),
            Err(err) => {
                eprintln!("session {}: cannot read the screen: {err}", self.key.id);
                true
            }
        }
    }

    /// The program closed the terminal: show the last screen and collect
    /// its exit code.
    fn paint_and_wait(&self, running: &mut Running) -> Option<i32> {
        self.paint(running);
        (0..100).find_map(|_| {
            let code = running.pty.exit_code();
            if code.is_none() {
                thread::sleep(Duration::from_millis(10));
            }
            code
        })?
    }
}

/// Reads the program's output on a thread of its own until the terminal
/// closes.
fn spawn_reader(mut reader: File, wake: Sender<Wakeup>) -> io::Result<()> {
    thread::Builder::new()
        .name("pty-reader".into())
        .spawn(move || {
            let mut buf = vec![0; 64 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if wake.send(Wakeup::Output(buf[..n].to_vec())).is_err() {
                            return;
                        }
                    }
                }
            }
            let _ = wake.send(Wakeup::Ended);
        })?;
    Ok(())
}

/// The path of an OSC 7 `file://host/path` URI, or the value as it is.
fn path_of(uri: &str) -> String {
    uri.strip_prefix("file://")
        .and_then(|rest| rest.find('/').map(|slash| rest[slash..].to_owned()))
        .unwrap_or_else(|| uri.to_owned())
}

#[cfg(test)]
mod tests;
