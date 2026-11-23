use std::collections::BTreeMap;
use std::sync::{Arc, PoisonError, RwLock};

use crate::StoreHandle;

/// The stores visible to introspection, keyed by name.
#[derive(Clone, Default)]
pub struct IntrospectionRegistry {
    handles: Arc<RwLock<BTreeMap<String, StoreHandle>>>,
}

impl IntrospectionRegistry {
    /// Adds `handle`, replacing any handle registered under the same name.
    pub fn register(&self, handle: StoreHandle) {
        self.handles
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(handle.name().to_owned(), handle);
    }

    pub fn get(&self, name: &str) -> Option<StoreHandle> {
        self.handles
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(name)
            .cloned()
    }

    /// All registered handles, sorted by name.
    pub fn handles(&self) -> Vec<StoreHandle> {
        self.handles
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .cloned()
            .collect()
    }
}
