//! Runs the `silon-mcp` binary over real stdio pipes against the real
//! introspection router, backed by a fake store.

use std::time::Duration;

use serde_json::{Value, json};
use silon_introspect::{IntrospectionCommand, IntrospectionRegistry, StoreHandle, router};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc;

const TIMEOUT: Duration = Duration::from_secs(10);

/// A store whose state is a number and whose only action is `"Increment"`.
fn counter_store() -> StoreHandle {
    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut count = 0_u64;
        while let Some(command) = rx.recv().await {
            match command {
                IntrospectionCommand::GetState { reply } => {
                    reply.send(Ok(json!(count))).ok();
                }
                IntrospectionCommand::DispatchAction { action_json, reply } => {
                    let result = if action_json == json!("Increment") {
                        count += 1;
                        Ok(json!(count))
                    } else {
                        Err(format!("invalid action: {action_json}"))
                    };
                    reply.send(result).ok();
                }
                IntrospectionCommand::GetSchema { reply } => {
                    reply.send(json!({ "enum": ["Increment"] })).ok();
                }
                IntrospectionCommand::GetHistory { reply } => {
                    let entries: Vec<_> = (0..count)
                        .map(|seq| json!({ "seq": seq, "action": "Increment" }))
                        .collect();
                    reply
                        .send(json!({ "base": 0, "dropped": 0, "entries": entries }))
                        .ok();
                }
                IntrospectionCommand::Replay { through, reply } => {
                    let state = through.map_or(count, |seq| seq + 1);
                    let replayed = json!({
                        "state": state,
                        "through": through,
                        "matches_current": state == count,
                    });
                    reply.send(Ok(replayed)).ok();
                }
            }
        }
    });
    StoreHandle::new("counter", tx)
}

/// Serves the router on a free loopback port and returns its URL.
async fn serve_app() -> String {
    let registry = IntrospectionRegistry::default();
    registry.register(counter_store());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router(registry)).await });
    format!("http://{addr}")
}

struct Client {
    _child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
    next_id: u64,
}

impl Client {
    fn spawn(url: &str) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_silon-mcp"))
            .args(["--url", url])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        Self {
            stdin: child.stdin.take().unwrap(),
            stdout: BufReader::new(child.stdout.take().unwrap()).lines(),
            _child: child,
            next_id: 1,
        }
    }

    async fn send(&mut self, message: Value) {
        let line = format!("{message}\n");
        self.stdin.write_all(line.as_bytes()).await.unwrap();
    }

    /// Sends a request and returns its `result`.
    async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await;
        loop {
            let line = tokio::time::timeout(TIMEOUT, self.stdout.next_line())
                .await
                .expect("no response in time")
                .unwrap()
                .expect("silon-mcp closed stdout");
            let message: Value = serde_json::from_str(&line).unwrap();
            if message["id"] == json!(id) {
                assert!(message.get("error").is_none(), "{message}");
                return message["result"].clone();
            }
        }
    }

    async fn initialize(url: &str) -> (Self, Value) {
        let mut client = Self::spawn(url);
        let result = client
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": { "name": "silon-mcp-test", "version": "0" },
                }),
            )
            .await;
        client
            .send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .await;
        (client, result)
    }

    /// Calls a tool and returns `(is_error, text)`.
    async fn call(&mut self, name: &str, arguments: Value) -> (bool, String) {
        let result = self
            .request(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            )
            .await;
        let text = result["content"][0]["text"].as_str().unwrap().to_owned();
        (result["isError"] == json!(true), text)
    }
}

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[test]
fn default_url_matches_the_introspection_server() {
    assert_eq!(
        silon_mcp::DEFAULT_URL,
        format!("http://{}", silon_introspect::DEFAULT_ADDR)
    );
}

#[tokio::test]
async fn initialize_negotiates_and_lists_the_tools() {
    let url = serve_app().await;
    let (mut client, init) = Client::initialize(&url).await;

    assert_eq!(init["protocolVersion"], json!("2025-06-18"));
    assert_eq!(init["serverInfo"]["name"], json!("silon-mcp"));
    assert!(init["capabilities"]["tools"].is_object(), "{init}");

    let tools = client.request("tools/list", json!({})).await;
    let mut names: Vec<_> = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "dispatch_action",
            "get_history",
            "get_state",
            "list_features",
            "replay"
        ]
    );
}

#[tokio::test]
async fn tools_forward_to_the_app() {
    let url = serve_app().await;
    let (mut client, _) = Client::initialize(&url).await;

    let (is_error, text) = client.call("list_features", json!({})).await;
    assert!(!is_error, "{text}");
    assert_eq!(
        parse(&text),
        json!([{ "name": "counter", "schema": { "enum": ["Increment"] } }])
    );

    let dispatch = json!({ "feature": "counter", "action": "Increment" });
    assert_eq!(
        client.call("dispatch_action", dispatch.clone()).await,
        (false, "1".into())
    );
    assert_eq!(
        client.call("dispatch_action", dispatch).await,
        (false, "2".into())
    );
    assert_eq!(
        client
            .call("get_state", json!({ "feature": "counter" }))
            .await,
        (false, "2".into())
    );

    let (_, text) = client
        .call("get_history", json!({ "feature": "counter" }))
        .await;
    assert_eq!(
        parse(&text)["entries"][1],
        json!({ "seq": 1, "action": "Increment" })
    );

    let (_, text) = client.call("replay", json!({ "feature": "counter" })).await;
    assert_eq!(
        parse(&text),
        json!({ "state": 2, "through": null, "matches_current": true })
    );
    let (_, text) = client
        .call("replay", json!({ "feature": "counter", "through": 0 }))
        .await;
    assert_eq!(
        parse(&text),
        json!({ "state": 1, "through": 0, "matches_current": false })
    );
}

#[tokio::test]
async fn api_errors_are_tool_errors() {
    let url = serve_app().await;
    let (mut client, _) = Client::initialize(&url).await;

    assert_eq!(
        client
            .call("get_state", json!({ "feature": "missing" }))
            .await,
        (true, "404 Not Found: unknown feature `missing`".into())
    );
    assert_eq!(
        client
            .call(
                "dispatch_action",
                json!({ "feature": "counter", "action": "Nope" })
            )
            .await,
        (true, r#"400 Bad Request: invalid action: "Nope""#.into())
    );
    // A name with a slash stays one path segment instead of reaching another route.
    let (is_error, text) = client
        .call("get_state", json!({ "feature": "../features" }))
        .await;
    assert_eq!(
        (is_error, text.as_str()),
        (true, "404 Not Found: unknown feature `../features`")
    );
}

#[tokio::test]
async fn an_app_that_is_not_running_is_a_tool_error() {
    // Bind and release a port so that nothing listens on it.
    let addr = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let (mut client, _) = Client::initialize(&format!("http://{addr}")).await;

    let (is_error, text) = client.call("list_features", json!({})).await;
    assert!(is_error);
    assert!(
        text.contains("Is it running with --features introspect?"),
        "{text}"
    );
}
