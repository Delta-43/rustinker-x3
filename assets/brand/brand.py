"""Draw the RustInker-X3 banner and icon.

Usage: python3 brand.py SCRIPT_FONT MONO_BOLD MONO_REGULAR OUTDIR

SCRIPT_FONT is Dancing Script (SIL OFL 1.1, google/fonts); the mono fonts are
Liberation Mono. Everything is drawn at 4x and scaled down, so edges are smooth.
"""
import math
import random
import sys

from PIL import Image, ImageDraw, ImageFont

K = 4  # supersampling factor
PAPER, INK, RUST, GREY = (244, 241, 234), (30, 30, 30), (183, 65, 14), (120, 116, 108)
BODY, SCREEN, BAR = (26, 26, 26), (226, 226, 228), (52, 52, 52)


def script(path, size):
    f = ImageFont.truetype(path, int(size * K))
    try:
        f.set_variation_by_name("Bold")
    except Exception:
        pass
    return f


def brush(im, x0, x1, y, weight, color, seed=7):
    """Calligraphy-pen underline: angled entry, slight arc, swell, flicked dry tail."""
    layer = Image.new("L", im.size, 0)
    d = ImageDraw.Draw(layer)
    rnd = random.Random(seed)
    n = 300
    top, bot = [], []
    for i in range(n + 1):
        t = i / n
        x = x0 + t * (x1 - x0)
        yy = y - math.sin(math.pi * t) * weight * 0.9 - (max(0.0, t - 0.82) / 0.18) ** 2 * weight * 1.6
        p = (0.55 + 0.45 * min(1.0, t / 0.05)) * (1 - t) ** 0.55 * (0.75 + 0.25 * math.sin(math.pi * t * 0.9))
        half = weight * max(p, 0.04) * 0.5
        wob = (rnd.random() - 0.5) * weight * 0.03
        top.append((x * K, (yy - half + wob) * K))
        bot.append((x * K, (yy + half + wob) * K))
    d.polygon(top + bot[::-1], fill=255)
    ex, ey = x0, (top[0][1] + bot[0][1]) / 2 / K
    h = weight * 0.42
    d.polygon([((ex - h * 0.35) * K, (ey + h * 0.55) * K), ((ex + h * 0.25) * K, (ey - h * 0.6) * K),
               ((ex + h * 1.6) * K, (ey - h * 0.5) * K), ((ex + h * 1.4) * K, (ey + h * 0.5) * K)], fill=255)
    for _ in range(5):  # dry-brush streaks in the tail
        a = rnd.uniform(0.62, 0.9)
        b = min(0.99, a + rnd.uniform(0.04, 0.12))
        off = rnd.uniform(-0.25, 0.25)
        pts = [((top[i][0] + bot[i][0]) / 2, (top[i][1] + bot[i][1]) / 2 + off * (bot[i][1] - top[i][1]))
               for i in range(int(a * n), int(b * n))]
        if len(pts) > 1:
            d.line(pts, fill=0, width=max(1, int(weight * 0.05 * K)))
    im.paste(Image.new("RGB", im.size, color), (0, 0), layer)


def rr(d, box, r, fill):
    d.rounded_rectangle([c * K for c in box], radius=r * K, fill=fill)


def device(im, font, x, y, w, logo=None):
    """The X3 from the front: black body, pale screen, side keys, top power key,
    two bottom button bars. Original line work."""
    d = ImageDraw.Draw(im)
    h = w * 1.53
    # Keys sit almost flush, as on the real unit: a lip of about 1.3 % of the width,
    # drawn as a long rounded slab mostly hidden behind the body so the ends blend in.
    lip = max(1.2, w * 0.013)
    deep = lip * 4
    rr(d, (x - lip, y + h * 0.18, x + deep, y + h * 0.33), lip * 2.2, BODY)
    rr(d, (x + w - deep, y + h * 0.18, x + w + lip, y + h * 0.33), lip * 2.2, BODY)
    rr(d, (x + w * 0.74, y - lip, x + w * 0.91, y + deep), lip * 2.2, BODY)
    rr(d, (x, y, x + w, y + h), w / 16, BODY)
    m = w * 0.085
    sx0, sy0, sx1, sy1 = x + m, y + m, x + w - m, y + h - h * 0.105
    d.rectangle([sx0 * K, sy0 * K, sx1 * K, sy1 * K], fill=SCREEN)
    by, bh = y + h - h * 0.05, max(0.8, w / 140)
    for a, b in ((0.17, 0.48), (0.52, 0.83)):
        rr(d, (x + w * a, by - bh, x + w * b, by + bh), bh, BAR)
    if logo:
        sw = sx1 - sx0
        size = sw * 0.30
        while True:
            f = script(font, size)
            bb = d.textbbox((0, 0), logo, font=f)
            if (bb[2] - bb[0]) / K <= sw * 0.80 or size < 4:
                break
            size *= 0.96
        tw, th = (bb[2] - bb[0]) / K, (bb[3] - bb[1]) / K
        cx, cy = (sx0 + sx1) / 2, (sy0 + sy1) / 2
        d.text(((cx - tw / 2) * K - bb[0], (cy - th / 2 - th * 0.15) * K - bb[1]), logo, font=f, fill=INK)
        brush(im, cx - tw / 2, cx + tw / 2 + tw * 0.04, cy + th / 2 + th * 0.12, max(2.5, th * 0.22), RUST)
    return h


def canvas(w, h):
    return Image.new("RGB", (w * K, h * K), PAPER)


def finish(im, w, h, path):
    im.resize((w, h), Image.LANCZOS).save(path, optimize=True)


def main():
    font, mono_bold, mono_reg, outdir = sys.argv[1:5]

    # Banner, 1440 x 640
    W, H = 1440, 640
    im = canvas(W, H)
    d = ImageDraw.Draw(im)
    title = script(font, 230)
    bb = d.textbbox((0, 0), "RustInker", font=title)
    tw, th = (bb[2] - bb[0]) / K, (bb[3] - bb[1]) / K
    dev_w, gap = 160, 80
    x0 = (W - (tw + gap + dev_w)) / 2
    ty = (H - th) / 2 - 30
    d.text((x0 * K - bb[0], ty * K - bb[1]), "RustInker", font=title, fill=INK)
    brush(im, x0 + 30, x0 + tw + 10, ty + th + 30, 26, RUST)
    dx = x0 + tw + gap
    dh = dev_w * 1.53
    dy = H / 2 - dh / 2 - 40
    device(im, font, dx, dy, dev_w, logo="Ri")
    d = ImageDraw.Draw(im)
    mb = ImageFont.truetype(mono_bold, 54 * K)
    lb = d.textbbox((0, 0), "X3", font=mb)
    d.text(((dx + dev_w / 2) * K - (lb[2] - lb[0]) / 2, (dy + dh + 18) * K), "X3", font=mb, fill=RUST)
    sub = ImageFont.truetype(mono_reg, 34 * K)
    s = "rust firmware for the xteink x3"
    sb = d.textbbox((0, 0), s, font=sub)
    d.text(((W * K - (sb[2] - sb[0])) / 2, (H - 92) * K), s, font=sub, fill=GREY)
    finish(im, W, H, f"{outdir}/banner.png")

    # Icon, 1024 x 1024 (and 256)
    im = canvas(1024, 1024)
    w = 560
    device(im, font, (1024 - w) / 2, (1024 - w * 1.53) / 2 + 6, w, logo="RustInker")
    finish(im, 1024, 1024, f"{outdir}/icon.png")
    finish(im, 256, 256, f"{outdir}/icon-256.png")


if __name__ == "__main__":
    main()
