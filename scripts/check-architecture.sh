#!/usr/bin/env bash
# Checks the boundaries from AGENTS.md: pure crates link neither gpui nor
# tokio, silon-introspect links no gpui, silon-gpui without `introspect` links
# neither silon-introspect nor axum, silon-ui links no silon-introspect,
# silon-mcp links neither gpui nor other Silon crates (it only speaks HTTP),
# an app built without the `introspect` feature links no axum, no app links
# rmcp, app views take colors from silon-ui tokens instead of raw color
# literals, and app screens use FeatureView + FeatureRoot instead of manual
# gpui::Render impls.
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

forbid silon-core 'gpui|tokio' --all-features
forbid counter-core 'gpui|tokio' --all-features
forbid todo-core 'gpui|tokio' --all-features
forbid herdboard-core 'gpui|tokio' --all-features
forbid silon-introspect 'gpui' --all-features
forbid silon-gpui 'silon-introspect|axum'
forbid silon-ui 'silon-introspect' --all-features
forbid silon-mcp 'gpui|silon-core|silon-gpui|silon-introspect|silon-ui' --all-features
forbid counter-app 'axum'
forbid todo-app 'axum'
forbid herdboard-app 'axum'
forbid counter-app 'rmcp' --all-features
forbid todo-app 'rmcp' --all-features
forbid herdboard-app 'rmcp' --all-features

raw_colors=$(grep -rnE '\b(rgb|rgba|hsla)\(' examples/*/*-app/src || true)
if [[ -n $raw_colors ]]; then
  echo "error: app sources use raw colors; use silon-ui tokens instead:" >&2
  while read -r line; do echo "  $line" >&2; done <<<"$raw_colors"
  status=1
else
  echo "ok: app sources use no raw colors"
fi

manual_render=$(grep -rnE 'impl.*Render\b.*for' examples/*/*-app/src || true)
if [[ -n $manual_render ]]; then
  echo "error: app sources implement Render manually; use FeatureView + FeatureRoot instead:" >&2
  while read -r line; do echo "  $line" >&2; done <<<"$manual_render"
  status=1
else
  echo "ok: app sources have no manual Render impls"
fi

exit "$status"
