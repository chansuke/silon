//! How `Store` runs the subscriptions a feature asks for, on GPUI's
//! deterministic test executor. spec/SubscriptionProtocol.qnt models the same
//! protocol.

use std::sync::{Arc, Mutex};

use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui::{AppContext, Entity, TestAppContext};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use silon_core::{BoxFuture, BoxStream, Command, EffectRunner, Feature};
use silon_gpui::Store;

/// Listens to a stream that reconnects: `Ended` moves to the next attempt,
/// which is a new subscription.
struct Feed;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct FeedState {
    listening: bool,
    attempt: u32,
    received: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
enum FeedAction {
    Listen,
    Stop,
    Item(u32, String),
    Ended(u32),
}

impl Feature for Feed {
    type State = FeedState;
    type Action = FeedAction;
    type Effect = std::convert::Infallible;
    type Subscription = u32;

    fn update(state: &mut FeedState, action: FeedAction) -> Command<Self::Effect> {
        match action {
            FeedAction::Listen => state.listening = true,
            FeedAction::Stop => state.listening = false,
            FeedAction::Item(attempt, text) => state.received.push(format!("{attempt}:{text}")),
            FeedAction::Ended(attempt) => {
                if attempt == state.attempt {
                    state.attempt += 1;
                }
            }
        }
        Command::none()
    }

    fn subscriptions(state: &FeedState) -> Vec<u32> {
        if state.listening {
            vec![state.attempt]
        } else {
            Vec::new()
        }
    }
}

/// The sending ends of the streams the store opened, in opening order.
type Opened = Arc<Mutex<Vec<(u32, UnboundedSender<FeedAction>)>>>;

/// Opens each stream as a channel whose sending end the test holds.
struct ChannelRunner(Opened);

impl EffectRunner<Feed> for ChannelRunner {
    fn run(&self, effect: std::convert::Infallible) -> BoxFuture<'static, Option<FeedAction>> {
        match effect {}
    }

    fn subscribe(&self, attempt: u32) -> BoxStream<'static, FeedAction> {
        let (sender, receiver) = unbounded();
        self.0.lock().unwrap().push((attempt, sender));
        Box::pin(receiver)
    }
}

fn new_store(
    state: FeedState,
    cx: &mut TestAppContext,
) -> (Entity<Store<Feed, ChannelRunner>>, Opened) {
    let opened = Opened::default();
    let runner = ChannelRunner(opened.clone());
    let store = cx.new(|cx| Store::new(state, runner, cx));
    (store, opened)
}

fn dispatch(
    store: &Entity<Store<Feed, ChannelRunner>>,
    action: FeedAction,
    cx: &mut TestAppContext,
) {
    store.update(cx, |store, cx| store.dispatch(action, cx));
    cx.run_until_parked();
}

fn send(opened: &Opened, index: usize, action: FeedAction) {
    opened.lock().unwrap()[index]
        .1
        .unbounded_send(action)
        .unwrap();
}

fn opened_keys(opened: &Opened) -> Vec<u32> {
    opened.lock().unwrap().iter().map(|(key, _)| *key).collect()
}

fn is_closed(opened: &Opened, index: usize) -> bool {
    opened.lock().unwrap()[index].1.is_closed()
}

fn received(store: &Entity<Store<Feed, ChannelRunner>>, cx: &mut TestAppContext) -> Vec<String> {
    store.read_with(cx, |store, _| store.state().received.clone())
}

#[gpui::test]
fn the_initial_state_starts_its_subscriptions(cx: &mut TestAppContext) {
    let listening = FeedState {
        listening: true,
        ..FeedState::default()
    };
    let (_store, opened) = new_store(listening, cx);

    assert_eq!(opened_keys(&opened), vec![0]);
}

#[gpui::test]
fn items_reach_the_state_in_stream_order(cx: &mut TestAppContext) {
    let (store, opened) = new_store(FeedState::default(), cx);
    dispatch(&store, FeedAction::Listen, cx);

    for text in ["a", "b", "c"] {
        send(&opened, 0, FeedAction::Item(0, text.into()));
    }
    cx.run_until_parked();

    assert_eq!(received(&store, cx), ["0:a", "0:b", "0:c"]);
}

#[gpui::test]
fn a_subscription_the_state_drops_closes_its_stream(cx: &mut TestAppContext) {
    let (store, opened) = new_store(FeedState::default(), cx);
    dispatch(&store, FeedAction::Listen, cx);
    assert!(!is_closed(&opened, 0));

    dispatch(&store, FeedAction::Stop, cx);

    assert!(is_closed(&opened, 0));
}

#[gpui::test]
fn items_after_the_one_that_stops_a_subscription_are_dropped(cx: &mut TestAppContext) {
    let (store, opened) = new_store(FeedState::default(), cx);
    dispatch(&store, FeedAction::Listen, cx);

    // All three are ready at once; `Ended(0)` moves the state to attempt 1.
    send(&opened, 0, FeedAction::Item(0, "a".into()));
    send(&opened, 0, FeedAction::Ended(0));
    send(&opened, 0, FeedAction::Item(0, "late".into()));
    cx.run_until_parked();

    assert_eq!(received(&store, cx), ["0:a"]);
    assert_eq!(opened_keys(&opened), vec![0, 1]);
    assert!(is_closed(&opened, 0));
}

#[gpui::test]
fn an_ended_stream_is_not_reopened_while_still_asked_for(cx: &mut TestAppContext) {
    let (store, opened) = new_store(FeedState::default(), cx);
    dispatch(&store, FeedAction::Listen, cx);

    opened.lock().unwrap()[0].1.close_channel();
    cx.run_until_parked();
    dispatch(&store, FeedAction::Item(9, "from an agent".into()), cx);

    assert_eq!(opened_keys(&opened), vec![0]);
}

#[gpui::test]
fn asking_for_a_stopped_key_again_opens_a_fresh_stream(cx: &mut TestAppContext) {
    let (store, opened) = new_store(FeedState::default(), cx);
    dispatch(&store, FeedAction::Listen, cx);
    dispatch(&store, FeedAction::Stop, cx);
    dispatch(&store, FeedAction::Listen, cx);

    send(&opened, 1, FeedAction::Item(0, "fresh".into()));
    cx.run_until_parked();

    assert_eq!(opened_keys(&opened), vec![0, 0]);
    assert_eq!(received(&store, cx), ["0:fresh"]);
}

#[gpui::test]
fn releasing_the_store_closes_its_streams(cx: &mut TestAppContext) {
    let (store, opened) = new_store(FeedState::default(), cx);
    dispatch(&store, FeedAction::Listen, cx);

    drop(store);
    // GPUI releases the entity when it next flushes effects.
    cx.update(|_| {});
    cx.run_until_parked();

    assert!(is_closed(&opened, 0));
}
