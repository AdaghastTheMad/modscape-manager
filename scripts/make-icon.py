#!/usr/bin/env python3
"""Draws the Modpack Planner app icon (original art, no game textures).

An isometric grass block whose upper-right corner dissolves, texel by texel,
into an architectural blueprint: blue paper, a fine grid, white wireframe
edges (hidden ones dashed) and construction lines. Writes a 1024x1024 PNG;
`pnpm tauri icon <png>` turns it into every icon size.

    python3 scripts/make-icon.py [out.png]
"""
import math
import random
import sys

from PIL import Image, ImageDraw, ImageFilter

N = 1024
SS = 2  # supersampling for smooth wireframe lines
W = N * SS
rng = random.Random(1123)  # fixed seed: the same icon every run

# ---------------------------------------------------------------- geometry
s = 400 * SS  # cube edge length on screen
c30, s30 = math.cos(math.radians(30)), math.sin(math.radians(30))
R = (s * c30, s * s30)    # right-down edge
L = (-s * c30, s * s30)   # left-down edge
D = (0, s)                # down edge
T = (W / 2, 118 * SS)     # top vertex


def add(*ps):
    return (sum(p[0] for p in ps), sum(p[1] for p in ps))


def mul(p, k):
    return (p[0] * k, p[1] * k)


C = add(T, R, L)  # front-top vertex (centre of the silhouette)
faces = {
    # name: (origin, u axis, v axis, shade)
    "top": (T, R, L, 1.0),
    "left": (add(T, L), R, D, 0.78),
    "right": (C, mul(L, -1), D, 0.60),
}

# ---------------------------------------------------------------- textures (16x16, original)
GRASS = [(96, 164, 58), (86, 150, 50), (110, 178, 66), (74, 136, 44), (120, 188, 74), (100, 158, 54)]
DIRT = [(134, 96, 67), (121, 85, 58), (150, 108, 75), (108, 76, 52), (142, 102, 70), (96, 68, 46)]
STONEY = [(128, 120, 112)]


def tex_top():
    return [[rng.choice(GRASS) for _ in range(16)] for _ in range(16)]


def tex_side():
    t = [[rng.choice(DIRT) if rng.random() > 0.04 else STONEY[0] for _ in range(16)] for _ in range(16)]
    # grass lip: 3 rows, then uneven drips
    for x in range(16):
        depth = 3 + (1 if rng.random() < 0.45 else 0) + (1 if rng.random() < 0.18 else 0)
        for y in range(depth):
            t[y][x] = rng.choice(GRASS[:4])
    return t


textures = {"top": tex_top(), "left": tex_side(), "right": tex_side()}

# ---------------------------------------------------------------- dissolve mask
# 0 = solid block, 1 = blueprint. Driven by distance toward the upper-right,
# with a per-texel random threshold so it breaks up in square pixels.
xs = [T[0] + L[0], T[0] + R[0]]
ys = [T[1], T[1] + 2 * s]


def blueprintness(px, py):
    nx = (px - xs[0]) / (xs[1] - xs[0])
    ny = 1 - (py - ys[0]) / (ys[1] - ys[0])
    t = (nx * 1.05 + ny * 0.95) / 2
    return min(1.0, max(0.0, (t - 0.53) / 0.22))


def shade(rgb, k):
    return tuple(max(0, min(255, int(v * k))) for v in rgb)


block = Image.new("RGBA", (W, W), (0, 0, 0, 0))
bd = ImageDraw.Draw(block)
dissolved = []  # texel polygons that turned to blueprint
for name, (o, u, v, k) in faces.items():
    tex = textures[name]
    du, dv = mul(u, 1 / 16), mul(v, 1 / 16)
    for j in range(16):
        for i in range(16):
            p0 = add(o, mul(du, i), mul(dv, j))
            poly = [p0, add(p0, du), add(p0, du, dv), add(p0, dv)]
            cx = sum(p[0] for p in poly) / 4
            cy = sum(p[1] for p in poly) / 4
            m = blueprintness(cx, cy)
            if m > 0 and rng.random() < m:
                dissolved.append(poly)
                continue
            colour = shade(tex[j][i], k)
            # Texels at the edge of the dissolve read as "half built": a lighter, bluish tint.
            if m > 0.15:
                colour = tuple(int(a * (1 - m * 0.35) + b * m * 0.35) for a, b in zip(colour, (120, 170, 230)))
            bd.polygon(poly, fill=colour + (255,))

# ---------------------------------------------------------------- blueprint layer
BLUE = (30, 84, 160, 255)
GRID = (120, 170, 235, 120)
GRID_MAJOR = (170, 205, 250, 200)
LINE = (235, 244, 255, 255)
bp = Image.new("RGBA", (W, W), (0, 0, 0, 0))
bpd = ImageDraw.Draw(bp)
for poly in dissolved:
    bpd.polygon(poly, fill=BLUE)
# fine texel grid on every face (shows through only where the block dissolved)
grid = Image.new("RGBA", (W, W), (0, 0, 0, 0))
gd = ImageDraw.Draw(grid)
for name, (o, u, v, _) in faces.items():
    for n in range(17):
        major = n % 4 == 0
        col, wdt = (GRID_MAJOR, 2 * SS) if major else (GRID, SS)
        a = add(o, mul(u, n / 16))
        gd.line([a, add(a, v)], fill=col, width=wdt)
        b = add(o, mul(v, n / 16))
        gd.line([b, add(b, u)], fill=col, width=wdt)
mask = Image.new("L", (W, W), 0)
md = ImageDraw.Draw(mask)
for poly in dissolved:
    md.polygon(poly, fill=255)
bp.alpha_composite(Image.composite(grid, Image.new("RGBA", (W, W), (0, 0, 0, 0)), mask))

# wireframe: visible edges solid, hidden back edges dashed, all fading in toward the corner
wire = Image.new("RGBA", (W, W), (0, 0, 0, 0))
wd = ImageDraw.Draw(wire)
back = add(T, D)  # the hidden back-bottom vertex
edges = [
    (T, add(T, R)), (T, add(T, L)), (add(T, R), C), (add(T, L), C),
    (C, add(C, D)), (add(T, R), add(T, R, D)), (add(T, L), add(T, L, D)),
    (add(C, D), add(T, R, D)), (add(C, D), add(T, L, D)),
]
hidden = [(T, back), (back, add(T, R, D)), (back, add(T, L, D))]


def seg(a, b, width, dash=None):
    n = 120
    for k in range(n):
        if dash and (k // dash) % 2:
            continue
        p = add(a, mul((b[0] - a[0], b[1] - a[1]), k / n))
        q = add(a, mul((b[0] - a[0], b[1] - a[1]), (k + 1) / n))
        m = blueprintness((p[0] + q[0]) / 2, (p[1] + q[1]) / 2)
        if m <= 0.02:
            continue
        alpha = int(255 * min(1, m * 1.4))
        wd.line([p, q], fill=LINE[:3] + (alpha,), width=width)


for a, b in edges:
    seg(a, b, 7 * SS)
# Hidden edges show only through the dissolved part (you see through the blueprint, not the dirt).
hid = Image.new("RGBA", (W, W), (0, 0, 0, 0))
_wd = wd
wd = ImageDraw.Draw(hid)
for a, b in hidden:
    seg(a, b, 4 * SS, dash=4)
wd = _wd
wire.alpha_composite(Image.composite(hid, Image.new("RGBA", (W, W), (0, 0, 0, 0)), mask))

# construction lines past the top-right corner, plus a dimension line with ticks
TR = add(T, R)
ext = 70 * SS
for a, d in [(TR, R), (TR, mul(L, -1)), (TR, mul(D, -1))]:
    ln = math.hypot(*d)
    e = add(a, mul(d, ext / ln))
    wd.line([a, e], fill=(70, 130, 210, 255), width=3 * SS)
off = mul((R[1], -R[0]), 38 * SS / s)  # perpendicular offset, up and to the right
p1, p2 = add(T, off, mul(R, 0.55)), add(TR, off)
DIM = (70, 130, 210, 255)  # dimension marks sit outside the block, so they need a colour that shows on light and dark
wd.line([p1, p2], fill=DIM, width=3 * SS)
for p in (p1, p2):
    wd.line([add(p, mul(off, -0.45)), add(p, mul(off, 0.45))], fill=DIM, width=3 * SS)
for k in range(1, 8):
    p = add(p1, mul((p2[0] - p1[0], p2[1] - p1[1]), k / 8))
    wd.line([p, add(p, mul(off, 0.22 if k % 4 else 0.38))], fill=DIM, width=2 * SS)

# ---------------------------------------------------------------- compose
out = Image.new("RGBA", (W, W), (0, 0, 0, 0))
# soft drop shadow under the block (classic desktop-icon style)
sh = Image.new("RGBA", (W, W), (0, 0, 0, 0))
sd = ImageDraw.Draw(sh)
base = add(C, D)
sd.ellipse([base[0] - s * 0.95, base[1] - s * 0.16, base[0] + s * 0.95, base[1] + s * 0.20], fill=(0, 0, 0, 110))
out.alpha_composite(sh.filter(ImageFilter.GaussianBlur(28 * SS)))
out.alpha_composite(block)
out.alpha_composite(bp)
# thin dark outline on the solid part's silhouette edges, like a pixel-art icon
outline = Image.new("RGBA", (W, W), (0, 0, 0, 0))
od = ImageDraw.Draw(outline)
for a, b in edges:
    m = blueprintness((a[0] + b[0]) / 2, (a[1] + b[1]) / 2)
    if m < 0.5:
        od.line([a, b], fill=(24, 18, 12, 160), width=2 * SS)
out.alpha_composite(outline)
out.alpha_composite(wire)
out = out.resize((N, N), Image.LANCZOS)
path = sys.argv[1] if len(sys.argv) > 1 else "icon-source.png"
out.save(path)
print("wrote", path)
