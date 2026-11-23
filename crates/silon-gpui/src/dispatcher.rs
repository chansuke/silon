use std::sync::Arc;

use gpui::{App, WeakEntity, Window};
use silon_core::{EffectRunner, Feature};

use crate::Store;

type SendFn<A> = dyn Fn(A, &mut App) + Send + Sync;

/// A cloneable handle that sends actions of type `A` into a store.
pub struct Dispatcher<A: 'static> {
    send: Arc<SendFn<A>>,
}

impl<A: 'static> Clone for Dispatcher<A> {
    fn clone(&self) -> Self {
        Self {
            send: Arc::clone(&self.send),
        }
    }
}

impl<A: 'static> Dispatcher<A> {
    pub fn dispatch(&self, action: A, cx: &mut App) {
        (self.send)(action, cx);
    }

    /// Dispatches into the store behind `weak`. Actions sent after the store
    /// has been released are dropped.
    pub fn from_store<F, R>(weak: WeakEntity<Store<F, R>>) -> Self
    where
        F: Feature<Action = A>,
        R: EffectRunner<F>,
    {
        Self {
            send: Arc::new(move |action, cx| {
                weak.update(cx, |store, cx| store.dispatch(action, cx)).ok();
            }),
        }
    }

    /// Returns a dispatcher for child actions `B`, converted with `f`.
    pub fn map<B: 'static>(&self, f: impl Fn(B) -> A + Send + Sync + 'static) -> Dispatcher<B> {
        let parent = self.clone();
        Dispatcher {
            send: Arc::new(move |action, cx| parent.dispatch(f(action), cx)),
        }
    }

    /// Returns an event listener that dispatches a copy of `action`, for use
    /// with GPUI callbacks such as `on_click`.
    pub fn on<E>(&self, action: A) -> impl Fn(&E, &mut Window, &mut App) + 'static
    where
        A: Clone,
    {
        let dispatcher = self.clone();
        move |_, _, cx| dispatcher.dispatch(action.clone(), cx)
    }
}
