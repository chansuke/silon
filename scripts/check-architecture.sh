#!/usr/bin/env bash
# Checks the crate boundaries from AGENTS.md: pure crates link neither gpui
# nor tokio, silon-introspect links no gpui, and an app built without the
# `introspect` feature links no axum.
set -euo pipefail

status=0

forbid() {
  local package=$1 pattern=$2
  local extra_args=("${@:3}")
  local found
  found=$(cargo tree --locked -p "$package" -e normal --prefix none "${extra_args[@]}" |
    grep -E "^($pattern) v" | sort -u || true)
  if [[ -n $found ]]; then
    echo "error: $package depends on forbidden crates:" >&2
    while read -r crate; do echo "  $crate" >&2; done <<<"$found"
    status=1
  else
    echo "ok: $package has no ${pattern//|/, }"
  fi
}

forbid silon-core 'gpui|tokio'
forbid counter-core 'gpui|tokio'
forbid silon-introspect 'gpui'
forbid counter-app 'axum'

exit "$status"
