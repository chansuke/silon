use std::io;
use std::time::Duration;

use herdboard_core::wire::{
    decode_send_reply, decode_stream_line, send_request, subscribe_request,
};
use herdboard_core::{Events, HerdAction, HerdEffect, Herdboard};
use silon_core::{BoxFuture, BoxStream, EffectRunner};
use smol::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use smol::net::unix::UnixStream;
use smol::stream::{self, StreamExt};

/// How long a new attempt waits before it dials a core that went away.
const RETRY: Duration = Duration::from_secs(1);

/// Talks to herdr-core over its Unix socket in plain JSON-RPC mode.
pub struct HerdRunner;

impl EffectRunner<Herdboard> for HerdRunner {
    fn run(&self, effect: HerdEffect) -> BoxFuture<'static, Option<HerdAction>> {
        match effect {
            HerdEffect::Send {
                socket,
                terminal,
                text,
            } => Box::pin(async move {
                let request = send_request(&terminal, &text);
                Some(match call(&socket, &request).await {
                    Ok(line) => decode_send_reply(&terminal, &line),
                    Err(err) => HerdAction::Sent {
                        terminal,
                        result: Err(err.to_string()),
                    },
                })
            }),
        }
    }

    fn subscribe(&self, events: Events) -> BoxStream<'static, HerdAction> {
        Box::pin(follow(events))
    }
}

type Reader = Lines<BufReader<UnixStream>>;

/// Connects, writes `request` and returns the reader for the answers.
async fn open(socket: &str, request: &str) -> io::Result<Reader> {
    let mut stream = UnixStream::connect(socket).await?;
    stream.write_all(request.as_bytes()).await?;
    Ok(BufReader::new(stream).lines())
}

async fn next_line(lines: &mut Reader) -> io::Result<String> {
    lines.next().await.unwrap_or_else(|| {
        Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "core closed the connection",
        ))
    })
}

/// One request, one reply, on a connection of its own.
async fn call(socket: &str, request: &str) -> io::Result<String> {
    next_line(&mut open(socket, request).await?).await
}

enum Step {
    Dial,
    Read(Box<Reader>),
    Over,
}

/// The actions of one event stream: its snapshot, then each change, then a
/// final `Disconnected` when it ends for any reason.
fn follow(
    Events { socket, attempt }: Events,
) -> impl stream::Stream<Item = HerdAction> + Send + use<> {
    stream::unfold(Step::Dial, move |step| {
        let socket = socket.clone();
        async move {
            let mut lines = match step {
                Step::Over => return None,
                Step::Read(lines) => *lines,
                Step::Dial => {
                    if attempt > 0 {
                        smol::Timer::after(RETRY).await;
                    }
                    match open(&socket, &subscribe_request()).await {
                        Ok(lines) => lines,
                        Err(err) => return Some((ended(attempt, err), Step::Over)),
                    }
                }
            };
            loop {
                let line = match next_line(&mut lines).await {
                    Ok(line) => line,
                    Err(err) => return Some((ended(attempt, err), Step::Over)),
                };
                if let Some(action) = decode_stream_line(attempt, &line) {
                    let next = match action {
                        HerdAction::Disconnected { .. } => Step::Over,
                        _ => Step::Read(Box::new(lines)),
                    };
                    return Some((action, next));
                }
            }
        }
    })
}

fn ended(attempt: u32, err: io::Error) -> HerdAction {
    HerdAction::Disconnected {
        attempt,
        error: err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use herdboard_core::Change;
    use smol::net::unix::UnixListener;

    use super::*;

    /// A socket path of its own, short enough for the OS limit.
    fn socket_path(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("hb-{}-{name}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    /// Accepts one connection, returns the line it got, and answers `reply`.
    async fn serve_once(listener: UnixListener, reply: &str) -> String {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = String::new();
        BufReader::new(stream.clone())
            .read_line(&mut request)
            .await
            .unwrap();
        stream.write_all(reply.as_bytes()).await.unwrap();
        request
    }

    const SNAPSHOT: &str = r#"{"id":1,"jsonrpc":"2.0","result":{"channel":null,"seq":2,"terminals":[{"address":"t1","state":"live"}]}}"#;
    const HEARTBEAT: &str = r#"{"jsonrpc":"2.0","method":"heartbeat","params":{"seq":2}}"#;
    const TITLE: &str = r#"{"jsonrpc":"2.0","method":"event","params":{"data":{"title":"job"},"seq":3,"terminal":"t1","type":"title"}}"#;

    #[test]
    fn a_stream_yields_the_snapshot_each_change_and_its_end() {
        smol::block_on(async {
            let path = socket_path("follow");
            let listener = UnixListener::bind(&path).unwrap();
            let socket = path.to_string_lossy().into_owned();
            let server = smol::spawn(async move {
                serve_once(listener, &format!("{SNAPSHOT}\n{HEARTBEAT}\n{TITLE}\n")).await
            });

            let actions: Vec<_> = follow(Events { socket, attempt: 0 }).collect().await;
            let request = server.await;
            let _ = std::fs::remove_file(&path);

            assert_eq!(request, subscribe_request());
            assert!(matches!(
                &actions[0],
                HerdAction::Snapshot { attempt: 0, seq: 2, terminals } if terminals.len() == 1
            ));
            assert_eq!(
                actions[1],
                HerdAction::Changed {
                    seq: 3,
                    terminal: "t1".into(),
                    change: Change::Title {
                        title: "job".into()
                    },
                }
            );
            assert!(matches!(
                &actions[2],
                HerdAction::Disconnected { attempt: 0, .. }
            ));
            assert_eq!(actions.len(), 3);
        });
    }

    #[test]
    fn a_stream_to_a_missing_socket_ends_at_once() {
        let socket = socket_path("missing").to_string_lossy().into_owned();
        let actions: Vec<_> = smol::block_on(follow(Events { socket, attempt: 0 }).collect());

        assert!(matches!(
            actions.as_slice(),
            [HerdAction::Disconnected { attempt: 0, .. }]
        ));
    }

    #[test]
    fn sending_returns_the_turn_the_text_went_in_at() {
        smol::block_on(async {
            let path = socket_path("send");
            let listener = UnixListener::bind(&path).unwrap();
            let socket = path.to_string_lossy().into_owned();
            let reply = "{\"id\":1,\"jsonrpc\":\"2.0\",\"result\":{\"turn\":4}}\n";
            let server = smol::spawn(async move { serve_once(listener, reply).await });

            let effect = HerdEffect::Send {
                socket,
                terminal: "t1".into(),
                text: "hello".into(),
            };
            let action = HerdRunner.run(effect).await;
            let request = server.await;
            let _ = std::fs::remove_file(&path);

            assert_eq!(request, send_request("t1", "hello"));
            assert_eq!(
                action,
                Some(HerdAction::Sent {
                    terminal: "t1".into(),
                    result: Ok(4),
                })
            );
        });
    }

    #[test]
    fn sending_to_a_missing_socket_reports_the_error() {
        let effect = HerdEffect::Send {
            socket: socket_path("nobody").to_string_lossy().into_owned(),
            terminal: "t1".into(),
            text: "hello".into(),
        };
        let action = smol::block_on(HerdRunner.run(effect));

        assert!(matches!(
            action,
            Some(HerdAction::Sent { result: Err(_), .. })
        ));
    }
}
