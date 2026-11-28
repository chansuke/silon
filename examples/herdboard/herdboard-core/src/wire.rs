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

fn request(method: &str, params: Value) -> String {
    let message = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
    format!("{message}\n")
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
