#!/usr/bin/env bash
# Checks the boundaries from AGENTS.md: pure crates link neither gpui nor
# tokio, silon-introspect links no gpui, silon-ui links no silon-introspect,
# an app built without the `introspect` feature links no axum, and app views
# take colors from silon-ui tokens instead of raw color literals.
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
forbid todo-core 'gpui|tokio'
forbid silon-introspect 'gpui'
forbid silon-ui 'silon-introspect' --all-features
forbid counter-app 'axum'
forbid todo-app 'axum'

raw_colors=$(grep -rnE '\b(rgb|rgba|hsla)\(' examples/*/*-app/src || true)
if [[ -n $raw_colors ]]; then
  echo "error: app sources use raw colors; use silon-ui tokens instead:" >&2
  while read -r line; do echo "  $line" >&2; done <<<"$raw_colors"
  status=1
else
  echo "ok: app sources use no raw colors"
fi

exit "$status"
