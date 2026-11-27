use crate::Feature;

/// Drives a [`Feature`] headlessly, recording every emitted effect.
pub struct TestHarness<F: Feature> {
    pub state: F::State,
    pub emitted_effects: Vec<F::Effect>,
}

impl<F: Feature> TestHarness<F> {
    pub fn new(initial: F::State) -> Self {
        Self {
            state: initial,
            emitted_effects: Vec::new(),
        }
    }

    /// Runs `update` with `action` and appends the resulting effects.
    pub fn send(&mut self, action: F::Action) -> &mut Self {
        let cmd = F::update(&mut self.state, action);
        self.emitted_effects.extend(cmd.into_effects());
        self
    }

    /// Returns the effects emitted so far and clears the record.
    pub fn take_effects(&mut self) -> Vec<F::Effect> {
        std::mem::take(&mut self.emitted_effects)
    }

    /// The subscriptions a store would run in the current state.
    pub fn subscriptions(&self) -> Vec<F::Subscription> {
        F::subscriptions(&self.state)
    }
}

#[cfg(test)]
mod tests {
    use super::TestHarness;
    use crate::{Command, Feature};
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    struct Echo;

    #[derive(Debug, Serialize, Deserialize, JsonSchema)]
    enum EchoAction {
        Push(u8),
        PushTwice(u8),
    }

    impl Feature for Echo {
        type State = Vec<u8>;
        type Action = EchoAction;
        type Effect = u8;
        type Subscription = u8;

        fn update(state: &mut Vec<u8>, action: EchoAction) -> Command<u8> {
            match action {
                EchoAction::Push(n) => {
                    state.push(n);
                    Command::perform(n)
                }
                EchoAction::PushTwice(n) => {
                    state.extend([n, n]);
                    Command::batch([Command::perform(n), Command::perform(n)])
                }
            }
        }

        /// One subscription per distinct value pushed so far.
        fn subscriptions(state: &Vec<u8>) -> Vec<u8> {
            let mut subs = state.clone();
            subs.sort_unstable();
            subs.dedup();
            subs
        }
    }

    #[test]
    fn send_updates_state_and_accumulates_effects() {
        let mut harness = TestHarness::<Echo>::new(Vec::new());
        harness
            .send(EchoAction::Push(1))
            .send(EchoAction::PushTwice(2));

        assert_eq!(harness.state, vec![1, 2, 2]);
        assert_eq!(harness.emitted_effects, vec![1, 2, 2]);
    }

    #[test]
    fn take_effects_drains_the_record() {
        let mut harness = TestHarness::<Echo>::new(Vec::new());
        harness.send(EchoAction::Push(7));

        assert_eq!(harness.take_effects(), vec![7]);
        assert!(harness.emitted_effects.is_empty());
        assert_eq!(harness.state, vec![7]);
    }

    #[test]
    fn subscriptions_follow_the_current_state() {
        let mut harness = TestHarness::<Echo>::new(Vec::new());
        assert!(harness.subscriptions().is_empty());

        harness
            .send(EchoAction::Push(3))
            .send(EchoAction::PushTwice(1));

        assert_eq!(harness.subscriptions(), vec![1, 3]);
    }
}
