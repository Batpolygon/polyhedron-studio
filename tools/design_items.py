"""Item models of the project, written as ordinary `project/items/*.ron` data.

Only the `parts` list of each file is replaced: stats and effects stay. Every
item stays editable in the Armoury, but running this script again replaces the
models listed here. Run from the root:

    python tools/design_items.py

Units are 1/16 of a block. Y is the length of the item: it starts at the grip
(y = 0) and points forward in the hand; -Z is the down side when held, so gun
grips hang toward -Z. X and Z are centred on 8.
"""
import os

ITEMS = 'project/items'


def rgb(hex_color):
    return tuple(int(hex_color[i:i + 2], 16) for i in (0, 2, 4))


class Model:
    def __init__(self):
        self.parts = []

    def box(self, name, start, end, color):
        assert all(a < b for a, b in zip(start, end)), (name, start, end)
        self.parts.append((name, start, end, rgb(color)))

    def bar(self, name, y0, y1, width, depth, color, x=8.0, z=8.0):
        """A box centred on (x, z), from y0 to y1."""
        self.box(name, (x - width / 2, y0, z - depth / 2), (x + width / 2, y1, z + depth / 2), color)

    def pair(self, name, x, y0, y1, width, depth, color, z=8.0):
        """The same box on both sides of the middle, `x` away from it."""
        self.bar(f'{name} L', y0, y1, width, depth, color, 8 - x, z)
        self.bar(f'{name} R', y0, y1, width, depth, color, 8 + x, z)

    def taper(self, name, y0, y1, w0, w1, depth, color, steps, z=8.0, d1=None):
        """Stacked boxes going from width w0 to width w1."""
        for i in range(steps):
            u = i / max(steps - 1, 1)
            y = y0 + (y1 - y0) * i / steps
            d = depth + ((d1 if d1 is not None else depth) - depth) * u
            self.bar(f'{name} {i + 1}', y, y + (y1 - y0) / steps, w0 + (w1 - w0) * u, d, color, z=z)


def ron(model):
    out = '    parts: [\n'
    for name, start, end, color in model.parts:
        out += f'        (\n            name: "{name}",\n            from: {nums(start)},\n            to: {nums(end)},\n            color: ({color[0]}, {color[1]}, {color[2]}),\n        ),\n'
    return out + '    ],\n'


def nums(values):
    def one(value):
        text = f'{float(value):.2f}'.rstrip('0')
        return text + '0' if text.endswith('.') else text
    return '(' + ', '.join(one(v) for v in values) + ')'


def write(name, model):
    path = os.path.join(ITEMS, name + '.ron')
    text = open(path, encoding='utf-8').read()
    start = text.index('    parts: [')
    end = text.index('\n    ],\n', start) + len('\n    ],\n')
    with open(path, 'w', encoding='utf-8', newline='\n') as out:
        out.write(text[:start] + ron(model) + text[end:])


def sword():
    m = Model()
    wood, wrap, gold, dark, steel, light, shade = '663f26', '3a2618', 'f0be50', 'a87c2c', 'd2dceb', 'f4f8ff', '8c96a8'
    m.bar('Pommel', 0, .9, 1.9, 1.9, gold)
    m.bar('Pommel gem', .15, .75, 2.1, .7, 'd83a4e')
    m.bar('Grip', .9, 3.6, 1.3, 1.3, wood)
    for i in range(4):
        m.bar(f'Wrap {i + 1}', 1.2 + i * .62, 1.5 + i * .62, 1.5, 1.5, wrap)
    m.bar('Guard core', 3.6, 4.4, 2.8, 1.7, gold)
    m.bar('Guard jewel', 3.75, 4.25, 1, 1.9, '4cc8ff')
    m.pair('Guard arm', 2.4, 3.7, 4.3, 2.4, 1.2, gold)
    m.pair('Guard tip', 3.8, 3.5, 4.5, .7, 1.2, dark)
    m.bar('Ricasso', 4.4, 5.4, 2, .7, shade)
    m.taper('Blade', 5.4, 14, 1.9, 1.1, .5, steel, 6)
    m.bar('Fuller', 5.4, 12, .4, .62, shade)
    m.pair('Edge', .85, 5.4, 9, .2, .3, light)
    m.pair('Edge tip', .62, 9, 13, .2, .3, light)
    m.taper('Point', 14, 15.8, .9, .2, .4, light, 3)
    return m


def astral_saber():
    m = Model()
    dark, metal, gold, glow, core = '2a2e3c', '6a7288', 'f0be50', 'ffec96', 'fffbe6'
    m.bar('Hilt', 0, 4, 1.5, 1.5, dark)
    m.bar('Hilt cap', 0, .5, 1.8, 1.8, gold)
    for i in range(4):
        m.bar(f'Grip ring {i + 1}', .9 + i * .6, 1.2 + i * .6, 1.7, 1.7, metal)
    m.bar('Switch', 2.2, 3.2, .5, 1.75, 'e8483c')
    m.bar('Collar', 4, 4.5, 1.9, 1.9, gold)
    m.bar('Emitter', 4.5, 5.2, 2.2, 2.2, metal)
    m.pair('Emitter fin', 1.3, 4.3, 5.6, .4, 1.2, gold)
    m.bar('Blade glow', 5.2, 15.4, 1.4, 1.4, glow)
    m.bar('Blade core', 5.2, 16, .8, .8, core)
    m.taper('Blade tip', 15.4, 16.2, 1.1, .5, 1.1, glow, 2)
    return m


def stone_axe():
    m = Model()
    wood, dark, lace, stone, light, shade = '96684a', '6b4630', 'd8c8a0', '8c96a5', 'b4bcc8', '646c7a'
    m.bar('Handle', 0, 14.5, 1.5, 1.5, wood)
    m.bar('Butt', 0, .7, 1.9, 1.9, dark)
    for i in range(3):
        m.bar(f'Grip wrap {i + 1}', 1 + i * 1, 1.5 + i * 1, 1.7, 1.7, dark)
    m.bar('Lashing 1', 8.8, 9.4, 1.8, 1.8, lace)
    m.bar('Lashing 2', 13, 13.6, 1.8, 1.8, lace)
    m.box('Cheek', (8.6, 9, 7.2), (10.5, 14, 8.8), stone)
    m.box('Blade', (10.5, 8.5, 7.4), (11.8, 14.5, 8.6), stone)
    m.box('Beard', (11.8, 7.8, 7.6), (13, 15, 8.4), light)
    m.box('Edge', (13, 7.4, 7.75), (13.8, 15.4, 8.25), light)
    m.box('Chip', (9, 11, 7.1), (10.1, 12.4, 8.9), shade)
    m.box('Poll', (5.6, 10, 7.3), (7.4, 13.2, 8.7), shade)
    m.box('Poll face', (5, 10.4, 7.5), (5.6, 12.8, 8.5), stone)
    return m


def stone_pickaxe():
    m = Model()
    wood, dark, lace, stone, light, shade = '96684a', '6b4630', 'd8c8a0', '8c96a5', 'b4bcc8', '646c7a'
    m.bar('Handle', 0, 14.2, 1.5, 1.5, wood)
    m.bar('Butt', 0, .7, 1.9, 1.9, dark)
    for i in range(3):
        m.bar(f'Grip wrap {i + 1}', 1 + i, 1.5 + i, 1.7, 1.7, dark)
    m.bar('Lashing', 11.4, 12, 1.8, 1.8, lace)
    m.bar('Socket', 12, 14.8, 3, 2.2, shade)
    m.bar('Socket cap', 14.8, 15.2, 2, 1.6, stone)
    for sign, side in ((-1, 'L'), (1, 'R')):
        m.bar(f'Arm {side}', 12.3, 14.5, 2.6, 1.8, stone, x=8 + sign * 2.6)
        m.bar(f'Arm top {side}', 14.5, 15, 2, 1.4, light, x=8 + sign * 2.6)
        m.bar(f'Bend {side}', 11.6, 14, 2, 1.5, stone, x=8 + sign * 4.8)
        m.bar(f'Point {side}', 10.2, 12.4, 1.3, 1.1, stone, x=8 + sign * 5.9)
        m.bar(f'Tip {side}', 8.8, 10.2, .7, .7, light, x=8 + sign * 6.4)
    return m


def wrench():
    m = Model()
    steel, light, shade, dark, grip, bolt = '78808e', 'd2dceb', '5a6270', '32363f', 'e0802c', 'f0be50'
    m.bar('Handle', 0, 10, 1.5, .9, steel)
    m.bar('Grip', .5, 6, 1.8, 1.2, grip)
    for i in range(4):
        m.bar(f'Grip ridge {i + 1}', 1 + i * 1.2, 1.4 + i * 1.2, 2, 1.3, dark)
    m.bar('Hang hole', .1, .4, 2, 1, dark)
    m.bar('Neck', 10, 11, 2.3, 1.1, shade)
    m.bar('Head', 11, 12.2, 5.4, 1.3, light)
    m.pair('Jaw', 2, 12.2, 15, 1.4, 1.3, light)
    m.pair('Jaw tip', 2.3, 15, 15.6, .8, 1.1, steel)
    m.bar('Worm screw', 10.8, 11.8, 1.1, 1.5, bolt)
    m.pair('Tooth', 1.15, 12.6, 13, .3, 1, dark)
    return m


def brush():
    m = Model()
    wood, dark, metal, light, paint, tip = '96684a', '6b4630', 'd2dceb', 'f4f8ff', 'e65aa0', 'ff9ad0'
    m.taper('Handle', 0, 10, 1.1, 1.6, 1.1, wood, 4, d1=1.6)
    m.bar('End dip', 0, 1.2, 1.25, 1.25, paint)
    m.bar('Band', 9.2, 9.7, 1.8, 1.8, dark)
    m.bar('Ferrule', 10, 11.8, 2.2, 1.6, metal)
    m.bar('Ferrule ring 1', 10.2, 10.5, 2.4, 1.8, light)
    m.bar('Ferrule ring 2', 11.2, 11.5, 2.4, 1.8, light)
    m.taper('Bristles', 11.8, 15, 2.6, 2.1, 1.7, 'efe6d6', 3, d1=1.4)
    m.taper('Paint', 13.6, 15.8, 2.2, .9, 1.5, paint, 3, d1=.9)
    m.bar('Paint drop', 15.8, 16.3, .5, .5, tip)
    return m


def gun(body, shade, accent, glow, metal='d2dceb', grip='3a2c24'):
    """Frame shared by the guns: barrel along Y, grip hanging toward -Z."""
    m = Model()
    m.box('Grip', (7, 0, 3.6), (9, 2.4, 7), grip)
    m.box('Grip plate', (6.8, .3, 4), (9.2, 2.1, 6.4), '20201e')
    m.box('Magazine', (7.3, 0, 3), (8.7, 2, 3.6), metal)
    m.box('Trigger guard', (7.4, 2.4, 5.2), (8.6, 4.6, 5.5), shade)
    m.box('Trigger', (7.75, 2.8, 5.5), (8.25, 3.3, 6.8), accent)
    m.box('Frame', (6.8, 0, 7), (9.2, 8.5, 10), body)
    m.box('Rear', (7, 0, 10), (9, 2, 10.6), shade)
    m.box('Top rail', (7.4, 1, 10), (8.6, 8, 10.4), shade)
    m.box('Sight rear', (7.5, 1.2, 10.4), (8.5, 1.8, 11), accent)
    m.box('Sight front', (7.8, 7, 10.4), (8.2, 7.6, 11), accent)
    for i in range(3):
        m.box(f'Vent {i + 1} L', (6.65, 2 + i * 1.3, 8), (6.8, 2.7 + i * 1.3, 9.4), glow)
        m.box(f'Vent {i + 1} R', (9.2, 2 + i * 1.3, 8), (9.35, 2.7 + i * 1.3, 9.4), glow)
    return m


def blaster():
    m = gun('dc50d2', '8c2e88', 'f0be50', 'ff9af6')
    m.bar('Barrel', 8.5, 12, 1.6, 1.6, 'd2dceb', z=8.5)
    m.bar('Barrel ring 1', 9.2, 9.6, 2, 2, 'dc50d2', z=8.5)
    m.bar('Barrel ring 2', 10.6, 11, 2, 2, 'dc50d2', z=8.5)
    m.bar('Muzzle', 12, 13, 2.2, 2.2, '8c2e88', z=8.5)
    m.bar('Muzzle light', 13, 13.2, 1.2, 1.2, 'ffc8fa', z=8.5)
    return m


def blastygun():
    m = gun('ff9646', 'a85420', 'f0d060', 'ffd070')
    m.box('Drum', (5.8, 3, 7.2), (10.2, 6.6, 9.8), 'a85420')
    m.box('Drum band', (5.6, 4.3, 7), (10.4, 5.3, 10), 'ffd070')
    m.bar('Barrel', 8.5, 11, 2.6, 2.6, '503020', z=8.5)
    m.bar('Bell 1', 11, 12, 3.4, 3.4, '503020', z=8.5)
    m.bar('Bell 2', 12, 13, 4.2, 4.2, 'ff9646', z=8.5)
    m.bar('Mouth', 13, 13.3, 3, 3, '20100a', z=8.5)
    return m


def etheragun():
    m = gun('c896ff', '6e4aa8', 'f0e0ff', 'e8d8ff')
    m.bar('Neck', 8.5, 10, 1.8, 1.8, '6e4aa8', z=8.5)
    m.bar('Orb', 10, 11.6, 2, 2, 'f0e0ff', z=8.5)
    for sign, side in ((-1, 'L'), (1, 'R')):
        m.box(f'Claw base {side}', (8 + sign * 2.2 - .6, 9.2, 8), (8 + sign * 2.2 + .6, 11, 9), 'c896ff')
        m.box(f'Claw tip {side}', (8 + sign * 1.5 - .4, 11, 8.2), (8 + sign * 1.5 + .4, 12.6, 8.8), 'f0e0ff')
    return m


def voltigun():
    m = gun('5aaaff', '2c5aa8', 'fff08a', 'c8f0ff')
    m.bar('Rod', 8.5, 13, .9, .9, 'd2dceb', z=8.5)
    for i in range(4):
        m.bar(f'Coil {i + 1}', 9 + i * .95, 9.5 + i * .95, 2.5 - i * .3, 2.5 - i * .3, 'c8e6ff', z=8.5)
    m.bar('Spark tip', 13, 13.8, 1.4, 1.4, 'fff08a', z=8.5)
    m.box('Battery', (6, 2, 7.6), (6.8, 6, 9.4), 'fff08a')
    return m


def atomixer():
    m = Model()
    dark, metal, light, core, ring = '32363f', '6a7288', 'f0f0ff', '8cdcff', 'd8fbff'
    m.bar('Handle', 0, 10.5, 1.7, 1.7, dark)
    m.bar('Grip', .6, 5, 2, 2, metal)
    for i in range(3):
        m.bar(f'Grip ring {i + 1}', 1.2 + i * 1.3, 1.6 + i * 1.3, 2.2, 2.2, ring)
    m.bar('Collar', 10.5, 11.2, 3, 3, metal)
    m.bar('Hammer', 11.2, 14.2, 7, 3, light)
    m.bar('Core', 11.8, 13.6, 2.2, 3.6, core)
    m.bar('Core light', 12.3, 13.1, 3.4, 3.8, ring)
    for sign, side in ((-1, 'L'), (1, 'R')):
        m.bar(f'Face {side}', 11.4, 14, 1, 2.6, metal, x=8 + sign * 4)
        m.bar(f'Tip {side}', 11.8, 13.6, 1, 1.8, core, x=8 + sign * 5)
        m.bar(f'Rib {side}', 11, 14.4, .4, 3.3, dark, x=8 + sign * 2)
    m.bar('Top', 14.2, 15, 2.6, 2, metal)
    return m


def binder():
    m = Model()
    wood, dark, ring, light, core = '663f26', '3a2618', '78e6a0', 'c8ffdc', 'ffffff'
    m.bar('Handle', 0, 6, 1.5, 1.5, wood)
    m.bar('Pommel', 0, .7, 2, 2, ring)
    for i in range(3):
        m.bar(f'Wrap {i + 1}', 1.2 + i * 1.3, 1.7 + i * 1.3, 1.7, 1.7, dark)
    m.bar('Socket', 6, 6.8, 2.2, 2.2, dark)
    m.bar('Ring base', 6.8, 7.6, 3.4, 1.2, ring)
    m.pair('Ring low', 2, 7.4, 8.4, 1.1, 1.2, ring)
    m.pair('Ring side', 2.6, 8.4, 11.2, 1, 1.2, ring)
    m.pair('Ring high', 2, 11.2, 12.2, 1.1, 1.2, ring)
    m.bar('Ring top', 12, 12.8, 3.4, 1.2, ring)
    m.pair('Glint', 2.6, 9, 10.6, 1.05, .3, light)
    m.bar('Core', 8.8, 10.8, 1.5, 1.5, core)
    m.bar('Core halo', 9.4, 10.2, 2.4, .8, light)
    return m


def cable():
    m = Model()
    rubber, ring, copper, plug, light = '282c34', '464c58', 'f0c85a', '5ac8f0', 'c8f0ff'
    for i in range(5):
        y = 1 + i * 1.3
        m.bar(f'Coil {i + 1}', y, y + 1.1, 9 - abs(i - 2) * .6, 5.2 - abs(i - 2) * .3, rubber)
        m.bar(f'Coil stripe {i + 1}', y + .4, y + .7, 9.2 - abs(i - 2) * .6, 5.4 - abs(i - 2) * .3, ring)
    m.bar('Hole', 2, 7, 3, 5.6, '14161a')
    m.bar('Tie', 3.6, 4.6, 1.4, 5.8, 'e8483c')
    m.box('Wire', (7.3, 7.6, 7.3), (8.7, 13.4, 8.7), rubber)
    m.box('Wire stripe', (7.2, 9, 7.2), (8.8, 9.6, 8.8), copper)
    m.bar('Plug', 13.4, 14.8, 2.8, 2.8, plug)
    m.bar('Plug band', 13.8, 14.2, 3, 3, light)
    m.pair('Pin', .7, 14.8, 16, .4, .7, copper)
    return m


def etherealink():
    m = Model()
    base, metal, gem, light, core = '3c4050', '6a7288', 'c896ff', 'e6d2ff', 'fff4ff'
    m.bar('Foot', 0, .7, 5, 5, base)
    m.bar('Plate', .7, 1.4, 4, 4, metal)
    m.bar('Cradle', 1.4, 2, 3, 3, base)
    for sign, side in ((-1, 'L'), (1, 'R')):
        m.bar(f'Prong {side}', 1, 4.4, .6, 1.2, metal, x=8 + sign * 2)
        m.bar(f'Prong back {side}', 1, 4.4, 1.2, .6, metal, z=8 + sign * 2)
    m.taper('Gem base', 2, 5, 1.8, 3.2, 1.8, gem, 3, d1=3.2)
    m.bar('Gem heart', 3.4, 5.4, 3.5, 2.2, light)
    m.taper('Gem top', 5, 8, 3, .8, 3, gem, 3, d1=.8)
    m.bar('Spark', 7.8, 8.6, .5, .5, core)
    return m


def remote():
    m = Model()
    case, shade, dark, screen, metal = '46aaeb', '2a70a8', '20242c', '9cf0c0', 'd2dceb'
    m.box('Case', (6.8, 0, 7.4), (9.2, 4.2, 8.6), case)
    m.box('Case top', (7, 4.2, 7.5), (9, 4.6, 8.5), shade)
    m.box('Battery cover', (7.1, .3, 8.6), (8.9, 2, 8.7), shade)
    m.box('Screen frame', (7.1, 2.5, 7.3), (8.9, 3.9, 7.4), dark)
    m.box('Screen', (7.25, 2.65, 7.22), (8.75, 3.75, 7.3), screen)
    m.box('Button A', (7.2, 1.4, 7.25), (7.8, 2, 7.4), 'f0c850')
    m.box('Button B', (8.2, 1.4, 7.25), (8.8, 2, 7.4), 'e8483c')
    m.box('Pad up', (7.8, .9, 7.28), (8.2, 1.2, 7.4), dark)
    m.box('Pad side', (7.5, .55, 7.28), (8.5, .85, 7.4), dark)
    m.box('Antenna base', (8.2, 4.6, 7.8), (8.8, 5.1, 8.2), metal)
    m.box('Antenna', (8.4, 5.1, 7.92), (8.6, 7.8, 8.08), metal)
    m.box('Antenna tip', (8.3, 7.8, 7.85), (8.7, 8.2, 8.15), 'e8483c')
    return m


def armor(main, shade, light, trim, gem):
    """Chest plate shared by the armours: back, front, shoulders, belt."""
    m = Model()
    m.box('Back', (4.2, 3, 8.8), (11.8, 12.4, 10), shade)
    m.box('Sides', (3.8, 4, 6.6), (12.2, 12, 9.4), main)
    m.box('Chest', (4.2, 3, 6), (11.8, 12.4, 7), main)
    m.box('Neck hole', (6, 12.2, 6.4), (10, 12.5, 9.6), '20242c')
    m.box('Collar front', (5.4, 12.4, 5.8), (10.6, 13.2, 6.6), trim)
    m.box('Collar back', (5, 12.4, 9.2), (11, 13.8, 10.2), trim)
    m.box('Sternum', (7.4, 4.2, 5.75), (8.6, 11.8, 6.1), trim)
    m.box('Gem', (7.2, 8.8, 5.55), (8.8, 10.4, 5.85), gem)
    for sign, side in ((-1, 'L'), (1, 'R')):
        cx = 8 + sign * 5.8
        m.bar(f'Shoulder {side}', 10, 12.6, 3.6, 4.6, main, x=cx)
        m.bar(f'Shoulder top {side}', 12.6, 13.2, 2.8, 3.8, light, x=cx)
        m.bar(f'Shoulder rim {side}', 9.6, 10, 3.8, 4.8, trim, x=cx)
        m.box(f'Pec {side}', (8 + sign * 2.6 - 1.7, 8.4, 5.8), (8 + sign * 2.6 + 1.7, 11.4, 6.05), light)
    for i in range(3):
        m.box(f'Rib {i + 1}', (5 + i * .3, 4.4 + i * 1.2, 5.8), (11 - i * .3, 5.1 + i * 1.2, 6.05), shade)
    m.box('Belt', (4, 2, 5.7), (12, 3.4, 10.3), '3c4050')
    m.box('Buckle', (7, 2.1, 5.5), (9, 3.3, 5.8), trim)
    return m


def mineral_coat():
    m = armor('969aa4', '6c707a', 'bcc0c8', '54565e', '8ad4ff')
    for name, a, b in (('Rock 1', (4.4, 6, 5.6), (6.2, 7.8, 6.2)), ('Rock 2', (9.6, 5.2, 5.6), (11.4, 7, 6.2)), ('Rock 3', (10.2, 11, 5.6), (11.6, 12.2, 6.2))):
        m.box(name, a, b, '7c8088')
    m.box('Crystal L', (2.6, 13.2, 7.4), (3.4, 14.8, 8.2), '8ad4ff')
    m.box('Crystal R', (12.8, 13.2, 7.4), (13.6, 14.4, 8.2), '8ad4ff')
    return m


def technic_armor():
    m = armor('a0aabe', '6e788c', 'd0d8e6', '3a4252', 'ffb040')
    for sign, side in ((-1, 'L'), (1, 'R')):
        for i in range(2):
            m.box(f'Bolt {side}{i + 1}', (8 + sign * 3.2 - .3, 5 + i * 5, 5.65), (8 + sign * 3.2 + .3, 5.6 + i * 5, 5.85), 'f0c850')
        m.box(f'Light {side}', (8 + sign * 5.8 - 1, 11.2, 5.45), (8 + sign * 5.8 + 1, 11.6, 5.75), 'ffb040')
    m.box('Panel', (5.6, 6, 5.72), (6.8, 7.8, 5.9), '3a4252')
    return m


def refreatech_armor():
    m = armor('c8dcff', '8ca8d8', 'eef4ff', '4a78c0', '8cf0ff')
    for sign, side in ((-1, 'L'), (1, 'R')):
        m.box(f'Pipe {side}', (8 + sign * 3.4 - .3, 3.6, 5.5), (8 + sign * 3.4 + .3, 8.2, 5.8), '8cf0ff')
        m.box(f'Tank {side}', (8 + sign * 2 - 1, 5, 10), (8 + sign * 2 + 1, 12, 11.2), '8cf0ff')
        m.box(f'Tank cap {side}', (8 + sign * 2 - .7, 12, 10.2), (8 + sign * 2 + .7, 12.5, 11), '4a78c0')
    for i in range(3):
        m.box(f'Vent {i + 1}', (6, 8.9 + i * .55 - 4.3, 5.68), (10, 9.2 + i * .55 - 4.3, 5.85), '4a78c0')
    return m


def astral_revetment():
    m = armor('ffec96', 'd8aa3e', 'fffbe6', 'b8862c', 'ffffff')
    m.box('Star v', (7.8, 8, 5.5), (8.2, 11.2, 5.65), 'ffffff')
    m.box('Star h', (6.6, 9.4, 5.5), (9.4, 9.8, 5.65), 'ffffff')
    for sign, side in ((-1, 'L'), (1, 'R')):
        m.box(f'Wing {side}', (8 + sign * 6.6 - .5, 11, 8.4), (8 + sign * 6.6 + .5, 15, 9.2), 'fffbe6')
        m.box(f'Wing tip {side}', (8 + sign * 7.3 - .3, 13, 8.5), (8 + sign * 7.3 + .3, 16, 9), 'ffec96')
        m.box(f'Spike {side}', (8 + sign * 5.8 - .4, 13.2, 7.6), (8 + sign * 5.8 + .4, 14.4, 8.4), 'ffffff')
    return m


def mnemo_symbiote():
    m = armor('dc4a5c', '9a2c3e', 'f8a0a8', '6a1c2c', 'ffd05a')
    for i, (x, y, h) in enumerate(((5, 4.2, 4), (6.4, 3.4, 3), (9.2, 3.6, 3.4), (10.8, 4.6, 4.6))):
        m.box(f'Vein {i + 1}', (x, y, 5.6), (x + .5, y + h, 5.85), 'f8a0a8')
    m.box('Eye white', (6.8, 8.6, 5.35), (9.2, 10.6, 5.6), 'f0ffe8')
    m.box('Eye iris', (7.4, 9, 5.25), (8.6, 10.2, 5.4), 'ffd05a')
    m.box('Eye pupil', (7.85, 9.2, 5.18), (8.15, 10, 5.3), '20101a')
    for sign, side in ((-1, 'L'), (1, 'R')):
        for i in range(3):
            m.box(f'Tendril {side}{i + 1}', (8 + sign * (5 + i * .9) - .3, 13.2 - i * .9, 7.6), (8 + sign * (5 + i * .9) + .3, 14.2 - i * .4, 8.2), 'dc4a5c')
    return m


DESIGNS = {
    'sword': sword, 'astral_saber': astral_saber, 'stone_axe': stone_axe, 'stone_pickaxe': stone_pickaxe,
    'wrench': wrench, 'brush': brush, 'blaster': blaster, 'blastygun': blastygun, 'etheragun': etheragun,
    'voltigun': voltigun, 'atomixer': atomixer, 'binder': binder, 'cable': cable, 'etherealink': etherealink,
    'remote': remote, 'mineral_coat': mineral_coat, 'technic_armor': technic_armor,
    'refreatech_armor': refreatech_armor, 'astral_revetment': astral_revetment, 'mnemo_symbiote': mnemo_symbiote,
}

# Their decorated side is the -Z of the drawings; icons and the Armoury look at +Z.
TURNED = {'remote', 'mineral_coat', 'technic_armor', 'refreatech_armor', 'astral_revetment', 'mnemo_symbiote'}


def turned(model):
    for index, (name, start, end, color) in enumerate(model.parts):
        model.parts[index] = (name, (start[0], start[1], 16 - end[2]), (end[0], end[1], 16 - start[2]), color)
    return model


if __name__ == '__main__':
    for name, design in DESIGNS.items():
        write(name, turned(design()) if name in TURNED else design())
    print(len(DESIGNS), 'items written')
