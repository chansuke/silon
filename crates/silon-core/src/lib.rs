//! Pure, IO-free core of the Silon framework.
//!
//! A [`Feature`] describes a unidirectional loop:
//! `Action -> update -> (State, Command<Effect>) -> View`.
//! Effects are plain data; an [`EffectRunner`] living in the application
//! crate turns them into futures that may produce follow-up actions.

mod harness;
mod history;

pub use harness::TestHarness;
pub use history::{History, ReplayError};

use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;

/// A boxed, sendable future.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Side effects requested by [`Feature::update`], described as data.
#[derive(Debug, Default)]
pub enum Command<E> {
    #[default]
    None,
    Perform(E),
    Batch(Vec<Command<E>>),
}

impl<E> Command<E> {
    pub fn none() -> Self {
        Self::None
    }

    pub fn perform(effect: E) -> Self {
        Self::Perform(effect)
    }

    pub fn batch(cmds: impl IntoIterator<Item = Command<E>>) -> Self {
        Self::Batch(cmds.into_iter().collect())
    }

    /// Flattens the command tree into effects, in declaration order.
    pub fn into_effects(self) -> Vec<E> {
        let mut effects = Vec::new();
        self.collect_into(&mut effects);
        effects
    }

    fn collect_into(self, effects: &mut Vec<E>) {
        match self {
            Self::None => {}
            Self::Perform(effect) => effects.push(effect),
            Self::Batch(cmds) => {
                for cmd in cmds {
                    cmd.collect_into(effects);
                }
            }
        }
    }
}

/// A unit of pure business logic.
pub trait Feature: 'static {
    type State: Clone + Debug + Serialize + DeserializeOwned + 'static;
    type Action: Debug + Serialize + DeserializeOwned + JsonSchema + Send + 'static;
    type Effect: Debug + Send + 'static;

    /// Applies `action` to `state`. Must be pure and deterministic.
    fn update(state: &mut Self::State, action: Self::Action) -> Command<Self::Effect>;
}

/// Executes the effects of a [`Feature`], optionally yielding a follow-up action.
pub trait EffectRunner<F: Feature>: 'static {
    fn run(&self, effect: F::Effect) -> BoxFuture<'static, Option<F::Action>>;
}

/// The runner for features without effects, i.e. whose `Effect` is
/// [`Infallible`](std::convert::Infallible).
pub struct NoEffects;

impl<F: Feature<Effect = std::convert::Infallible>> EffectRunner<F> for NoEffects {
    fn run(&self, effect: std::convert::Infallible) -> BoxFuture<'static, Option<F::Action>> {
        match effect {}
    }
}

#[cfg(test)]
mod tests {
    use super::Command;

    #[test]
    fn into_effects_flattens_nested_batches_in_order() {
        let cmd = Command::batch([
            Command::perform(1),
            Command::none(),
            Command::batch([Command::perform(2), Command::perform(3)]),
            Command::perform(4),
        ]);

        assert_eq!(cmd.into_effects(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn none_yields_no_effects() {
        assert!(Command::<i32>::none().into_effects().is_empty());
        assert!(Command::<i32>::default().into_effects().is_empty());
    }
}
