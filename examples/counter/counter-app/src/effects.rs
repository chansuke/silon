use std::convert::Infallible;
use std::time::Duration;

use counter_core::{CounterAction, CounterEffect, CounterFeature};
use silon_core::{BoxFuture, BoxStream, EffectRunner};

/// Executes counter effects with real timers.
pub struct CounterEffectRunner;

impl EffectRunner<CounterFeature> for CounterEffectRunner {
    fn run(&self, effect: CounterEffect) -> BoxFuture<'static, Option<CounterAction>> {
        match effect {
            CounterEffect::DelayedAdd { delta, delay_ms } => Box::pin(async move {
                smol::Timer::after(Duration::from_millis(delay_ms)).await;
                Some(CounterAction::AsyncAddCompleted(delta))
            }),
        }
    }

    fn subscribe(&self, subscription: Infallible) -> BoxStream<'static, CounterAction> {
        match subscription {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delayed_add_completes_with_the_requested_delta() {
        let action = smol::block_on(CounterEffectRunner.run(CounterEffect::DelayedAdd {
            delta: 5,
            delay_ms: 1,
        }));

        assert_eq!(action, Some(CounterAction::AsyncAddCompleted(5)));
    }
}
