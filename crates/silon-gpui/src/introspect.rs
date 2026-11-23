//! Serves [`IntrospectionCommand`]s for a [`Store`] on the GPUI main thread.

use gpui::Context;
use silon_core::{EffectRunner, Feature};
use silon_introspect::{IntrospectionCommand, StoreHandle};
use tokio::sync::mpsc;

use crate::Store;

impl<F: Feature, R: EffectRunner<F>> Store<F, R> {
    /// Returns a handle through which introspection clients read this store's
    /// state, dispatch actions and fetch the action schema.
    ///
    /// Commands are handled on the main thread, so a dispatched action goes
    /// through [`Store::dispatch`] exactly like one sent from the UI. The
    /// handle reports an error once the store is released.
    pub fn attach_introspection(
        &self,
        name: impl Into<String>,
        cx: &mut Context<Self>,
    ) -> StoreHandle {
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
        }
    }

    fn state_json(&self) -> Result<serde_json::Value, String> {
        serde_json::to_value(self.state()).map_err(|err| format!("cannot serialize state: {err}"))
    }
}
