"""Writes the test font the @font-face tests load: BoxTest, whose every
glyph (A to Z and space) is a filled box two ems wide, so text set in it is
far wider than the same text in the bundled DejaVu Sans. It is written as
TrueType, WOFF and WOFF2.

fontTools needs a brotli compressor for WOFF2. Brotli's own format has
stored (uncompressed) meta-blocks, so a compressor that writes only those
is a few lines and needs no package; every brotli decoder reads it.

    PYTHONUTF8=1 python core/tests/fixtures/make_boxfont.py
"""

import io
import pathlib
import types

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont, woff2

HERE = pathlib.Path(__file__).parent
UPM = 1000
ADVANCE = 2000


class Bits:
    def __init__(self):
        self.out = bytearray()
        self.acc = 0
        self.n = 0

    def put(self, value, count):
        for i in range(count):
            self.acc |= ((value >> i) & 1) << self.n
            self.n += 1
            if self.n == 8:
                self.flush()

    def flush(self):
        if self.n:
            self.out.append(self.acc)
            self.acc = 0
            self.n = 0


def stored_brotli(data, quality=None, mode=None, **_):
    """A brotli stream of stored meta-blocks: WBITS 16 (one 0 bit), then
    for each chunk ISLAST=0, MNIBBLES=4, MLEN-1, ISUNCOMPRESSED=1, padding
    to the byte, the bytes; then ISLAST=1, ISLASTEMPTY=1."""
    b = Bits()
    b.put(0, 1)
    for i in range(0, len(data), 65536):
        chunk = data[i : i + 65536]
        b.put(0, 1)
        b.put(0, 2)
        b.put(len(chunk) - 1, 16)
        b.put(1, 1)
        b.flush()
        b.out.extend(chunk)
    b.put(1, 1)
    b.put(1, 1)
    b.flush()
    return bytes(b.out)


def box_glyph():
    pen = TTGlyphPen(None)
    pen.moveTo((100, 0))
    pen.lineTo((100, 700))
    pen.lineTo((ADVANCE - 100, 700))
    pen.lineTo((ADVANCE - 100, 0))
    pen.closePath()
    return pen.glyph()


def empty_glyph():
    return TTGlyphPen(None).glyph()


def build():
    letters = [chr(c) for c in range(ord("A"), ord("Z") + 1)]
    names = [".notdef", "space"] + letters
    fb = FontBuilder(UPM, isTTF=True)
    fb.setupGlyphOrder(names)
    cmap = {ord(" "): "space"}
    cmap.update({ord(c): c for c in letters})
    fb.setupCharacterMap(cmap)
    glyphs = {".notdef": box_glyph(), "space": empty_glyph()}
    glyphs.update({c: box_glyph() for c in letters})
    fb.setupGlyf(glyphs)
    fb.setupHorizontalMetrics({n: (ADVANCE, 0) for n in names})
    fb.setupHorizontalHeader(ascent=800, descent=-200)
    fb.setupNameTable({"familyName": "BoxTest", "styleName": "Regular"})
    fb.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=800, usWinDescent=200)
    fb.setupPost()
    buf = io.BytesIO()
    fb.save(buf)
    return buf.getvalue()


def main():
    ttf = build()
    (HERE / "boxfont.ttf").write_bytes(ttf)
    for flavor in ("woff", "woff2"):
        font = TTFont(io.BytesIO(ttf))
        font.flavor = flavor
        out = io.BytesIO()
        font.save(out)
        (HERE / f"boxfont.{flavor}").write_bytes(out.getvalue())


woff2.brotli = types.SimpleNamespace(compress=stored_brotli, MODE_FONT=2)
woff2.haveBrotli = True

if __name__ == "__main__":
    main()
