This document defines the exact API contracts, GPUI API generation constraints, and phased vertical-slice milestones for bootstrapping the **Silon** framework.
Always follow the architectural rules in `AGENTS.md`.

---

## 1. Environment & GPUI API Generation Constraints (CRITICAL)

- **Rust Edition**: `2024` (Workspace-wide)
- **GPUI Dependency**: Use the modern `gpui` API (`gpui = "0.2"` or latest `crates.io` release / pinned Zed git revision).
- **Modern GPUI API ONLY**:
  - **USE**: `Entity<T>`, `WeakEntity<T>`, `Context<T>`, `App`, `Window`, `Render`, `RenderOnce`, `IntoElement`, `cx.new(...)`, `cx.spawn(async move |this, cx| ...)`.
  - **DO NOT USE (Deprecated/Removed APIs)**: `Model<T>`, `View<T>`, `ViewContext<T>`, `WindowContext`, `AppContext`, `cx.new_model(...)`, `cx.new_view(...)`.
- If `gpui` API signatures differ slightly in the resolved crate version, inspect the installed crate source / `cargo doc` first rather than guessing.

---

## 2. Core API Contracts (Exact Trait Signatures)

Agents MUST implement these exact public interfaces across crate boundaries.

### 2.1 `silon-core` (`crates/silon-core/src/lib.rs`)

```rust
use schemars::JsonSchema;
use serde::{de::DeserializeOwned, Serialize};
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output T> + Send + 'a>>;

#[derive(Debug, Default)]
pub enum Command<E> {
    #[default]
    None,
    Perform(E),
    Batch(Vec<Command<E>>),
}

impl<E> Command<E> {
    pub fn none() -> Self { Self::None }
    pub fn perform(effect: E) -> Self { Self::Perform(effect) }
    pub fn batch(cmds: impl IntoIterator<Item = Command<E>>) -> Self {
        Self::Batch(cmds.into_iter().collect())
    }
}

pub trait Feature: 'static {
    type State: Clone + Debug + Serialize + DeserializeOwned + 'static;
    type Action: Debug + Serialize + DeserializeOwned + JsonSchema + Send + 'static;
    type Effect: Debug + Send + 'static;

    fn update(state: &mut Self::State, action: Self::Action) -> Command<Self::Effect>;
}

pub trait EffectRunner<F: Feature>: 'static {
    fn run(&self, effect: F::Effect) -> BoxFuture<'static, Option<F::Action>>;
}
```

### 2.2 `silon-core` Test Harness (`crates/silon-core/src/harness.rs`)

```rust
pub struct TestHarness<F: Feature> {
    pub state: F::State,
    pub emitted_effects: Vec<F::Effect>,
}

impl<F: Feature> TestHarness<F> {
    pub fn new(initial: F::State) -> Self;
    pub fn send(&mut self, action: F::Action) -> &mut Self;
    pub fn take_effects(&mut self) -> Vec<F::Effect>;
}
```

### 2.3 `silon-gpui` (`crates/silon-gpui/src/lib.rs`)

```rust
pub struct Store<F: Feature, R: EffectRunner<F>> { /* state, runner */ }

impl<F: Feature, R: EffectRunner<F>> Store<F, R> {
    pub fn new(initial_state: F::State, runner: R) -> Self;
    pub fn state(&self) -> &F::State;
    pub fn dispatch(&mut self, action: F::Action, cx: &mut gpui::Context<Self>);
}

pub struct Dispatcher<A: 'static> { /* Arc<dyn Fn(A, &mut gpui::App) + Send + Sync> */ }

impl<A: 'static> Clone for Dispatcher<A> { ... }
impl<A: 'static> Dispatcher<A> {
    pub fn dispatch(&self, action: A, cx: &mut gpui::App);
    pub fn from_store<F, R>(weak: gpui::WeakEntity<Store<F, R>>) -> Self
    where
        F: Feature<Action A>,
        R: EffectRunner<F>;
    pub fn map<B: 'static>(&self, f: impl Fn(B) -> A + Send + Sync + 'static) -> Dispatcher<B>;
}

pub trait FeatureView<F: Feature>: 'static {
    fn view(
        state: &F::State,
        dispatcher: Dispatcher<F::Action>,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) -> impl gpui::IntoElement;
}

/// Wrapper implementing `gpui::Render` that connects `Entity<Store<F, R>>` to `FeatureView<F>`
pub struct FeatureRoot<F: Feature, R: EffectRunner<F>, V: FeatureView<F>> { ... }
```

### 2.4 `silon-introspect` (`crates/silon-introspect/src/lib.rs`)

```rust
pub enum IntrospectionCommand {
    GetState {
        reply: tokio::sync::oneshot::Sender<Result<serde_json::Value, String>>,
    },
    DispatchAction {
        action_json: serde_json::Value,
        reply: tokio::sync::oneshot::Sender<Result<serde_json::Value, String>>,
    },
    GetSchema {
        reply: tokio::sync::oneshot::Sender<serde_json::Value>,
    },
}

#[derive(Clone)]
pub struct StoreHandle { /* name, tx */ }

impl StoreHandle {
    pub fn new(name: impl Into<String>, tx: tokio::sync::mpsc::UnboundedSender<IntrospectionCommand>) -> Self;
    pub fn name(&self) -> &str;
    pub async fn get_state(&self) -> Result<serde_json::Value, String>;
    pub async fn dispatch_json(&self, action_json: serde_json::Value) -> Result<serde_json::Value, String>;
    pub async fn get_schema(&self) -> Result<serde_json::Value, String>;
}

#[derive(Clone, Default)]
pub struct IntrospectionRegistry { /* Arc<RwLock<HashMap<String, StoreHandle>>> */ }
```

---

## 3. Phased Implementation Milestones

Execute one phase at a time. **Do NOT proceed to the next phase until all acceptance checks of the current phase pass.**

### Phase 1: Workspace Bootstrap & Pure Core (`silon-core` + `counter-core`)
**Scope:**
1. Initialize root `Cargo.toml` workspace with `crates/silon-core`, `crates/silon-introspect`, `crates/silon-gpui`, `examples/counter/counter-core`, and `examples/counter/counter-app`.
2. Implement `silon-core` (`Feature`, `Command`, `EffectRunner`, `TestHarness`).
3. Implement `examples/counter/counter-core`:
   - `CounterState { count: i32, loading: bool }`
   - `CounterAction { Increment, Decrement, RequestAsyncAdd(i32), AsyncAddCompleted(i32) }`
   - `CounterEffect { DelayedAdd { delta: i32, delay_ms: u64 } }`
   - `CounterFeature` implementing `Feature`
   - Unit tests using `TestHarness<CounterFeature>` covering sync increments, decrements, and async effect emission/completion.

**Acceptance Checks:**
- `cargo test -p silon-core` passes.
- `cargo test -p counter-core` passes.
- `cargo tree -p counter-core` contains NO `gpui` and NO `tokio`.

---

### Phase 2: GPUI Bridge & Working Window (`silon-gpui` + `counter-app`)
**Scope:**
1. Implement `silon-gpui`:
   - `Store<F, R>` with automatic `cx.notify()` and `cx.spawn()` background effect execution.
   - `Dispatcher<A>` including `from_store` and `map`.
   - `FeatureView<F>` and `FeatureRoot<F, R, V>` implementing `gpui::Render`.
2. Implement `examples/counter/counter-app`:
   - `CounterEffectRunner` (executes `DelayedAdd` with a real async sleep and returns `Some(CounterAction::AsyncAddCompleted(delta))`).
   - `CounterView` implementing `FeatureView<CounterFeature>` with buttons for `-`, `+`, and `+5 (Async)`.
   - `main.rs` launching a GPUI window displaying `FeatureRoot`.

**Acceptance Checks:**
- `cargo check --workspace --all-targets` passes without warnings.
- `cargo clippy --workspace --all-targets -- -D warnings` passes.
- `cargo run -p counter-app` opens a window and updates the counter on button clicks (both sync and async).

---

### Phase 3: Introspection Bridge & Local HTTP Server (`silon-introspect` + feature flag)
**Scope:**
1. Implement `silon-introspect`:
   - `IntrospectionCommand`, `StoreHandle`, `IntrospectionRegistry`.
   - Local HTTP server (`axum`, default bind `127.0.0.1:3737`) exposing:
     - `GET /features` -> returns list of registered feature names and their `Action` JSON Schemas.
     - `GET /state/:name` -> returns current `State` JSON.
     - `POST /dispatch/:name` -> accepts `Action` JSON body, dispatches to GPUI main thread, returns updated `State` JSON.
2. Add `#[cfg(feature = "introspect")]` integration to `silon-gpui`:
   - `Store::attach_introspection(&self, name: impl Into<String>, cx: &mut Context<Self>) -> StoreHandle`.
   - Use `schemars::schema_for!(F::Action)` for `GetSchema`.
3. Wire `introspect` feature into `examples/counter/counter-app` so running with `--features introspect` starts the background HTTP server on `127.0.0.1:3737`.

**Acceptance Checks:**
- `cargo check --workspace` (without `introspect`) compiles cleanly and does NOT link `axum`.
- `cargo check --workspace --all-features` and `cargo clippy --workspace --all-features -- -D warnings` pass.
- Running `cargo run -p counter-app --features introspect` allows:
  - `curl http://127.0.0.1:3737/features` -> returns `counter` schema.
  - `curl http://127.0.0.1:3737/state/counter` -> returns `{"count":0,"loading":false}`.
  - `curl -X POST http://127.0.0.1:3737/dispatch/counter -H 'Content-Type: application/json' -d '"Increment"'` -> returns `{"count":1,"loading":false}` and updates the live GPUI window.

---

### Phase 4: Design Tokens, Primitives & Stateful Widget Adapters (`silon-ui`)
**Scope:**
1. Add `crates/silon-ui` (depends on `gpui` and `silon-gpui`; NO `silon-introspect`):
   - `Theme` (a `gpui::Global`) with semantic color tokens, a spacing scale (`Space`), radii and text sizes. `silon_ui::init(cx)` installs the default dark theme; `Theme::of(cx)` reads it.
   - Stateless `RenderOnce` primitives built only from tokens: `Stack` (vertical), `Row` (horizontal), `Panel`, `Text`, `Button`, `Badge`, `Checkbox`. `Stack::screen()` is the window-level container that paints the background.
   - `Button` and `Checkbox` dispatch through `Dispatcher` (`on_press(&dispatcher, action)`), so views never write raw event closures for simple actions.
2. Optional `components` feature wrapping `gpui-component = "=0.5.1"` (the last release on crates.io `gpui` 0.2.x; 0.6+ depends on the `gpui-pre` fork):
   - `TextInput` adapter: IME-aware single-line input. Cursor, selection and IME composition stay inside the widget (`Window::use_keyed_state`). It takes the current value from `State` and emits `Dispatcher<String>` callbacks only for `on_change`, `on_submit` (Enter) and `on_blur`. Values pushed from `State` (e.g. clearing after submit) do not echo back as `on_change`.
   - `components::init(cx)` and `components::window_root(view, window, cx)` (gpui-component widgets require `gpui_component::Root` as the window root).
3. Migrate `examples/counter/counter-app` to `silon-ui` primitives and remove its ad-hoc colors.
4. Add `examples/todo/{todo-core,todo-app}`:
   - `TodoState { draft: String, items: Vec<TodoItem>, next_id: u64 }`, `TodoItem { id: u64, title: String, done: bool }`.
   - `TodoAction { DraftChanged(String), Submit, Toggle(u64), Remove(u64), ClearCompleted }`; `Submit` trims the draft, ignores blank drafts, appends an item and clears the draft. No effects (`Effect = std::convert::Infallible`), run with `silon_core::NoEffects`, the runner for every feature without effects.
   - `todo-app` uses `TextInput` and the primitives, and supports `--features introspect` (feature name `todo`).

**Acceptance Checks:**
- All checks in `AGENTS.md` section 6 pass, and `scripts/check-architecture.sh` also covers `todo-core`, `silon-ui` (no `silon-introspect`) and forbids raw colors (`rgb(`, `rgba(`, `hsla(`) in `examples/*/*-app/src`.
- `cargo test -p todo-core` covers every `TodoAction` branch with `TestHarness`.
- `cargo run -p counter-app` looks and behaves as in Phase 2.
- `cargo run -p todo-app --features introspect`: typing and Enter adds an item and clears the input; `POST /dispatch/todo` with `{"DraftChanged":"..."}` updates the visible input text; `Toggle`/`Remove` via clicks and via HTTP both update the window.

---

## 4. Out of Scope for MVP (Do NOT Implement Yet)
- MCP (Model Context Protocol) stdio/SSE server wrapper (defer until HTTP endpoints in Phase 3 are verified).
- Action history ring buffer / time-travel replay.
