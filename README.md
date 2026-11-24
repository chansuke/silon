# Silon

A deterministic desktop application framework on top of GPUI, using The Elm
Architecture. See `AGENTS.md` for the architecture rules and `PLAN.md` for the
milestones.

## Build and run

```sh
nix develop
cargo test -p silon-core -p counter-core
scripts/check-architecture.sh                    # crate boundaries from AGENTS.md
cargo run -p counter-app
cargo run -p counter-app --features introspect   # HTTP API on 127.0.0.1:3737
```

GPUI renders through Vulkan. On a distribution other than NixOS, the loader
from Nix does not see the host's drivers. Use nixGL, or point the loader at
Mesa's software renderer:

```sh
export VK_DRIVER_FILES=$(nix build --no-link --print-out-paths nixpkgs#mesa)/share/vulkan/icd.d/lvp_icd.x86_64.json
```

## Introspection

```sh
curl http://127.0.0.1:3737/features
curl http://127.0.0.1:3737/state/counter
curl -X POST http://127.0.0.1:3737/dispatch/counter -H 'Content-Type: application/json' -d '"Increment"'
```
