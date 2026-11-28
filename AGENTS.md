## 1. Overview
**Silon** is a deterministic, agent-friendly desktop application framework built on top of **GPUI** using **The Elm Architecture (TEA)**.
Its primary goal is to isolate pure business logic from GPU rendering and side effects, enabling fast headless verification and structured runtime introspection for AI coding agents.

---

## 2. Workspace Structure & Dependency Rules (Strict DAG)

Every crate MUST strictly obey the following dependency direction. **Circular dependencies and upward imports are strictly forbidden.**

```text
silon-core (NO gpui, NO tokio, NO IO)
   ▲
   ├── silon-introspect (tokio, axum, serde_json; NO gpui)
   └── silon-gpui (gpui; optional dep on silon-introspect via `introspect` feature)
          ▲
          └── silon-ui (gpui, RenderOnce primitives, optional gpui-component adapters)
```

### Application Crate Split Rule
Every application built with Silon (including `examples/*`) MUST be split into two crates:
1. `<app>-core`: Depends ONLY on `silon-core` (and pure domain crates). **MUST NOT depend on `gpui` or `silon-gpui`.**
2. `<app>-app`: Depends on `<app>-core`, `silon-gpui`, and `silon-ui`. Contains `FeatureView` implementations, `EffectRunner` implementations, and the `main()` entry point.

---

## 3. Core Architecture Rules (TEA Loop)

All application logic MUST follow the unidirectional data flow: `Action -> update -> (State, Command<Effect>) -> View`.

### 3.1 `Feature` Trait Requirements (`*-core`)
- **`State`**: MUST be a pure, serializable data structure (`Clone + Debug + Serialize + DeserializeOwned + 'static`).
  - **MUST NOT** contain `gpui::Entity`, `gpui::Context`, `FocusHandle`, channels, or any non-serializable runtime handles.
- **`Action`**: MUST be an `enum` implementing `Debug + Serialize + DeserializeOwned + JsonSchema + Send + 'static`.
  - Every user interaction and async completion MUST be represented as an `Action` variant.
- **`Effect`**: MUST be a pure data `enum` describing *what* side effect to perform (e.g., `FetchData { url: String }`), NEVER executing IO directly.
- **`Subscription`**: MUST be a pure data value (`Clone + Eq + Hash`) naming a stream of actions (e.g., `Events { socket: PathBuf, attempt: u32 }`). Two equal values are the same stream.
- **`subscriptions(state: &State) -> Vec<Subscription>`**: MUST be pure. The `Store` runs exactly the subscriptions this returns after every dispatch: it opens new ones and drops the ones that disappear. To reconnect after a stream ends, change the value (e.g., bump `attempt`) instead of reopening it from the runner.
- **`update(state: &mut State, action: Action) -> Command<Effect>`**:
  - MUST be a **100% pure, deterministic function**.
  - **MUST NOT** perform IO, spawn threads/tasks, read system time directly (`SystemTime::now()`), or generate unseeded randomness. Pass time/randomness via `Action` or `Effect`.

### 3.2 Side Effects (`EffectRunner`)
- All async IO (HTTP, DB, filesystem, timers) MUST be implemented in `EffectRunner<F>` inside `<app>-app`.
- Long-lived streams (sockets, file watchers, intervals) MUST be opened in `EffectRunner::subscribe`, never kept in `State` or spawned from `run`.
- To write to an open stream (keys to a terminal, frames to a socket), open a `silon_core::Mailbox` in `subscribe` under the subscription value and `post` to it from `run` by the same value. Post in `run` itself, not in the future it returns, so messages keep dispatch order.
- Protocol logic (framing, parsing, handshakes) MUST be a `silon_core::Protocol` in `<app>-core`, tested by feeding it bytes and reading the `Exchange`. The runner only connects and hands the connection to `silon_core::converse`.
- Waiting (retry delays, timeouts) MUST be an `Effect` whose completion is an `Action`. A runner MUST NOT sleep on its own, so the state always shows what it waits for.
- `Store<F, R>` is the sole owner of `cx.spawn()` and `cx.notify()`. Application code **MUST NOT** call `cx.notify()` manually.

---

## 4. UI & Component Design Rules (`silon-ui` / `<app>-app`)

1. **`RenderOnce` by Default**:
   - Views MUST implement `FeatureView<F>` or `gpui::RenderOnce`.
   - Views receive ONLY `&F::State` (immutable) and `Dispatcher<F::Action>`.
   - Views **MUST NOT** hold mutable references to `Store<F, R>` or mutate state directly.
2. **Hybrid Component Boundary**:
   - **Stateless primitives** (`Stack`, `Row`, `Panel`, `Button`, `Text`, `Badge`): Build/use from `silon-ui` with semantic design tokens. Avoid raw hex colors or ad-hoc pixel layouts in app views.
   - **Complex stateful widgets** (IME-aware `TextInput`, `CodeEditor`, `VirtualTable`, `DatePicker`): Wrap `gpui-component` inside dedicated adapters in `silon-ui`. Keep ephemeral UI state (cursor, IME composition) local to the widget and emit `Action` via `Dispatcher` only on meaningful state transitions (e.g., `on_change`, `on_submit`, `on_blur`).
3. **Composition**:
   - For nested features, transform child actions to parent actions using `Dispatcher::map` rather than passing parent stores down.

---

## 5. Introspection & Agent Verification Protocol

Silon provides an opt-in introspection bridge behind the `introspect` Cargo feature flag (`silon-gpui/introspect`).

- **Zero Release Overhead**: All introspection server code MUST be gated behind `#[cfg(feature = "introspect")]`.
- **Schema Completeness**: Whenever you add or modify an `Action` variant, ensure `#[derive(JsonSchema)]` compiles cleanly so external agents can discover valid payloads via `GetSchema`.
- **Runtime Verification Flow** (when testing GUI behavior):
  1. Start the app with `--features introspect`.
  2. Query `GET /state/<feature>` to inspect current JSON state.
  3. Send `POST /dispatch/<feature>` with a valid `Action` JSON payload to trigger deterministic transitions and verify the returned state.

---

## 6. Testing & Verification Policy

Before concluding any task, agents MUST run and pass the following checks in order:

1. **Headless Core Tests (Mandatory, Fast Loop)**:
   ```bash
   cargo test -p silon-core
   cargo test -p <app>-core
   ```
   - Every new `Feature` or `Action` branch MUST have a corresponding headless state-transition test using `silon_core::TestHarness<F>`.
   - Assert both the resulting `State` and the exact `emitted_effects`.
   - For a feature with subscriptions, also assert `TestHarness::subscriptions()` after each transition that should open or close a stream.
2. **Workspace Typecheck & Lint (Mandatory)**:
   ```bash
   cargo check --workspace --all-targets
   cargo check --workspace --all-features
   cargo clippy --workspace --all-targets -- -D warnings
   ```
3. **Architecture Boundary Check**:
   - Verify `cargo tree -p <app>-core` contains **zero** occurrences of `gpui` or `tokio`.

---

## 7. Forbidden Anti-Patterns (Strict)

- **DO NOT** import `gpui` anywhere inside `silon-core`, `silon-introspect`, or `<app>-core`.
- **DO NOT** implement `gpui::Render` manually for application screens; always use `FeatureView<F>` + `FeatureRoot`.
- **DO NOT** bypass `Store::dispatch` to mutate `State`.
- **DO NOT** add `#[allow(dead_code)]` or silence `clippy` warnings to bypass lint failures.
- **DO NOT** write GUI-dependent tests for business logic that can be tested via `TestHarness<F>`.
