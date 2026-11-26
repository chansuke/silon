# Silon

A deterministic desktop application framework on top of GPUI, using The Elm
Architecture. See `AGENTS.md` for the architecture rules and `PLAN.md` for the
milestones.

## Build and run

```sh
nix develop
cargo test -p silon-core -p counter-core -p todo-core
cargo test --workspace --all-features            # GPUI, HTTP and MCP tests too
scripts/check-architecture.sh                    # crate boundaries from AGENTS.md
cargo run -p counter-app
cargo run -p counter-app --features introspect   # HTTP API on 127.0.0.1:3737
cargo run -p todo-app --features introspect      # text input, list, checkboxes
```

GPUI renders through Vulkan. On a distribution other than NixOS, the loader
from Nix does not see the host's drivers. Use nixGL, or point the loader at
Mesa's software renderer:

```sh
export VK_DRIVER_FILES=$(nix build --no-link --print-out-paths nixpkgs#mesa)/share/vulkan/icd.d/lvp_icd.x86_64.json
```

## Verification

Besides the tests and the checks from `AGENTS.md`, CI runs:

| Workflow | What it checks | Run it locally |
| --- | --- | --- |
| `lint.yml` | dependency licenses and advisories, spelling, workflows, shell scripts, Nix formatting | `cargo deny --locked check`, `typos`, `actionlint`, `shellcheck scripts/*.sh`, `nixfmt --check flake.nix` |
| `kani.yml` | proofs over all inputs within bounds: `History` replay/folding/restarts, the counter and todo `update` | `cargo kani` in `crates/silon-core`, `examples/*/*-core` (Kani 0.68, outside the dev shell) |
| `quint.yml` | `spec/StoreProtocol.qnt`: the store, effects finishing in any order, agents and the history | `nix develop .#spec`, then the commands in `quint.yml` |
| `mutants.yml` | every mutant of the logic crates makes a test fail (changed lines on pull requests, everything weekly) | `cargo mutants -p silon-core` |
| `zizmor.yml` | workflow security | `nix shell --inputs-from . nixpkgs#zizmor -c zizmor .` |
| `advisories.yml` | new advisories, daily | `cargo deny --locked check advisories` |

## Crates

- `silon-core`: `Feature`, `Command`, `EffectRunner`, `TestHarness`. No GPUI, no IO.
- `silon-gpui`: `Store`, `Dispatcher`, `FeatureView`, `FeatureRoot`.
- `silon-introspect`: the HTTP API, behind the `introspect` feature of `silon-gpui`.
- `silon-mcp`: an MCP server over stdio that forwards to the HTTP API.
- `silon-ui`: theme tokens and primitives (`Stack`, `Row`, `Panel`, `Text`,
  `Button`, `Badge`, `Checkbox`). The `components` feature adds `TextInput`,
  built on gpui-component 0.5.1, the last release for crates.io gpui 0.2.

## Introspection

```sh
curl http://127.0.0.1:3737/features
curl http://127.0.0.1:3737/state/counter
curl -X POST http://127.0.0.1:3737/dispatch/counter -H 'Content-Type: application/json' -d '"Increment"'
curl -X POST http://127.0.0.1:3737/dispatch/todo -H 'Content-Type: application/json' -d '{"DraftChanged":"milk"}'
curl http://127.0.0.1:3737/history/todo            # the last 256 actions
curl 'http://127.0.0.1:3737/replay/todo?through=3'  # the state after entry 3
```

The history starts when the app attaches introspection, and holds actions from
the UI, from effects and from the API. Replay recomputes states with `update`
only, so it never changes the running app.

## MCP

`silon-mcp` exposes the same API as the tools `list_features`, `get_state`,
`dispatch_action`, `get_history` and `replay`. Start an app with
`--features introspect`, then register the server with your agent, for
example with Claude Code:

```sh
cargo build -p silon-mcp
claude mcp add silon -- "$PWD/target/debug/silon-mcp"   # --url URL for another address
```
