"""Lists the characters of the sources and of the project data that the
interface font stack cannot draw.

egui shows those as empty squares. Interface text uses the Proportional family
(Ubuntu-Light, NotoEmoji, emoji-icon-font). Hack is only its last resort in
`theme.rs`, so characters only Hack has are reported too. Run from the root:

    python tools/check_glyphs.py
"""
import glob, os, pathlib, struct, sys, collections
sys.stdout.reconfigure(encoding='utf-8')

def cmap(path):
    d = open(path, 'rb').read()
    n = struct.unpack('>H', d[4:6])[0]
    off = None
    for i in range(n):
        tag, _, o, _ = struct.unpack('>4sIII', d[12 + i * 16:28 + i * 16])
        if tag == b'cmap':
            off = o
    count = struct.unpack('>H', d[off + 2:off + 4])[0]
    out = set()
    for i in range(count):
        _, _, sub = struct.unpack('>HHI', d[off + 4 + i * 8:off + 12 + i * 8])
        s = off + sub
        fmt = struct.unpack('>H', d[s:s + 2])[0]
        if fmt == 4:
            seg = struct.unpack('>H', d[s + 6:s + 8])[0] // 2
            end = struct.unpack(f'>{seg}H', d[s + 14:s + 14 + seg * 2])
            start = struct.unpack(f'>{seg}H', d[s + 16 + seg * 2:s + 16 + seg * 4])
            for a, b in zip(start, end):
                if a != 0xffff:
                    out.update(range(a, b + 1))
        elif fmt == 12:
            groups = struct.unpack('>I', d[s + 12:s + 16])[0]
            for g in range(groups):
                a, b, _ = struct.unpack('>III', d[s + 16 + g * 12:s + 28 + g * 12])
                out.update(range(a, b + 1))
    return out

root = next(pathlib.Path(os.path.expanduser('~/.cargo/registry/src')).glob('*/epaint_default_fonts-0.36*/fonts'))
fonts = {p.name: cmap(p) for p in root.glob('*.ttf')}
print({k: len(v) for k, v in fonts.items()})
emoji = fonts['Ubuntu-Light.ttf'] | fonts['NotoEmoji-Regular.ttf'] | fonts['emoji-icon-font.ttf']
hack = fonts['Hack-Regular.ttf']
# Filled shapes of the Toolbox: meaningful in Hack, and no emoji exists for them.
SHAPES = set('■▄▌▟▙▗◢◣◤')
counts = collections.Counter(); where = {}
paths = glob.glob('src/**/*.rs', recursive=True) + glob.glob('project/**/*.ron', recursive=True)
for f in paths:
    for i, line in enumerate(open(f, encoding='utf-8')):
        if line.strip().startswith('//'):
            continue
        for ch in line:
            if ord(ch) > 127 and ch not in 'éèêàçùôîâÉ':
                counts[ch] += 1
                where.setdefault(ch, []).append(f"{os.path.basename(f)}:{i + 1}")
missing = [c for c in counts if ord(c) not in emoji | hack]
fallback = [c for c in counts if ord(c) not in emoji and ord(c) in hack and c not in SHAPES and c != '→' and c != '️']
print('used', len(counts), 'missing', len(missing), 'hack only', len(fallback))
for c in missing + fallback:
    print(repr(c), hex(ord(c)), counts[c], where[c][:12])
sys.exit(1 if missing or fallback else 0)
