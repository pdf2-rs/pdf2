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


if __name__ == "__main__":
    make_xref_stream_pdf()
    make_classic_twin()
    make_hybrid()
    make_page_tree()
    make_text()
    make_classic_xref()
