//! Checks that introspection commands go through the store on the GPUI side.

use gpui::{AppContext, TestAppContext};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use silon_core::{BoxFuture, Command, EffectRunner, Feature};
use silon_gpui::Store;

struct Toggle;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct ToggleState {
    on: bool,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
enum ToggleAction {
    Flip,
}

impl Feature for Toggle {
    type State = ToggleState;
    type Action = ToggleAction;
    type Effect = ();

    fn update(state: &mut ToggleState, ToggleAction::Flip: ToggleAction) -> Command<()> {
        state.on = !state.on;
        Command::none()
    }
}

struct NoEffects;

impl EffectRunner<Toggle> for NoEffects {
    fn run(&self, (): ()) -> BoxFuture<'static, Option<ToggleAction>> {
        Box::pin(async { None })
    }
}

#[gpui::test]
fn handle_reads_dispatches_and_describes_the_store(cx: &mut TestAppContext) {
    let store = cx.new(|_| Store::<Toggle, NoEffects>::new(ToggleState::default(), NoEffects));
    let handle = store.update(cx, |store, cx| store.attach_introspection("toggle", cx));
    assert_eq!(handle.name(), "toggle");

    let state = cx.background_executor.block_test(handle.get_state());
    assert_eq!(state, Ok(json!({ "on": false })));

    let state = cx
        .background_executor
        .block_test(handle.dispatch_json(json!("Flip")));
    assert_eq!(state, Ok(json!({ "on": true })));
    assert!(store.read_with(cx, |store, _| store.state().on));

    let rejected = cx
        .background_executor
        .block_test(handle.dispatch_json(json!("Explode")));
    assert!(rejected.unwrap_err().starts_with("invalid action:"));

    let schema = cx
        .background_executor
        .block_test(handle.get_schema())
        .unwrap();
    assert_eq!(schema["title"], "ToggleAction");
}

/// Adds numbers; `AddLater(n)` adds `n` through an effect.
struct Sum;

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
enum SumAction {
    Add(i64),
    AddLater(i64),
}

impl Feature for Sum {
    type State = i64;
    type Action = SumAction;
    type Effect = i64;

    fn update(total: &mut i64, action: SumAction) -> Command<i64> {
        match action {
            SumAction::Add(n) => {
                *total += n;
                Command::none()
            }
            SumAction::AddLater(n) => Command::perform(n),
        }
    }
}

struct AddRunner;

impl EffectRunner<Sum> for AddRunner {
    fn run(&self, n: i64) -> BoxFuture<'static, Option<SumAction>> {
        Box::pin(async move { Some(SumAction::Add(n)) })
    }
}

#[gpui::test]
fn history_records_ui_effect_and_handle_actions_from_attach(cx: &mut TestAppContext) {
    let store = cx.new(|_| Store::<Sum, AddRunner>::new(0, AddRunner));
    // Dispatched before attach, so part of the base rather than the history.
    store.update(cx, |store, cx| store.dispatch(SumAction::Add(100), cx));
    let handle = store.update(cx, |store, cx| store.attach_introspection("sum", cx));

    store.update(cx, |store, cx| store.dispatch(SumAction::AddLater(2), cx));
    cx.run_until_parked();
    let state = cx
        .background_executor
        .block_test(handle.dispatch_json(json!({ "Add": 3 })));
    assert_eq!(state, Ok(json!(105)));

    let history = cx.background_executor.block_test(handle.get_history());
    assert_eq!(
        history,
        Ok(json!({
            "base": 100,
            "dropped": 0,
            "entries": [
                { "seq": 0, "action": { "AddLater": 2 } },
                { "seq": 1, "action": { "Add": 2 } },
                { "seq": 2, "action": { "Add": 3 } },
            ],
        }))
    );

    let replay = |through| cx.background_executor.block_test(handle.replay(through));
    assert_eq!(
        replay(None),
        Ok(json!({ "state": 105, "through": null, "matches_current": true }))
    );
    assert_eq!(
        replay(Some(1)),
        Ok(json!({ "state": 102, "through": 1, "matches_current": false }))
    );
    assert_eq!(
        replay(Some(3)),
        Err("entry 3 has not been recorded".to_string())
    );
    // Replay leaves the live store alone.
    assert_eq!(store.read_with(cx, |store, _| *store.state()), 105);
}

#[gpui::test]
fn history_keeps_the_last_capacity_actions(cx: &mut TestAppContext) {
    let store = cx.new(|_| Store::<Sum, AddRunner>::new(0, AddRunner));
    let handle = store.update(cx, |store, cx| store.attach_introspection("sum", cx));
    let extra = 3;
    for n in 1..=(silon_gpui::HISTORY_CAPACITY as i64 + extra) {
        store.update(cx, |store, cx| store.dispatch(SumAction::Add(n), cx));
    }

    let history = cx
        .background_executor
        .block_test(handle.get_history())
        .unwrap();
    assert_eq!(history["base"], json!(1 + 2 + 3));
    assert_eq!(history["dropped"], json!(extra));
    assert_eq!(
        history["entries"].as_array().unwrap().len(),
        silon_gpui::HISTORY_CAPACITY
    );
    assert_eq!(
        history["entries"][0],
        json!({ "seq": 3, "action": { "Add": 4 } })
    );

    let replayed = cx
        .background_executor
        .block_test(handle.replay(None))
        .unwrap();
    assert_eq!(replayed["matches_current"], json!(true));
}
