"""Write the Silon logo, wordmark and icon SVGs, with the wordmark outlined.

Usage: generate_svg.py SPACE_GROTESK_TTF OUT_DIR
"""

import sys
from pathlib import Path

import uharfbuzz as hb
from fontTools.pens.boundsPen import BoundsPen
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont

font_path, out_dir = Path(sys.argv[1]), Path(sys.argv[2])
out_dir.mkdir(parents=True, exist_ok=True)

INK, PAPER = "#0E1116", "#F5F6F3"
ACCENT, ACCENT_ON_INK = "#3B4CE0", "#8C97FF"

RING = 'd="M44.86 16.68A20 20 0 1 1 19.14 16.68"'
ARROW = 'd="M26.03 10.89 23.64 22.04 14.64 11.32Z"'


def mark(stroke: str, dot: str, transform: str = "") -> str:
    attr = f' transform="{transform}"' if transform else ""
    return (
        f"<g{attr}>"
        f'<path {RING} fill="none" stroke="{stroke}" stroke-width="6" stroke-linecap="round"/>'
        f'<path {ARROW} fill="{stroke}"/>'
        f'<circle cx="32" cy="32" r="6.5" fill="{dot}"/>'
        "</g>"
    )


def outline(text: str, size: float, tracking_em: float, weight: float):
    """Return (path data, ink bounds) for text at the origin baseline, y down."""
    blob = hb.Blob.from_file_path(str(font_path))
    hb_font = hb.Font(hb.Face(blob))
    hb_font.set_variations({"wght": weight})
    buf = hb.Buffer()
    buf.add_str(text)
    buf.guess_segment_properties()
    hb.shape(hb_font, buf, {"kern": True, "liga": True})

    tt = TTFont(font_path)
    upem = tt["head"].unitsPerEm
    glyphs = tt.getGlyphSet(location={"wght": weight})
    order = tt.getGlyphOrder()
    scale = size / upem

    svg_pen = SVGPathPen(glyphs, ntos=lambda v: f"{v:.2f}".rstrip("0").rstrip("."))
    bounds_pen = BoundsPen(glyphs)
    x = 0.0
    for info, pos in zip(buf.glyph_infos, buf.glyph_positions):
        name = order[info.codepoint]
        t = (scale, 0, 0, -scale, x + pos.x_offset * scale, -pos.y_offset * scale)
        glyphs[name].draw(TransformPen(svg_pen, t))
        glyphs[name].draw(TransformPen(bounds_pen, t))
        x += pos.x_advance * scale + tracking_em * size
    return svg_pen.getCommands(), bounds_pen.bounds


def write(name: str, w: float, h: float, body: str, label: str) -> None:
    svg = (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{w:g}" height="{h:g}" '
        f'viewBox="0 0 {w:g} {h:g}" role="img" aria-label="{label}">'
        f"<title>{label}</title>{body}</svg>\n"
    )
    (out_dir / name).write_text(svg)


# Mark alone, for light and dark backgrounds.
write("logo.svg", 64, 64, mark(INK, ACCENT), "Silon")
write("logo-dark.svg", 64, 64, mark(PAPER, ACCENT_ON_INK), "Silon")

# App icon: the mark on an ink tile.
write(
    "icon.svg",
    160,
    160,
    f'<rect width="160" height="160" rx="36" fill="{INK}"/>'
    + mark(PAPER, ACCENT_ON_INK, "translate(24 24) scale(1.75)"),
    "Silon",
)

# Wordmark: a 76 px mark, a 20 px gap, then "silon" at 72 px, -0.035 em tracking.
MARK, GAP, SIZE = 76, 20, 72
d, (x0, y0, x1, y1) = outline("silon", SIZE, -0.035, 600)
pad = 4
height = MARK + 2 * pad
text_x = MARK + GAP - x0
# Center the ink of the text on the center of the mark.
text_y = pad + MARK / 2 - (y0 + y1) / 2
width = text_x + x1 + pad
for name, fg, dot in (("wordmark.svg", INK, ACCENT), ("wordmark-dark.svg", PAPER, ACCENT_ON_INK)):
    write(
        name,
        round(width, 1),
        height,
        mark(fg, dot, f"translate(0 {pad}) scale({MARK / 64:g})")
        + f'<path transform="translate({text_x:.2f} {text_y:.2f})" fill="{fg}" d="{d}"/>',
        "Silon",
    )
