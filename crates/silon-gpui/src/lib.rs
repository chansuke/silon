//! GPUI bridge for Silon.
//!
//! [`Store`] owns a feature's state, runs `update` on every dispatched action,
//! notifies observers and executes the resulting effects. [`FeatureRoot`]
//! renders a [`FeatureView`] from the store's state.

mod dispatcher;
#[cfg(feature = "introspect")]
mod history;
#[cfg(feature = "introspect")]
mod introspect;
mod root;
mod store;

pub use dispatcher::Dispatcher;
pub use root::{FeatureRoot, FeatureView};
pub use store::Store;

#[cfg(feature = "introspect")]
pub use history::HISTORY_CAPACITY;
pub use silon_core::{Command, EffectRunner, Feature};
#[cfg(feature = "introspect")]
pub use silon_introspect::{IntrospectionRegistry, StoreHandle};
