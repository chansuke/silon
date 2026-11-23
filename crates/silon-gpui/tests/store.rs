//! Wiring tests for `Store` and `Dispatcher` on GPUI's deterministic test
//! executor. Business logic itself is covered headlessly in the core crates.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{AppContext, TestAppContext};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use silon_core::{BoxFuture, Command, EffectRunner, Feature};
use silon_gpui::{Dispatcher, Store};

struct Tally;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct TallyState {
    total: i32,
    log: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
enum TallyAction {
    Add(i32),
    /// Emits two effects that each come back as `Add`.
    AddLater(i32, i32),
}

#[derive(Debug)]
enum TallyEffect {
    Echo(i32),
}

impl Feature for Tally {
    type State = TallyState;
    type Action = TallyAction;
    type Effect = TallyEffect;

    fn update(state: &mut TallyState, action: TallyAction) -> Command<TallyEffect> {
        state.log.push(format!("{action:?}"));
        match action {
            TallyAction::Add(n) => {
                state.total += n;
                Command::none()
            }
            TallyAction::AddLater(a, b) => Command::batch([
                Command::perform(TallyEffect::Echo(a)),
                Command::perform(TallyEffect::Echo(b)),
            ]),
        }
    }
}

struct EchoRunner;

impl EffectRunner<Tally> for EchoRunner {
    fn run(&self, effect: TallyEffect) -> BoxFuture<'static, Option<TallyAction>> {
        let TallyEffect::Echo(n) = effect;
        Box::pin(async move { Some(TallyAction::Add(n)) })
    }
}

type TallyStore = Store<Tally, EchoRunner>;

#[gpui::test]
fn dispatch_updates_state_and_notifies_observers(cx: &mut TestAppContext) {
    let store = cx.new(|_| TallyStore::new(TallyState::default(), EchoRunner));
    let notified = Rc::new(Cell::new(0));
    let _observer = cx.update(|cx| {
        let notified = notified.clone();
        cx.observe(&store, move |_, _| notified.set(notified.get() + 1))
    });

    store.update(cx, |store, cx| store.dispatch(TallyAction::Add(3), cx));
    cx.run_until_parked();

    assert_eq!(store.read_with(cx, |store, _| store.state().total), 3);
    assert_eq!(notified.get(), 1);
}

#[gpui::test]
fn effect_results_are_dispatched_back_into_the_store(cx: &mut TestAppContext) {
    let store = cx.new(|_| TallyStore::new(TallyState::default(), EchoRunner));

    store.update(cx, |store, cx| {
        store.dispatch(TallyAction::AddLater(2, 5), cx)
    });
    assert_eq!(store.read_with(cx, |store, _| store.state().total), 0);

    cx.run_until_parked();

    let state = store.read_with(cx, |store, _| store.state().clone());
    assert_eq!(state.total, 7);
    assert_eq!(state.log.len(), 3);
    assert_eq!(state.log[0], "AddLater(2, 5)");
}

#[gpui::test]
fn dispatcher_map_converts_child_actions(cx: &mut TestAppContext) {
    let store = cx.new(|_| TallyStore::new(TallyState::default(), EchoRunner));
    let dispatcher = Dispatcher::from_store(store.downgrade());
    let doubled: Dispatcher<i32> = dispatcher.map(|n| TallyAction::Add(n * 2));

    cx.update(|cx| {
        doubled.dispatch(4, cx);
        dispatcher.dispatch(TallyAction::Add(1), cx);
    });

    assert_eq!(store.read_with(cx, |store, _| store.state().total), 9);
}

#[gpui::test]
fn dispatch_after_store_release_is_ignored(cx: &mut TestAppContext) {
    let store = cx.new(|_| TallyStore::new(TallyState::default(), EchoRunner));
    let dispatcher = Dispatcher::from_store(store.downgrade());
    drop(store);
    cx.run_until_parked();

    cx.update(|cx| dispatcher.dispatch(TallyAction::Add(1), cx));
}
