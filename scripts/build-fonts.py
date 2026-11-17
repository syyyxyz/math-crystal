#!/usr/bin/env python3
"""Regenerate the fonts bundled into the binary (assets/fonts/).

Needs fontTools (pyftsubset) and the Arch packages inter-font, ttf-dejavu,
ttf-jetbrains-mono-nerd, ttf-nerd-fonts-symbols and adobe-source-han-sans-cn-fonts.
Run it again whenever new Chinese interface text or a new icon is added; the
`bundled_fonts_cover_source_text` test fails until you do.

  - Inter, DejaVu Sans / Sans Mono: copied unchanged.
  - JetBrains Mono NL (Nerd Font Mono build): Private Use Area glyphs removed.
  - Symbols Nerd Font: only the icons listed in src/ui/theme.rs.
  - Source Han Sans CN: GB2312 (all 6763 hanzi + its symbol rows) plus every
    character used in the sources. It is renamed "Crystal Han Sans" because
    the OFL reserves the name "Source" for unmodified copies.
"""

import re
import shutil
import subprocess
import sys
from pathlib import Path

from fontTools.ttLib import TTFont

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "fonts"
SYS = Path("/usr/share/fonts")

INTER = SYS / "inter/InterVariable.ttf"
DEJAVU = SYS / "TTF/DejaVuSans.ttf"
DEJAVU_MONO = SYS / "TTF/DejaVuSansMono.ttf"
JB_MONO = SYS / "TTF/JetBrainsMonoNLNerdFontMono-Regular.ttf"
SYMBOLS = SYS / "TTF/SymbolsNerdFont-Regular.ttf"
HAN = SYS / "adobe-source-han-sans/SourceHanSansCN-Regular.otf"

LICENSES = {
    "Inter-OFL.txt": Path("/usr/share/licenses/inter-font/LICENSE.txt"),
    "JetBrainsMono-OFL.txt": Path("/usr/share/licenses/ttf-jetbrains-mono-nerd/OFL.txt"),
    "DejaVu-LICENSE.txt": Path("/usr/share/licenses/ttf-dejavu/LICENSE"),
    "NerdFonts-LICENSE.txt": Path("/usr/share/licenses/ttf-nerd-fonts-symbols/LICENSE"),
    "SourceHanSans-OFL.txt": Path("/usr/share/licenses/adobe-source-han-sans-cn-fonts/LICENSE.txt"),
}


def source_chars() -> set[str]:
    chars: set[str] = set()
    for d in ("src", "examples"):
        for p in (ROOT / d).rglob("*.rs"):
            chars.update(c for c in p.read_text(encoding="utf-8") if ord(c) > 0x7F)
    return chars


def gb2312() -> set[str]:
    chars: set[str] = set()
    for b1 in range(0xA1, 0xF8):
        for b2 in range(0xA1, 0xFF):
            try:
                chars.add(bytes([b1, b2]).decode("gb2312"))
            except UnicodeDecodeError:
                pass
    return chars


def icon_codepoints() -> list[int]:
    text = (ROOT / "src/ui/theme.rs").read_text(encoding="utf-8")
    return sorted({int(h, 16) for h in re.findall(r'"\\u\{(F[0-9A-Fa-f]{4})\}"', text)})


def subset(src: Path, dst: Path, unicodes: str) -> None:
    subprocess.run(
        [
            "pyftsubset", str(src), f"--output-file={dst}", f"--unicodes={unicodes}",
            "--layout-features=*", "--name-IDs=*", "--name-languages=*",
            "--notdef-outline", "--no-hinting",
        ],
        check=True,
    )


def rename_han(path: Path) -> None:
    family, ps = "Crystal Han Sans", "CrystalHanSans-Regular"
    font = TTFont(path)
    for rec in font["name"].names:
        if rec.nameID in (1, 16):
            rec.string = family
        elif rec.nameID == 4:
            rec.string = f"{family} Regular"
        elif rec.nameID in (3, 6):
            rec.string = ps
        elif rec.nameID == 17:
            rec.string = "Regular"
    cff = font["CFF "].cff
    cff.fontNames = [ps]
    top = cff.topDictIndex[0]
    for attr, val in (("FullName", f"{family} Regular"), ("FamilyName", family)):
        if hasattr(top, attr):
            setattr(top, attr, val)
    font.save(path)


def main() -> int:
    missing = [p for p in (INTER, DEJAVU, DEJAVU_MONO, JB_MONO, SYMBOLS, HAN, *LICENSES.values()) if not p.exists()]
    if missing:
        print("missing:", *missing, sep="\n  ", file=sys.stderr)
        return 1
    OUT.mkdir(parents=True, exist_ok=True)

    shutil.copyfile(INTER, OUT / "InterVariable.ttf")
    shutil.copyfile(DEJAVU, OUT / "DejaVuSans.ttf")
    shutil.copyfile(DEJAVU_MONO, OUT / "DejaVuSansMono.ttf")
    # Everything except the Private Use Areas (the Nerd Font icons).
    subset(JB_MONO, OUT / "JetBrainsMonoNL-Regular.ttf", "U+0000-DFFF,U+F900-FFFF,U+10000-EFFFF")
    icons = icon_codepoints()
    subset(SYMBOLS, OUT / "SymbolsNerdFont-Subset.ttf", ",".join(f"U+{c:X}" for c in icons))

    han = {c for c in gb2312() | source_chars() if ord(c) >= 0x2E80}
    han |= {chr(c) for c in range(0x3000, 0x3040)}  # CJK punctuation
    han |= {chr(c) for c in range(0xFF01, 0xFF5F)}  # full-width forms
    (OUT / "han-chars.txt").write_text("".join(sorted(han)), encoding="utf-8")
    subprocess.run(
        [
            "pyftsubset", str(HAN), f"--output-file={OUT / 'CrystalHanSans-Regular.otf'}",
            f"--text-file={OUT / 'han-chars.txt'}", "--layout-features=*",
            "--name-IDs=*", "--name-languages=*", "--notdef-outline", "--no-hinting",
        ],
        check=True,
    )
    (OUT / "han-chars.txt").unlink()
    rename_han(OUT / "CrystalHanSans-Regular.otf")

    for name, src in LICENSES.items():
        shutil.copyfile(src, OUT / name)
    print(f"{len(icons)} icons, {len(han)} CJK characters")
    for p in sorted(OUT.iterdir()):
        print(f"  {p.stat().st_size / 1024:8.0f} KiB  {p.name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
