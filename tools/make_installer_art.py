"""Builds the images of the installer's wizard from the logo.

Usage: python tools/make_installer_art.py

Reads assets/icon/teeth.png (the icon), assets/icon/teeth-mark.png (the white silhouette,
made by make_icon.py) and assets/icon/teeth-wordmark-white.png (the name in white, from
the user's kit) and writes, in installer/art:
- wizard-small-N.png, the icon for the wizard's top corner, at the sizes Inno Setup uses
  from 100 % to 250 % scaling;
- wizard-W.png, the silhouette over the name, centred on black, for the side of the first
  and last pages, at the same scalings.

The logo is the minimalist one, the user's choice of 7 November 2026.

Only black and white, as the logo's guide asks. The results are kept in the repository, so
building the installer does not need Python.
"""

from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
ICON = ROOT / "assets" / "icon"
OUT = ROOT / "installer" / "art"
# Sizes of the wizard's image areas at 100, 125, 150, 175, 200, 225 and 250 % (Inno Setup 6.7).
SMALL = (58, 77, 97, 116, 124, 143, 159)
SIDE = ((202, 386), (269, 515), (336, 643), (403, 772), (430, 824), (498, 953), (534, 1022))
# Widths on the side image, as shares of its width: the silhouette, the name under it, and
# the gap between them as a share of the silhouette's width. The name is the widest part and
# leaves a free margin of a fifth of the width on each side, as the logo's guide asks.
MARK_SHARE = 0.36
NAME_SHARE = 0.6
GAP_SHARE = 0.22


def fit(image: Image.Image, width: int) -> Image.Image:
    return image.resize((width, round(image.height * width / image.width)), Image.LANCZOS)


def side(mark: Image.Image, name: Image.Image, size: tuple[int, int]) -> Image.Image:
    width, height = size
    mark = fit(mark, round(width * MARK_SHARE))
    name = fit(name, round(width * NAME_SHARE))
    gap = round(mark.width * GAP_SHARE)
    top = (height - mark.height - gap - name.height) // 2
    image = Image.new("RGBA", size, (0, 0, 0, 255))
    image.alpha_composite(mark, ((width - mark.width) // 2, top))
    image.alpha_composite(name, ((width - name.width) // 2, top + mark.height + gap))
    return image.convert("RGB")


def main() -> None:
    icon = Image.open(ICON / "teeth.png").convert("RGBA")
    mark = Image.open(ICON / "teeth-mark.png").convert("RGBA")
    wordmark = Image.open(ICON / "teeth-wordmark-white.png").convert("RGBA")
    name = wordmark.crop(wordmark.getchannel("A").getbbox())
    OUT.mkdir(parents=True, exist_ok=True)
    for s in SMALL:
        icon.resize((s, s), Image.LANCZOS).save(OUT / f"wizard-small-{s}.png", optimize=True)
    for size in SIDE:
        side(mark, name, size).save(OUT / f"wizard-{size[0]}.png", optimize=True)


if __name__ == "__main__":
    main()
