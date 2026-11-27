use std::collections::{HashMap, HashSet};

use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{Context, Task};
use silon_core::{EffectRunner, Feature};

/// Owns a feature's state and is the only place where it changes.
pub struct Store<F: Feature, R: EffectRunner<F>> {
    state: F::State,
    runner: R,
    /// The running subscriptions. Dropping an entry's task stops its stream.
    subscriptions: HashMap<F::Subscription, Task<()>>,
    /// Recorded actions, started by `attach_introspection`.
    #[cfg(feature = "introspect")]
    pub(crate) history: Option<crate::history::ActionHistory<F>>,
}

impl<F: Feature, R: EffectRunner<F>> Store<F, R> {
    /// Creates the store and starts the subscriptions of `initial_state`.
    pub fn new(initial_state: F::State, runner: R, cx: &mut Context<Self>) -> Self {
        let mut store = Self {
            state: initial_state,
            runner,
            subscriptions: HashMap::new(),
            #[cfg(feature = "introspect")]
            history: None,
        };
        store.reconcile_subscriptions(cx);
        store
    }

    pub fn state(&self) -> &F::State {
        &self.state
    }

    /// Applies `action`, notifies observers, then runs each emitted effect
    /// and brings the running subscriptions in line with the new state.
    ///
    /// Effect futures run on the background executor; a follow-up action they
    /// return is dispatched back on the main thread. Follow-ups are dropped if
    /// the store has been released by then.
    pub fn dispatch(&mut self, action: F::Action, cx: &mut Context<Self>) {
        #[cfg(feature = "introspect")]
        let recorded = self
            .history
            .is_some()
            .then(|| serde_json::to_value(&action));

        let effects = F::update(&mut self.state, action).into_effects();

        #[cfg(feature = "introspect")]
        if let (Some(history), Some(recorded)) = (&mut self.history, recorded) {
            history.record(recorded, &self.state);
        }

        cx.notify();

        for effect in effects {
            let future = self.runner.run(effect);
            let task = cx.background_executor().spawn(future);
            cx.spawn(async move |this, cx| {
                if let Some(next) = task.await {
                    // The store may be gone (e.g. window closed); nothing to do then.
                    this.update(cx, |store, cx| store.dispatch(next, cx)).ok();
                }
            })
            .detach();
        }

        self.reconcile_subscriptions(cx);
    }

    /// Stops the subscriptions the state no longer asks for and starts the
    /// new ones. A subscription that is still asked for keeps running, even
    /// if its stream has ended. New ones open in the order the state lists them.
    fn reconcile_subscriptions(&mut self, cx: &mut Context<Self>) {
        let wanted = F::subscriptions(&self.state);
        let kept: HashSet<&F::Subscription> = wanted.iter().collect();
        self.subscriptions.retain(|key, _| kept.contains(key));
        for key in wanted {
            if !self.subscriptions.contains_key(&key) {
                let task = self.start(key.clone(), cx);
                self.subscriptions.insert(key, task);
            }
        }
    }

    /// Polls the stream on the background executor and dispatches its items
    /// on the main thread, in the order the stream yields them.
    fn start(&self, key: F::Subscription, cx: &mut Context<Self>) -> Task<()> {
        let mut stream = self.runner.subscribe(key.clone());
        let (sender, mut items) = mpsc::unbounded();
        let pump = cx.background_executor().spawn(async move {
            while let Some(item) = stream.next().await {
                if sender.unbounded_send(item).is_err() {
                    break;
                }
            }
        });

        cx.spawn(async move |this, cx| {
            // Dropping this task drops the pump, which drops the stream.
            let _pump = pump;
            while let Some(item) = items.next().await {
                // An item dispatched earlier in this loop can stop this very
                // subscription; its remaining items must not reach the state.
                // A stopped task is never polled again, so a key started
                // again later belongs to a new task.
                let delivered = this.update(cx, |store, cx| {
                    let current = store.subscriptions.contains_key(&key);
                    if current {
                        store.dispatch(item, cx);
                    }
                    current
                });
                if !matches!(delivered, Ok(true)) {
                    break;
                }
            }
        })
    }
}
