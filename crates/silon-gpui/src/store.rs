use gpui::Context;
use silon_core::{EffectRunner, Feature};

/// Owns a feature's state and is the only place where it changes.
pub struct Store<F: Feature, R: EffectRunner<F>> {
    state: F::State,
    runner: R,
}

impl<F: Feature, R: EffectRunner<F>> Store<F, R> {
    pub fn new(initial_state: F::State, runner: R) -> Self {
        Self {
            state: initial_state,
            runner,
        }
    }

    pub fn state(&self) -> &F::State {
        &self.state
    }

    /// Applies `action`, notifies observers, then runs each emitted effect.
    ///
    /// Effect futures run on the background executor; a follow-up action they
    /// return is dispatched back on the main thread. Follow-ups are dropped if
    /// the store has been released by then.
    pub fn dispatch(&mut self, action: F::Action, cx: &mut Context<Self>) {
        let effects = F::update(&mut self.state, action).into_effects();
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
    }
}
