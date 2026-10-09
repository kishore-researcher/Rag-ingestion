"""Hand-builds testdata/cid_font_report.pdf: a Type0/Identity-H font whose
text is glyph indices (GID = ASCII - 29, as TrueType subsets typically are),
decodable only through the /ToUnicode CMap. Without the ToUnicode decoder
the extracted text is control-byte garbage."""
import sys
out = sys.argv[1] if len(sys.argv) > 1 else "testdata/cid_font_report.pdf"
lines = [(72, 720, "Annual Report 2026"), (72, 690, "Revenue grew 12 percent year over year."),
         (72, 670, "Operating margin improved on lower costs.")]
def hexstr(t): return "<" + "".join("%04X" % (ord(c) - 29) for c in t) + ">"
content = "BT /F1 14 Tf\n" + "".join(f"1 0 0 1 {x} {y} Tm {hexstr(t)} Tj\n" for x, y, t in lines) + "ET"
chars = sorted({c for _, _, t in lines for c in t})
cmap = ("/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n/CMapName /Adobe-Identity-UCS def\n"
        "1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n%d beginbfchar\n" % len(chars)
        + "".join("<%04X> <%04X>\n" % (ord(c) - 29, ord(c)) for c in chars) + "endbfchar\nendcmap end end")
objs = {
 1: "<< /Type /Catalog /Pages 2 0 R >>",
 2: "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
 3: "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
 4: f"<< /Length {len(content)} >>\nstream\n{content}\nendstream",
 5: "<< /Type /Font /Subtype /Type0 /BaseFont /ABCDEF+Sub /Encoding /Identity-H /DescendantFonts [6 0 R] /ToUnicode 7 0 R >>",
 6: "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /ABCDEF+Sub /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /DW 500 >>",
 7: f"<< /Length {len(cmap)} >>\nstream\n{cmap}\nendstream",
}
pdf = "%PDF-1.5\n"; offs = {}
for n, b in objs.items():
    offs[n] = len(pdf.encode()); pdf += f"{n} 0 obj\n{b}\nendobj\n"
xref = len(pdf.encode())
pdf += f"xref\n0 {len(objs)+1}\n0000000000 65535 f \n" + "".join("%010d 00000 n \n" % offs[n] for n in objs)
pdf += f"trailer\n<< /Size {len(objs)+1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
open(out, "wb").write(pdf.encode())
print("wrote", out)
