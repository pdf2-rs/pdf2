#!/usr/bin/env python3
"""Generate the hand-built PDF fixtures in this directory.

Run from the repository root:

    python3 tests/fixtures/make_fixtures.py

The fixtures are committed, and `cargo test` never regenerates them: this
script exists so the binary files can be reviewed and rebuilt byte-for-byte.
Every file is a minimal PDF 2.0 document with cross-reference offsets computed
here, so no external PDF tool is involved.
"""

import os
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))

CATALOG = b"<< /Type /Catalog /Pages 2 0 R >>"
PAGES = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
# The font is written inline so the page stays a three-member object stream.
PAGE = (
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R"
    b" /Resources << /Font << /F1 << /Type /Font /Subtype /Type1"
    b" /BaseFont /Helvetica /Encoding /WinAnsiEncoding >> >> >> >>"
)
CONTENT = b"BT /F1 24 Tf 72 700 Td (Hello, PDF 2.0) Tj ET"
UPDATED_CONTENT = b"BT /F1 24 Tf 72 700 Td (Hello again) Tj ET"


class Writer:
    """A byte buffer that records object offsets."""

    def __init__(self):
        self.out = bytearray(b"%PDF-2.0\n")
        self.offsets = {}

    def obj(self, number, body):
        self.offsets[number] = len(self.out)
        self.out += b"%d 0 obj\n" % number
        self.out += body
        self.out += b"\nendobj\n"

    def stream(self, number, entries, data, compress=False):
        if compress:
            data = zlib.compress(data)
            entries = entries + b" /Filter /FlateDecode"
        self.obj(
            number,
            b"<< /Length %d %s >>\nstream\n" % (len(data), entries) + data + b"\nendstream",
        )
        return data


def classic_table(out, offsets, trailer_keys, start_at):
    """Append a classic xref table and its trailer; return its offset."""
    xref = len(out)
    out += b"xref\n0 %d\n" % (max(offsets) + 1)
    out += b"0000000000 65535 f \n"
    for number in range(1, max(offsets) + 1):
        out += b"%010d 00000 n \n" % offsets[number]
    out += b"trailer\n<< /Size %d %s >>\n" % (max(offsets) + 1, trailer_keys)
    out += b"startxref\n%d\n" % xref
    out += b"%%EOF\n"
    assert xref == start_at
    return xref


def object_stream(header_objects, first_offsets):
    """Build an /ObjStm body: header pairs then the objects themselves."""
    header = b" ".join(
        b"%d %d" % (number, offset)
        for (number, _), offset in zip(header_objects, first_offsets)
    )
    return header + b"\n"


def write(name, data):
    path = os.path.join(HERE, name)
    with open(path, "wb") as handle:
        handle.write(data)
    print(f"{name}: {len(data)} bytes")


def build_xref_stream_entries(rows, width):
    """rows: list of (type, field2, field3) -> packed big-endian fields."""
    w0, w1, w2 = width
    out = bytearray()
    for kind, a, b in rows:
        out += kind.to_bytes(w0, "big")
        out += a.to_bytes(w1, "big")
        out += b.to_bytes(w2, "big")
    return bytes(out)


W = (1, 4, 2)


def make_xref_stream_pdf():
    """A file whose cross-reference is a single /XRef stream."""
    writer = Writer()
    writer.stream(4, b"/Type /EmbeddedFile", CONTENT)
    content_offset = writer.offsets[4]

    # Objects 1, 2 and 3 live inside object stream 5.
    members = [(1, CATALOG), (2, PAGES), (3, PAGE)]
    header_offsets = []
    body = bytearray()
    for _, payload in members:
        header_offsets.append(len(body))
        body += payload + b" "
    header = b" ".join(
        b"%d %d" % (number, offset)
        for (number, _), offset in zip(members, header_offsets)
    ) + b"\n"
    objstm = header + bytes(body)
    writer.stream(
        5,
        b"/Type /ObjStm /N %d /First %d" % (len(members), len(header)),
        objstm,
        compress=True,
    )

    # The first cross-reference stream covers objects 0..6.
    rows = [(0, 0, 65535)]
    rows += [(2, 5, index) for index in range(3)]
    rows += [(1, content_offset, 0), (1, writer.offsets[5], 0), (1, 0, 0)]
    xref_number = 6
    rows[-1] = (1, len(writer.out), 0)
    data = build_xref_stream_entries(rows, W)
    writer.stream(
        xref_number,
        b"/Type /XRef /Size 7 /W [1 4 2] /Root 1 0 R",
        data,
        compress=True,
    )
    first_xref = writer.offsets[xref_number]
    writer.out += b"startxref\n%d\n%%%%EOF\n" % first_xref
    write("xref-stream.pdf", bytes(writer.out))

    # Incremental update: replace the content stream and add a second xref
    # stream whose /Prev points at the first one.
    writer.stream(4, b"/Type /EmbeddedFile", UPDATED_CONTENT)
    updated_offset = writer.offsets[4]
    rows = [(1, updated_offset, 0), (1, len(writer.out), 0)]
    data = build_xref_stream_entries(rows, W)
    writer.stream(
        7,
        b"/Type /XRef /Size 8 /W [1 4 2] /Index [4 1 7 1] /Prev %d /Root 1 0 R" % first_xref,
        data,
        compress=True,
    )
    writer.out += b"startxref\n%d\n%%%%EOF\n" % writer.offsets[7]
    write("xref-stream-prev.pdf", bytes(writer.out))


HELVETICA = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"


def make_classic_twin():
    """The same object graph as xref-stream.pdf, written with a classic table."""
    writer = Writer()
    writer.obj(1, CATALOG)
    writer.obj(2, PAGES)
    writer.obj(3, PAGE)
    writer.stream(4, b"/Type /EmbeddedFile", CONTENT)
    expected = len(writer.out)
    classic_table(writer.out, writer.offsets, b"/Root 1 0 R", start_at=expected)
    write("classic-twin.pdf", bytes(writer.out))


def make_hybrid():
    """A hybrid file: classic table plus /XRefStm hiding objects 1..3."""
    writer = Writer()
    writer.stream(4, b"/Type /EmbeddedFile", CONTENT)
    content_offset = writer.offsets[4]

    members = [(1, CATALOG), (2, PAGES), (3, PAGE)]
    header_offsets = []
    body = bytearray()
    for _, payload in members:
        header_offsets.append(len(body))
        body += payload + b" "
    header = b" ".join(
        b"%d %d" % (number, offset)
        for (number, _), offset in zip(members, header_offsets)
    ) + b"\n"
    objstm = header + bytes(body)
    writer.stream(
        5,
        b"/Type /ObjStm /N %d /First %d" % (len(members), len(header)),
        objstm,
        compress=True,
    )

    # The hidden cross-reference stream only covers the compressed objects.
    rows = [(2, 5, index) for index in range(3)]
    data = build_xref_stream_entries(rows, W)
    writer.stream(
        6,
        b"/Type /XRef /Size 7 /Index [1 3] /W [1 4 2] /Root 1 0 R",
        data,
        compress=True,
    )
    xref_stream_offset = writer.offsets[6]

    # The classic table covers only the plain objects, as in a real hybrid file.
    xref = len(writer.out)
    writer.out += b"xref\n0 1\n0000000000 65535 f \n"
    writer.out += b"4 3\n"
    for number in (4, 5, 6):
        writer.out += b"%010d 00000 n \n" % writer.offsets[number]
    writer.out += b"trailer\n<< /Size 7 /Root 1 0 R /XRefStm %d >>\n" % xref_stream_offset
    writer.out += b"startxref\n%d\n%%%%EOF\n" % xref
    write("hybrid-xref.pdf", bytes(writer.out))


XMP = (
    b'<?xpacket begin="\xef\xbb\xbf" id="W5M0MpCehiHzreSzNTczkc9d"?>'
    b'<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"/></x:xmpmeta>'
    b'<?xpacket end="w"?>'
)


def make_page_tree():
    """A three-page tree that exercises every inheritable attribute.

    Root node 2 carries /MediaBox and /Resources; node 3 adds /Rotate 90 for
    its two children; page 5 overrides /MediaBox, page 6 overrides /CropBox,
    page 7 overrides /Rotate. The catalog carries an XMP /Metadata stream.
    """
    writer = Writer()
    writer.obj(1, b"<< /Type /Catalog /Pages 2 0 R /Metadata 9 0 R >>")
    writer.obj(
        2,
        b"<< /Type /Pages /Kids [3 0 R 7 0 R] /Count 3"
        b" /MediaBox [0 0 612 792] /Resources << /Font << /F1 8 0 R >> >> >>",
    )
    writer.obj(
        3,
        b"<< /Type /Pages /Parent 2 0 R /Kids [5 0 R 6 0 R] /Count 2 /Rotate 90 >>",
    )
    writer.obj(
        5,
        b"<< /Type /Page /Parent 3 0 R /MediaBox [0 0 200 300] /Contents 4 0 R >>",
    )
    writer.obj(6, b"<< /Type /Page /Parent 3 0 R /CropBox [10 20 190 280] >>")
    writer.obj(7, b"<< /Type /Page /Parent 2 0 R /Rotate 180 >>")
    writer.obj(8, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    writer.stream(9, b"/Type /Metadata /Subtype /XML", XMP)
    writer.stream(4, b"/Type /EmbeddedFile", b"BT /F1 18 Tf 20 40 Td (page one) Tj ET")
    offsets = writer.offsets
    xref = len(writer.out)
    writer.out += b"xref\n0 %d\n" % (max(offsets) + 1)
    writer.out += b"0000000000 65535 f \n"
    for number in range(1, max(offsets) + 1):
        writer.out += b"%010d 00000 n \n" % offsets[number]
    writer.out += b"trailer\n<< /Size %d /Root 1 0 R >>\n" % (max(offsets) + 1)
    writer.out += b"startxref\n%d\n%%%%EOF\n" % xref
    write("page-tree.pdf", bytes(writer.out))


TEXT_CONTENT = (
    b"BT\n"
    b"/F1 18 Tf\n"
    b"72 700 Td\n"
    b"(Hello, PDF 2.0) Tj\n"
    b"0 -24 Td\n"
    b"(Second line) Tj\n"
    b"0 -24 Td\n"
    b"[(spaced) -300 (out)] TJ\n"
    b"14 TL\n"
    b"T*\n"
    b"(fourth line) '\n"
    b"ET\n"
    b"BT\n"
    b"/F2 14 Tf\n"
    b"1 0 0 1 72 520 Tm\n"
    b"(A dot) Tj\n"
    b"ET\n"
    b"BT\n"
    b"/F1 12 Tf\n"
    b"1 0 0 1 72 480 Tm\n"
    b"(caf\\351 ok) Tj\n"
    b"ET\n"
)

BULLET_FONT = (
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Symbolish"
    b" /Encoding << /Type /Encoding /BaseEncoding /WinAnsiEncoding"
    b" /Differences [65 /bullet] >> >>"
)


def make_text():
    """A page whose /Contents is uncompressed and exercises the text operators."""
    writer = Writer()
    writer.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
    writer.obj(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
    writer.obj(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792]"
        b" /Contents 4 0 R /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> >>",
    )
    writer.stream(4, b"/Type /EmbeddedFile", TEXT_CONTENT)
    writer.obj(5, HELVETICA)
    writer.obj(6, BULLET_FONT)
    offsets = writer.offsets
    xref = len(writer.out)
    writer.out += b"xref\n0 %d\n" % (max(offsets) + 1)
    writer.out += b"0000000000 65535 f \n"
    for number in range(1, max(offsets) + 1):
        writer.out += b"%010d 00000 n \n" % offsets[number]
    writer.out += b"trailer\n<< /Size %d /Root 1 0 R >>\n" % (max(offsets) + 1)
    writer.out += b"startxref\n%d\n%%%%EOF\n" % xref
    write("text.pdf", bytes(writer.out))


CLASSIC_CONTENT = b"BT /F1 24 Tf 72 700 Td (Incremental) Tj ET"


def make_classic_xref():
    """A classic table plus an incremental update that repaints page 3.

    The update gives the page a `/Resources` entry for `/F1`, so the content
    stream it points at is interpretable: the first section stays byte-for-byte
    what it always was, which keeps the `/Prev 186` offset valid.
    """
    writer = Writer()
    writer.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
    writer.obj(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
    writer.obj(3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>")
    first_xref = len(writer.out)
    writer.out += b"xref\n0 4\n"
    writer.out += b"0000000000 65535 f \n"
    for number in (1, 2, 3):
        writer.out += b"%010d 00000 n \n" % writer.offsets[number]
    writer.out += b"trailer\n<< /Size 4 /Root 1 0 R >>\n"
    writer.out += b"startxref\n%d\n%%%%EOF\n" % first_xref
    assert first_xref == 186, first_xref

    writer.stream(4, b"", CLASSIC_CONTENT)
    writer.obj(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R"
        b" /Resources << /Font << /F1 << /Type /Font /Subtype /Type1"
        b" /BaseFont /Helvetica /Encoding /WinAnsiEncoding >> >> >> >>",
    )
    second_xref = len(writer.out)
    writer.out += b"xref\n3 2\n"
    for number in (3, 4):
        writer.out += b"%010d 00000 n \n" % writer.offsets[number]
    writer.out += b"trailer\n<< /Size 5 /Root 1 0 R /Prev %d >>\n" % first_xref
    writer.out += b"startxref\n%d\n%%%%EOF\n" % second_xref
    write("classic-xref.pdf", bytes(writer.out))


PATHS_CONTENT = (
    b"q 1 0 0 rg 20 20 60 60 re f Q\n"
    b"q 0 0 1 rg 100 20 60 60 re 120 40 60 60 re f* Q\n"
    b"q 0 1 0 RG 8 w 20 120 60 40 re S Q\n"
    b"q 0 1 1 rg 20 140 40 40 re W n 0 140 200 40 re f Q\n"
    b"q 0 0 0 rg 1 0 0 1 140 120 cm 0.5 0 0 0.5 0 0 cm 0 0 20 20 re f Q\n"
    b"q 1 0 1 RG 2 w [6 4] 0 d 90 160 m 190 160 l S Q\n"
    b"BT /F1 12 Tf 20 190 Td (text waits for Q7) Tj ET\n"
)


def make_paths():
    """Paths, clips, transforms and dashes, for the pdf2-render golden image."""
    writer = Writer()
    writer.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
    writer.obj(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
    writer.obj(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R"
        b" /Resources << /Font << /F1 5 0 R >> >> >>",
    )
    writer.stream(4, b"", PATHS_CONTENT)
    writer.obj(5, HELVETICA)
    offsets = writer.offsets
    xref = len(writer.out)
    writer.out += b"xref\n0 %d\n" % (max(offsets) + 1)
    writer.out += b"0000000000 65535 f \n"
    for number in range(1, max(offsets) + 1):
        writer.out += b"%010d 00000 n \n" % offsets[number]
    writer.out += b"trailer\n<< /Size %d /Root 1 0 R >>\n" % (max(offsets) + 1)
    writer.out += b"startxref\n%d\n%%%%EOF\n" % xref
    write("paths.pdf", bytes(writer.out))


IMAGES_CONTENT = (
    b"q 80 0 0 80 20 20 cm /Im1 Do Q\n"
    b"q 80 0 0 80 110 20 cm /Im2 Do Q\n"
    b"BT /F1 16 Tf 20 150 Td (IMAGES 2026) Tj ET\n"
)


def image_pixels():
    """An 8x8 RGB image: red/green on the top half, blue/white on the bottom."""
    rows = []
    for row in range(8):
        for column in range(8):
            if row < 4:
                rows.extend((255, 0, 0) if column < 4 else (0, 255, 0))
            else:
                rows.extend((0, 0, 255) if column < 4 else (255, 255, 255))
    return bytes(rows)


def make_images():
    """An image XObject drawn twice (nearest and bilinear) plus a text line."""
    writer = Writer()
    writer.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
    writer.obj(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
    writer.obj(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R"
        b" /Resources << /XObject << /Im1 5 0 R /Im2 6 0 R >>"
        b" /Font << /F1 7 0 R >> >> >>",
    )
    writer.stream(4, b"", IMAGES_CONTENT)
    image_dict = (
        b"/Type /XObject /Subtype /Image /Width 8 /Height 8"
        b" /ColorSpace /DeviceRGB /BitsPerComponent 8"
    )
    writer.stream(5, image_dict, image_pixels(), compress=True)
    writer.stream(
        6,
        image_dict + b" /Interpolate true",
        image_pixels(),
        compress=True,
    )
    writer.obj(7, HELVETICA)
    offsets = writer.offsets
    xref = len(writer.out)
    writer.out += b"xref\n0 %d\n" % (max(offsets) + 1)
    writer.out += b"0000000000 65535 f \n"
    for number in range(1, max(offsets) + 1):
        writer.out += b"%010d 00000 n \n" % offsets[number]
    writer.out += b"trailer\n<< /Size %d /Root 1 0 R >>\n" % (max(offsets) + 1)
    writer.out += b"startxref\n%d\n%%%%EOF\n" % xref
    write("images.pdf", bytes(writer.out))


CALIBRATION_FONT = (
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica"
    b" /Encoding /WinAnsiEncoding >>"
)

CALIBRATION_COLOUR = b"".join(
    b"q %s %d %d 40 40 re f Q\n" % (operator, x, y)
    for operator, x, y in (
        (b"0 g", 10, 150),
        (b"0.5 g", 56, 150),
        (b"0.25 g", 102, 150),
        (b"0.75 g", 148, 150),
        (b"1 0 0 rg", 10, 104),
        (b"0 1 0 rg", 56, 104),
        (b"0 0 1 rg", 102, 104),
        (b"0.2 0.4 1 rg", 148, 104),
        (b"1 0 0 0 k", 10, 58),
        (b"0 0 0 0.5 k", 56, 58),
        (b"0 1 1 0 k", 102, 58),
        (b"1 1 0 1 k", 148, 58),
    )
)

# Four cubic Beziers approximating the circle of radius 30 at (250, 230); the
# control-point offset is the usual 0.5522847498 r.
CALIBRATION_CIRCLE = (
    b"280 230 m"
    b" 280 246.56854 266.56854 260 250 260 c"
    b" 233.43146 260 220 246.56854 220 230 c"
    b" 220 213.43146 233.43146 200 250 200 c"
    b" 266.56854 200 280 213.43146 280 230 c h"
)

CALIBRATION_STROKES = (
    b"q 12 w 0 J 40 260 m 140 260 l S Q\n"
    b"q 12 w 1 J 40 230 m 140 230 l S Q\n"
    b"q 12 w 2 J 40 200 m 140 200 l S Q\n"
    b"q 12 w 0 j 20 120 m 80 120 l 80 60 l S Q\n"
    b"q 12 w 2 j 120 120 m 180 120 l 180 60 l S Q\n"
    b"q 1 0 0 rg 20 20 60 60 re 40 40 20 20 re f Q\n"
    b"q 0 0 1 rg 140 20 60 60 re 160 40 20 20 re f* Q\n"
    b"q 1 0 1 RG 4 w [20 10] 5 d 120 160 m 280 160 l S Q\n"
    b"q 0 0.6 0 rg " + CALIBRATION_CIRCLE + b" f Q\n"
)

# A 2x2 RGB checkerboard: red and green on the top row, blue and white below.
CALIBRATION_CHECKER = bytes(
    (255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255)
)

CALIBRATION_IMAGES = (
    b"q 120 0 0 120 20 20 cm /Chk Do Q\n"
    b"q 120 0 0 120 20 160 cm /Inv Do Q\n"
    b"q 60 0 0 60 160 20 cm /Gray Do Q\n"
    b"q 60 0 0 60 240 20 cm /Idx Do Q\n"
    b"q 0 120 -120 0 280 160 cm /Chk Do Q\n"
)

CALIBRATION_PAGE_BOX = (
    b"q 1 0 0 rg 50 60 20 20 re f Q\n"
    b"q 0 0 1 rg 230 240 20 20 re f Q\n"
)

CALIBRATION_ROTATED = (
    b"q 1 0 0 rg 0 0 100 100 re f Q\n"
    b"q 0 0 1 rg 200 0 100 100 re f Q\n"
)

# Text state persists across `BT`, so every run states the values it relies on.
CALIBRATION_TEXT = (
    b"BT /F1 40 Tf 100 Tz 20 150 Td (HELLO) Tj ET\n"
    b"BT /F1 40 Tf 50 Tz 20 80 Td (HELLO) Tj ET\n"
    b"BT /F1 20 Tf 100 Tz 0 Tw 20 60 Td (A A A) Tj ET\n"
    b"BT /F1 20 Tf 100 Tz 40 Tw 20 20 Td (A A A) Tj ET\n"
)


# /Widths in 1/1000 em, indexed from /FirstChar. The widths page's fonts differ
# only in that table, so their glyph positions are arithmetic.
def calibration_widths(first, last, overrides):
    return b"[" + b" ".join(
        b"%d" % overrides.get(code, 500) for code in range(first, last + 1)
    ) + b"]"


CALIBRATION_WIDTHS_TEXT = (
    b"BT /F1 20 Tf 20 150 Td (I I) Tj ET\n"
    b"BT /F2 20 Tf 20 100 Td (I I) Tj ET\n"
    b"BT /F3 20 Tf 20 50 Td (Z Z) Tj ET\n"
)

CALIBRATION_UNSUPPORTED_TEXT = b"BT /F1 12 Tf 20 100 Td (hi) Tj ET\n"



# A real TrueType program, written here so the embedded-font path has a fixture
# and so a later composite-font fixture has glyphs whose shape is known: `A` is
# a filled box, `B` the same box with a hole, `space` empty. Units per em 1000,
# advance 600.
def truetype_font():
    import struct

    units = 1000
    # Glyphs are the renderer's 5x7 bitmap alphabet, at 100 units per cell, so
    # the pages read as letters instead of anonymous boxes while every pixel is
    # still derivable: `A` and `B` keep a closed hole.
    patterns = {
        "A": [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        "B": [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        "H": [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
    }
    cell = 100
    left = 50

    def contours_for(rows):
        contours = []
        for row, bits in enumerate(rows):
            for column in range(5):
                if bits & (0b10000 >> column):
                    x0 = left + column * cell
                    y0 = (6 - row) * cell
                    contours.append(
                        [(x0, y0), (x0 + cell, y0), (x0 + cell, y0 + cell), (x0, y0 + cell)]
                    )
        return contours

    glyphs = [[]] + [contours_for(patterns[name]) for name in ("A", "B")]
    cmap = {0x20: 0, 0x41: 1, 0x42: 2}
    advance = 600

    def glyf_entry(contours):
        if not contours:
            return b""
        points = [point for contour in contours for point in contour]
        xs = [x for x, _ in points]
        ys = [y for _, y in points]
        data = struct.pack(">hhhhh", len(contours), min(xs), min(ys), max(xs), max(ys))
        end = -1
        for contour in contours:
            end += len(contour)
            data += struct.pack(">H", end)
        data += struct.pack(">H", 0)  # no instructions
        data += bytes([0x01]) * len(points)  # on-curve, 16-bit deltas
        previous = 0
        for x, _ in points:
            data += struct.pack(">h", x - previous)
            previous = x
        previous = 0
        for _, y in points:
            data += struct.pack(">h", y - previous)
            previous = y
        padded = data + bytes((-len(data)) % 4)
        return padded

    def checksum(data):
        padded = data + bytes((-len(data)) % 4)
        return sum(struct.unpack(">%dI" % (len(padded) // 4), padded)) & 0xFFFFFFFF

    glyf = b"".join(glyf_entry(contours) for contours in glyphs)
    offsets = []
    position = 0
    for contours in glyphs:
        offsets.append(position)
        position += len(glyf_entry(contours))
    offsets.append(position)
    loca = b"".join(struct.pack(">H", offset // 2) for offset in offsets)

    # cmap format 4: one segment per code plus the mandatory 0xFFFF sentinel.
    codes = sorted(code for code in cmap if cmap[code] != 0)
    segments = len(codes) + 1
    ends = b"".join(struct.pack(">H", code) for code in codes) + struct.pack(">H", 0xFFFF)
    starts = b"".join(struct.pack(">H", code) for code in codes) + struct.pack(">H", 0xFFFF)
    deltas = b"".join(
        struct.pack(">H", (cmap[code] - code) % 0x10000) for code in codes
    ) + struct.pack(">H", 1)
    ranges = b"\x00\x00" * segments
    search = 2 ** (segments.bit_length() - 1) * 2
    subtable_body = ends + b"\x00\x00" + starts + deltas + ranges
    subtable = struct.pack(
        ">HHHHHHH",
        4,
        14 + len(subtable_body),
        0,
        segments * 2,
        search,
        search.bit_length() - 2,
        segments * 2 - search,
    ) + subtable_body
    cmap_table = struct.pack(">HHHHI", 0, 1, 3, 1, 12) + subtable

    hmtx = b"".join(struct.pack(">Hh", advance, 50) for _ in glyphs)
    hhea_fields = [
        0x00010000,  # version
        800, -200, 0,  # ascender, descender, lineGap
        advance,  # advanceWidthMax
        50, 250, 550,  # min left/right side bearing, xMaxExtent
        1, 0, 0,  # caret slope rise/run, caret offset
        0, 0, 0, 0,  # reserved
        0,  # metricDataFormat
        len(glyphs),  # numberOfHMetrics
    ]
    hhea = struct.pack(">I" + "hhh" + "H" + "h" * 11 + "H", *hhea_fields)
    maxp = struct.pack(">IH", 0x00010000, len(glyphs)) + b"\x00\x32" * 13
    head = struct.pack(
        ">IIIIHHQQhhhhHHhhh",
        0x00010000, 0x00010000, 0, 0x5F0F3CF5, 0x000B, units,
        0, 0, -50, -200, 600, 750, 0, 8, 2, 0, 0,
    )
    post = struct.pack(">IIhhIIIII", 0x00030000, 0, -100, 50, 1, 0, 0, 0, 0)
    name = struct.pack(">HHH", 0, 1, 18) + struct.pack(
        ">HHHHHH", 3, 1, 0x0409, 1, len(b"pdf2 fixture"), 0
    ) + b"pdf2 fixture"

    tables = {
        b"cmap": cmap_table,
        b"glyf": glyf,
        b"head": head,
        b"hhea": hhea,
        b"hmtx": hmtx,
        b"loca": loca,
        b"maxp": maxp,
        b"name": name,
        b"post": post,
    }
    tags = sorted(tables)
    count = len(tags)
    search = 2 ** (count.bit_length() - 1) * 16
    header = struct.pack(">IHHHH", 0x00010000, count, search, count.bit_length() - 1, count * 16 - search)
    offset = 12 + count * 16
    directory = b""
    body = b""
    for tag in tags:
        data = tables[tag]
        padded = data + bytes((-len(data)) % 4)
        directory += struct.pack(">4sIII", tag, checksum(data), offset, len(data))
        body += padded
        offset += len(padded)
    font = bytearray(header + directory + body)
    adjustment = (0xB1B0AFBA - checksum(bytes(font))) & 0xFFFFFFFF
    head_at = 12 + count * 16 + sum(
        len(tables[tag]) + (-len(tables[tag]) % 4) for tag in tags[: tags.index(b"head")]
    )
    struct.pack_into(">I", font, head_at + 8, adjustment)
    return bytes(font)


CALIBRATION_EMBEDDED_TEXT = b"BT /F1 40 Tf 20 150 Td (ABA) Tj ET\n"

# Two-byte codes: Identity-H maps code to CID, and /CIDToGIDMap maps CID to glyph
# id. CID 1 is glyph 1 (`A`) and CID 2 is glyph 2 (`B`); page 11 swaps them.
CALIBRATION_CID_TEXT = b"BT /F1 40 Tf 20 150 Td <000100020001> Tj ET\n"


# A tiny TrueType reader, so the book page can be typeset with the font's own
# metrics instead of guessed ones: cmap format 4 for code -> glyph, hmtx for
# advances. The font itself is a Crimson Text subset (OFL), committed beside it.
def truetype_metrics(path):
    import struct

    data = open(path, "rb").read()
    table_count = struct.unpack(">H", data[4:6])[0]
    tables = {}
    for index in range(table_count):
        record = 12 + index * 16
        tag = data[record : record + 4]
        offset, length = struct.unpack(">II", data[record + 8 : record + 16])
        tables[tag] = offset
    units = struct.unpack(">H", data[tables[b"head"] + 18 : tables[b"head"] + 20])[0]
    metrics = struct.unpack(">H", data[tables[b"hhea"] + 34 : tables[b"hhea"] + 36])[0]
    glyphs = struct.unpack(">H", data[tables[b"maxp"] + 4 : tables[b"maxp"] + 6])[0]
    hmtx = tables[b"hmtx"]
    advances = []
    for index in range(metrics):
        advances.append(struct.unpack(">H", data[hmtx + index * 4 : hmtx + index * 4 + 2])[0])
    advances.extend([advances[-1]] * (glyphs - metrics))

    cmap = tables[b"cmap"]
    subtable = None
    for index in range(struct.unpack(">H", data[cmap + 2 : cmap + 4])[0]):
        record = cmap + 4 + index * 8
        platform, encoding, offset = struct.unpack(">HHI", data[record : record + 8])
        if platform == 3 and encoding == 1:
            subtable = cmap + offset
    fmt, _, _, segments_x2 = struct.unpack(">HHHH", data[subtable : subtable + 8])
    assert fmt == 4, fmt
    segments = segments_x2 // 2
    ends_at = subtable + 14
    starts_at = ends_at + segments * 2 + 2
    deltas_at = starts_at + segments * 2
    ranges_at = deltas_at + segments * 2
    mapping = {}
    for segment in range(segments):
        end = struct.unpack(">H", data[ends_at + segment * 2 : ends_at + segment * 2 + 2])[0]
        start = struct.unpack(">H", data[starts_at + segment * 2 : starts_at + segment * 2 + 2])[0]
        delta = struct.unpack(">h", data[deltas_at + segment * 2 : deltas_at + segment * 2 + 2])[0]
        range_offset = struct.unpack(
            ">H", data[ranges_at + segment * 2 : ranges_at + segment * 2 + 2]
        )[0]
        for code in range(start, min(end, 0xFF) + 1):
            if code == 0xFFFF:
                continue
            if range_offset == 0:
                glyph = (code + delta) & 0xFFFF
            else:
                address = ranges_at + segment * 2 + range_offset + (code - start) * 2
                glyph = struct.unpack(">H", data[address : address + 2])[0]
                if glyph:
                    glyph = (glyph + delta) & 0xFFFF
            if glyph:
                mapping[code] = glyph
    return units, mapping, advances


# A page of a book, set in the committed Crimson Text subset: public-domain
# prose, wrapped to the measure with the font's own advances.
BOOK_PARAGRAPHS = [
    "The Time Traveller (for so it will be convenient to speak of him) was "
    "expounding a recondite matter to us. His grey eyes shone and twinkled, and "
    "his usually pale face was flushed and animated. The fire burned brightly, "
    "and the soft radiance of the incandescent lights in the lilies of silver "
    "caught the bubbles that flashed and passed in our glasses.",
    "Our chairs, being his patents, embraced and caressed us rather than "
    "submitted to be sat upon, and there was that luxurious after-dinner "
    "atmosphere when thought runs gracefully free of the trammels of precision.",
    "And he put it to us in this way - marking the points with a lean "
    "forefinger - as we sat and lazily admired his earnestness over this new "
    "paradox (as we thought it) and his fecundity.",
    "\"You must follow me carefully. I shall have to controvert one or two ideas "
    "that are almost universally accepted. The geometry, for instance, they "
    "taught you at school is founded on a misconception.\"",
    "\"Is not that rather a large thing to expect us to begin upon?\" said "
    "Filby, an argumentative person with red hair.",
    "\"I do not mean to ask you to accept anything without reasonable ground for "
    "it. You will soon admit as much as I need from you. You know of course "
    "that a mathematical line, a line of thickness nil, has no real existence.",
]


def make_book():
    """A page of prose in an embedded serif font, for the demo and the goldens."""
    units, mapping, advances = truetype_metrics(
        os.path.join(HERE, "fonts", "CrimsonText-Regular-subset.ttf")
    )
    size = 11.5
    leading = 16.0
    left = 54.0
    top = 520.0
    measure = 420.0 - 2 * left
    font_program = open(
        os.path.join(HERE, "fonts", "CrimsonText-Regular-subset.ttf"), "rb"
    ).read()

    def text_width(text, at_size=size):
        return sum(advances[mapping.get(ord(character), 0)] for character in text) / units * at_size

    def escape(text):
        return text.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")

    paragraphs = []
    for paragraph in BOOK_PARAGRAPHS:
        lines = []
        line = ""
        for word in paragraph.split():
            candidate = word if not line else f"{line} {word}"
            if text_width(candidate) > measure and line:
                lines.append(line)
                line = word
            else:
                line = candidate
        if line:
            lines.append(line)
        paragraphs.append(lines)

    content = bytearray(b"BT\n/F1 %g Tf\n" % size)
    y = top
    for number, lines in enumerate(paragraphs):
        if number > 0:
            y -= leading * 0.7  # a paragraph break, not a full line
        for line in lines:
            content += b"1 0 0 1 %g %g Tm (%s) Tj\n" % (left, y, escape(line).encode("latin-1"))
            y -= leading
    content += b"ET\n"

    writer = Writer()
    writer.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
    writer.obj(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
    writer.obj(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 420 595] /Contents 4 0 R"
        b" /Resources << /Font << /F1 5 0 R >> >> >>",
    )
    writer.stream(4, b"", bytes(content), compress=True)

    first, last = 32, max(mapping)
    widths = [advances[mapping.get(code, 0)] for code in range(first, last + 1)]
    writer.obj(
        5,
        b"<< /Type /Font /Subtype /TrueType /BaseFont /CrimsonText-Regular"
        b" /Encoding /WinAnsiEncoding /FirstChar %d /LastChar %d /Widths [%s]"
        b" /FontDescriptor 6 0 R >>"
        % (first, last, b" ".join(b"%d" % width for width in widths)),
    )
    writer.obj(
        6,
        b"<< /Type /FontDescriptor /FontName /CrimsonText-Regular /Flags 32"
        b" /FontBBox [0 -200 1000 800] /ItalicAngle 0 /Ascent 800 /Descent -200"
        b" /CapHeight 660 /StemV 80 /MissingWidth 500 /FontFile2 7 0 R >>",
    )
    writer.stream(
        7,
        b"/Length1 %d" % len(font_program),
        font_program,
        compress=True,
    )
    offsets = writer.offsets
    xref = len(writer.out)
    writer.out += b"xref\n0 %d\n" % (max(offsets) + 1)
    writer.out += b"0000000000 65535 f \n"
    for number in range(1, max(offsets) + 1):
        writer.out += b"%010d 00000 n \n" % offsets[number]
    writer.out += b"trailer\n<< /Size %d /Root 1 0 R >>\n" % (max(offsets) + 1)
    writer.out += b"startxref\n%d\n%%%%EOF\n" % xref
    write("book.pdf", bytes(writer.out))



CROPBOX_CONTENT = (
    b"q 1 0 0 rg 60 70 30 30 re f Q\n"
    b"q 0 0 1 rg 200 220 30 30 re f Q\n"
    b"q 0 0.6 0 rg 230 60 40 40 re f Q\n"
)


def make_cropbox():
    """A page whose visible box and user unit are not the media box's.

    `/MediaBox [0 0 300 200]` with `/CropBox [50 60 250 260]` and `/UserUnit 2`:
    the raster is the crop box at twice its size (400x400), the origin is the
    crop box's, and the content outside the crop box is not visible.
    """
    writer = Writer()
    writer.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
    writer.obj(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
    writer.obj(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200]"
        b" /CropBox [50 60 250 260] /UserUnit 2 /Contents 4 0 R >>",
    )
    writer.stream(4, b"", CROPBOX_CONTENT)
    offsets = writer.offsets
    xref = len(writer.out)
    writer.out += b"xref\n0 %d\n" % (max(offsets) + 1)
    writer.out += b"0000000000 65535 f \n"
    for number in range(1, max(offsets) + 1):
        writer.out += b"%010d 00000 n \n" % offsets[number]
    writer.out += b"trailer\n<< /Size %d /Root 1 0 R >>\n" % (max(offsets) + 1)
    writer.out += b"startxref\n%d\n%%%%EOF\n" % xref
    write("cropbox.pdf", bytes(writer.out))



def make_generation_edge():
    """A classic table whose free-entry generation is 65536, not 65535.

    Writers exist that put 65536 in the free head's generation field (the
    standard says all-ones, 65535). The value is informational — a free entry is
    never looked up — so a reader clamps it rather than refusing the file. The
    fixture is the corpus edge, reduced to its pattern.
    """
    writer = Writer()
    writer.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
    writer.obj(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
    writer.obj(3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>")
    writer.stream(4, b"", b"q 1 0 0 rg 10 10 20 20 re f Q")
    offsets = writer.offsets
    xref = len(writer.out)
    writer.out += b"xref\n0 %d\n" % (max(offsets) + 1)
    writer.out += b"0000000000 65536 f \n"
    for number in range(1, max(offsets) + 1):
        writer.out += b"%010d 00000 n \n" % offsets[number]
    writer.out += b"trailer\n<< /Size %d /Root 1 0 R >>\n" % (max(offsets) + 1)
    writer.out += b"startxref\n%d\n%%%%EOF\n" % xref
    write("xref-generation.pdf", bytes(writer.out))



def make_reading_order():
    """Three lines painted bottom, top, middle — reading order is by position.

    A writer may paint a page in any order; the extracted text must follow the
    page, not the paint order. The middle line gets its space from a `TJ`
    adjustment rather than a space glyph, so both sources are covered.
    """
    content = (
        b"BT /F1 12 Tf\n"
        b"1 0 0 1 50 300 Tm (third line) Tj\n"
        b"1 0 0 1 50 400 Tm [(first) -400 (line)] TJ\n"
        b"1 0 0 1 50 350 Tm (second line) Tj\n"
        b"ET"
    )
    writer = Writer()
    writer.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
    writer.obj(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
    writer.obj(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R "
        b"/Resources << /Font << /F1 5 0 R >> >> >>",
    )
    writer.stream(4, b"", content)
    writer.obj(5, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>")
    classic_table(writer.out, writer.offsets, b"/Root 1 0 R", len(writer.out))
    write("reading-order.pdf", bytes(writer.out))



TOUNICODE_CMAP = b"""/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
/CMapName /Adobe-Identity-UCS def
1 begincodespacerange
<0000> <FFFF>
endcodespacerange
2 beginbfchar
<0003> <0041>
<0004> <0044>
endbfchar
1 beginbfrange
<0010> <0012> [<0061> <0062> <0063>]
endbfrange
1 beginbfrange
<0020> <0022> <0078>
endbfrange
endcmap
CMapName currentdict /CMap defineresource pop
end
end
"""


def make_tounicode():
    """A Type0 page whose text comes only from its `/ToUnicode` CMap.

    The codes are deliberately meaningless (0x0003 has no relation to "A") so
    that text can only come out right through the mapping: two `bfchar` entries,
    a `bfrange` with an array destination and one with a single destination that
    increments. The content stream also carries a code the CMap does not map, so
    a mapping failure cannot hide as a dropped glyph.
    """
    content = (
        b"BT /F1 12 Tf 50 700 Td "
        b"<00030004001000110012002000210022> Tj "
        b"120 0 Td <0003> Tj ET"
    )
    writer = Writer()
    writer.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
    writer.obj(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
    writer.obj(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R "
        b"/Resources << /Font << /F1 5 0 R >> >> >>",
    )
    writer.stream(4, b"", content)
    writer.obj(
        5,
        b"<< /Type /Font /Subtype /Type0 /BaseFont /Helvetica /Encoding /Identity-H "
        b"/DescendantFonts [7 0 R] /ToUnicode 6 0 R >>",
    )
    writer.stream(6, b"", TOUNICODE_CMAP)
    writer.obj(
        7,
        b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Helvetica "
        b"/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> "
        b"/DW 1000 /W [3 [500 700]] >>",
    )
    classic_table(writer.out, writer.offsets, b"/Root 1 0 R", len(writer.out))
    write("tounicode.pdf", bytes(writer.out))



def make_startxref_hint():
    """A table whose `startxref` points *inside* the table, not at `xref`.

    The corpus file `encrypted-attachment.pdf` does exactly this (offset 2242
    against `xref` at 2223), and refusing it for a bad hint rejects a file that is
    otherwise fine. The reader falls back to the nearest `xref` keyword before the
    hint; this fixture is that pattern without the encryption.
    """
    writer = Writer()
    writer.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
    writer.obj(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
    writer.obj(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R "
        b"/Resources << /Font << /F1 5 0 R >> >> >>",
    )
    writer.stream(4, b"", b"BT /F1 12 Tf 10 50 Td (hint) Tj ET")
    writer.obj(5, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    xref = len(writer.out)
    writer.out += b"xref\n0 %d\n" % (max(writer.offsets) + 1)
    writer.out += b"0000000000 65535 f \n"
    for number in range(1, max(writer.offsets) + 1):
        writer.out += b"%010d 00000 n \n" % writer.offsets[number]
    writer.out += b"trailer\n<< /Size %d /Root 1 0 R >>\n" % (max(writer.offsets) + 1)
    # Deliberately wrong: 19 bytes into the table, which is where a writer that
    # counted the wrong offset lands.
    writer.out += b"startxref\n%d\n%%%%EOF\n" % (xref + 19)
    write("startxref-hint.pdf", bytes(writer.out))



def make_text_mode():
    """Two identical lines, one painted and one in `Tr 3` (invisible).

    The PDF text rendering mode decides whether glyphs are filled, stroked or
    drawn at all; a renderer that ignores it paints text a viewer hides, which is
    how watermarks and OCR layers leak into images.
    """
    content = (
        b"BT /F1 12 Tf\n"
        b"1 0 0 1 20 150 Tm 0 Tr (AB) Tj\n"
        b"1 0 0 1 20 100 Tm 3 Tr (CD) Tj\n"
        b"ET"
    )
    writer = Writer()
    writer.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
    writer.obj(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
    writer.obj(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R "
        b"/Resources << /Font << /F1 5 0 R >> >> >>",
    )
    writer.stream(4, b"", content)
    writer.obj(5, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    classic_table(writer.out, writer.offsets, b"/Root 1 0 R", len(writer.out))
    write("text-mode.pdf", bytes(writer.out))



def make_invisible_ink():
    """A page that paints in the background colour: every operator supported, no ink.

    The fixture for the renderer's honesty rule: a page that ran painting
    operators and produced an empty raster reports `drew-nothing` instead of
    claiming everything was drawn.
    """
    content = b"1 1 1 rg 10 10 180 180 re f\nQ 1 1 1 RG 2 w 30 30 m 150 150 l S\n"
    writer = Writer()
    writer.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
    writer.obj(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
    writer.obj(3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R >>")
    writer.stream(4, b"", content)
    classic_table(writer.out, writer.offsets, b"/Root 1 0 R", len(writer.out))
    write("invisible-ink.pdf", bytes(writer.out))




def make_blank_page():
    """Two blank pages: one with an empty content stream, one with none at all.

    Blank by design, so the renderer must not report `drew-nothing` — that note
    is for a page that painted and produced nothing.
    """
    writer = Writer()
    writer.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
    writer.obj(
        2,
        b"<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>",
    )
    writer.obj(3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>")
    writer.stream(4, b"", b"")
    writer.obj(5, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>")
    classic_table(writer.out, writer.offsets, b"/Root 1 0 R", len(writer.out))
    write("blank-page.pdf", bytes(writer.out))



def make_forms():
    """Four form XObjects, each proving one rule of 8.10.

    - `/F1` carries a `/Matrix` that scales by two and translates to (20, 20), so
      its red square lands where the matrix says and nowhere else.
    - `/F2` has a 40 × 40 `/BBox` but paints 120 × 120, so the overflow must be
      clipped by the box rather than by the canvas.
    - `/F3` has **no** `/Resources` of its own and draws `/Inner`, which lives in
      the page's XObject dictionary: resource inheritance, proved by ink.
    - `/F4` draws `/Deep`, which is a form drawing a form.
    """
    page_content = b"/F1 Do\nq 1 0 0 1 0 0 cm /F2 Do Q\n/F3 Do\n/F4 Do\n"
    writer = Writer()
    writer.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
    writer.obj(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
    writer.obj(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R "
        b"/Resources << /XObject << /F1 5 0 R /F2 6 0 R /F3 7 0 R /F4 8 0 R "
        b"/Inner 9 0 R /Deep 10 0 R >> >> >>",
    )
    writer.stream(4, b"", page_content)
    writer.stream(
        5,
        b"/Type /XObject /Subtype /Form /BBox [0 0 50 50] /Matrix [2 0 0 2 20 20]",
        b"1 0 0 rg 0 0 25 25 re f",
    )
    writer.stream(
        6,
        b"/Type /XObject /Subtype /Form /BBox [0 0 40 40]",
        b"0 1 0 rg 0 0 120 120 re f",
    )
    # No /Resources entry: the page's dictionary has to be inherited.
    writer.stream(7, b"/Type /XObject /Subtype /Form /BBox [0 0 200 200]", b"/Inner Do")
    writer.stream(8, b"/Type /XObject /Subtype /Form /BBox [0 0 200 200]", b"/Deep Do")
    writer.stream(
        9,
        b"/Type /XObject /Subtype /Form /BBox [0 0 20 20] /Matrix [1 0 0 1 150 150]",
        b"1 0 1 rg 0 0 20 20 re f",
    )
    writer.stream(
        10,
        b"/Type /XObject /Subtype /Form /BBox [0 0 20 20] /Matrix [1 0 0 1 100 20]",
        b"0 0 1 rg 0 0 20 20 re f",
    )
    classic_table(writer.out, writer.offsets, b"/Root 1 0 R", len(writer.out))
    write("forms.pdf", bytes(writer.out))


def make_form_cycle():
    """A form that draws itself: the depth cap must report it, not follow it."""
    writer = Writer()
    writer.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
    writer.obj(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
    writer.obj(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R "
        b"/Resources << /XObject << /Loop 5 0 R >> >> >>",
    )
    writer.stream(4, b"", b"/Loop Do")
    writer.stream(
        5,
        b"/Type /XObject /Subtype /Form /BBox [0 0 100 100]",
        b"0 0 1 rg 0 0 10 10 re f /Loop Do",
    )
    classic_table(writer.out, writer.offsets, b"/Root 1 0 R", len(writer.out))
    write("form-cycle.pdf", bytes(writer.out))



def make_annotations():
    """A page whose only content is annotation appearance streams.

    This is the shape that made 25 of the 49 silent-blank sample pages blank: an
    empty `/Contents` and everything visible inside `/Annots → /AP → /N`. Three
    annotations, each proving one rule of 12.5.5:

    - a square whose appearance has `/BBox [0 0 10 10]` and `/Matrix [2 0 0 2 0 0]`
      painting a 5 × 5 red square — the transformed box is what maps onto `/Rect`;
    - a widget whose `/AP /N` is a *dictionary* keyed by `/AS`, with `/On` green
      and `/Off` grey, so the state selection is visible;
    - a hidden one (`/F 2`) painting blue where nothing may appear.
    """
    writer = Writer()
    writer.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
    writer.obj(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
    writer.obj(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R "
        b"/Annots [10 0 R 11 0 R 12 0 R] >>",
    )
    writer.stream(4, b"", b"")
    writer.stream(
        5,
        b"/Type /XObject /Subtype /Form /BBox [0 0 10 10] /Matrix [2 0 0 2 0 0]",
        b"1 0 0 rg 0 0 5 5 re f",
    )
    writer.stream(
        6,
        b"/Type /XObject /Subtype /Form /BBox [0 0 50 50]",
        b"0 1 0 rg 0 0 50 50 re f",
    )
    writer.stream(
        7,
        b"/Type /XObject /Subtype /Form /BBox [0 0 50 50]",
        b"0.5 0.5 0.5 rg 0 0 50 50 re f",
    )
    writer.stream(
        8,
        b"/Type /XObject /Subtype /Form /BBox [0 0 50 50]",
        b"0 0 1 rg 0 0 50 50 re f",
    )
    writer.obj(9, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    writer.obj(
        10,
        b"<< /Type /Annot /Subtype /Square /Rect [50 50 100 100] /F 4 /AP << /N 5 0 R >> >>",
    )
    writer.obj(
        11,
        b"<< /Type /Annot /Subtype /Widget /Rect [120 50 170 100] /F 4 /AS /On "
        b"/AP << /N << /On 6 0 R /Off 7 0 R >> >> >>",
    )
    writer.obj(
        12,
        b"<< /Type /Annot /Subtype /Square /Rect [50 120 100 170] /F 2 /AP << /N 8 0 R >> >>",
    )
    classic_table(writer.out, writer.offsets, b"/Root 1 0 R", len(writer.out))
    write("annotations.pdf", bytes(writer.out))



def make_alpha():
    """`/ExtGState` constant alpha, with known arithmetic.

    50 % red over white is (255, 128, 128) — the canvas blends
    `source·alpha + destination·(1-alpha)`, so the number is checkable rather than
    "looks pinker". Three states: fill alpha, stroke alpha, and one that only
    sets a dash and a line width.
    """
    content = (
        b"q /GSFill gs 1 0 0 rg 20 20 60 60 re f Q\n"
        b"q 1 0 0 rg 120 20 60 60 re f Q\n"
        b"q /GSStroke gs 0 0 1 RG 4 w 20 120 m 180 120 l S Q\n"
        b"q /GSDash gs 0 0 0 RG 6 w 20 160 m 180 160 l S Q\n"
    )
    writer = Writer()
    writer.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
    writer.obj(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
    writer.obj(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R "
        b"/Resources << /ExtGState << /GSFill 5 0 R /GSStroke 6 0 R /GSDash 7 0 R >> >> >>",
    )
    writer.stream(4, b"", content)
    writer.obj(5, b"<< /Type /ExtGState /ca 0.5 /CA 1 /BM /Normal >>")
    writer.obj(6, b"<< /Type /ExtGState /ca 1 /CA 0.5 >>")
    writer.obj(7, b"<< /Type /ExtGState /LW 6 /LC 0 /LJ 1 /ML 4 /D [[12 12] 0] >>")
    classic_table(writer.out, writer.offsets, b"/Root 1 0 R", len(writer.out))
    write("alpha.pdf", bytes(writer.out))

def make_calibration():
    """Pages whose correct pixels follow from the specification.

    Page 1 is colour: DeviceGray, DeviceRGB and DeviceCMYK cells whose byte
    values are arithmetic. Page 2 is strokes: the three cap styles, a miter and
    a bevel join, the nonzero and even-odd fill rules on identical geometry, a
    dash phase, and a Bezier circle whose area is pi r^2. Page 3 is images: a
    2x2 checkerboard at integer scale (exact block edges), the same image
    inverted by /Decode, gray and /Indexed images, and the checkerboard on a
    quarter-turn matrix. Page 4 is page boxes: a /MediaBox with a non-zero
    origin, and a /Rotate 90 page. Page 5 is text metrics: horizontal scaling
    and word spacing measured against the same string at 100%.

    Object numbers: 1 catalog, 2 page tree, pages at 3/5/7/9/11/13 with their
    content streams at 4/6/8/10/12/14, 15 the font, 16..19 the images, 20/22/28
    the /Widths, unsupported-font and embedded-TrueType pages with their streams
    at 21/23/29, 24..27 the fonts those pages use, and 30..32 the embedded
    TrueType font, its descriptor and its program.
    """
    writer = Writer()
    writer.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
    writer.obj(
        2,
        b"<< /Type /Pages /Kids [3 0 R 5 0 R 7 0 R 9 0 R 11 0 R 13 0 R 20 0 R 22 0 R 28 0 R"
        b" 33 0 R 39 0 R] /Count 11 >>",
    )

    def page(number, media, content, resources=b"", rotate=None):
        extra = b" /Rotate %d" % rotate if rotate is not None else b""
        writer.stream(number + 1, b"", content)
        writer.obj(
            number,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [%s] /Contents %d 0 R%s"
            b" /Resources << %s >> >>"
            % (media, number + 1, extra, resources),
        )

    page(3, b"0 0 200 200", CALIBRATION_COLOUR)
    page(5, b"0 0 300 300", CALIBRATION_STROKES)
    page(
        7,
        b"0 0 400 300",
        CALIBRATION_IMAGES,
        b"/XObject << /Chk 16 0 R /Inv 17 0 R /Gray 18 0 R /Idx 19 0 R >>",
    )
    page(9, b"50 60 250 260", CALIBRATION_PAGE_BOX)
    page(11, b"0 0 300 100", CALIBRATION_ROTATED, rotate=90)

    def image(number, extra, pixels, width, height):
        writer.stream(
            number,
            b"/Type /XObject /Subtype /Image /Width %d /Height %d"
            b" /BitsPerComponent 8" % (width, height) + extra,
            pixels,
            compress=True,
        )

    image(16, b" /ColorSpace /DeviceRGB", CALIBRATION_CHECKER, 2, 2)
    image(
        17,
        b" /ColorSpace /DeviceRGB /Decode [1 0 1 0 1 0]",
        CALIBRATION_CHECKER,
        2,
        2,
    )
    image(18, b" /ColorSpace /DeviceGray", bytes((0, 255)), 2, 1)
    image(
        19,
        b" /ColorSpace [/Indexed /DeviceRGB 1 <FF00000000FF>]",
        bytes((0, 1)),
        2,
        1,
    )

    writer.obj(15, CALIBRATION_FONT)
    page(13, b"0 0 300 200", CALIBRATION_TEXT, b"/Font << /F1 15 0 R >>")

    # Page 7: three fonts that differ only in /Widths, so the distance between
    # two identical glyphs is the space advance — 250, 750 and, for a code the
    # array does not cover, the descriptor's /MissingWidth of 900.
    page(
        20,
        b"0 0 300 200",
        CALIBRATION_WIDTHS_TEXT,
        b"/Font << /F1 24 0 R /F2 25 0 R /F3 26 0 R >>",
    )

    # Page 8: a Type 0 (CID) font, which this renderer cannot draw yet; it has to
    # be reported rather than painted with fallback glyphs.
    page(22, b"0 0 300 200", CALIBRATION_UNSUPPORTED_TEXT, b"/Font << /F1 27 0 R >>")

    def width_font(number, first, last, overrides, descriptor=b""):
        writer.obj(
            number,
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica"
            b" /Encoding /WinAnsiEncoding /FirstChar %d /LastChar %d"
            b" /Widths %s %s >>"
            % (first, last, calibration_widths(first, last, overrides), descriptor),
        )

    width_font(24, 32, 73, {32: 250, 73: 333})
    width_font(25, 32, 73, {32: 750, 73: 333})
    width_font(
        26,
        65,
        65,
        {},
        b" /FontDescriptor << /Type /FontDescriptor /FontName /Helvetica"
        b" /Flags 32 /MissingWidth 900 /FontBBox [0 0 1000 1000]"
        b" /ItalicAngle 0 /Ascent 700 /Descent -200 /CapHeight 700 /StemV 80 >>",
    )
    # Page 9: a real embedded TrueType program, written by this generator. `A`
    # is a filled box and `B` a box with a hole, so a probe can tell an outline
    # from the bitmap fallback.
    page(28, b"0 0 300 200", CALIBRATION_EMBEDDED_TEXT, b"/Font << /F1 30 0 R >>")
    widths = [0] * (66 - 32 + 1)
    widths[32 - 32] = 300
    widths[65 - 32] = 600
    widths[66 - 32] = 600
    writer.obj(
        30,
        b"<< /Type /Font /Subtype /TrueType /BaseFont /Pdf2Fixture"
        b" /Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 66"
        b" /Widths [" + b" ".join(b"%d" % width for width in widths) + b"]"
        b" /FontDescriptor 31 0 R >>",
    )
    font_program = truetype_font()
    writer.obj(
        31,
        b"<< /Type /FontDescriptor /FontName /Pdf2Fixture /Flags 32"
        b" /FontBBox [0 -200 600 750] /ItalicAngle 0 /Ascent 800"
        b" /Descent -200 /CapHeight 700 /StemV 80 /MissingWidth 600"
        b" /FontFile2 32 0 R >>",
    )
    writer.stream(
        32,
        b"/Length1 %d" % len(font_program),
        font_program,
        compress=True,
    )

    # Pages 10 and 11: composite fonts over the same program. The first maps CID
    # to glyph id by identity, so it draws A B A; the second's /CIDToGIDMap swaps
    # the two glyphs and draws B A B, which no fallback could fake.
    page(33, b"0 0 300 200", CALIBRATION_CID_TEXT, b"/Font << /F1 35 0 R >>")
    writer.obj(
        35,
        b"<< /Type /Font /Subtype /Type0 /BaseFont /Pdf2Fixture"
        b" /Encoding /Identity-H /DescendantFonts [36 0 R] >>",
    )
    writer.obj(
        36,
        b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Pdf2Fixture"
        b" /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity)"
        b" /Supplement 0 >> /FontDescriptor 37 0 R"
        b" /CIDToGIDMap /Identity /DW 1000 /W [1 [600 600]] >>",
    )
    writer.obj(
        37,
        b"<< /Type /FontDescriptor /FontName /Pdf2Fixture /Flags 32"
        b" /FontBBox [0 -200 600 750] /ItalicAngle 0 /Ascent 800"
        b" /Descent -200 /CapHeight 700 /StemV 80 /MissingWidth 600"
        b" /FontFile2 38 0 R >>",
    )
    writer.stream(38, b"/Length1 %d" % len(font_program), font_program, compress=True)

    page(39, b"0 0 300 200", CALIBRATION_CID_TEXT, b"/Font << /F1 41 0 R >>")
    writer.obj(
        41,
        b"<< /Type /Font /Subtype /Type0 /BaseFont /Pdf2Fixture"
        b" /Encoding /Identity-H /DescendantFonts [42 0 R] >>",
    )
    writer.obj(
        42,
        b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Pdf2Fixture"
        b" /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity)"
        b" /Supplement 0 >> /FontDescriptor 37 0 R"
        b" /CIDToGIDMap 43 0 R /DW 1000 /W [1 [600 600]] >>",
    )
    # CID 0 -> glyph 0, CID 1 -> glyph 2 (B), CID 2 -> glyph 1 (A).
    writer.stream(43, b"", bytes((0, 0, 0, 2, 0, 1)), compress=False)
    writer.obj(
        27,
        b"<< /Type /Font /Subtype /Type0 /BaseFont /NotoSansCJK"
        b" /Encoding /Identity-H /DescendantFonts [<< /Type /Font"
        b" /Subtype /CIDFontType2 /BaseFont /NotoSansCJK"
        b" /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity)"
        b" /Supplement 0 >> /DW 1000 >>] >>",
    )

    offsets = writer.offsets
    xref = len(writer.out)
    writer.out += b"xref\n0 %d\n" % (max(offsets) + 1)
    writer.out += b"0000000000 65535 f \n"
    for number in range(1, max(offsets) + 1):
        writer.out += b"%010d 00000 n \n" % offsets[number]
    writer.out += b"trailer\n<< /Size %d /Root 1 0 R >>\n" % (max(offsets) + 1)
    writer.out += b"startxref\n%d\n%%%%EOF\n" % xref
    write("calibration.pdf", bytes(writer.out))

if __name__ == "__main__":
    make_xref_stream_pdf()
    make_classic_twin()
    make_hybrid()
    make_page_tree()
    make_text()
    make_classic_xref()
    make_paths()
    make_images()
    make_calibration()
    make_book()
    make_cropbox()
    make_generation_edge()
    make_reading_order()
    make_tounicode()
    make_startxref_hint()
    make_text_mode()
    make_invisible_ink()
    make_blank_page()
    make_forms()
    make_form_cycle()
    make_annotations()
    make_alpha()
