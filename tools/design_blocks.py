"""Block designs of the project, written as ordinary `project/blocks/*.ron` data.

Nothing here reaches the engine code: this script only fills the fields the
Forge edits (model elements, animated parts, face colours, flecks). Every
block stays editable in the Forge afterwards, but running this script again
replaces the edits of the blocks listed here. The nine synthetic shapes and
the plain liquids are left alone. Run from the project root:

    python tools/design_blocks.py

Units are 1/16 of a block. The front of a block is -Z (it faces the player who
places it), `+Y` is up. Model elements are collidable and stay inside the
cell; parts are visual, can leave it and are the ones that move.
"""
import os

BLOCKS = 'project/blocks'

# Palette.
SOOT = (40, 38, 40)
IRON = (74, 80, 92)
DARK = (38, 42, 52)
STEEL = (124, 132, 146)
LIGHT = (176, 184, 196)
WHITE = (232, 236, 244)
BRASS = (196, 154, 72)
GOLD = (236, 196, 84)
COPPER = (190, 106, 62)
WOOD = (122, 84, 52)
WOOD2 = (148, 104, 64)
BRICK = (150, 66, 48)
BRICK2 = (118, 52, 42)
STONE = (112, 116, 126)
STONE2 = (86, 90, 100)
EMBER = (255, 120, 30)
FLAME = (255, 186, 60)
CYAN = (90, 230, 240)
BLUE = (70, 120, 240)
NAVY = (22, 30, 66)
VIOLET = (160, 90, 240)
MAGENTA = (230, 70, 200)
LIME = (140, 240, 90)
GREEN = (60, 170, 80)
MNEMO = (236, 52, 70)
FLESH = (176, 44, 56)
FLESH2 = (112, 24, 38)
BONE = (232, 222, 196)
RED = (240, 60, 50)
YELLOW = (244, 214, 50)
BLACK = (14, 14, 18)
GLASS = (170, 225, 240)


def E(name, box, color):
    """A model element."""
    return {'name': name, 'box': box, 'color': color}


def P(name, box, color, **kw):
    """A part. `pivot` defaults to the centre of the box."""
    part = {'name': name, 'box': box, 'color': color}
    part.update(kw)
    return part


def center(box):
    return ((box[0] + box[3]) / 2, (box[1] + box[4]) / 2, (box[2] + box[5]) / 2)


def around(c, half, thick, axis='y'):
    """The four bars of a square ring around an axis through `c`."""
    x, y, z = c
    h, t = half, thick / 2
    if axis == 'y':
        return [(x - h, y - t, z - h, x + h, y + t, z - h + thick), (x - h, y - t, z + h - thick, x + h, y + t, z + h),
                (x - h, y - t, z - h, x - h + thick, y + t, z + h), (x + h - thick, y - t, z - h, x + h, y + t, z + h)]
    if axis == 'z':
        return [(x - h, y - h, z - t, x + h, y - h + thick, z + t), (x - h, y + h - thick, z - t, x + h, y + h, z + t),
                (x - h, y - h, z - t, x - h + thick, y + h, z + t), (x + h - thick, y - h, z - t, x + h, y + h, z + t)]
    return [(x - t, y - h, z - h, x + t, y - h + thick, z + h), (x - t, y + h - thick, z - h, x + t, y + h, z + h),
            (x - t, y - h, z - h, x + t, y + h, z - h + thick), (x - t, y - h, z + h - thick, x + t, y + h, z + h)]


def ring(name, c, half, thick, color, axis='y', **kw):
    """A square ring of parts turning together around `c`."""
    kw.setdefault('pivot', c)
    return [P(f'{name} {i + 1}', box, color, **kw) for i, box in enumerate(around(c, half, thick, axis))]


def corners(y0, y1, color, inset=0, size=2, name='Post'):
    a, b = inset, 16 - inset - size
    return [E(f'{name} {i + 1}', (x, y0, z, x + size, y1, z + size), color) for i, (x, z) in enumerate([(a, a), (b, a), (a, b), (b, b)])]


def combustor():
    fire = dict(motion='Powered', pivot=(8, 3, 6), speed=11.0)
    return [
        E('Plinth', (0, 0, 0, 16, 2, 16), SOOT),
        E('Left wall', (1, 2, 1, 5, 12, 15), BRICK),
        E('Right wall', (11, 2, 1, 15, 12, 15), BRICK2),
        E('Back wall', (5, 2, 9, 11, 12, 15), BRICK),
        E('Arch', (5, 9, 1, 11, 12, 9), BRICK2),
        E('Firebox', (5, 2, 1, 11, 3, 9), SOOT),
        E('Hood', (1, 12, 1, 15, 14, 15), IRON),
        E('Chimney', (9, 14, 9, 14, 16, 14), DARK),
        E('Grate 1', (6, 3, 1, 7, 9, 2), IRON),
        E('Grate 2', (9, 3, 1, 10, 9, 2), IRON),
    ], [
        P('Embers', (5.5, 3, 2, 10.5, 4, 8.5), EMBER, emissive=0.3, powered_color=EMBER, powered_emissive=4.0),
        P('Flame', (6, 3, 4, 10, 8, 7), EMBER, emissive=0.2, powered_color=EMBER, powered_emissive=3.0, pulse=0.28, **fire),
        P('Heart of the flame', (7, 3, 4.5, 9, 6.5, 6.5), FLAME, emissive=0.2, powered_color=FLAME, powered_emissive=5.0, pulse=0.34, phase=1.1, **fire),
        P('Chimney mouth', (10, 15.8, 10, 13, 16.2, 13), BLACK),
        P('Smoke', (10.5, 16.5, 10.5, 12.5, 18.5, 12.5), (120, 120, 126), bob=0.12, speed=3.0, motion='Powered'),
        P('Smoke 2', (10, 19, 10, 13, 21, 13), (150, 150, 156), bob=0.16, speed=2.2, phase=1.6, motion='Powered'),
    ]


def solar():
    tilt = dict(pivot=(8, 7, 8), swing=26.0, spin_axis=(1, 0, 0), speed=0.35, rotation=(0, 0, 0))
    cells = []
    for i in range(3):
        for j in range(3):
            x, z = 1.5 + i * 4.5, 1.5 + j * 4.5
            cells.append(P(f'Cell {i}-{j}', (x, 6.7, z, x + 4, 7.2, z + 4), (26, 44, 120), metallic=0.7, emissive=0.15, powered_color=(40, 90, 200), powered_emissive=0.8, **tilt))
    return [
        E('Base', (2, 0, 2, 14, 1, 14), IRON),
        E('Mast', (7, 1, 7, 9, 5, 9), STEEL),
        E('Bearing', (6, 5, 6, 10, 6, 10), DARK),
    ], cells + [
        P('Panel', (1, 6, 1, 15, 6.7, 15), DARK, metallic=0.5, **tilt),
        P('Rail', (7.8, 6.6, 1, 8.2, 7.4, 15), LIGHT, **tilt),
    ]


def synthesizer():
    stir = dict(pivot=(8, 8, 8), spin=5.0, motion='Powered')
    return [
        E('Bench', (0, 0, 0, 16, 3, 16), IRON),
        E('Tank floor', (3, 3, 3, 13, 4, 13), DARK),
        E('Tank lid', (3, 12, 3, 13, 14, 13), STEEL),
        *corners(4, 12, STEEL, 3, 1, 'Tank post'),
        E('Floral hopper', (0, 11, 5, 4, 16, 11), GREEN),
        E('Mineral hopper', (12, 11, 5, 16, 16, 11), STEEL),
        E('Feed pipe L', (3, 12, 7, 5, 13, 9), COPPER),
        E('Feed pipe R', (11, 12, 7, 13, 13, 9), COPPER),
        E('Outlet', (6, 0, 0, 10, 2.5, 3), COPPER),
    ], [
        P('Brew', (4, 4, 4, 12, 8, 12), (70, 220, 190), emissive=0.5, powered_emissive=1.5, powered_color=(120, 255, 220), pulse=0.04, speed=3.0, pivot=(8, 4, 8)),
        P('Shaft', (7.5, 5, 7.5, 8.5, 12, 8.5), LIGHT, metallic=0.8, **stir),
        P('Paddle 1', (5, 6, 7.5, 11, 7, 8.5), LIGHT, metallic=0.8, **stir),
        P('Paddle 2', (7.5, 8, 5, 8.5, 9, 11), LIGHT, metallic=0.8, **stir),
        P('Bubble', (5, 8, 5, 6, 9, 6), WHITE, emissive=0.8, bob=0.1, speed=4.0, motion='Powered'),
        P('Bubble 2', (10, 8, 9, 11, 9, 10), WHITE, emissive=0.8, bob=0.12, speed=5.0, phase=2.0, motion='Powered'),
    ]


def infuser():
    return [
        E('Burner', (2, 0, 2, 14, 2, 14), DARK),
        *corners(2, 4, BRASS, 3, 2, 'Foot'),
        E('Flask base', (3, 4, 3, 13, 5, 13), GLASS),
        E('Flask belly', (2, 5, 2, 14, 9, 14), (190, 150, 240)),
        E('Flask shoulder', (4, 9, 4, 12, 11, 12), (200, 170, 245)),
        E('Flask neck', (6, 11, 6, 10, 15, 10), GLASS),
        E('Stopper', (5.5, 15, 5.5, 10.5, 16, 10.5), BRASS),
        E('Still pipe', (10, 13, 7, 16, 14, 9), COPPER),
    ], [
        P('Coil', (3, 2.5, 3, 13, 3.5, 13), EMBER, emissive=0.3, powered_color=EMBER, powered_emissive=3.5),
        P('Essence', (3, 5.5, 3, 13, 8.5, 13), MAGENTA, emissive=0.8, powered_color=(255, 110, 240), powered_emissive=2.2, pulse=0.05, pivot=(8, 5, 8), speed=2.0),
        P('Bubble 1', (5, 8.8, 5, 6.5, 10.3, 6.5), WHITE, emissive=1.0, bob=0.16, speed=4.2, motion='Powered'),
        P('Bubble 2', (9, 8.8, 8, 10.5, 10.3, 9.5), WHITE, emissive=1.0, bob=0.2, speed=3.3, phase=1.4, motion='Powered'),
        P('Bubble 3', (7.3, 11, 7.3, 8.5, 12.2, 8.5), WHITE, emissive=1.0, bob=0.14, speed=5.0, phase=2.6, motion='Powered'),
    ]


def vacuum():
    fan = dict(pivot=(8, 8, 2), spin=16.0, spin_axis=(0, 0, 1), motion='Powered', metallic=0.7)
    return [
        E('Body', (2, 2, 4, 14, 14, 13), STEEL),
        E('Skid', (1, 0, 3, 15, 2, 15), DARK),
        E('Bag', (4, 3, 13, 12, 13, 16), (170, 130, 240)),
        E('Bag strap', (3.5, 7, 12.5, 12.5, 8, 16), DARK),
        E('Cowl top', (1, 13, 0, 15, 15, 4), IRON),
        E('Cowl bottom', (1, 1, 0, 15, 3, 4), IRON),
        E('Cowl left', (1, 3, 0, 3, 13, 4), IRON),
        E('Cowl right', (13, 3, 0, 15, 13, 4), IRON),
        E('Intake', (3, 3, 3, 13, 13, 4), BLACK),
        E('Astral hose', (13, 13, 8, 16, 16, 11), BLUE),
    ], [
        P('Blade 1', (7, 3.5, 2, 9, 12.5, 2.6), LIGHT, **fan),
        P('Blade 2', (3.5, 7, 2, 12.5, 9, 2.6), LIGHT, **fan),
        P('Blade 3', (7, 3.5, 2, 9, 12.5, 2.6), STEEL, rotation=(0, 0, 45), **fan),
        P('Blade 4', (7, 3.5, 2, 9, 12.5, 2.6), STEEL, rotation=(0, 0, -45), **fan),
        P('Hub', (6.5, 6.5, 1.5, 9.5, 9.5, 3), BRASS, **fan),
        P('Gauge', (5, 15.2, 5, 11, 15.8, 11), DARK),
        P('Gauge light', (6, 15.5, 6, 10, 16.2, 10), CYAN, emissive=1.0, powered_emissive=3.0, powered_color=VIOLET, pulse=0.1, speed=6.0),
    ]


def harvester():
    arm = lambda side, phase: dict(pivot=(8 + side * 5, 8, 8), swing=24.0, spin_axis=(0, 0, 1), speed=3.0, phase=phase, motion='Powered')
    return [
        E('Root mat', (0, 0, 0, 16, 2, 16), (70, 56, 40)),
        E('Gut', (2, 2, 2, 14, 9, 14), (196, 176, 84)),
        E('Rind', (3, 9, 3, 13, 12, 13), (170, 150, 66)),
        E('Crown', (5, 12, 5, 11, 14, 11), (140, 118, 54)),
        E('Vein L', (1, 2, 7, 2, 8, 9), MNEMO),
        E('Vein R', (14, 2, 7, 15, 8, 9), MNEMO),
        E('Mouth lip', (5, 4, 1, 11, 8, 2), FLESH2),
    ], [
        P('Eye', (6, 12.5, 6, 10, 15.5, 10), MNEMO, emissive=1.5, powered_emissive=3.5, powered_color=(255, 110, 100), pulse=0.1, speed=2.0),
        P('Arm L', (0, 8, 7, 3, 14, 9), (150, 130, 60), **arm(-1, 0.0)),
        P('Claw L', (-2, 13, 6.5, 2, 15, 9.5), BONE, **arm(-1, 0.0)),
        P('Arm R', (13, 8, 7, 16, 14, 9), (150, 130, 60), **arm(1, 3.14)),
        P('Claw R', (14, 13, 6.5, 18, 15, 9.5), BONE, **arm(1, 3.14)),
        P('Maw', (6, 5, 0.6, 10, 7, 1.4), BLACK),
    ]


def energizer():
    return [
        E('Base', (1, 0, 1, 15, 2, 15), DARK),
        E('Step', (3, 2, 3, 13, 3, 13), IRON),
        E('Pylon', (6, 3, 6, 10, 13, 10), STEEL),
        E('Coil 1', (4, 4, 4, 12, 5, 12), COPPER),
        E('Coil 2', (4.5, 6, 4.5, 11.5, 7, 11.5), COPPER),
        E('Coil 3', (5, 8, 5, 11, 9, 11), COPPER),
        E('Coil 4', (5.5, 10, 5.5, 10.5, 11, 10.5), COPPER),
        *corners(2, 5, BRASS, 1, 1, 'Terminal'),
    ], [
        P('Core', (5.5, 12, 5.5, 10.5, 16, 10.5), BLUE, emissive=1.2, powered_color=CYAN, powered_emissive=4.0, pulse=0.12, speed=7.0, pivot=(8, 14, 8)),
        P('Top', (7, 15.5, 7, 9, 17, 9), WHITE, emissive=2.0, powered_emissive=5.0, powered_color=WHITE),
        *ring('Field', (8, 9, 8), 7.5, 0.6, CYAN, emissive=1.0, bob=0.22, speed=3.2, motion='Powered', spin=1.4),
        *ring('Field low', (8, 5, 8), 7.5, 0.6, BLUE, emissive=1.0, bob=0.18, speed=3.2, phase=2.1, motion='Powered', spin=-1.4),
    ]


def container():
    lock = dict(pivot=(8, 9.5, 0.5), swing=12.0, spin_axis=(1, 0, 0), speed=2.0)
    return [
        E('Crate', (1, 0, 1, 15, 11, 15), WOOD),
        E('Plank L', (1, 1, 0.5, 4, 10, 1), WOOD2),
        E('Plank R', (12, 1, 0.5, 15, 10, 1), WOOD2),
        E('Plank M', (6.5, 1, 0.5, 9.5, 10, 1), WOOD2),
        E('Band low', (0.5, 2, 0.5, 15.5, 3, 15.5), IRON),
        E('Band high', (0.5, 8, 0.5, 15.5, 9, 15.5), IRON),
        E('Lid', (0, 11, 0, 16, 14, 16), WOOD2),
        E('Lid crown', (1, 14, 1, 15, 16, 15), WOOD),
        E('Lid rib', (7, 14, 0, 9, 16.0, 16), IRON),
        *corners(0, 14, IRON, 0, 1, 'Corner'),
    ], [
        P('Hasp', (7, 9, 0, 9, 12, 0.6), BRASS, metallic=0.9),
        P('Padlock', (6.8, 6.5, -0.4, 9.2, 9, 0.4), GOLD, metallic=0.9, **lock),
    ]


def lamp():
    return [
        E('Foot', (3, 0, 3, 13, 2, 13), BRASS),
        E('Collar', (5, 2, 5, 11, 3, 11), DARK),
        E('Cap', (3, 13, 3, 13, 14.5, 13), BRASS),
        E('Cap top', (5, 14.5, 5, 11, 15.5, 11), DARK),
        E('Hook', (7, 15.5, 7, 9, 16, 9), BRASS),
        *corners(3, 13, BRASS, 3.5, 1, 'Cage bar'),
        E('Glass', (5, 3, 5, 11, 13, 11), (255, 240, 190)),
    ], [
        P('Glow', (4.6, 3.2, 4.6, 11.4, 12.8, 11.4), (255, 232, 160), emissive=0.2, powered_color=(255, 236, 170), powered_emissive=4.0),
        P('Filament', (7.3, 5, 7.3, 8.7, 11, 8.7), WHITE, emissive=0.4, powered_color=WHITE, powered_emissive=7.0, pulse=0.06, speed=12.0, motion='Powered'),
    ]


def assembler():
    rock = dict(pivot=(13, 8, 8), swing=30.0, spin_axis=(0, 0, 1), speed=2.4, motion='Powered')
    return [
        E('Bench', (0, 5, 0, 16, 7, 16), (96, 120, 150)),
        *corners(0, 5, DARK, 1, 2, 'Bench leg'),
        E('Drawer', (3, 2, 1, 13, 5, 3), IRON),
        E('Drawer handle', (6, 3, 0.5, 10, 4, 1), BRASS),
        E('Arm column', (11, 7, 11, 15, 9, 15), DARK),
        E('Arm tower', (12, 9, 12, 14, 16, 14), STEEL),
        E('Work plate', (2, 7, 2, 10, 7.5, 10), LIGHT),
    ], [
        P('Arm', (4, 13.5, 7, 14, 15, 9), YELLOW, metallic=0.4, pivot=(13, 14, 13), swing=22.0, speed=2.4, motion='Powered'),
        P('Wrist', (3, 10, 7, 5, 14, 9), STEEL, pivot=(13, 14, 13), swing=22.0, speed=2.4, motion='Powered'),
        P('Torch', (3.4, 8.5, 7.4, 4.6, 10, 8.6), CYAN, emissive=1.0, powered_color=WHITE, powered_emissive=6.0, pivot=(13, 14, 13), swing=22.0, speed=2.4, motion='Powered'),
        P('Workpiece', (4, 7.5, 5, 8, 10, 9), COPPER, spin=0.8, pivot=(6, 8, 7), motion='Powered', metallic=0.6),
        P('Lamp', (13, 16, 13, 13.8, 16.8, 13.8), RED, emissive=0.5, powered_color=LIME, powered_emissive=3.0),
    ]


def omnichorus():
    pipes = []
    for i, (x, h, tone) in enumerate([(1, 12, WHITE), (4, 15, LIGHT), (10, 15, LIGHT), (13, 12, WHITE)]):
        pipes.append(E(f'Pipe {i + 1}', (x, 3, 11, x + 2, h, 13), tone))
        pipes.append(E(f'Pipe lip {i + 1}', (x - 0.4, h, 10.6, x + 2.4, h + 1 if h < 15 else 16, 13.4), GOLD))
    return [
        E('Dais', (0, 0, 0, 16, 2, 16), WHITE),
        E('Dais step', (2, 2, 1, 14, 3, 15), GOLD),
        E('Console', (3, 3, 2, 13, 5, 8), LIGHT),
        E('Keys', (4, 5, 3, 12, 5.5, 6), BLACK),
        E('Tall pipe', (7, 3, 11, 9, 16, 13), GOLD),
        *pipes,
    ], [
        P('Omnicore', (6, 9, 5, 10, 13, 9), (250, 250, 255), emissive=3.0, spin=1.5, bob=0.12, speed=1.8, rotation=(45, 0, 35), pivot=(8, 11, 7)),
        *ring('Halo', (8, 11, 7), 4.5, 0.5, GOLD, emissive=1.5, spin=-1.0, bob=0.12, speed=1.8, axis='y'),
        P('Note 1', (4.4, 16, 11.4, 5.6, 17.2, 12.6), CYAN, emissive=2.0, bob=0.16, speed=3.0, motion='Powered'),
        P('Note 2', (10.4, 16, 11.4, 11.6, 17.2, 12.6), MAGENTA, emissive=2.0, bob=0.16, speed=3.0, phase=3.14, motion='Powered'),
    ]


def repulsor():
    wave = dict(pivot=(8, 10, 8), speed=7.0, motion='Powered')
    return [
        E('Base', (1, 0, 1, 15, 3, 15), IRON),
        E('Neck', (4, 3, 4, 12, 5, 12), DARK),
        E('Cone low', (3, 5, 3, 13, 7, 13), STEEL),
        E('Cone mid', (2, 7, 2, 14, 9, 14), LIGHT),
        E('Cone rim', (1, 9, 1, 15, 10, 15), DARK),
        E('Membrane', (3, 10, 3, 13, 11, 13), (70, 150, 200)),
        *corners(0, 10, CYAN, 0.5, 1, 'Spring'),
    ], [
        P('Dome', (5, 11, 5, 11, 13, 11), CYAN, emissive=1.0, powered_emissive=3.0, powered_color=WHITE, pulse=0.2, **wave),
        *ring('Blast', (8, 13, 8), 6, 0.5, CYAN, emissive=2.0, pulse=0.35, **wave),
        *ring('Blast high', (8, 15.5, 8), 4, 0.5, WHITE, emissive=2.0, pulse=0.35, phase=1.5, **wave),
    ]


def terminal():
    return [
        E('Desk', (0, 0, 0, 16, 3, 16), (60, 66, 80)),
        E('Keyboard', (2, 3, 0.5, 14, 4, 5), (50, 54, 64)),
        E('Keys', (3, 4, 1, 13, 4.4, 4.5), LIGHT),
        E('Monitor', (2, 3, 6, 14, 15, 15), (70, 76, 90)),
        E('Bezel', (1.5, 5, 5, 14.5, 15.5, 6.5), DARK),
        E('Screen', (3, 6.5, 4.8, 13, 14, 5), BLACK),
        E('Vent', (5, 15, 8, 11, 15.5, 13), DARK),
        E('Disk slot', (9, 3.5, 15, 13, 5, 15.5), BLACK),
    ], [
        P('Line 1', (4, 12.3, 4.6, 11, 13, 4.8), LIME, emissive=0.6, powered_emissive=2.5, powered_color=LIME),
        P('Line 2', (4, 10.8, 4.6, 9, 11.5, 4.8), LIME, emissive=0.6, powered_emissive=2.5, powered_color=LIME),
        P('Line 3', (4, 9.3, 4.6, 12, 10, 4.8), LIME, emissive=0.6, powered_emissive=2.5, powered_color=LIME),
        P('Line 4', (4, 7.8, 4.6, 7, 8.5, 4.8), LIME, emissive=0.6, powered_emissive=2.5, powered_color=LIME),
        P('Cursor', (7.5, 7.7, 4.5, 8.5, 8.6, 4.8), WHITE, emissive=3.0, pulse=0.9, speed=7.0),
        P('Power light', (12.2, 5.4, 4.8, 13, 6, 5), RED, emissive=0.5, powered_color=LIME, powered_emissive=3.0),
    ]


def comandor():
    lever = lambda x, color, phase: [
        P(f'Rod {x}', (x, 9, 7.5, x + 1, 14, 8.5), LIGHT, pivot=(x + 0.5, 8.5, 8), swing=22.0, spin_axis=(1, 0, 0), speed=1.6, phase=phase, metallic=0.8, motion='Powered'),
        P(f'Knob {x}', (x - 0.5, 14, 7, x + 1.5, 16, 9), color, emissive=0.3, pivot=(x + 0.5, 8.5, 8), swing=22.0, spin_axis=(1, 0, 0), speed=1.6, phase=phase, motion='Powered'),
    ]
    return [
        E('Pedestal', (3, 0, 3, 13, 7, 13), (60, 70, 96)),
        E('Foot', (1, 0, 1, 15, 2, 15), DARK),
        E('Desk', (1, 7, 1, 15, 9, 15), (80, 92, 122)),
        E('Slot', (3, 8.6, 6.5, 13, 9.2, 9.5), BLACK),
        E('Dial plate', (2, 9, 11, 14, 10, 14), STEEL),
        E('Name plate', (5, 3, 2.6, 11, 6, 3), BRASS),
    ], [
        *lever(4, RED, 0.0), *lever(7.5, YELLOW, 1.8), *lever(11, LIME, 3.4),
        P('Button 1', (3, 9, 2.5, 5, 9.8, 4.5), CYAN, emissive=1.5, pulse=0.2, speed=5.0),
        P('Button 2', (7, 9, 2.5, 9, 9.8, 4.5), MAGENTA, emissive=1.5, pulse=0.2, speed=5.0, phase=2.0),
        P('Button 3', (11, 9, 2.5, 13, 9.8, 4.5), YELLOW, emissive=1.5, pulse=0.2, speed=5.0, phase=4.0),
        P('Needle', (7.7, 10, 12.2, 8.3, 10.6, 13.8), RED, emissive=1.0, swing=50.0, pivot=(8, 10, 13), speed=1.4),
    ]


def transmitter():
    return [
        E('Board', (0, 0, 0, 16, 3, 16), (34, 90, 60)),
        E('Trace 1', (1, 3, 3, 15, 3.2, 3.6), GOLD),
        E('Trace 2', (1, 3, 12.4, 15, 3.2, 13), GOLD),
        E('Trace 3', (3, 3, 3, 3.6, 3.2, 13), GOLD),
        E('Chip', (4, 3, 5, 12, 6, 11), BLACK),
        E('Chip mark', (5, 6, 6, 7, 6.2, 8), LIGHT),
        *[E(f'Pin {i}', (4.5 + i * 2, 3, 4, 5.5 + i * 2, 4, 12), LIGHT) for i in range(4)],
        E('Mast', (7, 6, 7, 9, 14, 9), STEEL),
        E('Mast tip', (7.5, 14, 7.5, 8.5, 16, 8.5), LIGHT),
    ], [
        P('Gate A', (1, 3, 6, 3, 4.5, 8), RED, emissive=0.5, powered_color=LIME, powered_emissive=3.0),
        P('Gate B', (1, 3, 9, 3, 4.5, 11), RED, emissive=0.5, powered_color=LIME, powered_emissive=3.0),
        P('Out', (13, 3, 7, 15, 4.5, 9), CYAN, emissive=1.0, pulse=0.25, speed=6.0),
        *ring('Wave', (8, 12, 8), 3.5, 0.4, CYAN, emissive=2.0, bob=0.14, speed=5.0, motion='Powered'),
        *ring('Wave up', (8, 15, 8), 2.5, 0.4, WHITE, emissive=2.0, bob=0.14, speed=5.0, phase=2.0, motion='Powered'),
    ]


def visualiser():
    bars = []
    for i, tone in enumerate([LIME, LIME, YELLOW, YELLOW, RED]):
        x = 3.6 + i * 1.9
        bars.append(P(f'Bar {i + 1}', (x, 5, 1.7, x + 1.3, 11, 1.9), tone, emissive=1.0, powered_emissive=2.5, powered_color=tone, pivot=(x + 0.6, 5, 1.8), pulse=0.45, speed=4.0 + i * 1.3, phase=i * 1.7))
    return [
        E('Cabinet', (1, 0, 2, 15, 13, 15), (44, 48, 56)),
        E('Bezel low', (2, 2, 1, 14, 4, 2.2), (70, 76, 88)),
        E('Bezel high', (2, 12, 1, 14, 13, 2.2), (70, 76, 88)),
        E('Bezel left', (2, 4, 1, 3, 12, 2.2), (70, 76, 88)),
        E('Bezel right', (13, 4, 1, 14, 12, 2.2), (70, 76, 88)),
        E('Tube', (3, 4, 1.9, 13, 12, 2.1), (8, 22, 14)),
        E('Knob L', (3, 0.5, 1, 5, 2, 2), LIGHT),
        E('Knob R', (11, 0.5, 1, 13, 2, 2), LIGHT),
        E('Handle', (4, 13, 7, 12, 14, 9), DARK),
        E('Handle bar', (4, 14, 7, 12, 15, 9), STEEL),
        E('Grille', (3, 3, 15, 13, 11, 15.5), DARK),
    ], bars + [
        P('Grid', (3, 7.7, 1.75, 13, 8, 1.8), (40, 120, 70), emissive=0.6),
    ]


def emitter():
    return [
        E('Base', (2, 0, 2, 14, 3, 14), (60, 110, 190)),
        E('Plate', (3, 3, 3, 13, 4, 13), DARK),
        E('Mast low', (6, 4, 6, 10, 9, 10), STEEL),
        E('Mast high', (7, 9, 7, 9, 13, 9), LIGHT),
        E('Strut L', (3, 4, 7, 6, 5, 9), IRON),
        E('Strut R', (10, 4, 7, 13, 5, 9), IRON),
        E('Switch box', (4, 3, 2.5, 12, 7, 5), IRON),
    ], [
        P('Toggle', (7.5, 7, 3, 8.5, 9, 4), RED, pivot=(8, 7, 3.5), swing=30.0, spin_axis=(1, 0, 0), speed=1.0),
        P('Lens', (6, 13, 6, 10, 16, 10), (90, 170, 255), emissive=2.0, pulse=0.2, speed=5.0),
        *ring('Pulse', (8, 13, 8), 6, 0.5, (90, 170, 255), emissive=2.0, pulse=0.5, speed=3.5),
    ]


def receiver():
    dish = dict(pivot=(8, 8, 8), spin=0.6, rotation=(-35, 0, 0))
    return [
        E('Base', (1, 0, 1, 15, 2, 15), (70, 120, 180)),
        E('Column', (6, 2, 6, 10, 6, 10), STEEL),
        E('Turntable', (4, 6, 4, 12, 7, 12), DARK),
        E('Cable box', (10, 2, 10, 14, 5, 14), IRON),
    ], [
        P('Yoke', (7, 7, 7, 9, 10, 9), STEEL, pivot=(8, 8, 8), spin=0.6),
        P('Dish', (2, 8.8, 2, 14, 10.2, 14), LIGHT, metallic=0.6, **dish),
        P('Dish rim', (3.5, 10, 3.5, 12.5, 10.8, 12.5), STEEL, **dish),
        P('Dish well', (5.5, 10.6, 5.5, 10.5, 11.2, 10.5), DARK, **dish),
        P('Horn', (7.5, 11, 7.5, 8.5, 15, 8.5), STEEL, **dish),
        P('Receiver head', (6.8, 14.6, 6.8, 9.2, 16.2, 9.2), CYAN, emissive=2.0, powered_emissive=4.0, powered_color=WHITE, **dish),
    ]


def screen():
    return [
        E('Case', (0, 0, 0, 16, 14, 16), (28, 30, 36)),
        E('Rim front', (0, 14, 0, 16, 16, 1), (48, 52, 60)),
        E('Rim back', (0, 14, 15, 16, 16, 16), (48, 52, 60)),
        E('Rim left', (0, 14, 1, 1, 16, 15), (48, 52, 60)),
        E('Rim right', (15, 14, 1, 16, 16, 15), (48, 52, 60)),
        E('Panel', (1, 14, 1, 15, 15.6, 15), BLACK),
        E('Front badge', (5, 5, -0.0, 11, 7, 0.4), STEEL),
        E('Vent 1', (2, 2, 0, 14, 3, 0.4), BLACK),
        E('Vent 2', (2, 10, 0, 14, 11, 0.4), BLACK),
    ], [
        P('Standby', (13, 12.5, -0.2, 14.5, 13.5, 0.2), RED, emissive=1.5, powered_color=LIME, powered_emissive=3.0),
    ]


def atomiser():
    orbit = lambda name, axis, spin, color, phase: [
        P(f'{name} {i + 1}', box, color, emissive=1.2, pivot=(8, 9, 8), spin=spin, spin_axis=axis, phase=phase, motion='Powered')
        for i, box in enumerate(around((8, 9, 8), 6, 0.5, {(0, 1, 0): 'y', (1, 0, 0): 'x', (0, 0, 1): 'z'}[axis]))
    ]
    return [
        E('Base', (0, 0, 0, 16, 2, 16), WHITE),
        E('Well', (2, 2, 2, 14, 3, 14), LIGHT),
        *corners(2, 15, LIGHT, 0, 2, 'Pillar'),
        E('Lid front', (0, 15, 0, 16, 16, 2), WHITE),
        E('Lid back', (0, 15, 14, 16, 16, 16), WHITE),
        E('Lid left', (0, 15, 2, 2, 16, 14), WHITE),
        E('Lid right', (14, 15, 2, 16, 16, 14), WHITE),
        *[E(f'Warning {i}', box, YELLOW) for i, box in enumerate([(3, 0.5, -0.1, 6, 1.5, 0.1), (7, 0.5, -0.1, 10, 1.5, 0.1), (11, 0.5, -0.1, 14, 1.5, 0.1)])],
    ], [
        P('Nucleus', (6, 7, 6, 10, 11, 10), (240, 240, 255), emissive=2.0, powered_color=CYAN, powered_emissive=6.0, pulse=0.18, speed=9.0),
        *orbit('Orbit A', (0, 1, 0), 3.0, CYAN, 0.0),
        *orbit('Orbit B', (1, 0, 0), -4.0, MAGENTA, 0.0),
        *orbit('Orbit C', (0, 0, 1), 5.0, YELLOW, 0.0),
    ]


def ressourcer():
    scan = dict(bob=0.32, bob_axis=(0, 0, 1), speed=2.0, motion='Powered')
    tiles = [E(f'Tile {i}-{j}', (1.2 + i * 4.7, 15, 1.2 + j * 4.7, 5.2 + i * 4.7, 16, 5.2 + j * 4.7), (130, 210, 130)) for i in range(3) for j in range(3)]
    return [
        E('Housing', (0, 0, 0, 16, 14, 16), (70, 110, 80)),
        E('Grate', (0.6, 14, 0.6, 15.4, 15, 15.4), DARK),
        E('Hatch', (4, 3, -0.2, 12, 11, 0.4), (54, 90, 66)),
        E('Hatch handle', (6, 6, -0.6, 10, 7, 0), BRASS),
        E('Pipe L', (-0.0, 8, 4, 0.5, 10, 12), COPPER),
        E('Pipe R', (15.5, 8, 4, 16, 10, 12), COPPER),
        *tiles,
    ], [
        P('Scanner', (1, 16, 7.6, 15, 16.5, 8.4), LIME, emissive=2.0, powered_emissive=4.0, powered_color=CYAN, **scan),
        P('Status', (6.5, 11.5, -0.3, 9.5, 13, 0.2), LIME, emissive=1.0, pulse=0.2, speed=3.0),
    ]


def ringer():
    bell = dict(pivot=(8, 15, 8), swing=30.0, spin_axis=(0, 0, 1), speed=3.2, motion='Powered')
    clap = dict(pivot=(8, 15, 8), swing=42.0, spin_axis=(0, 0, 1), speed=3.2, phase=-0.5, motion='Powered')
    return [
        E('Base', (0, 0, 0, 16, 2, 16), STONE2),
        E('Post L', (0, 2, 6, 3, 16, 10), WOOD),
        E('Post R', (13, 2, 6, 16, 16, 10), WOOD),
        E('Beam', (0, 14, 7, 16, 16, 9), WOOD2),
        E('Brace L', (3, 12, 7, 5, 14, 9), WOOD2),
        E('Brace R', (11, 12, 7, 13, 14, 9), WOOD2),
        E('Step', (4, 2, 4, 12, 3, 12), STONE),
    ], [
        P('Bell top', (6, 12, 6, 10, 14, 10), GOLD, metallic=0.9, **bell),
        P('Bell waist', (5, 9, 5, 11, 12, 11), BRASS, metallic=0.9, **bell),
        P('Bell skirt', (4, 7, 4, 12, 9, 12), GOLD, metallic=0.9, **bell),
        P('Bell lip', (3.5, 6, 3.5, 12.5, 7, 12.5), BRASS, metallic=0.9, **bell),
        P('Clapper', (7.4, 4.6, 7.4, 8.6, 9, 8.6), DARK, **clap),
        P('Ball', (7, 3.6, 7, 9, 5.2, 9), IRON, **clap),
    ]


def laser():
    head = dict(pivot=(8, 9, 8), swing=28.0, speed=0.9)
    return [
        E('Base', (1, 0, 1, 15, 3, 15), (150, 44, 44)),
        E('Stripe', (0.8, 1, 0.8, 15.2, 2, 15.2), YELLOW),
        E('Ring', (3, 3, 3, 13, 5, 13), DARK),
        E('Column', (6, 5, 6, 10, 7, 10), STEEL),
    ], [
        P('Head', (4, 7, 4, 12, 12, 13), (190, 54, 54), **head),
        P('Head cap', (5, 12, 5, 11, 13, 12), DARK, **head),
        P('Barrel', (6, 8, -1, 10, 11, 4), DARK, **head),
        P('Muzzle', (5.5, 7.5, -2, 10.5, 11.5, -1), STEEL, **head),
        P('Lens', (6.5, 8.5, -2.4, 9.5, 10.5, -2), RED, emissive=2.0, powered_emissive=7.0, powered_color=(255, 160, 150), pulse=0.1, speed=9.0, swing=28.0, pivot=(8, 9, 8)),
        *ring('Cooler', (8, 9.5, 1.5), 3, 0.7, CYAN, axis='z', emissive=1.0, spin=0.0, swing=28.0, spin_axis=(0, 1, 0), speed=0.9),
    ]


def etheraltar():
    return [
        E('Step 1', (0, 0, 0, 16, 2, 16), (120, 110, 150)),
        E('Step 2', (2, 2, 2, 14, 4, 14), (146, 134, 180)),
        E('Altar', (4, 4, 4, 12, 9, 12), (176, 160, 216)),
        E('Altar top', (3, 9, 3, 13, 10, 13), (214, 200, 244)),
        *corners(2, 14, (120, 110, 150), 0.5, 2, 'Obelisk'),
        *[E(f'Rune {i}', box, VIOLET) for i, box in enumerate([(7, 5, 3.8, 9, 8, 4), (7, 5, 12, 9, 8, 12.2), (3.8, 5, 7, 4, 8, 9), (12, 5, 7, 12.2, 8, 9)])],
    ], [
        P('Ethereal shard', (6, 11, 6, 10, 15, 10), (210, 170, 255), emissive=3.0, rotation=(45, 0, 35), spin=1.2, bob=0.1, speed=1.8, pivot=(8, 13, 8)),
        *ring('Halo', (8, 13, 8), 4.5, 0.4, WHITE, emissive=2.0, spin=-0.8, bob=0.1, speed=1.8),
        *[P(f'Flame {i + 1}', (x, 14, z, x + 1, 15.6, z + 1), VIOLET, emissive=3.0, pulse=0.3, speed=9.0, phase=i * 1.3, pivot=(x + 0.5, 14, z + 0.5)) for i, (x, z) in enumerate([(1, 1), (14, 1), (1, 14), (14, 14)])],
    ]


def kineticster():
    plate = dict(bob=0.1, speed=2.4)
    gear = dict(pivot=(8, 6, 0), spin=3.0, spin_axis=(0, 0, 1), metallic=0.7)
    return [
        E('Base', (0, 0, 0, 16, 3, 16), IRON),
        *corners(3, 13, DARK, 0, 2, 'Guide'),
        E('Back case', (2, 3, 8, 14, 11, 14), (120, 120, 136)),
        E('Front case', (2, 3, 1, 14, 5, 8), (96, 96, 110)),
        E('Cylinder 1', (4, 11, 4, 7, 12, 7), STEEL),
        E('Cylinder 2', (9, 11, 4, 12, 12, 7), STEEL),
        E('Cylinder 3', (4, 11, 9, 7, 12, 12), STEEL),
        E('Cylinder 4', (9, 11, 9, 12, 12, 12), STEEL),
    ], [
        P('Plate', (1, 14, 1, 15, 16, 15), STEEL, metallic=0.5, **plate),
        P('Tread', (3, 16, 3, 13, 16.4, 13), YELLOW, **plate),
        *[P(f'Rod {i + 1}', (x, 12, z, x + 1, 14.5, z + 1), LIGHT, metallic=0.8, **plate) for i, (x, z) in enumerate([(5, 5), (10, 5), (5, 10), (10, 10)])],
        P('Gear rim', (3.5, 5, -0.6, 12.5, 11, 0.4), COPPER, pivot=(8, 8, 0), spin=3.0, spin_axis=(0, 0, 1), metallic=0.7),
        P('Gear rim 2', (5, 3.5, -0.6, 11, 12.5, 0.4), COPPER, pivot=(8, 8, 0), spin=3.0, spin_axis=(0, 0, 1), metallic=0.7),
        P('Gear hub', (6.5, 6.5, -1, 9.5, 9.5, 0.6), BRASS, pivot=(8, 8, 0), spin=3.0, spin_axis=(0, 0, 1), metallic=0.7),
    ]


def reconstructor():
    head = dict(bob=0.3, bob_axis=(0, 1, 0), speed=1.6, pivot=(8, 8, 8), motion='Powered')
    return [
        E('Bed', (0, 0, 0, 16, 3, 16), LIGHT),
        E('Bed plate', (2, 3, 2, 14, 3.5, 14), DARK),
        E('Rail L', (0, 3, 6, 2, 16, 10), STEEL),
        E('Rail R', (14, 3, 6, 16, 16, 10), STEEL),
        E('Bridge', (0, 14.5, 6, 16, 16, 10), (160, 210, 240)),
        E('Reel', (5, 12.5, 6.5, 11, 14.5, 9.5), DARK),
    ], [
        P('Gantry', (2, 7.5, 7, 14, 8.5, 9), (180, 230, 255), emissive=0.4, **head),
        P('Scan plane', (3, 7.2, 3, 13, 7.5, 13), CYAN, emissive=3.0, powered_color=WHITE, powered_emissive=4.0, **head),
        P('Hologram', (5, 4, 5, 11, 10, 11), (120, 220, 255), emissive=1.6, spin=1.0, pivot=(8, 7, 8), rotation=(0, 0, 0), pulse=0.05, speed=3.0),
        P('Core', (7, 6, 7, 9, 8, 9), WHITE, emissive=3.0, spin=-2.0, pivot=(8, 7, 8)),
    ]


def excavator():
    drill = dict(pivot=(8, 8, 8), spin=9.0, motion='Powered', metallic=0.6)
    return [
        E('Tread L', (0, 0, 0, 3, 3, 16), DARK),
        E('Tread R', (13, 0, 0, 16, 3, 16), DARK),
        E('Deck', (1, 3, 0, 15, 5, 16), YELLOW),
        E('Deck stripe', (0.8, 3.5, 0, 15.2, 4.5, 3), BLACK),
        E('Cab', (9, 5, 9, 15, 11, 15), (230, 200, 40)),
        E('Cab window', (9.5, 7, 8.8, 14.5, 10, 9), GLASS),
        *corners(5, 16, IRON, 3, 1, 'Derrick'),
        E('Derrick top', (3, 14.5, 3, 13, 16, 13), IRON),
    ], [
        P('Screw', (6.5, 3, 6.5, 9.5, 14.5, 9.5), STEEL, **drill),
        P('Flight 1', (4.5, 12, 7.2, 11.5, 13, 8.8), LIGHT, **drill),
        P('Flight 2', (7.2, 10, 4.5, 8.8, 11, 11.5), LIGHT, **drill),
        P('Flight 3', (5, 8, 7.2, 11, 9, 8.8), LIGHT, **drill),
        P('Flight 4', (7.2, 6, 5, 8.8, 7, 11), LIGHT, **drill),
        P('Bit', (7, -1, 7, 9, 3, 9), BRASS, **drill),
        P('Beacon', (11, 11, 11, 13, 12.5, 13), EMBER, emissive=1.0, powered_emissive=5.0, powered_color=EMBER, pulse=0.3, speed=8.0),
    ]


def ethereaper():
    blade = dict(pivot=(8, 19, 8), spin=2.0, motion='Powered')
    bars = [E(f'Bar {i}', (1.5 + i * 3, 15, 1, 2.5 + i * 3, 16, 15), (60, 50, 80)) for i in range(5)]
    return [
        E('Plinth', (0, 0, 0, 16, 3, 16), (24, 22, 30)),
        E('Vat', (1, 3, 1, 15, 14, 15), (34, 30, 44)),
        E('Rim front', (0, 14, 0, 16, 16, 1), (52, 46, 66)),
        E('Rim back', (0, 14, 15, 16, 16, 16), (52, 46, 66)),
        E('Rim left', (0, 14, 1, 1, 16, 15), (52, 46, 66)),
        E('Rim right', (15, 14, 1, 16, 16, 15), (52, 46, 66)),
        E('Skull plate', (5, 6, 0.6, 11, 11, 1), BONE),
        E('Eye L', (6, 8.5, 0.5, 7.5, 10, 0.7), BLACK),
        E('Eye R', (8.5, 8.5, 0.5, 10, 10, 0.7), BLACK),
        E('Jaw', (6.5, 6, 0.4, 9.5, 7, 0.7), BLACK),
        E('Mast', (14, 16 - 2, 7, 15, 16, 9), DARK),
        *bars,
    ], [
        P('Pool', (2, 13.4, 2, 14, 14.4, 14), VIOLET, emissive=2.0, powered_emissive=4.0, powered_color=(200, 150, 255), pulse=0.03, speed=2.0),
        P('Snath', (7.5, 15.5, 7.5, 8.5, 22, 8.5), (70, 54, 40), **blade),
        P('Scythe', (0, 21, 7.6, 8, 22.2, 8.4), LIGHT, metallic=0.9, **blade),
        P('Scythe tip', (-2, 19.5, 7.6, 1, 21.2, 8.4), WHITE, metallic=0.9, **blade),
        P('Wisp', (7, 17, 7, 9, 19, 9), (210, 170, 255), emissive=3.0, bob=0.14, speed=3.0, motion='Powered'),
    ]


def omnireactor():
    return [
        E('Base', (0, 0, 0, 16, 3, 16), (24, 24, 30)),
        E('Cap', (0, 13, 0, 16, 16, 16), (24, 24, 30)),
        E('Cap inlay', (3, 15.9, 3, 13, 16, 13), GOLD),
        *corners(3, 13, (46, 46, 58), 0, 2, 'Fin block'),
        E('Fin 1', (3, 11, 0, 13, 12, 1), (70, 70, 84)),
        E('Fin 2', (3, 4, 0, 13, 5, 1), (70, 70, 84)),
        E('Fin 3', (3, 11, 15, 13, 12, 16), (70, 70, 84)),
        E('Fin 4', (3, 4, 15, 13, 5, 16), (70, 70, 84)),
        E('Bus L', (0.5, 6, 6, 2, 10, 10), COPPER),
        E('Bus R', (14, 6, 6, 15.5, 10, 10), COPPER),
    ], [
        P('Core', (5.5, 5.5, 5.5, 10.5, 10.5, 10.5), (250, 250, 255), emissive=4.0, powered_emissive=9.0, powered_color=WHITE, pulse=0.16, speed=5.0, rotation=(45, 45, 0)),
        *ring('Torus', (8, 8, 8), 5, 0.8, GOLD, emissive=1.5, spin=4.0, motion='Powered'),
        *ring('Torus cross', (8, 8, 8), 5, 0.8, CYAN, axis='x', emissive=1.5, spin=-3.0, spin_axis=(1, 0, 0), motion='Powered'),
    ]


def invocateral():
    runes = [P(f'Rune {i + 1}', (x, 16, z, x + 2, 16.3, z + 2), VIOLET, emissive=2.0, pulse=0.2, speed=3.0, phase=i * 1.5) for i, (x, z) in enumerate([(7, 1.5), (7, 12.5), (1.5, 7), (12.5, 7)])]
    return [
        E('Pad', (0, 0, 0, 16, 3, 16), (70, 50, 110)),
        E('Disc', (1, 3, 1, 15, 4, 15), (120, 80, 180)),
        E('Circle', (2, 4, 2, 14, 4.3, 14), (60, 40, 100)),
        E('Inner', (4, 4.3, 4, 12, 4.5, 12), (170, 110, 240)),
        *corners(4, 13, (70, 50, 110), 0, 2, 'Horn'),
        *corners(13, 15, GOLD, 0.3, 1.4, 'Horn tip'),
    ], [
        P('Pyramid 1', (4, 8, 4, 12, 9, 12), VIOLET, emissive=1.5, spin=1.0, bob=0.1, speed=1.6, pivot=(8, 11, 8)),
        P('Pyramid 2', (5, 9, 5, 11, 10, 11), (190, 130, 255), emissive=1.8, spin=1.0, bob=0.1, speed=1.6, pivot=(8, 11, 8)),
        P('Pyramid 3', (6, 10, 6, 10, 11, 10), (210, 160, 255), emissive=2.0, spin=1.0, bob=0.1, speed=1.6, pivot=(8, 11, 8)),
        P('Pyramid 4', (7, 11, 7, 9, 12, 9), WHITE, emissive=3.0, spin=1.0, bob=0.1, speed=1.6, pivot=(8, 11, 8)),
        P('Eye', (7, 13.5, 7, 9, 15.5, 9), MAGENTA, emissive=3.0, pulse=0.2, speed=4.0, bob=0.1),
        *ring('Portal', (8, 8, 8), 7, 0.5, VIOLET, emissive=2.0, spin=-1.5, pulse=0.08, speed=2.0, motion='Powered'),
    ]


def televerseur():
    return [
        E('Plinth', (0, 0, 0, 16, 2, 16), WHITE),
        E('Pad', (1, 2, 1, 15, 3, 15), LIGHT),
        E('Emitter', (4, 3, 4, 12, 4, 12), CYAN),
        *corners(3, 14, WHITE, 0, 1.5, 'Pylon'),
        E('Back panel', (2, 3, 14, 14, 11, 15.5), LIGHT),
        E('Slot', (4, 6, 13.7, 12, 7.5, 14), DARK),
    ], [
        *ring('Rise 1', (8, 5, 8), 5.5, 0.6, CYAN, emissive=2.0, bob=0.34, speed=2.0, motion='Powered'),
        *ring('Rise 2', (8, 9, 8), 4.5, 0.6, BLUE, emissive=2.0, bob=0.34, speed=2.0, phase=2.1, motion='Powered'),
        P('Data', (6, 7, 6, 10, 11, 10), (130, 230, 255), emissive=2.0, spin=2.0, rotation=(45, 0, 35), pivot=(8, 9, 8)),
        P('Arrow shaft', (7.4, 14, 7.4, 8.6, 17, 8.6), WHITE, emissive=2.0, bob=0.08, speed=4.0),
        P('Arrow head', (6, 17, 6, 10, 18, 10), WHITE, emissive=2.0, bob=0.08, speed=4.0),
    ]


def mnemo_digestor():
    jaw = lambda: dict(pivot=(0, 15, 8), swing=16.0, spin_axis=(0, 0, 1), speed=3.0)
    jaw2 = lambda: dict(pivot=(16, 15, 8), swing=16.0, spin_axis=(0, 0, 1), speed=3.0, phase=3.14)
    teeth = lambda x0, side, kw: [P(f'Tooth {side}{i}', (x0, 14.5, 2 + i * 3, x0 + 2, 17.5, 3.6 + i * 3), BONE, **kw) for i in range(4)]
    return [
        E('Mass', (0, 0, 0, 16, 12, 16), FLESH2),
        E('Folds', (1, 12, 1, 15, 14, 15), FLESH),
        E('Gullet', (5, 14, 2, 11, 14.5, 14), BLACK),
        E('Vein 1', (2, 1, -0.1, 3, 11, 0.2), (220, 70, 80)),
        E('Vein 2', (7, 1, -0.1, 8, 9, 0.2), (220, 70, 80)),
        E('Vein 3', (12, 3, -0.1, 13, 11, 0.2), (220, 70, 80)),
        E('Sore', (9, 3, -0.2, 14, 6, 0.2), MNEMO),
    ], [
        P('Lip L', (0, 14, 1, 5, 16, 15), FLESH, **jaw()),
        P('Lip R', (11, 14, 1, 16, 16, 15), FLESH, **jaw2()),
        *teeth(3, 'L', jaw()),
        *teeth(11, 'R', jaw2()),
        P('Throat', (6, 13, 6, 10, 15, 10), RED, emissive=1.2, pulse=0.2, speed=3.0, powered_color=(255, 90, 80), powered_emissive=3.0),
    ]


def mnemo_heart():
    """A living heart: two chambers that beat, vessels that pulse."""
    beat = lambda phase=0.0: dict(pulse=0.16, speed=5.0, phase=phase, emissive=1.4)
    return [
        E('Bed', (1, 0, 1, 15, 2, 15), (84, 20, 30)),
        E('Root L', (0, 0, 7, 3, 1, 9), (120, 26, 38)),
        E('Root R', (13, 0, 6, 16, 1, 8), (120, 26, 38)),
    ], [
        P('Chamber L', (3, 2, 4, 9, 11, 12), (176, 34, 48), **beat()),
        P('Chamber R', (8, 2, 3, 13, 9, 11), (150, 28, 42), **beat(1.2)),
        P('Crown', (5, 10, 6, 11, 13, 10), (206, 44, 60), **beat(0.6)),
        P('Vessel L', (4, 12, 7, 6, 16, 9), MNEMO, pulse=0.1, speed=5.0, swing=6.0, spin_axis=(0, 0, 1), pivot=(5, 12, 8), emissive=2.0),
        P('Vessel R', (10, 12, 6, 12, 15, 8), MNEMO, pulse=0.1, speed=5.0, phase=1.0, swing=6.0, spin_axis=(1, 0, 0), pivot=(11, 12, 7), emissive=2.0),
    ]


def mnemo_cloner():
    return [
        E('Base', (1, 0, 1, 15, 3, 15), FLESH2),
        E('Roots', (0, 0, 6, 16, 1.5, 10), (70, 20, 30)),
        E('Tube rim', (2, 3, 2, 14, 4, 14), BONE),
        *corners(4, 13, BONE, 2, 1, 'Rib'),
        E('Glass', (3, 4, 3, 13, 13, 13), (190, 80, 90)),
        E('Dome', (2, 13, 2, 14, 14, 14), BONE),
        E('Crown', (4, 14, 4, 12, 16, 12), FLESH),
    ], [
        P('Fluid', (3.2, 4.2, 3.2, 12.8, 12.6, 12.8), (255, 40, 40), emissive=0.8, powered_emissive=1.8, powered_color=(255, 70, 70)),
        P('Embryo body', (6, 5, 6, 10, 9, 10), (250, 190, 170), emissive=0.4, pulse=0.1, speed=2.0, bob=0.08, pivot=(8, 7, 8)),
        P('Embryo head', (6.5, 9, 6.5, 9.5, 12, 9.5), (255, 205, 185), emissive=0.4, bob=0.08, speed=2.0),
        P('Cord', (7.6, 12, 7.6, 8.4, 13, 8.4), MNEMO, emissive=2.0),
        P('Heart', (5, 14.6, 5, 11, 16.4, 11), RED, emissive=2.0, pulse=0.14, speed=6.0),
    ]


def astral_modifier():
    return [
        E('Base', (1, 0, 1, 15, 2, 15), NAVY),
        E('Step', (3, 2, 3, 13, 3, 13), (40, 60, 150)),
        E('Stem', (7, 3, 7, 9, 5, 9), GOLD),
        E('Arc front', (2, 3, 7.5, 3, 15, 8.5), GOLD),
        E('Arc back', (13, 3, 7.5, 14, 15, 8.5), GOLD),
        E('Arc top', (2, 15, 7.5, 14, 16, 8.5), GOLD),
    ], [
        P('Star', (6.5, 7.5, 6.5, 9.5, 10.5, 9.5), (120, 170, 255), emissive=4.0, rotation=(45, 0, 45), spin=2.0, pulse=0.14, speed=5.0, pivot=(8, 9, 8)),
        *ring('Gimbal A', (8, 9, 8), 5.2, 0.6, GOLD, spin=1.0, emissive=0.4),
        *ring('Gimbal B', (8, 9, 8), 4.2, 0.6, (120, 170, 255), axis='x', spin=-1.6, spin_axis=(1, 0, 0), emissive=1.2),
        *ring('Gimbal C', (8, 9, 8), 3.2, 0.6, WHITE, axis='z', spin=2.4, spin_axis=(0, 0, 1), emissive=1.2),
    ]


def astral_container():
    lid = dict(pivot=(8, 11, 15), swing=9.0, spin_axis=(1, 0, 0), speed=1.2)
    stars = [(3, 3), (11, 6), (6, 8), (13, 2), (2, 7)]
    return [
        E('Hull', (1, 0, 1, 15, 10, 15), NAVY),
        E('Foot', (0, 0, 0, 16, 1.5, 16), (36, 20, 120)),
        E('Rim', (0, 9, 0, 16, 11, 16), (54, 40, 200)),
        *corners(0, 11, GOLD, 0, 1, 'Edge'),
        E('Lock', (6, 5, 0.4, 10, 9, 1), GOLD),
        *[E(f'Star {i + 1}', (x, y, 0.8, x + 1, y + 1, 1), WHITE) for i, (x, y) in enumerate(stars)],
    ], [
        P('Lid', (0, 11, 0, 16, 14, 16), (40, 26, 150), **lid),
        P('Lid top', (2, 14, 2, 14, 15, 14), NAVY, **lid),
        P('Lid band', (7, 11, -0.2, 9, 15.2, 16.2), GOLD, **lid),
        P('Gap glow', (1, 10.8, 1, 15, 11.2, 15), (140, 120, 255), emissive=3.0, pulse=0.1, speed=2.4),
        P('Stardust', (6.5, 16, 6.5, 9.5, 19, 9.5), (150, 140, 255), emissive=3.0, rotation=(45, 0, 45), spin=1.4, bob=0.1, speed=1.8, pivot=(8, 17.5, 8)),
    ]


def frame():
    return [
        *corners(0, 16, IRON, 0, 2, 'Beam'),
        E('Low front', (2, 0, 0, 14, 2, 2), STEEL),
        E('Low back', (2, 0, 14, 14, 2, 16), STEEL),
        E('Low left', (0, 0, 2, 2, 2, 14), STEEL),
        E('Low right', (14, 0, 2, 16, 2, 14), STEEL),
        E('High front', (2, 14, 0, 14, 16, 2), STEEL),
        E('High back', (2, 14, 14, 14, 16, 16), STEEL),
        E('High left', (0, 14, 2, 2, 16, 14), STEEL),
        E('High right', (14, 14, 2, 16, 16, 14), STEEL),
        E('Deck', (2, 0.5, 2, 14, 1.5, 14), DARK),
        E('Mesh', (2, 14.5, 2, 14, 15.5, 14), (60, 66, 78)),
        E('Core', (6, 2, 6, 10, 14, 10), (66, 72, 84)),
        E('Brace', (2, 7, 7, 14, 9, 9), STEEL),
        E('Brace 2', (7, 7, 2, 9, 9, 14), STEEL),
    ], []


def motor():
    hub = dict(pivot=(8, 8, 8), spin=1.0, motion='Motor', metallic=0.8)
    fins = [E(f'Fin {i + 1}', (1.5, 3 + i * 2, 1.5, 14.5, 4 + i * 2, 14.5), (230, 134, 72)) for i in range(4)]
    return [
        E('Mount', (0, 0, 0, 16, 2, 16), DARK),
        *[E(f'Bolt {i + 1}', (x, 2, z, x + 2, 2.6, z + 2), STEEL) for i, (x, z) in enumerate([(0.5, 0.5), (13.5, 0.5), (0.5, 13.5), (13.5, 13.5)])],
        E('Stator', (3, 2, 3, 13, 12, 13), (150, 84, 44)),
        *fins,
        E('Flange', (2, 12, 2, 14, 14, 14), IRON),
        E('Terminal box', (5, 4, 0, 11, 9, 2), DARK),
        E('Plate', (6, 5, -0.2, 10, 8, 0), BRASS),
    ], [
        P('Rotor', (3, 14, 3, 13, 16, 13), LIGHT, **hub),
        P('Rotor bar', (1, 14.4, 7, 15, 15.6, 9), BRASS, **hub),
        P('Rotor bar 2', (7, 14.4, 1, 9, 15.6, 15), BRASS, **hub),
        P('Shaft', (6.5, 15.8, 6.5, 9.5, 16.4, 9.5), DARK, **hub),
    ]


def controller():
    stick = dict(pivot=(5, 7, 6), swing=20.0, spin_axis=(1, 0, 0.5), speed=2.0)
    return [
        E('Base', (1, 0, 1, 15, 5, 15), (50, 130, 190)),
        E('Trim', (0.6, 1, 0.6, 15.4, 2, 15.4), DARK),
        E('Panel', (2, 5, 2, 14, 7, 14), DARK),
        E('Screen', (8, 7, 3, 13, 7.4, 8), (20, 60, 40)),
        E('Socket', (3.5, 7, 4.5, 6.5, 7.6, 7.5), BLACK),
        E('Mast', (11, 7, 11, 13, 11, 13), STEEL),
    ], [
        P('Stick', (4.5, 7.5, 5.5, 5.5, 12, 6.5), LIGHT, metallic=0.8, **stick),
        P('Stick ball', (3.8, 12, 4.8, 6.2, 14.4, 7.2), RED, **stick),
        P('Blip', (9, 7.3, 4, 11, 7.6, 6), LIME, emissive=2.0, pulse=0.5, speed=4.0),
        P('Radar', (8, 11.5, 11.4, 16, 12.5, 12.6), CYAN, emissive=1.0, spin=3.0, pivot=(12, 12, 12)),
        P('Radar tip', (7, 11, 11, 9, 13, 13), LIGHT, spin=3.0, pivot=(12, 12, 12)),
        P('Beacon', (11.4, 13, 11.4, 12.6, 15, 12.6), RED, emissive=2.0, pulse=0.3, speed=6.0),
    ]


def wagon_seat():
    wheel = lambda x, z: [
        E(f'Tyre {x}-{z}', (x + 0.6, 0, z, x + 1.4, 6, z + 6), DARK),
        E(f'Rim {x}-{z}', (x + 0.3, 1, z + 1, x + 1.7, 5, z + 5), WOOD2),
        E(f'Hub {x}-{z}', (x, 2.5, z + 2.5, x + 2, 3.5, z + 3.5), IRON),
    ]
    sway = dict(pivot=(8, 15.5, 0.5), swing=14.0, spin_axis=(0, 0, 1), speed=2.6)
    return [
        *wheel(0, 1), *wheel(14, 1), *wheel(0, 9), *wheel(14, 9),
        E('Axle front', (1, 2.5, 3.5, 15, 3.5, 4.5), IRON),
        E('Axle back', (1, 2.5, 11.5, 15, 3.5, 12.5), IRON),
        E('Bed', (2, 3, 0, 14, 5, 16), (150, 62, 60)),
        E('Side L', (2, 5, 0, 3, 8, 16), WOOD),
        E('Side R', (13, 5, 0, 14, 8, 16), WOOD),
        E('Bench', (3, 5, 4, 13, 8, 12), (180, 70, 70)),
        E('Cushion', (3.5, 8, 4.5, 12.5, 9, 11.5), (200, 92, 86)),
        E('Backrest', (3, 8, 12, 13, 15, 14), (180, 70, 70)),
        E('Back frame', (2, 5, 14, 14, 16, 16), WOOD),
        E('Lantern hook', (7.5, 8, 0, 8.5, 15.5, 1), IRON),
    ], [
        P('Lantern', (7, 11, -0.5, 9, 14, 1.5), FLAME, emissive=3.0, **sway),
        P('Lantern cap', (6.6, 14, -0.9, 9.4, 14.8, 1.9), DARK, **sway),
    ]


DESIGNS = {
    'combustor': combustor, 'solar': solar, 'synthesizer': synthesizer, 'infuser': infuser, 'vacuum': vacuum, 'harvester': harvester,
    'energizer': energizer, 'container': container, 'lamp': lamp, 'assembler': assembler, 'omnichorus': omnichorus, 'repulsor': repulsor,
    'terminal': terminal, 'comandor': comandor, 'transmitter': transmitter, 'visualiser': visualiser, 'emitter': emitter, 'receiver': receiver,
    'screen': screen, 'atomiser': atomiser, 'ressourcer': ressourcer, 'ringer': ringer, 'laser': laser, 'etheraltar': etheraltar,
    'kineticster': kineticster, 'reconstructor': reconstructor, 'excavator': excavator, 'ethereaper': ethereaper, 'omnireactor': omnireactor,
    'invocateral': invocateral, 'televerseur': televerseur, 'mnemo_digestor': mnemo_digestor, 'mnemo_cloner': mnemo_cloner, 'mnemo_heart': mnemo_heart,
    'astral_modifier': astral_modifier, 'astral_container': astral_container, 'frame': frame, 'motor': motor, 'controller': controller,
    'wagon_seat': wagon_seat,
}

DIRT = (124, 92, 66)
SIDES = ('Right', 'Left', 'Front', 'Back')


def sides(color, bottom=None):
    faces = [(label, color) for label in SIDES]
    faces.append(('Bottom', bottom or color))
    return faces


def fleck(color, amount, cells, glow=0.0):
    return dict(color=color, amount=amount, cells=cells, glow=glow)


# Terrain stays made of full blocks (it must hide its neighbours and merge into
# big quads): faces and flecks only, no model and no part.
TERRAIN = {
    'grass': dict(face_colors=sides(DIRT), accent=fleck((70, 226, 70), 0.3, 9)),
    'grass_dark': dict(face_colors=sides((98, 74, 54)), accent=fleck((22, 128, 40), 0.35, 9)),
    'dirt': dict(accent=fleck((92, 66, 46), 0.3, 7)),
    'stone': dict(accent=fleck((126, 134, 148), 0.22, 5)),
    'sand': dict(accent=fleck((236, 214, 156), 0.25, 14)),
    'red_sand': dict(accent=fleck((226, 146, 96), 0.25, 14)),
    'snow': dict(face_colors=[('Bottom', (190, 200, 214))], accent=fleck((255, 255, 255), 0.2, 16, 0.3)),
    'ice': dict(face_colors=[('Top', (196, 232, 250))], accent=fleck((226, 246, 255), 0.14, 3)),
    'mud': dict(face_colors=[('Top', (74, 54, 40))], accent=fleck((58, 42, 30), 0.35, 5)),
    'basalt': dict(accent=fleck((70, 66, 78), 0.3, 3)),
    'deepslate': dict(accent=fleck((44, 46, 56), 0.4, 6)),
    'moss': dict(face_colors=sides((84, 92, 70)), accent=fleck((116, 196, 90), 0.3, 8)),
    'mineral_ore': dict(color=(88, 94, 106), emissive=0.0, accent=fleck((206, 214, 226), 0.26, 4, 0.1)),
    'crystal_ore': dict(color=(62, 70, 84), emissive=0.0, accent=fleck((110, 236, 246), 0.28, 3.5, 2.2)),
    'astral_ore': dict(color=(50, 46, 70), emissive=0.0, accent=fleck((255, 236, 150), 0.26, 3.5, 3.0)),
    'mnemonical_ore': dict(color=(74, 24, 34), emissive=0.0, wind=0.2, accent=fleck((255, 70, 90), 0.3, 3.5, 3.0)),
    'lava': dict(color=(52, 30, 28), emissive=0.0, accent=fleck((255, 120, 36), 0.5, 3, 3.2)),
    'molten_lava': dict(accent=fleck((255, 214, 90), 0.2, 3, 3.0)),
    'leaves': dict(accent=fleck((34, 150, 52), 0.35, 6)),
    'pine_leaves': dict(accent=fleck((16, 104, 50), 0.4, 8)),
    'palm_leaves': dict(accent=fleck((150, 226, 90), 0.3, 5)),
    'log': dict(face_colors=[('Top', (226, 176, 112)), ('Bottom', (226, 176, 112))], color=(138, 84, 44), accent=fleck((104, 62, 34), 0.3, 5)),
    'burnt_log': dict(face_colors=[('Top', (60, 44, 38)), ('Bottom', (60, 44, 38))], accent=fleck((255, 100, 30), 0.05, 6, 2.0)),
    'burnt_ground': dict(accent=fleck((255, 90, 30), 0.04, 6, 2.0)),
    'sandstone': dict(face_colors=[('Top', (236, 214, 158)), ('Bottom', (190, 162, 108))], accent=fleck((204, 176, 118), 0.3, 6)),
    'temple_brick': dict(face_colors=[('Top', (172, 160, 120))], accent=fleck((120, 112, 82), 0.45, 2.5)),
    'slab_stone': dict(face_colors=[('Top', (128, 138, 154))], accent=fleck((84, 92, 106), 0.2, 4)),
    'alien_crust': dict(emissive=0.0, face_colors=[('Top', (150, 80, 214))], accent=fleck((230, 90, 255), 0.18, 6, 2.5)),
    'mnemo_rot': dict(color=(112, 26, 38), emissive=0.0, wind=0.45, accent=fleck((255, 64, 84), 0.34, 7, 2.2)),
    'glow_moss': dict(color=(70, 130, 70), emissive=0.0, accent=fleck((180, 255, 130), 0.4, 6, 2.6)),
}


def num(value):
    text = f'{float(value):.3f}'.rstrip('0')
    return text + '0' if text.endswith('.') else text


def triple(values, cast=num):
    return '(' + ', '.join(cast(v) for v in values) + ')'


def rgb(color):
    return triple(color, lambda v: str(int(v)))


def element_ron(element):
    box = element['box']
    return f'(name: "{element["name"]}", from: {triple(box[:3])}, to: {triple(box[3:])}, color: Some({rgb(element["color"])}))'


def part_ron(part):
    box = part['box']
    kw = part
    fields = [f'name: "{kw["name"]}"', f'from: {triple(box[:3])}', f'to: {triple(box[3:])}']
    fields.append(f'rotation: {triple(kw.get("rotation", (0, 0, 0)))}')
    fields.append(f'color: {rgb(kw["color"])}')
    fields.append(f'emissive: {num(kw.get("emissive", 0))}')
    fields.append(f'metallic: {num(kw.get("metallic", 0))}')
    if 'powered_color' in kw:
        fields.append(f'powered_color: Some({rgb(kw["powered_color"])})')
        fields.append(f'powered_emissive: {num(kw.get("powered_emissive", 0))}')
    fields.append(f'pivot: {triple(kw.get("pivot", center(box)))}')
    fields.append(f'spin: {num(kw.get("spin", 0))}')
    fields.append(f'spin_axis: {triple(kw.get("spin_axis", (0, 1, 0)))}')
    fields.append(f'bob: {num(kw.get("bob", 0))}')
    fields.append(f'bob_axis: {triple(kw.get("bob_axis", (0, 1, 0)))}')
    fields.append(f'swing: {num(kw.get("swing", 0))}')
    fields.append(f'pulse: {num(kw.get("pulse", 0))}')
    fields.append(f'speed: {num(kw.get("speed", 2.2))}')
    fields.append(f'phase: {num(kw.get("phase", 0))}')
    fields.append(f'motion: {kw.get("motion", "Always")}')
    return '(' + ', '.join(fields) + ')'


def top_level(text):
    """(key, raw value) of the top-level fields of a `.ron` block file."""
    start = text.index('(') + 1
    entries, depth, in_string, begin, i = [], 0, False, start, start
    while i < len(text):
        ch = text[i]
        if in_string:
            if ch == '\\':
                i += 1
            elif ch == '"':
                in_string = False
        elif ch == '"':
            in_string = True
        elif ch in '([{':
            depth += 1
        elif ch in ')]}':
            if depth == 0:
                entries.append(text[begin:i])
                break
            depth -= 1
        elif ch == ',' and depth == 0:
            entries.append(text[begin:i])
            begin = i + 1
        i += 1
    fields = []
    for entry in entries:
        entry = entry.strip()
        if entry:
            key, value = entry.split(':', 1)
            fields.append([key.strip(), value.strip()])
    return fields


def update_file(block_id, changes):
    path = os.path.join(BLOCKS, block_id + '.ron')
    fields = top_level(open(path, encoding='utf-8').read())
    drop = [key for key, value in changes.items() if value is None]
    fields = [f for f in fields if f[0] not in drop]
    for key, value in changes.items():
        if value is None:
            continue
        for field in fields:
            if field[0] == key:
                field[1] = value
                break
        else:
            fields.append([key, value])
    with open(path, 'w', encoding='utf-8', newline='\n') as out:
        out.write('(\n' + ''.join(f'    {key}: {value},\n' for key, value in fields) + ')\n')


def design_machines():
    for block_id, design in DESIGNS.items():
        elements, parts = design()
        # Elements stay in the cell: a detail that sticks out is a visual part.
        outside = [e for e in elements if not all(0 <= v <= 16 for v in e['box'])]
        elements = [e for e in elements if e not in outside]
        parts = parts + [P(e['name'], e['box'], e['color']) for e in outside]
        model = 'Some((\n        elements: [\n' + ''.join(f'            {element_ron(e)},\n' for e in elements) + '        ],\n    ))'
        changes = {'model': model, 'emissive': '0.0', 'parts': None, 'face_colors': None, 'accent': None}
        if parts:
            changes['parts'] = 'Some([\n' + ''.join(f'        {part_ron(p)},\n' for p in parts) + '    ])'
        update_file(block_id, changes)


def design_terrain():
    for block_id, look in TERRAIN.items():
        changes = {'face_colors': None, 'accent': None}
        if 'color' in look:
            changes['color'] = rgb(look['color'])
        if 'emissive' in look:
            changes['emissive'] = num(look['emissive'])
        if 'wind' in look:
            changes['wind'] = num(look['wind'])
        if 'face_colors' in look:
            changes['face_colors'] = '[' + ', '.join(f'("{label}", {rgb(color)})' for label, color in look['face_colors']) + ']'
        if 'accent' in look:
            a = look['accent']
            changes['accent'] = f'Some((color: {rgb(a["color"])}, amount: {num(a["amount"])}, cells: {num(a["cells"])}, glow: {num(a["glow"])}))'
        update_file(block_id, changes)


if __name__ == '__main__':
    design_machines()
    design_terrain()
    print(f'{len(DESIGNS)} machines, {len(TERRAIN)} terrain blocks written.')
