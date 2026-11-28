use std::convert::Infallible;
use std::io;

use herdboard_core::protocols::{Attach, Events, SendText};
use herdboard_core::{HerdAction, HerdEffect, Herdboard, Stream};
use silon_core::{BoxFuture, BoxStream, EffectRunner, Mailboxes, converse};
use smol::net::unix::UnixStream;
use smol::stream::{self, StreamExt};

/// Connects herdr-core's protocols to its Unix socket. Everything the
/// conversations say and decide lives in `herdboard_core::protocols`.
#[derive(Default)]
pub struct HerdRunner {
    /// Bytes typed into each open attach stream.
    typing: Mailboxes<Stream, Vec<u8>>,
}

async fn connect(socket: String) -> io::Result<UnixStream> {
    UnixStream::connect(socket).await
}

fn nothing_to_post() -> stream::Pending<Infallible> {
    stream::pending()
}

impl EffectRunner<Herdboard> for HerdRunner {
    fn run(&self, effect: HerdEffect) -> BoxFuture<'static, Option<HerdAction>> {
        match effect {
            HerdEffect::Send {
                socket,
                terminal,
                text,
            } => {
                let mut reply = converse(
                    SendText::new(terminal, text),
                    connect(socket),
                    nothing_to_post(),
                );
                Box::pin(async move { reply.next().await })
            }
            HerdEffect::Wait { attempt, delay } => Box::pin(async move {
                smol::Timer::after(delay).await;
                Some(HerdAction::Retry { attempt })
            }),
            HerdEffect::Type { to, bytes } => {
                // Posted here, not in the future, so keys keep their order.
                // Keys for a stream that has just closed have nowhere to go.
                let _ = self.typing.post(&to, bytes);
                Box::pin(async { None })
            }
        }
    }

    fn subscribe(&self, stream: Stream) -> BoxStream<'static, HerdAction> {
        match stream.clone() {
            Stream::Events { socket, attempt } => {
                converse(Events::new(attempt), connect(socket), nothing_to_post())
            }
            Stream::Attach {
                socket,
                terminal,
                attempt,
            } => {
                let typed = self.typing.open(stream);
                converse(Attach::new(terminal, attempt), connect(socket), typed)
            }
        }
    }
}
