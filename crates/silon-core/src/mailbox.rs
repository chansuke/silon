//! Lets effects reach a stream that a subscription keeps open.
//!
//! A runner keeps one [`Mailboxes`]. `EffectRunner::subscribe` opens a
//! mailbox under the subscription value and reads it inside the stream;
//! `EffectRunner::run` posts to it by the same value. State names the
//! subscription and nothing else, and the mailbox closes when the store
//! drops the subscription, because the stream that owns it is dropped.

use std::collections::HashMap;
use std::hash::Hash;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll};

use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use futures_core::Stream;

struct Registry<K, M> {
    next_id: u64,
    open: HashMap<K, (u64, UnboundedSender<M>)>,
}

/// The open mailboxes of a runner, by subscription value.
pub struct Mailboxes<K, M> {
    registry: Arc<Mutex<Registry<K, M>>>,
}

impl<K, M> Clone for Mailboxes<K, M> {
    fn clone(&self) -> Self {
        Self {
            registry: self.registry.clone(),
        }
    }
}

impl<K: Eq + Hash, M> Default for Mailboxes<K, M> {
    fn default() -> Self {
        Self {
            registry: Arc::new(Mutex::new(Registry {
                next_id: 0,
                open: HashMap::new(),
            })),
        }
    }
}

fn lock<K, M>(registry: &Mutex<Registry<K, M>>) -> MutexGuard<'_, Registry<K, M>> {
    // Every critical section leaves the map consistent, so a panic elsewhere
    // does not make it unusable.
    registry.lock().unwrap_or_else(PoisonError::into_inner)
}

impl<K: Eq + Hash + Clone, M> Mailboxes<K, M> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens the mailbox of `key`, replacing one already open under it.
    pub fn open(&self, key: K) -> Mailbox<K, M> {
        let (sender, receiver) = unbounded();
        let mut registry = lock(&self.registry);
        let id = registry.next_id;
        registry.next_id += 1;
        registry.open.insert(key.clone(), (id, sender));
        Mailbox {
            key,
            id,
            receiver,
            registry: self.registry.clone(),
        }
    }

    /// Posts `message` to the mailbox of `key`. Messages arrive in the order
    /// they were posted. Gives the message back when no mailbox is open
    /// under `key`, for example because its subscription has stopped.
    ///
    /// Post from `EffectRunner::run` itself, not from the future it returns:
    /// the store calls `run` in dispatch order, but effect futures may finish
    /// in any order.
    pub fn post(&self, key: &K, message: M) -> Result<(), M> {
        match lock(&self.registry).open.get(key) {
            Some((_, sender)) => sender
                .unbounded_send(message)
                .map_err(|err| err.into_inner()),
            None => Err(message),
        }
    }
}

/// The receiving end of one mailbox. Closes the mailbox when dropped.
pub struct Mailbox<K: Eq + Hash, M> {
    key: K,
    id: u64,
    receiver: UnboundedReceiver<M>,
    registry: Arc<Mutex<Registry<K, M>>>,
}

// The receiver is Unpin and the key is never pinned in place.
impl<K: Eq + Hash, M> Unpin for Mailbox<K, M> {}

impl<K: Eq + Hash, M> Stream for Mailbox<K, M> {
    type Item = M;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<M>> {
        Pin::new(&mut self.get_mut().receiver).poll_next(cx)
    }
}

impl<K: Eq + Hash, M> Drop for Mailbox<K, M> {
    fn drop(&mut self) {
        let mut registry = lock(&self.registry);
        // A later `open` of the same key owns the entry now; leave it.
        if registry
            .open
            .get(&self.key)
            .is_some_and(|(id, _)| *id == self.id)
        {
            registry.open.remove(&self.key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(mailbox: &mut Mailbox<&'static str, u32>) -> Vec<u32> {
        let mut received = Vec::new();
        while let Ok(message) = mailbox.receiver.try_recv() {
            received.push(message);
        }
        received
    }

    #[test]
    fn posts_reach_the_open_mailbox_in_order() {
        let boxes = Mailboxes::new();
        let mut inbox = boxes.open("a");

        for n in [1, 2, 3] {
            assert_eq!(boxes.post(&"a", n), Ok(()));
        }

        assert_eq!(drain(&mut inbox), [1, 2, 3]);
    }

    #[test]
    fn a_post_to_a_key_without_a_mailbox_comes_back() {
        let boxes = Mailboxes::<&str, u32>::new();
        let _other = boxes.open("b");

        assert_eq!(boxes.post(&"a", 7), Err(7));
    }

    #[test]
    fn dropping_a_mailbox_closes_it() {
        let boxes = Mailboxes::<&str, u32>::new();
        drop(boxes.open("a"));

        assert_eq!(boxes.post(&"a", 7), Err(7));
        assert!(lock(&boxes.registry).open.is_empty());
    }

    #[test]
    fn dropping_a_replaced_mailbox_keeps_its_replacement_open() {
        let boxes = Mailboxes::new();
        let mut old = boxes.open("a");
        let mut new = boxes.open("a");

        drop(std::mem::replace(&mut old, boxes.open("other")));
        assert_eq!(boxes.post(&"a", 7), Ok(()));

        assert_eq!(drain(&mut new), [7]);
        assert_eq!(drain(&mut old), Vec::<u32>::new());
    }

    #[test]
    fn clones_share_the_open_mailboxes() {
        let boxes = Mailboxes::new();
        let mut inbox = boxes.clone().open("a");

        assert_eq!(boxes.post(&"a", 1), Ok(()));
        assert_eq!(drain(&mut inbox), [1]);
    }
}
