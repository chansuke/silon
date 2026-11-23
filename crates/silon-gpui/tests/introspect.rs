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
