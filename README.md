<p align="center">
  <img src="assets/banner.png" alt="Silon: deterministic desktop apps on GPUI, built with The Elm Architecture" width="800">
</p>

[![CI](https://img.shields.io/github/actions/workflow/status/chansuke/silon/ci.yml?branch=main&label=CI&labelColor=3A3F48)](https://github.com/chansuke/silon/actions/workflows/ci.yml)
[![License: 0BSD](https://img.shields.io/badge/license-0BSD-3B4CE0?labelColor=3A3F48)](#license)
[![Pull requests: not accepted](https://img.shields.io/badge/pull%20requests-not%20accepted-5B616E?labelColor=3A3F48)](CONTRIBUTING.md)

A deterministic desktop application framework on top of
[GPUI](https://www.gpui.rs/), using The Elm Architecture. Business logic is a
pure `update` function that tests run without a window, and a running app can
expose its state and actions to coding agents over HTTP or MCP.

## Crates

| Crate | What it holds |
| --- | --- |
| `silon-core` | `Feature`, `Command`, `EffectRunner`, `TestHarness`. No GPUI, no IO. |
| `silon-gpui` | `Store`, `Dispatcher`, `FeatureView`, `FeatureRoot`. |
| `silon-introspect` | The HTTP API, behind the `introspect` feature of `silon-gpui`. |
| `silon-mcp` | An MCP server over stdio that forwards to the HTTP API. |
| `silon-ui` | Theme tokens and primitives. The `components` feature adds `TextInput`. |

The architecture rules are in [AGENTS.md](AGENTS.md).

## Build and run

```sh
nix develop
cargo test --workspace --all-features
cargo run -p counter-app --features introspect   # HTTP API on 127.0.0.1:3737
```

`herdboard-app` shows the terminals of a running
[herdr-core](https://github.com/herdrdev/herdr-core). Click a live terminal's
screen to type into it.
Pass the socket path as the first argument, or it uses `HC_SOCKET` or the default
path of the `HC_SESSION` session.

`terminal-app` is a terminal: your shell in a PTY, with
[libghostty-vt](https://ghostty.org) as the VT emulator and Silon for the
window. On Linux it builds libghostty-vt with Zig 0.17, which the dev shell
provides.

On a Linux distribution other than NixOS, GPUI cannot find the host's Vulkan
drivers from the Nix shell. Use nixGL, or Mesa's software renderer:

```sh
export VK_DRIVER_FILES=$(nix build --no-link --print-out-paths nixpkgs#mesa)/share/vulkan/icd.d/lvp_icd.x86_64.json
```

## Introspection

```sh
curl http://127.0.0.1:3737/state/counter
curl -X POST http://127.0.0.1:3737/dispatch/counter -H 'Content-Type: application/json' -d '"Increment"'
curl 'http://127.0.0.1:3737/replay/counter?through=3'
```

To use it from an agent, register `silon-mcp`, for example with Claude Code:

```sh
cargo build -p silon-mcp
claude mcp add silon -- "$PWD/target/debug/silon-mcp"
```

## Contributing

Silon does not accept pull requests. Bug reports are welcome as issues. See
[CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md).

## License

[0BSD](LICENSE). You can use, copy, modify and distribute Silon without
attribution.
