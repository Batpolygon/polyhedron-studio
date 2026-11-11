"""Icon of Polyhedron Studio: a cube in the colours of the studio.

Drawn here, nothing is imported. Writes to `assets/icon/`:

- `polyhedron.ico`: every size, built into the executable by `build.rs`;
- `window_64.rgba`: raw pixels, shown on the window by `src/main.rs`.

Run from the root:

    python tools/design_icon.py
"""
import math
import os
import struct
import zlib

FOLDER = 'assets/icon'
SIZES = (16, 24, 32, 48, 64, 128, 256)
SAMPLES = 4

TOP, UP_RIGHT, LOW_RIGHT = (.5, .05), (.92, .285), (.92, .715)
BOTTOM, LOW_LEFT, UP_LEFT = (.5, .95), (.08, .715), (.08, .285)
CENTER = (.5, .52)
FACES = (
    ((TOP, UP_RIGHT, CENTER, UP_LEFT), (140, 240, 224)),
    ((UP_LEFT, CENTER, BOTTOM, LOW_LEFT), (78, 210, 190)),
    ((CENTER, UP_RIGHT, LOW_RIGHT, BOTTOM), (38, 120, 110)),
)
OUTLINE = (TOP, UP_RIGHT, LOW_RIGHT, BOTTOM, LOW_LEFT, UP_LEFT)
RIDGES = ((CENTER, UP_LEFT), (CENTER, UP_RIGHT), (CENTER, BOTTOM))
DARK, LIGHT = (10, 26, 30), (232, 255, 250)


def inside(point, polygon):
    x, y = point
    sign = 0
    for (ax, ay), (bx, by) in zip(polygon, polygon[1:] + polygon[:1]):
        cross = (bx - ax) * (y - ay) - (by - ay) * (x - ax)
        if cross:
            if sign and (cross > 0) != (sign > 0):
                return False
            sign = cross
    return True


def distance(point, a, b):
    px, py = point[0] - a[0], point[1] - a[1]
    dx, dy = b[0] - a[0], b[1] - a[1]
    t = max(0.0, min(1.0, (px * dx + py * dy) / (dx * dx + dy * dy)))
    return math.hypot(px - t * dx, py - t * dy)


def sample(point, size):
    """Colour of one point, or None outside the cube."""
    border = max(.028, 1.1 / size)
    edge = min(distance(point, a, b) for a, b in zip(OUTLINE, OUTLINE[1:] + OUTLINE[:1]))
    if edge < border and not inside(point, OUTLINE):
        return DARK
    if not inside(point, OUTLINE):
        return None
    if edge < border:
        return DARK
    if min(distance(point, a, b) for a, b in RIDGES) < border * .55:
        return LIGHT
    for polygon, color in FACES:
        if inside(point, list(polygon)):
            return color
    return FACES[0][1]


def draw(size):
    pixels = bytearray()
    for y in range(size):
        for x in range(size):
            total, red, green, blue = 0, 0, 0, 0
            for j in range(SAMPLES):
                for i in range(SAMPLES):
                    color = sample(((x + (i + .5) / SAMPLES) / size, (y + (j + .5) / SAMPLES) / size), size)
                    if color:
                        total += 1
                        red, green, blue = red + color[0], green + color[1], blue + color[2]
            if total:
                pixels += bytes((red // total, green // total, blue // total, total * 255 // SAMPLES ** 2))
            else:
                pixels += bytes(4)
    return bytes(pixels)


def png(size, pixels):
    def chunk(tag, data):
        return struct.pack('>I', len(data)) + tag + data + struct.pack('>I', zlib.crc32(tag + data))
    rows = b''.join(b'\0' + pixels[y * size * 4:(y + 1) * size * 4] for y in range(size))
    header = struct.pack('>IIBBBBB', size, size, 8, 6, 0, 0, 0)
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', header) + chunk(b'IDAT', zlib.compress(rows, 9)) + chunk(b'IEND', b'')


def bitmap(size, pixels):
    """Classic icon entry: small sizes stay readable by every Windows viewer."""
    header = struct.pack('<IiiHHIIiiII', 40, size, size * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    colors = bytearray()
    for y in reversed(range(size)):
        for x in range(size):
            r, g, b, a = pixels[(y * size + x) * 4:(y * size + x) * 4 + 4]
            colors += bytes((b, g, r, a))
    return header + bytes(colors) + bytes(((size + 31) // 32) * 4 * size)


if __name__ == '__main__':
    os.makedirs(FOLDER, exist_ok=True)
    images = {size: draw(size) for size in SIZES}
    entries = [png(size, images[size]) if size == 256 else bitmap(size, images[size]) for size in SIZES]
    offset = 6 + 16 * len(SIZES)
    directory = struct.pack('<HHH', 0, 1, len(SIZES))
    for size, data in zip(SIZES, entries):
        directory += struct.pack('<BBBBHHII', size % 256, size % 256, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    with open(os.path.join(FOLDER, 'polyhedron.ico'), 'wb') as out:
        out.write(directory + b''.join(entries))
    with open(os.path.join(FOLDER, 'window_64.rgba'), 'wb') as out:
        out.write(images[64])
    with open(os.path.join(FOLDER, 'preview_256.png'), 'wb') as out:
        out.write(png(256, images[256]))
    print('icon written')
