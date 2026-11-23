//! GPUI bridge for Silon.
//!
//! [`Store`] owns a feature's state, runs `update` on every dispatched action,
//! notifies observers and executes the resulting effects. [`FeatureRoot`]
//! renders a [`FeatureView`] from the store's state.

mod dispatcher;
mod root;
mod store;

pub use dispatcher::Dispatcher;
pub use root::{FeatureRoot, FeatureView};
pub use store::Store;

pub use silon_core::{Command, EffectRunner, Feature};
