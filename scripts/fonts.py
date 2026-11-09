#!/usr/bin/env python3
"""Builds assets/fonts/: the fonts the game draws its lines with in the AI Hub's languages
(docs/LANGUAGES.md). Downloads pinned Noto fonts (google/fonts, SIL Open Font License) into
.cache/fonts, checks their hashes, pins each variable font to its regular weight and cuts it
down to the characters its languages use:

- NotoSans: Latin (with Vietnamese, Turkish, Hausa and Yoruba letters and the combining marks),
  Greek, Cyrillic, punctuation.
- NotoSansArabic (Arabic, Urdu), NotoSansDevanagari (Hindi), NotoSansBengali, NotoSansEthiopic
  (Amharic): their blocks, plus ASCII so a line with a number stays in one font.
- CJK: Simplified Chinese (GB 2312 level 1, the 3,755 commonest characters), Traditional
  (Big5 level 1, 5,401), Japanese (kana and JIS level 1 kanji, 2,965) and Korean (the 2,350
  Hangul syllables of KS X 1001), each with CJK punctuation and full-width forms.

Needs fontTools (pip install fonttools). Run it when the sets change; its output is committed,
so builds never need the network.

  python3 scripts/fonts.py
"""
import hashlib
import os
import sys
import urllib.request

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
CACHE = os.path.join(ROOT, ".cache", "fonts")
OUT = os.path.join(ROOT, "assets", "fonts")
BASE = "https://raw.githubusercontent.com/google/fonts/main/ofl/"
SOURCES = {
    "NotoSans": ("notosans/NotoSans%5Bwdth,wght%5D.ttf", "bfb7bb691513f12e734dc346c03a03f784912432d7e3fa8e56efcf906fe86b3d"),
    "NotoSansArabic": ("notosansarabic/NotoSansArabic%5Bwdth,wght%5D.ttf", "63111b5b2e074dd48cc67692e0a2726d86ee94c1c37fe8598257b7b4e87e869e"),
    "NotoSansBengali": ("notosansbengali/NotoSansBengali%5Bwdth,wght%5D.ttf", "dcd42978094e584a849c84a51450eeac40c8826057d566ea6d4b9627a403a05a"),
    "NotoSansDevanagari": ("notosansdevanagari/NotoSansDevanagari%5Bwdth,wght%5D.ttf", "14ec4af41f27482216d1c2229f417ff9b1425e1babb014e57d1d40d03229853e"),
    "NotoSansEthiopic": ("notosansethiopic/NotoSansEthiopic%5Bwdth,wght%5D.ttf", "0dbccc00b22d180ebd6a4bd8a733918a29e709fa6798023adc3e6cd40da65077"),
    "NotoSansJP": ("notosansjp/NotoSansJP%5Bwght%5D.ttf", "c2f3b4d463500a2ddcd3849cded1fceeb9fd6d1c32e6cbecd568453ba50fc68f"),
    "NotoSansKR": ("notosanskr/NotoSansKR%5Bwght%5D.ttf", "194018e6b2b293a7964f037b25c0249ce1418bc9ab3c971060a03aa57861e252"),
    "NotoSansSC": ("notosanssc/NotoSansSC%5Bwght%5D.ttf", "a3041811a78c361b1de50f953c805e0244951c21c5bd412f7232ef0d899af0da"),
    "NotoSansTC": ("notosanstc/NotoSansTC%5Bwght%5D.ttf", "864727d210d54f2537bbe23b3a839436c3992af72de9322af5270897246bd44f"),
}


def rng(a, b):
    return set(range(a, b + 1))


def codec_chars(codec, lead, trail):
    out = set()
    for a in lead:
        for b in trail:
            try:
                c = bytes([a, b]).decode(codec)
            except UnicodeDecodeError:
                continue
            if len(c) == 1:
                out.add(ord(c))
    return out


ASCII = rng(0x20, 0x7E)
PUNCT = rng(0x2000, 0x206F) | {0x00A0, 0x00B7, 0x00D7, 0x2190, 0x2191, 0x2192, 0x2193}
CJK_COMMON = ASCII | PUNCT | rng(0x3000, 0x303F) | rng(0xFF00, 0xFFEF)
SETS = {
    "NotoSans": rng(0x20, 0x2AF) | rng(0x2B0, 0x36F) | rng(0x370, 0x3FF) | rng(0x400, 0x52F) | rng(0x1E00, 0x1EFF) | rng(0x2000, 0x20CF) | rng(0x2100, 0x21FF),
    "NotoSansArabic": ASCII | PUNCT | rng(0x600, 0x6FF) | rng(0x750, 0x77F) | rng(0xFB50, 0xFDFF) | rng(0xFE70, 0xFEFF),
    "NotoSansDevanagari": ASCII | PUNCT | rng(0x900, 0x97F) | rng(0xA8E0, 0xA8FF),
    "NotoSansBengali": ASCII | PUNCT | rng(0x980, 0x9FF),
    "NotoSansEthiopic": ASCII | PUNCT | rng(0x1200, 0x139F) | rng(0x2D80, 0x2DDF) | rng(0xAB00, 0xAB2F),
    # GB 2312 level 1: rows 16-55 (lead bytes 0xB0-0xD7).
    "NotoSansSC": CJK_COMMON | codec_chars("gb2312", range(0xB0, 0xD8), range(0xA1, 0xFF)) | codec_chars("gb2312", range(0xA1, 0xAA), range(0xA1, 0xFF)),
    # Big5 level 1: lead bytes 0xA4-0xC6.
    "NotoSansTC": CJK_COMMON | codec_chars("big5", range(0xA4, 0xC7), list(range(0x40, 0x7F)) + list(range(0xA1, 0xFF))) | codec_chars("big5", range(0xA1, 0xA4), list(range(0x40, 0x7F)) + list(range(0xA1, 0xFF))),
    # JIS X 0208 level 1 kanji: rows 16-47 (EUC lead bytes 0xB0-0xCF), with kana and symbols.
    "NotoSansJP": CJK_COMMON | rng(0x3040, 0x30FF) | rng(0x31F0, 0x31FF) | codec_chars("euc_jp", range(0xB0, 0xD0), range(0xA1, 0xFF)) | codec_chars("euc_jp", range(0xA1, 0xA9), range(0xA1, 0xFF)),
    # KS X 1001 Hangul syllables (lead bytes 0xB0-0xC8) and compatibility jamo.
    "NotoSansKR": CJK_COMMON | rng(0x3130, 0x318F) | codec_chars("euc_kr", range(0xB0, 0xC9), range(0xA1, 0xFF)),
}


def fetch(name):
    path, sha = SOURCES[name]
    os.makedirs(CACHE, exist_ok=True)
    dest = os.path.join(CACHE, name + ".ttf")
    if not os.path.exists(dest):
        print(f"downloading {name}")
        urllib.request.urlretrieve(BASE + path, dest)
    digest = hashlib.sha256(open(dest, "rb").read()).hexdigest()
    if digest != sha:
        sys.exit(f"{dest}: sha256 {digest}, expected {sha} (upstream changed: check the font and update the pin)")
    return dest


def main():
    os.makedirs(OUT, exist_ok=True)
    for name, chars in SETS.items():
        font = TTFont(fetch(name))
        if "fvar" in font:
            axes = {a.axisTag: (400 if a.axisTag == "wght" else 100) for a in font["fvar"].axes}
            font = instancer.instantiateVariableFont(font, axes)
        opts = subset.Options()
        opts.layout_features = ["*"]
        opts.name_IDs = ["*"]
        opts.notdef_outline = True
        opts.hinting = False
        opts.drop_tables += ["meta"]
        s = subset.Subsetter(opts)
        s.populate(unicodes=chars)
        s.subset(font)
        dest = os.path.join(OUT, name + ".ttf")
        font.save(dest)
        print(f"{name:20s} {len(chars):6d} characters  {os.path.getsize(dest) / 1e6:5.2f} MB")


if __name__ == "__main__":
    main()
