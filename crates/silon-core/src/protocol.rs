//! Sans-IO protocols for long-lived streams.
//!
//! A [`Protocol`] turns bytes read from a peer, and messages posted to it,
//! into actions and bytes to write back. It never touches a socket, so a
//! test drives it with plain byte slices and reads the [`Exchange`].
//! [`converse`] runs one over a real connection inside
//! `EffectRunner::subscribe`.

use std::collections::VecDeque;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::Stream;
use futures_io::{AsyncRead, AsyncWrite};

use crate::BoxStream;

/// What a protocol step produced: bytes for the peer and actions for the
/// store, in order.
#[derive(Debug)]
pub struct Exchange<A> {
    written: Vec<u8>,
    actions: Vec<A>,
    finished: bool,
}

impl<A> Default for Exchange<A> {
    fn default() -> Self {
        Self {
            written: Vec::new(),
            actions: Vec::new(),
            finished: false,
        }
    }
}

impl<A> Exchange<A> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues `bytes` for the peer.
    pub fn write(&mut self, bytes: &[u8]) {
        self.written.extend_from_slice(bytes);
    }

    /// Queues `action` for the store.
    pub fn emit(&mut self, action: A) {
        self.actions.push(action);
    }

    /// Ends the stream once the queued bytes are written, without calling
    /// [`Protocol::closed`].
    pub fn finish(&mut self) {
        self.finished = true;
    }

    /// The bytes queued so far, taken out.
    pub fn take_written(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.written)
    }

    /// The actions queued so far, taken out.
    pub fn take_actions(&mut self) -> Vec<A> {
        std::mem::take(&mut self.actions)
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }
}

/// One side of a conversation over a byte stream, without the IO.
pub trait Protocol: Send + 'static {
    type Action: Send + 'static;
    /// Messages effects post to the open stream (see [`crate::Mailboxes`]).
    type Input: Send + 'static;

    /// The connection is up; write the greeting, if any.
    fn open(&mut self, exchange: &mut Exchange<Self::Action>) {
        let _ = exchange;
    }

    /// `bytes` arrived. They may hold part of a message, or several.
    fn receive(&mut self, bytes: &[u8], exchange: &mut Exchange<Self::Action>);

    /// An effect posted `input`.
    fn input(&mut self, input: Self::Input, exchange: &mut Exchange<Self::Action>);

    /// The connection failed or ended because of `error`; the returned
    /// action, if any, is the last one of the stream.
    fn closed(self, error: io::Error) -> Option<Self::Action>;
}

/// Bytes read from the connection at a time (64 KiB).
const READ_SIZE: usize = 65_536;

enum Phase<C, T> {
    Connecting(Pin<Box<C>>),
    Open(T),
    Over,
}

struct Conversation<P: Protocol, C, T, I> {
    protocol: Option<P>,
    phase: Phase<C, T>,
    inputs: Option<I>,
    unwritten: Vec<u8>,
    actions: VecDeque<P::Action>,
    finished: bool,
    buffer: Box<[u8]>,
}

/// Runs `protocol` over the connection `connect` yields, feeding it what the
/// peer sends and what is posted to `inputs`. The stream yields the actions
/// the protocol emits and ends after [`Protocol::closed`] or
/// [`Exchange::finish`]. Dropping the stream drops the connection.
pub fn converse<P, C, T, I>(protocol: P, connect: C, inputs: I) -> BoxStream<'static, P::Action>
where
    P: Protocol,
    C: Future<Output = io::Result<T>> + Send + 'static,
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    I: Stream<Item = P::Input> + Unpin + Send + 'static,
{
    Box::pin(Conversation {
        protocol: Some(protocol),
        phase: Phase::Connecting(Box::pin(connect)),
        inputs: Some(inputs),
        unwritten: Vec::new(),
        actions: VecDeque::new(),
        finished: false,
        buffer: vec![0; READ_SIZE].into_boxed_slice(),
    })
}

// No field is ever pinned in place: the connect future is boxed, and the
// connection and inputs are required to be Unpin where they are polled.
impl<P: Protocol, C, T, I> Unpin for Conversation<P, C, T, I> {}

impl<P: Protocol, C, T, I> Conversation<P, C, T, I> {
    fn absorb(&mut self, mut exchange: Exchange<P::Action>) {
        self.unwritten.append(&mut exchange.written);
        self.actions.extend(exchange.actions);
        self.finished |= exchange.finished;
    }

    fn step(&mut self, run: impl FnOnce(&mut P, &mut Exchange<P::Action>)) {
        let mut exchange = Exchange::new();
        if let Some(protocol) = &mut self.protocol {
            run(protocol, &mut exchange);
        }
        self.absorb(exchange);
    }

    fn close(&mut self, error: io::Error) {
        self.phase = Phase::Over;
        if let Some(action) = self.protocol.take().and_then(|p| p.closed(error)) {
            self.actions.push_back(action);
        }
    }
}

impl<P, C, T, I> Conversation<P, C, T, I>
where
    P: Protocol,
    T: AsyncWrite + AsyncRead + Unpin,
{
    /// Writes what it can of the queued bytes.
    fn flush(&mut self, cx: &mut Context<'_>) -> io::Result<()> {
        let Phase::Open(connection) = &mut self.phase else {
            return Ok(());
        };
        while !self.unwritten.is_empty() {
            match Pin::new(&mut *connection).poll_write(cx, &self.unwritten) {
                Poll::Ready(Ok(0)) => return Err(io::ErrorKind::WriteZero.into()),
                Poll::Ready(Ok(n)) => {
                    self.unwritten.drain(..n);
                }
                Poll::Ready(Err(error)) => return Err(error),
                Poll::Pending => break,
            }
        }
        Ok(())
    }

    fn read(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        match &mut self.phase {
            Phase::Open(connection) => Pin::new(connection).poll_read(cx, &mut self.buffer),
            _ => Poll::Ready(Ok(0)),
        }
    }
}

impl<P, C, T, I> Stream for Conversation<P, C, T, I>
where
    P: Protocol,
    C: Future<Output = io::Result<T>>,
    T: AsyncRead + AsyncWrite + Unpin,
    I: Stream<Item = P::Input> + Unpin,
{
    type Item = P::Action;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<P::Action>> {
        let this = self.get_mut();
        loop {
            if let Some(action) = this.actions.pop_front() {
                return Poll::Ready(Some(action));
            }
            if let Phase::Connecting(connect) = &mut this.phase {
                match connect.as_mut().poll(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Err(error)) => this.close(error),
                    Poll::Ready(Ok(connection)) => {
                        this.phase = Phase::Open(connection);
                        this.step(|p, exchange| p.open(exchange));
                    }
                }
                continue;
            }
            if let Phase::Over = this.phase {
                return Poll::Ready(None);
            }

            if let Err(error) = this.flush(cx) {
                this.close(error);
                continue;
            }
            if this.finished {
                if this.unwritten.is_empty() {
                    this.phase = Phase::Over;
                    continue;
                }
                return Poll::Pending;
            }

            if let Some(inputs) = &mut this.inputs {
                match Pin::new(inputs).poll_next(cx) {
                    Poll::Ready(Some(input)) => {
                        this.step(|p, exchange| p.input(input, exchange));
                        continue;
                    }
                    // Nobody can post any more; keep reading the peer.
                    Poll::Ready(None) => this.inputs = None,
                    Poll::Pending => {}
                }
            }

            match this.read(cx) {
                Poll::Ready(Ok(0)) => this.close(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "the peer closed the connection",
                )),
                Poll::Ready(Ok(n)) => {
                    let bytes = this.buffer[..n].to_vec();
                    this.step(|p, exchange| p.receive(&bytes, exchange));
                }
                Poll::Ready(Err(error)) => this.close(error),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::Mailboxes;

    /// A connection that yields `incoming` chunk by chunk, then end of file,
    /// and records what is written to it.
    struct Script {
        incoming: VecDeque<Vec<u8>>,
        written: Arc<Mutex<Vec<u8>>>,
    }

    impl AsyncRead for Script {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut [u8],
        ) -> Poll<io::Result<usize>> {
            let Some(chunk) = self.incoming.pop_front() else {
                return Poll::Ready(Ok(0));
            };
            buf[..chunk.len()].copy_from_slice(&chunk);
            Poll::Ready(Ok(chunk.len()))
        }
    }

    impl AsyncWrite for Script {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            self.written.lock().unwrap().extend_from_slice(buf);
            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    /// Greets with `hi`, answers each chunk with `ok`, writes each input,
    /// and finishes on `bye`.
    struct Chat;

    #[derive(Debug, PartialEq)]
    enum Heard {
        Chunk(String),
        Closed(io::ErrorKind),
    }

    impl Protocol for Chat {
        type Action = Heard;
        type Input = &'static str;

        fn open(&mut self, exchange: &mut Exchange<Heard>) {
            exchange.write(b"hi;");
        }

        fn receive(&mut self, bytes: &[u8], exchange: &mut Exchange<Heard>) {
            let text = String::from_utf8_lossy(bytes).into_owned();
            if text == "bye" {
                exchange.write(b"later;");
                exchange.finish();
            } else {
                exchange.write(b"ok;");
            }
            exchange.emit(Heard::Chunk(text));
        }

        fn input(&mut self, input: &'static str, exchange: &mut Exchange<Heard>) {
            exchange.write(input.as_bytes());
        }

        fn closed(self, error: io::Error) -> Option<Heard> {
            Some(Heard::Closed(error.kind()))
        }
    }

    fn run(
        incoming: &[&str],
        inputs: impl Stream<Item = &'static str> + Unpin + Send + 'static,
    ) -> (Vec<Heard>, String) {
        let written = Arc::new(Mutex::new(Vec::new()));
        let script = Script {
            incoming: incoming.iter().map(|s| s.as_bytes().to_vec()).collect(),
            written: written.clone(),
        };
        let stream = converse(Chat, async move { Ok(script) }, inputs);
        let heard = smol::block_on(collect(stream));
        let written = String::from_utf8(written.lock().unwrap().clone()).unwrap();
        (heard, written)
    }

    async fn collect<S: Stream + Unpin>(mut stream: S) -> Vec<S::Item> {
        let mut items = Vec::new();
        while let Some(item) = std::future::poll_fn(|cx| Pin::new(&mut stream).poll_next(cx)).await
        {
            items.push(item);
        }
        items
    }

    fn no_inputs() -> impl Stream<Item = &'static str> + Unpin + Send + 'static {
        Mailboxes::<(), &'static str>::new().open(())
    }

    #[test]
    fn greets_then_answers_each_chunk_and_reports_the_end() {
        let (heard, written) = run(&["a", "b"], no_inputs());

        assert_eq!(
            heard,
            [
                Heard::Chunk("a".into()),
                Heard::Chunk("b".into()),
                Heard::Closed(io::ErrorKind::UnexpectedEof),
            ]
        );
        assert_eq!(written, "hi;ok;ok;");
    }

    #[test]
    fn posted_inputs_are_written_in_order() {
        let boxes = Mailboxes::new();
        let inbox = boxes.open(());
        for input in ["1;", "2;", "3;"] {
            boxes.post(&(), input).unwrap();
        }

        let (_, written) = run(&[], inbox);

        assert_eq!(written, "hi;1;2;3;");
    }

    #[test]
    fn finish_writes_what_is_queued_and_ends_without_closed() {
        let (heard, written) = run(&["a", "bye", "never read"], no_inputs());

        assert_eq!(
            heard,
            [Heard::Chunk("a".into()), Heard::Chunk("bye".into())]
        );
        assert_eq!(written, "hi;ok;later;");
    }

    #[test]
    fn a_failed_connect_is_the_only_action() {
        let refused = async { Err::<Script, _>(io::Error::from(io::ErrorKind::ConnectionRefused)) };
        let stream = converse(Chat, refused, no_inputs());

        assert_eq!(
            smol::block_on(collect(stream)),
            [Heard::Closed(io::ErrorKind::ConnectionRefused)]
        );
    }

    #[test]
    fn an_exchange_hands_back_what_a_step_queued() {
        let mut exchange = Exchange::new();
        assert!(!exchange.is_finished());
        Chat.receive(b"bye", &mut exchange);

        assert_eq!(exchange.take_written(), b"later;");
        assert_eq!(exchange.take_actions(), [Heard::Chunk("bye".into())]);
        assert!(exchange.is_finished());
        assert!(exchange.take_written().is_empty());
    }
}
