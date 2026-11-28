//! The JSON-RPC lines herdr-core speaks in plain mode: one message per line.
//! Building and reading them is pure; the app only moves the bytes.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{Change, HerdAction, Terminal};

/// Asks for every terminal and every change after it.
pub fn subscribe_request() -> String {
    request("events.subscribe", json!({}))
}

/// Pastes `text` into `terminal` and presses Enter.
pub fn send_request(terminal: &str, text: &str) -> String {
    request(
        "terminal.send",
        json!({ "terminal": terminal, "text": text }),
    )
}

/// Switches the connection to framed mode after the reply.
pub fn hello_request() -> String {
    request("hello", json!({ "framed": true }))
}

/// Attaches to `terminal` at `cols` x `rows`, as a framed JSON-RPC message
/// (no newline).
pub fn attach_request(terminal: &str, (cols, rows): (u16, u16)) -> Vec<u8> {
    let message = json!({
        "jsonrpc": "2.0",
        "id": ATTACH_ID,
        "method": "terminal.attach",
        "params": { "terminal": terminal, "cols": cols, "rows": rows },
    });
    message.to_string().into_bytes()
}

/// The id of the attach request, so its reply can be told apart.
const ATTACH_ID: u64 = 2;

fn request(method: &str, params: Value) -> String {
    let message = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
    format!("{message}\n")
}

/// Takes every complete line out of `pending`, leaving a partial last line.
pub fn take_lines(pending: &mut Vec<u8>) -> Vec<String> {
    let Some(end) = pending.iter().rposition(|&b| b == b'\n') else {
        return Vec::new();
    };
    let rest = pending.split_off(end + 1);
    let complete = std::mem::replace(pending, rest);
    String::from_utf8_lossy(&complete)
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Frame kinds of framed mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Data = 0,
    Credit = 1,
    Close = 2,
    Resize = 3,
}

/// One frame: `[channel u32 LE][kind u8][len u32 LE][payload]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub channel: u32,
    pub kind: u8,
    pub payload: Vec<u8>,
}

const HEADER: usize = 9;

impl Frame {
    pub fn new(channel: u32, kind: Kind, payload: Vec<u8>) -> Self {
        Self {
            channel,
            kind: kind as u8,
            payload,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let len = u32::try_from(self.payload.len()).expect("frames are at most 16 MiB");
        let mut bytes = Vec::with_capacity(HEADER + self.payload.len());
        bytes.extend_from_slice(&self.channel.to_le_bytes());
        bytes.push(self.kind);
        bytes.extend_from_slice(&len.to_le_bytes());
        bytes.extend_from_slice(&self.payload);
        bytes
    }

    /// Takes the first complete frame out of `pending`, if there is one.
    pub fn take(pending: &mut Vec<u8>) -> Option<Frame> {
        let header = pending.get(..HEADER)?;
        let channel = u32::from_le_bytes(header[..4].try_into().ok()?);
        let kind = header[4];
        let len = u32::from_le_bytes(header[5..].try_into().ok()?) as usize;
        if pending.len() < HEADER + len {
            return None;
        }
        let payload = pending[HEADER..HEADER + len].to_vec();
        pending.drain(..HEADER + len);
        Some(Frame {
            channel,
            kind,
            payload,
        })
    }

    pub fn is(&self, kind: Kind) -> bool {
        self.kind == kind as u8
    }
}

/// How the reply to the attach request went: the stream's channel, or why
/// there is none.
pub fn decode_attach_reply(payload: &[u8]) -> Option<Result<u32, String>> {
    let message: Value = serde_json::from_slice(payload).ok()?;
    if message["id"] != ATTACH_ID {
        return None;
    }
    if let Some(error) = message["error"]["message"].as_str() {
        return Some(Err(error.to_owned()));
    }
    Some(
        message["result"]["channel"]
            .as_u64()
            .and_then(|c| u32::try_from(c).ok())
            .ok_or_else(|| "the attach reply named no channel".to_owned()),
    )
}

/// Why the core closed a stream, from the payload of its CLOSE frame.
pub fn close_reason(payload: &[u8]) -> String {
    let message: Value = serde_json::from_slice(payload).unwrap_or(Value::Null);
    let reason = message["reason"].as_str().unwrap_or("closed");
    match message["exit_code"].as_i64() {
        Some(code) => format!("{reason}, exit code {code}"),
        None => reason.to_owned(),
    }
}

/// The error of a JSON-RPC reply line, if it is one.
pub fn reply_error(line: &str) -> Option<String> {
    let message: Value = serde_json::from_str(line).ok()?;
    message["error"]["message"].as_str().map(str::to_owned)
}

#[derive(Deserialize)]
struct Message {
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    params: Value,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<RpcError>,
}

#[derive(Deserialize)]
struct RpcError {
    message: String,
}

#[derive(Deserialize)]
struct Snapshot {
    seq: u64,
    terminals: Vec<Terminal>,
}

#[derive(Deserialize)]
struct Event {
    seq: u64,
    /// Absent on document events, which the dashboard does not follow.
    terminal: Option<String>,
    #[serde(flatten)]
    change: Value,
}

#[derive(Deserialize)]
struct SendResult {
    turn: u64,
}

/// What one line of the event stream of `attempt` means for the dashboard.
/// `None` for lines it does not follow: heartbeats, document events and
/// event types it does not show.
pub fn decode_stream_line(attempt: u32, line: &str) -> Option<HerdAction> {
    let message: Message = serde_json::from_str(line).ok()?;
    if let Some(error) = message.error {
        return Some(HerdAction::Disconnected {
            attempt,
            error: error.message,
        });
    }
    let snapshot = |value: Value| {
        let Snapshot { seq, terminals } = serde_json::from_value(value).ok()?;
        Some(HerdAction::Snapshot {
            attempt,
            seq,
            terminals,
        })
    };
    if let Some(result) = message.result {
        return snapshot(result);
    }
    match message.method.as_deref()? {
        "missed" => snapshot(message.params),
        "event" => {
            let event: Event = serde_json::from_value(message.params).ok()?;
            let change: Change = serde_json::from_value(event.change).ok()?;
            Some(HerdAction::Changed {
                seq: event.seq,
                terminal: event.terminal?,
                change,
            })
        }
        _ => None,
    }
}

/// The reply to [`send_request`] for `terminal`.
pub fn decode_send_reply(terminal: &str, line: &str) -> HerdAction {
    let result = match serde_json::from_str::<Message>(line) {
        Ok(Message {
            error: Some(error), ..
        }) => Err(error.message),
        Ok(Message {
            result: Some(result),
            ..
        }) => serde_json::from_value::<SendResult>(result)
            .map(|sent| sent.turn)
            .map_err(|err| format!("unexpected reply: {err}")),
        Ok(_) => Err("reply had neither a result nor an error".into()),
        Err(err) => Err(format!("unreadable reply: {err}")),
    };
    HerdAction::Sent {
        terminal: terminal.to_owned(),
        result,
    }
}
