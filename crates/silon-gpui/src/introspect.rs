//! Serves [`IntrospectionCommand`]s for a [`Store`] on the GPUI main thread.

use gpui::Context;
use silon_core::{EffectRunner, Feature};
use silon_introspect::{IntrospectionCommand, StoreHandle};
use tokio::sync::mpsc;

use crate::Store;
use crate::history::ActionHistory;

impl<F: Feature, R: EffectRunner<F>> Store<F, R> {
    /// Returns a handle through which introspection clients read this store's
    /// state, dispatch actions and fetch the action schema.
    ///
    /// Commands are handled on the main thread, so a dispatched action goes
    /// through [`Store::dispatch`] exactly like one sent from the UI. The
    /// handle reports an error once the store is released.
    ///
    /// From now on the store records every dispatched action, keeping the
    /// last [`HISTORY_CAPACITY`](crate::HISTORY_CAPACITY) of them for `GetHistory` and `Replay`.
    pub fn attach_introspection(
        &mut self,
        name: impl Into<String>,
        cx: &mut Context<Self>,
    ) -> StoreHandle {
        if self.history.is_none() {
            self.history = Some(ActionHistory::new(self.state().clone()));
        }
        let (tx, mut rx) = mpsc::unbounded_channel();
        cx.spawn(async move |this, cx| {
            while let Some(command) = rx.recv().await {
                if this
                    .update(cx, |store, cx| store.answer(command, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        StoreHandle::new(name, tx)
    }

    fn answer(&mut self, command: IntrospectionCommand, cx: &mut Context<Self>) {
        match command {
            IntrospectionCommand::GetState { reply } => {
                reply.send(self.state_json()).ok();
            }
            IntrospectionCommand::DispatchAction { action_json, reply } => {
                let result = match serde_json::from_value::<F::Action>(action_json) {
                    Ok(action) => {
                        self.dispatch(action, cx);
                        self.state_json()
                    }
                    Err(err) => Err(format!("invalid action: {err}")),
                };
                reply.send(result).ok();
            }
            IntrospectionCommand::GetSchema { reply } => {
                let schema = schemars::schema_for!(F::Action);
                reply
                    .send(serde_json::to_value(schema).unwrap_or_default())
                    .ok();
            }
            IntrospectionCommand::GetHistory { reply } => {
                let history = self.history.as_ref().map(ActionHistory::to_json);
                reply.send(history.unwrap_or_default()).ok();
            }
            IntrospectionCommand::Replay { through, reply } => {
                reply.send(self.replay_json(through)).ok();
            }
        }
    }

    fn replay_json(&self, through: Option<u64>) -> Result<serde_json::Value, String> {
        let history = self
            .history
            .as_ref()
            .ok_or("introspection is not attached")?;
        let replayed = serde_json::to_value(history.replay(through)?)
            .map_err(|err| format!("cannot serialize state: {err}"))?;
        let current = self.state_json()?;
        Ok(serde_json::json!({
            "matches_current": replayed == current,
            "state": replayed,
            "through": through,
        }))
    }

    fn state_json(&self) -> Result<serde_json::Value, String> {
        serde_json::to_value(self.state()).map_err(|err| format!("cannot serialize state: {err}"))
    }
}
