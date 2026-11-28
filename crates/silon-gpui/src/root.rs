use std::marker::PhantomData;

use gpui::{App, Context, Entity, IntoElement, Render, Subscription, Window};
use silon_core::{EffectRunner, Feature};

use crate::{Dispatcher, Store};

/// Renders a feature's state. Views receive only immutable state and a
/// [`Dispatcher`]; they never touch the store directly. The returned element
/// must not borrow from the arguments, so copy what it needs out of `state`.
pub trait FeatureView<F: Feature>: 'static {
    fn view(
        state: &F::State,
        dispatcher: Dispatcher<F::Action>,
        window: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement + use<F, Self>;
}

/// The `gpui::Render` implementation that connects a [`Store`] to a
/// [`FeatureView`], re-rendering whenever the store notifies.
pub struct FeatureRoot<F: Feature, R: EffectRunner<F>, V: FeatureView<F>> {
    store: Entity<Store<F, R>>,
    dispatcher: Dispatcher<F::Action>,
    _observe_store: Subscription,
    _view: PhantomData<fn() -> V>,
}

impl<F: Feature, R: EffectRunner<F>, V: FeatureView<F>> FeatureRoot<F, R, V> {
    pub fn new(store: Entity<Store<F, R>>, cx: &mut Context<Self>) -> Self {
        let observe_store = cx.observe(&store, |_, _, cx| cx.notify());
        Self {
            dispatcher: Dispatcher::from_store(store.downgrade()),
            store,
            _observe_store: observe_store,
            _view: PhantomData,
        }
    }

    pub fn store(&self) -> &Entity<Store<F, R>> {
        &self.store
    }
}

impl<F: Feature, R: EffectRunner<F>, V: FeatureView<F>> Render for FeatureRoot<F, R, V> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Borrow the state through `update` so the view gets `cx` mutably
        // without a copy of the state. The view cannot reach the store, so
        // nothing re-enters it during the call.
        let dispatcher = self.dispatcher.clone();
        self.store
            .update(cx, |store, cx| V::view(store.state(), dispatcher, window, cx))
    }
}
