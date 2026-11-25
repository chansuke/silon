//! Introspection bridge between external agents and running Silon stores.
//!
//! The UI side owns each store and answers [`IntrospectionCommand`]s sent
//! through a [`StoreHandle`]. The HTTP server in [`server`] looks handles up in
//! an [`IntrospectionRegistry`]. Nothing here depends on GPUI.

mod registry;
pub mod server;

pub use registry::IntrospectionRegistry;
pub use server::{DEFAULT_ADDR, router, spawn_server};

use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};

/// A request to the UI side that owns a store.
pub enum IntrospectionCommand {
    GetState {
        reply: oneshot::Sender<Result<serde_json::Value, String>>,
    },
    DispatchAction {
        action_json: serde_json::Value,
        reply: oneshot::Sender<Result<serde_json::Value, String>>,
    },
    GetSchema {
        reply: oneshot::Sender<serde_json::Value>,
    },
    /// Asks for the recorded actions, see [`StoreHandle::get_history`].
    GetHistory {
        reply: oneshot::Sender<serde_json::Value>,
    },
    /// Asks for a replay of the recorded actions, see [`StoreHandle::replay`].
    Replay {
        through: Option<u64>,
        reply: oneshot::Sender<Result<serde_json::Value, String>>,
    },
}

/// A named, cloneable channel to one store.
#[derive(Clone)]
pub struct StoreHandle {
    name: Arc<str>,
    tx: mpsc::UnboundedSender<IntrospectionCommand>,
}

impl StoreHandle {
    pub fn new(name: impl Into<String>, tx: mpsc::UnboundedSender<IntrospectionCommand>) -> Self {
        Self {
            name: name.into().into(),
            tx,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the store's current state as JSON.
    pub async fn get_state(&self) -> Result<serde_json::Value, String> {
        self.request(|reply| IntrospectionCommand::GetState { reply })
            .await?
    }

    /// Deserializes `action_json` into the store's action type, dispatches it
    /// and returns the state right after `update` (before any effect completes).
    pub async fn dispatch_json(
        &self,
        action_json: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.request(|reply| IntrospectionCommand::DispatchAction { action_json, reply })
            .await?
    }

    /// Returns the JSON Schema of the store's action type.
    pub async fn get_schema(&self) -> Result<serde_json::Value, String> {
        self.request(|reply| IntrospectionCommand::GetSchema { reply })
            .await
    }

    /// Returns the actions recorded since introspection was attached:
    /// `{"base": state, "dropped": n, "entries": [{"seq": n, "action": action}]}`.
    /// Applying the entries to `base` in order gives the current state.
    pub async fn get_history(&self) -> Result<serde_json::Value, String> {
        self.request(|reply| IntrospectionCommand::GetHistory { reply })
            .await
    }

    /// Replays the recorded actions from the base up to and including entry
    /// `through` (all of them for `None`) without touching the live store:
    /// `{"state": state, "through": seq, "matches_current": bool}`.
    pub async fn replay(&self, through: Option<u64>) -> Result<serde_json::Value, String> {
        self.request(|reply| IntrospectionCommand::Replay { through, reply })
            .await?
    }

    async fn request<T>(
        &self,
        command: impl FnOnce(oneshot::Sender<T>) -> IntrospectionCommand,
    ) -> Result<T, String> {
        let (reply, response) = oneshot::channel();
        self.tx.send(command(reply)).map_err(|_| self.gone())?;
        response.await.map_err(|_| self.gone())
    }

    fn gone(&self) -> String {
        format!("store `{}` is no longer running", self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Answers commands like a store whose state is a single integer and whose
    /// only action is `"Increment"`.
    pub(crate) fn fake_store(name: &str) -> StoreHandle {
        let (tx, mut rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut count = 0_u64;
            while let Some(command) = rx.recv().await {
                match command {
                    IntrospectionCommand::GetState { reply } => {
                        reply.send(Ok(json!({ "count": count }))).ok();
                    }
                    IntrospectionCommand::DispatchAction { action_json, reply } => {
                        let result = if action_json == json!("Increment") {
                            count += 1;
                            Ok(json!({ "count": count }))
                        } else {
                            Err(format!("unknown action: {action_json}"))
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
                        let history =
                            json!({ "base": { "count": 0 }, "dropped": 0, "entries": entries });
                        reply.send(history).ok();
                    }
                    IntrospectionCommand::Replay { through, reply } => {
                        let result = match through {
                            None => Ok(
                                json!({ "state": { "count": count }, "through": null, "matches_current": true }),
                            ),
                            Some(seq) if seq < count => Ok(json!({
                                "state": { "count": seq + 1 },
                                "through": seq,
                                "matches_current": seq + 1 == count,
                            })),
                            Some(seq) => Err(format!("entry {seq} has not been recorded")),
                        };
                        reply.send(result).ok();
                    }
                }
            }
        });
        StoreHandle::new(name, tx)
    }

    #[tokio::test]
    async fn handle_round_trips_commands() {
        let handle = fake_store("counter");

        assert_eq!(handle.name(), "counter");
        assert_eq!(handle.get_state().await, Ok(json!({ "count": 0 })));
        assert_eq!(
            handle.dispatch_json(json!("Increment")).await,
            Ok(json!({ "count": 1 }))
        );
        assert!(handle.dispatch_json(json!("Nope")).await.is_err());
        assert_eq!(
            handle.get_schema().await,
            Ok(json!({ "enum": ["Increment"] }))
        );
        assert_eq!(
            handle.get_history().await,
            Ok(json!({
                "base": { "count": 0 },
                "dropped": 0,
                "entries": [{ "seq": 0, "action": "Increment" }],
            }))
        );
        assert_eq!(
            handle.replay(Some(0)).await,
            Ok(json!({ "state": { "count": 1 }, "through": 0, "matches_current": true }))
        );
        assert_eq!(
            handle.replay(Some(1)).await,
            Err("entry 1 has not been recorded".to_string())
        );
    }

    #[tokio::test]
    async fn handle_reports_a_store_that_stopped() {
        let (tx, rx) = mpsc::unbounded_channel();
        drop(rx);
        let handle = StoreHandle::new("gone", tx);

        assert_eq!(
            handle.get_state().await,
            Err("store `gone` is no longer running".to_string())
        );
    }
}
