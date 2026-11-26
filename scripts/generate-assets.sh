#!/usr/bin/env bash
# Regenerates assets/: the logo, wordmark and icon SVGs and the banner PNG.
# Run it from the assets dev shell, which provides the tools and fonts:
#   nix develop .#assets -c scripts/generate-assets.sh
set -euo pipefail

# Space Grotesk is not in nixpkgs, so fetch it from Google Fonts at a fixed
# commit and check its hash.
font_commit=2861cb7b12f90c0a294a12ed666e381e2211872f
font_sha256=acad6de1fc93436f5c0f1f4137751ef04f1aea3063e7036535970ffcfbd79f72
font_url="https://raw.githubusercontent.com/google/fonts/$font_commit/ofl/spacegrotesk/SpaceGrotesk%5Bwght%5D.ttf"

: "${INTER_FONT:?run from nix develop .#assets}"
: "${JETBRAINS_MONO_FONT:?run from nix develop .#assets}"

root=$(git rev-parse --show-toplevel)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

mkdir "$work/fonts"
curl -fsSL -o "$work/fonts/SpaceGrotesk.ttf" "$font_url"
echo "$font_sha256  $work/fonts/SpaceGrotesk.ttf" | sha256sum -c --quiet
cp "$INTER_FONT" "$work/fonts/InterVariable.ttf"
cp "$JETBRAINS_MONO_FONT" "$work/fonts/JetBrainsMono-Regular.ttf"

python3 -I "$root/scripts/assets/generate_svg.py" "$work/fonts/SpaceGrotesk.ttf" "$root/assets"

# The banner is drawn at 2x, so it stays sharp in the README and fits the
# social preview size (1280x640, under 1 MB).
cp "$root/scripts/assets/banner.html" "$root/assets/wordmark-dark.svg" "$work/"
chromium --headless --no-sandbox --disable-gpu --hide-scrollbars \
  --allow-file-access-from-files --force-device-scale-factor=2 \
  --window-size=1280,640 --virtual-time-budget=3000 \
  --screenshot="$root/assets/banner.png" "file://$work/banner.html" 2>/dev/null
