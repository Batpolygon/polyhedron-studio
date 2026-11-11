"""Structures of the world: trees, ruins, camps, and what lives in the caves.

Writes `project/structures/*.ron`, the new blocks they need, the structure
rules of every biome and the cave rules of `project/biomes/_world.ron`, the
same files the structure and biome studios edit. A structure fits in 16 x 16
blocks, the largest footprint the generator takes.

Run from the root:

    python tools/design_structures.py
"""
import math
import os
import random
import re

STRUCTURES = 'project/structures'
BIOMES = 'project/biomes'
BLOCKS = 'project/blocks'

NEW_BLOCKS = {
    'plank': ('Plank', (158, 112, 66), 'Wood', 'Cells30', 1.2, 'Some("plank")', (120, 82, 46), 0.3, 5.0, 0.0),
    'thatch': ('Thatch', (196, 164, 84), 'Grass', 'Cells200', 0.5, 'None', (150, 118, 54), 0.4, 9.0, 0.0),
    'cactus': ('Cactus', (58, 146, 74), 'Grass', 'Cells30', 0.5, 'None', (190, 220, 130), 0.1, 6.0, 0.0),
    'bone': ('Bone', (226, 214, 186), 'Stone', 'Cells30', 1.0, 'None', (176, 162, 132), 0.2, 4.0, 0.0),
    'mushroom_stem': ('Mushroom Stem', (214, 206, 190), 'Wood', 'Cells30', 0.8, 'None', (170, 156, 140), 0.25, 6.0, 0.0),
    'mushroom_cap': ('Mushroom Cap', (84, 60, 170), 'Grass', 'Cells30', 0.6, 'None', (140, 240, 255), 0.22, 3.0, 2.6),
    'mushroom_gill': ('Mushroom Gill', (120, 220, 240), 'Grass', 'Cells200', 0.3, 'None', (220, 255, 255), 0.3, 6.0, 2.4),
    'cave_crystal': ('Cave Crystal', (70, 190, 220), 'Glass', 'Cells3', 2.0, 'Some("crystal_ore")', (190, 250, 255), 0.4, 2.5, 3.2),
    'mnemo_heart': ('Mnemo Heart', (120, 26, 40), 'Dirt', 'Cells30', 2.0, 'Some("mnemonical")', (255, 70, 90), 0.3, 3.0, 2.0),
}


class S:
    """A structure being built: one block per cell, the last one wins."""

    def __init__(self, name, size, anchors=None):
        self.name, self.size, self.cells, self.anchors = name, size, {}, anchors or []

    def put(self, x, y, z, block):
        x, y, z = round(x), round(y), round(z)
        if 0 <= x < self.size[0] and 0 <= y < self.size[1] and 0 <= z < self.size[2]:
            self.cells[(x, y, z)] = block

    def clear(self, x, y, z):
        self.cells.pop((round(x), round(y), round(z)), None)

    def box(self, x0, y0, z0, x1, y1, z1, block):
        for x in range(x0, x1 + 1):
            for y in range(y0, y1 + 1):
                for z in range(z0, z1 + 1):
                    self.put(x, y, z, block)

    def cut(self, x0, y0, z0, x1, y1, z1):
        for x in range(x0, x1 + 1):
            for y in range(y0, y1 + 1):
                for z in range(z0, z1 + 1):
                    self.clear(x, y, z)

    def shell(self, x0, y0, z0, x1, y1, z1, block):
        self.box(x0, y0, z0, x1, y1, z1, block)
        self.cut(x0 + 1, y0, z0 + 1, x1 - 1, y1, z1 - 1)

    def blob(self, cx, cy, cz, rx, ry, rz, block, rng=None, holes=0.0, keep=True):
        """An ellipsoid with a rough edge."""
        for x in range(math.floor(cx - rx), math.ceil(cx + rx) + 1):
            for y in range(math.floor(cy - ry), math.ceil(cy + ry) + 1):
                for z in range(math.floor(cz - rz), math.ceil(cz + rz) + 1):
                    d = ((x - cx) / max(rx, .1)) ** 2 + ((y - cy) / max(ry, .1)) ** 2 + ((z - cz) / max(rz, .1)) ** 2
                    edge = 1.0 + (rng.uniform(-.28, .1) if rng else 0)
                    if d <= edge and not (rng and d > .55 and rng.random() < holes):
                        if keep and (x, y, z) in self.cells:
                            continue
                        self.put(x, y, z, block)

    def line(self, a, b, block, radius=0.0):
        steps = max(1, round(max(abs(a[i] - b[i]) for i in range(3)) * 2))
        for i in range(steps + 1):
            t = i / steps
            p = [a[k] + (b[k] - a[k]) * t for k in range(3)]
            if radius:
                r = math.ceil(radius)
                for dx in range(-r, r + 1):
                    for dy in range(-r, r + 1):
                        for dz in range(-r, r + 1):
                            if dx * dx + dy * dy + dz * dz <= radius * radius:
                                self.put(p[0] + dx, p[1] + dy, p[2] + dz, block)
            else:
                self.put(*p, block)

    def ron(self):
        cells = ''.join(f'        (pos: ({x}, {y}, {z}), block: "{block}"),\n' for (x, y, z), block in sorted(self.cells.items(), key=lambda c: (c[0][1], c[0][2], c[0][0])))
        anchors = ''.join(f'        ({x}, {y}, {z}),\n' for x, y, z in self.anchors)
        return f'(\n    name: "{self.name}",\n    size: ({self.size[0]}, {self.size[1]}, {self.size[2]}),\n    cells: [\n{cells}    ],\n    anchors: [\n{anchors}    ],\n)\n'


def grounded(s, *cells):
    s.anchors = list(cells)
    return s


# --- Trees -------------------------------------------------------------------

def oak(name, height, spread, seed, leaves='leaves', trunk='log', branches=4):
    rng = random.Random(seed)
    size = spread * 2 + 1
    s = S(name, (size, height + spread + 2, size))
    c = spread
    for y in range(height):
        s.put(c, y, c, trunk)
    for x, z in ((1, 0), (-1, 0), (0, 1), (0, -1)):
        if rng.random() < .7:
            s.put(c + x, 0, c + z, trunk)
    for i in range(branches):
        angle = i / branches * math.tau + rng.uniform(-.4, .4)
        y = height - rng.randint(1, 3)
        reach = spread * rng.uniform(.5, .75)
        tip = (c + math.cos(angle) * reach, y + rng.randint(1, 3), c + math.sin(angle) * reach)
        s.line((c, y, c), tip, trunk)
        s.blob(*tip, spread * .5, 2.2, spread * .5, leaves, rng, .14)
    s.blob(c, height + .5, c, spread * .75, 3, spread * .75, leaves, rng, .12)
    return grounded(s, (c, 0, c))


def giant_tree(name, seed):
    rng = random.Random(seed)
    s = S(name, (16, 34, 16))
    for y in range(24):
        r = 2.6 if y < 18 else 1.7
        for x in range(16):
            for z in range(16):
                if (x - 7.5) ** 2 + (z - 7.5) ** 2 < r * r:
                    s.put(x, y, z, 'log')
    for i in range(7):
        angle = i / 7 * math.tau + rng.uniform(-.2, .2)
        s.line((7.5, 4, 7.5), (7.5 + math.cos(angle) * 7, 0, 7.5 + math.sin(angle) * 7), 'log', .8)
    for i in range(6):
        angle = i / 6 * math.tau + rng.uniform(-.3, .3)
        y = rng.randint(17, 23)
        tip = (7.5 + math.cos(angle) * 6, y + 3, 7.5 + math.sin(angle) * 6)
        s.line((7.5, y, 7.5), tip, 'log', .6)
        s.blob(*tip, 3.4, 2.4, 3.4, 'leaves', rng, .1)
    s.blob(7.5, 27, 7.5, 7, 5, 7, 'leaves', rng, .1)
    for i in range(14):
        x, z = rng.randint(1, 14), rng.randint(1, 14)
        for y in range(rng.randint(19, 22), 24):
            if (x, y + 1, z) in s.cells or (x, y, z) in s.cells:
                s.put(x, y, z, 'leaves')
    s.box(3, 12, 11, 5, 12, 11, 'glow_moss')
    return grounded(s, (8, 0, 8), (7, 0, 7))


def pine(name, height, radius, seed, leaves='pine_leaves'):
    rng = random.Random(seed)
    size = radius * 2 + 1
    s = S(name, (size, height + 3, size))
    c = radius
    for y in range(height):
        s.put(c, y, c, 'log')
    y = height + 1
    tier = 0
    while y > 2:
        r = min(radius, 1 + tier * .75)
        for x in range(size):
            for z in range(size):
                d = math.hypot(x - c, z - c)
                if d <= r + rng.uniform(-.5, .3):
                    s.put(x, y, z, leaves)
                    if d > r - 1 and rng.random() < .5:
                        s.put(x, y - 1, z, leaves)
        y -= 2
        tier += 1
        if tier % 3 == 0:
            y += 1
    return grounded(s, (c, 0, c))


def dead_pine():
    s = S('Dead Pine', (7, 12, 7))
    for y in range(11):
        s.put(3, y, 3, 'burnt_log' if y > 7 else 'log')
    for y, x, z in ((5, 1, 3), (7, 5, 3), (8, 3, 1), (9, 3, 5)):
        s.line((3, y, 3), (x, y + 1, z), 'log')
    return grounded(s, (3, 0, 3))


def palm(name, height, lean, seed):
    rng = random.Random(seed)
    s = S(name, (13, height + 4, 13))
    head = (6 + lean, height, 6)
    for y in range(height + 1):
        t = (y / height) ** 2
        s.put(6 + lean * t, y, 6, 'log')
    for i in range(7):
        angle = i / 7 * math.tau + rng.uniform(-.2, .2)
        px, pz = head[0], head[2]
        for k in range(1, 7):
            drop = (k - 2) ** 2 * .22 - .8
            s.put(px + math.cos(angle) * k, height + 1 - drop, pz + math.sin(angle) * k, 'palm_leaves')
            if k in (2, 3, 4):
                s.put(px + math.cos(angle + .4) * k, height - drop, pz + math.sin(angle + .4) * k, 'palm_leaves')
    s.put(head[0] + 1, height - 1, 6, 'half_cylinder_peach')
    s.put(head[0] - 1, height - 1, 6, 'half_cylinder_peach')
    return grounded(s, (6, 0, 6))


def swamp_tree(seed):
    rng = random.Random(seed)
    s = S('Swamp Tree', (13, 14, 13))
    for y in range(8):
        s.put(6, y, 6, 'log')
    for x, z in ((4, 6), (8, 6), (6, 4), (6, 8), (5, 8), (8, 5)):
        s.line((6, 3, 6), (x, 0, z), 'log')
    s.blob(6, 9, 6, 5.5, 2.4, 5.5, 'leaves', rng, .12)
    for i in range(22):
        angle, r = rng.uniform(0, math.tau), rng.uniform(2.5, 5.8)
        x, z = 6 + math.cos(angle) * r, 6 + math.sin(angle) * r
        for y in range(7, 7 - rng.randint(1, 5), -1):
            s.put(x, y, z, 'moss' if y < 6 else 'leaves')
    return grounded(s, (6, 0, 6))


def burnt_tree(name, height, seed):
    rng = random.Random(seed)
    s = S(name, (9, height + 3, 9))
    for y in range(height):
        s.put(4 + (y > height / 2 and 1), y, 4, 'burnt_log')
    for i in range(rng.randint(3, 5)):
        angle = rng.uniform(0, math.tau)
        y = rng.randint(height // 2, height - 1)
        s.line((4, y, 4), (4 + math.cos(angle) * 4, y + rng.randint(1, 3), 4 + math.sin(angle) * 4), 'burnt_log')
    for x, z in ((3, 4), (5, 4), (4, 3)):
        s.put(x, 0, z, 'burnt_log')
    s.put(4, 2, 4, 'lava')
    return grounded(s, (4, 0, 4))


def bush(seed):
    rng = random.Random(seed)
    s = S('Bush', (5, 3, 5))
    s.blob(2, .6, 2, 2.3, 1.5, 2.3, 'leaves', rng, .1)
    return grounded(s, (2, 0, 2))


def fallen_log():
    s = S('Fallen Log', (12, 4, 5))
    s.box(1, 0, 2, 10, 0, 2, 'log')
    s.box(1, 1, 2, 10, 1, 2, 'log')
    s.box(1, 0, 1, 10, 0, 1, 'log')
    s.cut(4, 1, 2, 5, 1, 2)
    for x in (2, 6, 7, 9):
        s.put(x, 2, 2, 'moss')
    s.put(7, 1, 3, 'glow_moss')
    s.put(0, 0, 2, 'dirt')
    s.put(0, 1, 2, 'dirt')
    return grounded(s, (2, 0, 2), (9, 0, 2))


def stump(block='log', top='moss'):
    s = S('Old Stump' if block == 'log' else 'Ember Stump', (5, 4, 5))
    s.box(1, 0, 1, 3, 2, 3, block)
    s.cut(2, 2, 2, 2, 2, 2)
    s.put(2, 1, 2, 'glow_moss' if block == 'log' else 'lava')
    s.put(0, 0, 2, block)
    s.put(4, 0, 2, block)
    s.put(2, 0, 0, block)
    s.put(1, 3, 1, top)
    return grounded(s, (2, 0, 2))


def cactus(name, height, arms, seed):
    rng = random.Random(seed)
    s = S(name, (7, height + 1, 7))
    for y in range(height):
        s.put(3, y, 3, 'cactus')
    for i in range(arms):
        dx, dz = ((1, 0), (-1, 0), (0, 1), (0, -1))[i]
        y = rng.randint(2, max(2, height - 3))
        s.put(3 + dx, y, 3 + dz, 'cactus')
        s.put(3 + dx * 2, y, 3 + dz * 2, 'cactus')
        for k in range(1, rng.randint(2, 4)):
            s.put(3 + dx * 2, y + k, 3 + dz * 2, 'cactus')
    s.put(3, height, 3, 'cylinder_pink')
    return grounded(s, (3, 0, 3))


# --- Stone and shelters ------------------------------------------------------

def boulder(name, block, radius, seed, moss=None):
    rng = random.Random(seed)
    size = radius * 2 + 2
    s = S(name, (size, radius * 2, size))
    c = size / 2 - .5
    s.blob(c, radius * .6, c, radius, radius * .9, radius * .8, block, rng)
    s.blob(c + 1, radius * .4, c + 1, radius * .6, radius * .6, radius * .6, 'mineral_ore' if seed % 2 else block, rng)
    if moss:
        for (x, y, z), value in list(s.cells.items()):
            if (x, y + 1, z) not in s.cells and rng.random() < .6:
                s.cells[(x, y, z)] = moss
    return grounded(s, (round(c), 0, round(c)))


def standing_stones():
    rng = random.Random(4)
    s = S('Standing Stones', (13, 7, 13))
    for i in range(7):
        angle = i / 7 * math.tau
        x, z = round(6 + math.cos(angle) * 5.2), round(6 + math.sin(angle) * 5.2)
        height = rng.randint(3, 6)
        for y in range(height):
            s.put(x, y, z, 'slab_stone')
            if y < height - 2:
                s.put(x + 1, y, z, 'stone')
        s.put(x, height - 1, z, 'moss')
    s.box(5, 0, 5, 7, 0, 7, 'slab_stone')
    s.put(6, 1, 6, 'crystal_ore')
    return grounded(s, (6, 0, 6))


def ruin(name, block, top, seed):
    rng = random.Random(seed)
    s = S(name, (13, 8, 11))
    s.box(0, 0, 0, 12, 0, 10, 'slab_stone')
    s.shell(1, 1, 1, 11, 7, 9, block)
    s.cut(5, 1, 1, 7, 4, 1)
    for x in (3, 9):
        s.cut(x, 3, 9, x + 1, 5, 9)
    # The walls fall from one corner.
    for (x, y, z) in list(s.cells):
        if y > 1 and (x + z * .7 + y * 1.3 + rng.uniform(-2, 2)) > 15 and y > 0:
            del s.cells[(x, y, z)]
    for (x, y, z), value in list(s.cells.items()):
        if y > 2 and (x, y + 1, z) not in s.cells and rng.random() < .35:
            s.cells[(x, y, z)] = top
    for i in range(10):
        s.put(rng.randint(0, 12), 1, rng.randint(0, 10), block)
    s.box(5, 1, 7, 7, 1, 7, 'slab_stone')
    s.put(6, 2, 7, 'crystal_ore')
    return grounded(s, (1, 0, 1), (11, 0, 1), (1, 0, 9), (11, 0, 9))


def hut():
    s = S('Hut', (11, 10, 11))
    s.box(1, 0, 1, 9, 0, 9, 'plank')
    s.shell(2, 1, 2, 8, 4, 8, 'plank')
    for x, z in ((2, 2), (8, 2), (2, 8), (8, 8)):
        s.box(x, 0, z, x, 5, z, 'log')
    s.cut(5, 1, 2, 5, 2, 2)
    s.cut(3, 2, 2, 3, 3, 2)
    s.cut(7, 2, 2, 7, 3, 2)
    s.cut(2, 2, 5, 2, 3, 5)
    s.cut(8, 2, 5, 8, 3, 5)
    for i in range(5):
        s.box(1 + i, 5 + i, 1 + i, 9 - i, 5 + i, 9 - i, 'thatch')
        if i < 4:
            s.cut(2 + i, 5 + i, 2 + i, 8 - i, 5 + i, 8 - i)
    s.box(7, 1, 6, 7, 7, 6, 'stone')
    s.put(7, 8, 6, 'slab_stone')
    s.put(6, 1, 7, 'container')
    s.put(4, 1, 7, 'lamp')
    return grounded(s, (2, 0, 2), (8, 0, 2), (2, 0, 8), (8, 0, 8))


def stilt_hut():
    s = S('Stilt Hut', (11, 13, 11))
    for x, z in ((1, 1), (9, 1), (1, 9), (9, 9), (5, 1), (5, 9)):
        s.box(x, 0, z, x, 4, z, 'log')
    s.box(0, 4, 0, 10, 4, 10, 'plank')
    s.shell(2, 5, 2, 8, 7, 8, 'plank')
    s.cut(5, 5, 2, 5, 6, 2)
    s.cut(2, 6, 5, 2, 6, 5)
    for i in range(4):
        s.box(1 + i, 8 + i, 1 + i, 9 - i, 8 + i, 9 - i, 'thatch')
    for y in range(4):
        s.put(5, y, 0, 'plank' if y % 2 else 'log')
    s.put(7, 5, 7, 'lamp')
    s.put(0, 5, 0, 'moss')
    return grounded(s, (1, 0, 1), (9, 0, 9))


def watchtower():
    s = S('Watchtower', (9, 16, 9))
    s.shell(2, 0, 2, 6, 11, 6, 'stone')
    s.box(1, 11, 1, 7, 11, 7, 'slab_stone')
    s.shell(1, 12, 1, 7, 13, 7, 'stone')
    for x in (1, 3, 5, 7):
        s.cut(x, 13, 1, x, 13, 1)
        s.cut(x, 13, 7, x, 13, 7)
        s.cut(1, 13, x, 1, 13, x)
        s.cut(7, 13, x, 7, 13, x)
    s.cut(4, 0, 2, 4, 2, 2)
    s.cut(2, 7, 4, 2, 8, 4)
    s.cut(6, 5, 4, 6, 6, 4)
    for y in range(11):
        s.put(3 + (y % 4 in (1, 2)), y, 3 + (y % 4 in (2, 3)) * 2, 'slab_stone')
    s.put(4, 12, 4, 'lamp')
    return grounded(s, (2, 0, 2), (6, 0, 6))


def mountain_shrine():
    s = S('Mountain Shrine', (9, 8, 9))
    s.box(0, 0, 0, 8, 0, 8, 'slab_stone')
    s.box(1, 1, 1, 7, 1, 7, 'temple_brick')
    for x, z in ((1, 1), (7, 1), (1, 7), (7, 7)):
        s.box(x, 2, z, x, 5, z, 'stone')
    s.box(0, 6, 0, 8, 6, 8, 'temple_brick')
    s.cut(2, 6, 2, 6, 6, 6)
    s.box(4, 2, 4, 4, 3, 4, 'crystal_ore')
    s.put(4, 4, 4, 'cave_crystal')
    return grounded(s, (1, 0, 1), (7, 0, 7))


def mesa_arch():
    s = S('Mesa Arch', (15, 12, 7))
    for x in range(15):
        y = 9 + (abs(x - 7) > 5)
        s.box(x, y, 1, x, 11, 5, 'sandstone')
    s.box(0, 0, 1, 3, 11, 5, 'red_sand')
    s.box(11, 0, 1, 14, 11, 5, 'red_sand')
    s.box(4, 8, 2, 10, 8, 4, 'sandstone')
    s.box(0, 4, 1, 3, 4, 5, 'sandstone')
    s.box(11, 6, 1, 14, 6, 5, 'sandstone')
    return grounded(s, (1, 0, 3), (13, 0, 3))


def rock_pillar(seed):
    rng = random.Random(seed)
    s = S('Rock Pillar', (9, 18, 9))
    for y in range(18):
        r = 3.8 - y * .07 + rng.uniform(-.5, .5)
        block = 'sandstone' if y % 5 == 2 else 'red_sand'
        for x in range(9):
            for z in range(9):
                if math.hypot(x - 4, z - 4) < r:
                    s.put(x, y, z, block)
    return grounded(s, (4, 0, 4))


def pyramid():
    s = S('Pyramid', (16, 12, 16))
    for i in range(8):
        s.box(i, i, i, 15 - i, i, 15 - i, 'sandstone' if i % 2 else 'temple_brick')
    s.box(7, 8, 7, 8, 8, 8, 'sphere_gold')
    s.cut(3, 1, 3, 12, 3, 12)
    for x in (4, 11):
        for z in (4, 11):
            s.box(x, 1, z, x, 3, z, 'temple_brick')
    s.cut(7, 0, 0, 8, 3, 3)
    s.box(7, 0, 0, 8, 0, 3, 'slab_stone')
    s.box(6, 4, 0, 9, 4, 1, 'temple_brick')
    s.box(6, 1, 10, 9, 1, 11, 'temple_brick')
    s.put(7, 2, 10, 'container')
    s.put(8, 2, 10, 'crystal_ore')
    s.put(4, 2, 7, 'lamp')
    s.put(11, 2, 7, 'lamp')
    return grounded(s, (0, 0, 0), (15, 0, 0), (0, 0, 15), (15, 0, 15))


def temple():
    s = S('Temple', (15, 11, 15))
    s.box(0, 0, 0, 14, 0, 14, 'temple_brick')
    s.box(1, 1, 1, 13, 1, 13, 'slab_stone')
    for z in (2, 5, 9, 12):
        for x in (2, 12):
            s.box(x, 2, z, x, 7, z, 'sandstone')
            s.put(x, 2, z, 'temple_brick')
            s.put(x, 7, z, 'temple_brick')
    for x in (5, 9):
        for z in (2, 12):
            s.box(x, 2, z, x, 7, z, 'sandstone')
    s.box(1, 8, 1, 13, 8, 13, 'temple_brick')
    s.box(3, 9, 3, 11, 9, 11, 'sandstone')
    s.box(5, 10, 5, 9, 10, 9, 'temple_brick')
    s.cut(4, 8, 4, 10, 9, 10)
    s.box(6, 2, 8, 8, 2, 10, 'temple_brick')
    s.box(7, 3, 9, 7, 4, 9, 'sphere_gold')
    s.put(7, 5, 9, 'cave_crystal')
    return grounded(s, (0, 0, 0), (14, 0, 0), (0, 0, 14), (14, 0, 14))


def obelisk():
    s = S('Obelisk', (7, 15, 7))
    s.box(0, 0, 0, 6, 0, 6, 'slab_stone')
    s.box(1, 1, 1, 5, 1, 5, 'temple_brick')
    s.box(2, 2, 2, 4, 9, 4, 'sandstone')
    s.box(3, 10, 3, 3, 13, 3, 'sandstone')
    for y in (3, 5, 7):
        s.put(3, y, 1, 'temple_brick')
        s.put(3, y, 5, 'temple_brick')
    s.put(3, 14, 3, 'sphere_gold')
    return grounded(s, (0, 0, 0), (6, 0, 6))


def desert_camp():
    s = S('Desert Camp', (13, 6, 11))
    for i in range(5):
        s.box(1 + i, i, 2, 9 - i, i, 8, 'thatch')
        if i < 4:
            s.cut(2 + i, i, 3, 8 - i, i, 7)
    s.cut(4, 0, 2, 6, 1, 2)
    s.put(11, 0, 3, 'log')
    s.put(11, 0, 4, 'lava')
    s.put(12, 0, 4, 'log')
    s.put(11, 0, 5, 'log')
    s.put(5, 0, 6, 'container')
    return grounded(s, (1, 0, 2), (9, 0, 8))


def skeleton():
    s = S('Giant Bones', (15, 9, 11))
    for x in range(2, 14):
        s.put(x, 3 + (x > 7) - (x > 11) * 2, 5, 'bone')
    for x in range(3, 11, 2):
        for dz, sign in ((1, 1), (1, -1)):
            for k in range(1, 5):
                s.put(x, 2 + round(math.sin(k / 4 * 3) * 3), 5 + k * sign, 'bone')
    s.box(0, 1, 4, 2, 3, 6, 'bone')
    s.clear(0, 2, 4)
    s.clear(0, 2, 6)
    return grounded(s, (3, 0, 5))


def igloo():
    s = S('Igloo', (11, 6, 13))
    s.blob(5, 0, 6, 5, 5.4, 5, 'snow')
    s.blob(5, 0, 6, 3.8, 4.2, 3.8, 'air')
    for key, value in list(s.cells.items()):
        if value == 'air':
            del s.cells[key]
    for x in range(11):
        for y in range(6):
            for z in range(13):
                if math.sqrt((x - 5) ** 2 + y ** 2 + (z - 6) ** 2) < 3.9:
                    s.clear(x, y, z)
    s.box(4, 0, 0, 6, 2, 3, 'ice')
    s.cut(5, 0, 0, 5, 1, 3)
    s.cut(5, 0, 4, 5, 1, 4)
    s.put(5, 0, 8, 'lava')
    s.put(3, 0, 6, 'container')
    return grounded(s, (5, 0, 2), (5, 0, 10))


def ice_spire(name, height, seed):
    rng = random.Random(seed)
    s = S(name, (11, height, 11))
    for y in range(height):
        r = (1 - y / height) ** 1.2 * 4.6
        cx, cz = 5 + math.sin(y * .2) * .8, 5 + math.cos(y * .17) * .8
        for x in range(11):
            for z in range(11):
                if math.hypot(x - cx, z - cz) < r + rng.uniform(-.3, .3):
                    s.put(x, y, z, 'ice' if rng.random() < .85 else 'snow')
    return grounded(s, (5, 0, 5))


def frozen_wreck():
    s = S('Frozen Wreck', (15, 9, 7))
    for x in range(15):
        half = 3 - abs(x - 7) // 3
        s.box(x, 1, 3 - half, x, 3, 3 + half, 'plank')
        s.cut(x, 2, 4 - half, x, 3, 2 + half)
    s.box(7, 1, 3, 7, 8, 3, 'log')
    s.box(5, 6, 3, 9, 6, 3, 'log')
    s.box(5, 0, 2, 9, 0, 4, 'ice')
    s.put(3, 4, 3, 'snow')
    s.put(11, 2, 3, 'container')
    return grounded(s, (7, 0, 3))


def basalt_vent():
    s = S('Basalt Vent', (9, 7, 9))
    for y in range(6):
        r = 4.2 - y * .55
        for x in range(9):
            for z in range(9):
                if math.hypot(x - 4, z - 4) < r:
                    s.put(x, y, z, 'basalt')
    s.box(4, 1, 4, 4, 5, 4, 'molten_lava')
    s.put(3, 3, 4, 'lava')
    return grounded(s, (4, 0, 4))


def basalt_columns(seed):
    rng = random.Random(seed)
    s = S('Basalt Columns', (11, 13, 11))
    for x in range(1, 10, 2):
        for z in range(1, 10, 2):
            if math.hypot(x - 5, z - 5) < 5:
                height = max(2, round(11 - math.hypot(x - 5, z - 5) * 2 + rng.uniform(-2, 2)))
                s.box(x, 0, z, x + 1, height, z + 1, 'basalt')
    return grounded(s, (5, 0, 5))


def alien_pod():
    s = S('Alien Pod', (9, 12, 9))
    s.blob(4, 5, 4, 3.6, 5, 3.6, 'alien_crust')
    s.blob(4, 5, 4, 2.3, 3.6, 2.3, 'air', keep=False)
    for key, value in list(s.cells.items()):
        if value == 'air':
            del s.cells[key]
    s.cut(4, 2, 0, 4, 6, 1)
    s.box(4, 1, 4, 4, 3, 4, 'astral_ore')
    s.put(4, 4, 4, 'inner_violet')
    for x, z in ((0, 0), (8, 0), (0, 8), (8, 8)):
        s.line((x, 0, z), (4 + (x - 4) * .4, 3, 4 + (z - 4) * .4), 'alien_crust')
    s.put(4, 11, 4, 'outer_magenta')
    return grounded(s, (0, 0, 0), (8, 0, 8))


def alien_spire():
    s = S('Alien Spire', (7, 16, 7))
    for y in range(15):
        r = 2.8 - y * .17 + math.sin(y * .9) * .5
        cx = 3 + math.sin(y * .4)
        for x in range(7):
            for z in range(7):
                if math.hypot(x - cx, z - 3) < r:
                    s.put(x, y, z, 'alien_crust' if y % 4 else 'outer_magenta')
    s.put(3, 15, 3, 'astral_ore')
    return grounded(s, (3, 0, 3))


def crystal_spire():
    rng = random.Random(9)
    s = S('Crystal Spire', (9, 14, 9))
    s.blob(4, 0, 4, 3.6, 1.8, 3.6, 'stone', rng)
    for (dx, dz, height, lean) in ((0, 0, 13, 0), (2, 1, 7, .3), (-2, 1, 8, -.3), (1, -2, 6, .2), (-1, -2, 5, -.2)):
        for y in range(height):
            r = 1.3 * (1 - y / height)
            for x in range(9):
                for z in range(9):
                    if math.hypot(x - 4 - dx - lean * y, z - 4 - dz) <= r:
                        s.put(x, y, z, 'crystal_ore' if y < height / 2 else 'cave_crystal')
    return grounded(s, (4, 0, 4))


# --- Mnemo -------------------------------------------------------------------

def mnemo_tendril(seed):
    rng = random.Random(seed)
    s = S('Mnemo Tendril', (9, 13, 9))
    s.blob(4, 0, 4, 3.4, 1.4, 3.4, 'mnemo_rot', rng)
    for i in range(3):
        angle = i / 3 * math.tau + rng.uniform(0, 1)
        p = (4, 1, 4)
        for k in range(1, 5):
            q = (4 + math.cos(angle + k * .8) * k * .8, k * 2.6, 4 + math.sin(angle + k * .8) * k * .8)
            s.line(p, q, 'mnemo_rot')
            p = q
        s.put(*p, 'mnemonical_ore')
    return grounded(s, (4, 0, 4))


def mnemo_nest(seed):
    rng = random.Random(seed)
    s = S('Mnemo Nest', (15, 12, 15))
    s.blob(7, 1, 7, 7, 2.4, 7, 'mnemo_rot', rng)
    for i in range(8):
        angle = i / 8 * math.tau
        s.line((7 + math.cos(angle) * 6.5, 1, 7 + math.sin(angle) * 6.5), (7 + math.cos(angle) * 2, 10, 7 + math.sin(angle) * 2), 'mnemo_rot', .7)
    s.blob(7, 5, 7, 2.6, 3.6, 2.6, 'mnemo_rot', rng, keep=False)
    s.put(7, 3, 7, 'mnemo_heart')
    s.cut(7, 4, 7, 7, 6, 7)
    s.put(7, 7, 7, 'mnemo_heart')
    s.cut(7, 3, 4, 7, 6, 5)
    for x, z in ((3, 3), (11, 4), (4, 11), (11, 11)):
        s.put(x, 2, z, 'mnemonical_ore')
        s.put(x, 3, z, 'mnemonical_ore')
    return grounded(s, (7, 0, 7))


# --- Caves -------------------------------------------------------------------

def giant_mushroom(name, height, radius, seed):
    rng = random.Random(seed)
    size = radius * 2 + 1
    s = S(name, (size, height + radius // 2 + 3, size))
    c = radius
    lean = rng.uniform(-1.2, 1.2)
    for y in range(height + 1):
        x = c + lean * (y / height) ** 2
        s.put(x, y, c, 'mushroom_stem')
        if y < height * .4:
            s.put(x + 1, y, c, 'mushroom_stem')
            s.put(x, y, c + 1, 'mushroom_stem')
    top = height
    cx = c + lean
    for x in range(size):
        for z in range(size):
            d = math.hypot(x - cx, z - c)
            if d <= radius - .3:
                y = top + round((1 - (d / radius) ** 2) * (radius * .55))
                s.put(x, y, z, 'mushroom_cap')
                s.put(x, y - 1, z, 'mushroom_gill' if d < radius - 1 else 'mushroom_cap')
    for i in range(radius * 2):
        angle = rng.uniform(0, math.tau)
        s.put(cx + math.cos(angle) * (radius - 1), top - 2 - rng.randint(0, 1), c + math.sin(angle) * (radius - 1), 'mushroom_gill')
    return s


def mushroom_grove(seed):
    rng = random.Random(seed)
    s = S('Mushroom Grove', (11, 6, 11))
    for i in range(6):
        x, z, h = rng.randint(1, 9), rng.randint(1, 9), rng.randint(1, 3)
        s.box(x, 0, z, x, h, z, 'mushroom_stem')
        s.box(x - 1, h + 1, z - 1, x + 1, h + 1, z + 1, 'mushroom_cap')
        s.put(x, h + 2, z, 'mushroom_cap')
        s.put(rng.randint(0, 10), 0, rng.randint(0, 10), 'glow_moss')
    return s


def crystal_cluster(name, seed, up=True):
    rng = random.Random(seed)
    s = S(name, (11, 10, 11))
    for i in range(9):
        angle, spread = rng.uniform(0, math.tau), rng.uniform(.2, .75)
        height, thick = rng.randint(4, 9), rng.uniform(.9, 1.6)
        for k in range(height):
            t = k / height
            x, z = 5 + math.cos(angle) * (spread * k + (i and 1.5)), 5 + math.sin(angle) * (spread * k + (i and 1.5))
            r = thick * (1 - t)
            for dx in range(-2, 3):
                for dz in range(-2, 3):
                    if math.hypot(dx, dz) <= r:
                        s.put(x + dx, k if up else 9 - k, z + dz, 'cave_crystal' if t > .25 else 'crystal_ore')
    return s


def hanging_roots(seed):
    rng = random.Random(seed)
    s = S('Hanging Roots', (11, 10, 11))
    for i in range(16):
        x, z = rng.randint(0, 10), rng.randint(0, 10)
        length = rng.randint(3, 10)
        for k in range(length):
            s.put(x, 9 - k, z, 'log' if k < 2 else 'moss')
        s.put(x, 9 - length, z, 'glow_moss')
    return s


def mine_support():
    s = S('Mine Support', (13, 7, 7))
    for x in (1, 6, 11):
        s.box(x, 0, 1, x, 5, 1, 'log')
        s.box(x, 0, 5, x, 5, 5, 'log')
        s.box(x, 5, 1, x, 5, 5, 'plank')
    s.box(0, 6, 3, 12, 6, 3, 'plank')
    s.box(0, 0, 2, 12, 0, 2, 'slab_stone')
    s.box(0, 0, 4, 12, 0, 4, 'slab_stone')
    for x in range(0, 13, 2):
        s.box(x, 0, 2, x, 0, 4, 'plank')
    s.put(6, 4, 3, 'lamp')
    s.box(3, 1, 3, 4, 1, 3, 'frame')
    s.box(3, 2, 2, 4, 2, 4, 'mineral_ore')
    s.put(9, 1, 1, 'container')
    return s


def mine_shaft():
    s = S('Mine Shaft', (9, 14, 9))
    for x, z in ((1, 1), (7, 1), (1, 7), (7, 7)):
        s.box(x, 0, z, x, 13, z, 'log')
    for y in (0, 4, 8, 12):
        s.shell(1, y, 1, 7, y, 7, 'plank')
    for y in range(0, 12):
        angle = y * math.pi / 2
        s.put(4 + round(math.cos(angle) * 2), y, 4 + round(math.sin(angle) * 2), 'plank')
    s.put(4, 9, 1, 'lamp')
    s.box(0, 0, 3, 0, 2, 5, 'mineral_ore')
    return s


def deep_shrine():
    s = S('Deep Shrine', (15, 12, 15))
    s.box(0, 0, 0, 14, 0, 14, 'deepslate')
    s.box(2, 1, 2, 12, 1, 12, 'temple_brick')
    s.box(4, 2, 4, 10, 2, 10, 'slab_stone')
    for x, z in ((1, 1), (13, 1), (1, 13), (13, 13), (7, 1), (7, 13), (1, 7), (13, 7)):
        height = 5 + (x + z) % 6
        s.box(x, 1, z, x, height, z, 'deepslate')
        s.put(x, height + 1, z, 'cave_crystal')
    s.box(6, 3, 6, 8, 3, 8, 'temple_brick')
    s.box(7, 4, 7, 7, 5, 7, 'sphere_gold')
    s.put(7, 6, 7, 'cave_crystal')
    s.put(7, 3, 4, 'container')
    s.box(5, 1, 0, 9, 1, 1, 'temple_brick')
    return s


def lost_camp():
    s = S('Lost Camp', (13, 6, 11))
    s.box(2, 0, 2, 6, 0, 2, 'log')
    for i in range(3):
        s.box(2 + i, 1 + i, 4 + i, 8 - i, 1 + i, 4 + i, 'thatch')
        s.box(2 + i, 1 + i, 9 - i - 0, 8 - i, 1 + i, 9 - i, 'thatch')
    s.put(10, 0, 3, 'log')
    s.put(11, 0, 3, 'lamp')
    s.put(9, 0, 8, 'container')
    s.box(10, 0, 7, 12, 1, 9, 'mineral_ore')
    s.box(1, 1, 9, 1, 3, 9, 'stone_slab' if False else 'slab_stone')
    return s


def bone_pile(seed):
    rng = random.Random(seed)
    s = S('Bone Pile', (11, 5, 11))
    s.blob(5, 0, 5, 4.6, 2.4, 4.6, 'bone', rng, .4)
    for i in range(5):
        angle = rng.uniform(0, math.tau)
        s.line((5, 1, 5), (5 + math.cos(angle) * 5, rng.randint(1, 4), 5 + math.sin(angle) * 5), 'bone')
    s.put(5, 3, 5, 'glow_moss')
    return s


def mnemo_hive(seed):
    rng = random.Random(seed)
    s = S('Mnemo Hive', (15, 14, 15))
    s.blob(7, 0, 7, 7, 3, 7, 'mnemo_rot', rng)
    for i in range(5):
        angle = i / 5 * math.tau + rng.uniform(-.3, .3)
        s.line((7 + math.cos(angle) * 5, 1, 7 + math.sin(angle) * 5), (7 + math.cos(angle) * 2, 13, 7 + math.sin(angle) * 2), 'mnemo_rot', .9)
    s.put(7, 4, 7, 'mnemo_heart')
    s.put(7, 8, 7, 'mnemo_heart')
    s.box(7, 5, 7, 7, 7, 7, 'mnemo_rot')
    for i in range(6):
        s.put(rng.randint(2, 12), 2, rng.randint(2, 12), 'mnemonical_ore')
    return s


def all_structures():
    return [
        oak('Oak Tree', 5, 4, 1), oak('Oak Tree Wide', 6, 6, 2, branches=6), oak('Oak Tree Tall', 9, 5, 3, branches=5),
        giant_tree('Giant Tree', 4), giant_tree('Giant Tree Old', 5),
        pine('Pine Tree', 9, 3, 6), pine('Pine Tree Tall', 15, 5, 7), dead_pine(),
        palm('Palm Tree', 8, 0, 8), palm('Palm Tree Leaning', 9, 3, 9),
        swamp_tree(10), burnt_tree('Burnt Tree', 7, 11), burnt_tree('Burnt Tree Tall', 12, 12),
        bush(13), fallen_log(), stump(), stump('burnt_log', 'burnt_log'),
        cactus('Cactus', 5, 2, 14), cactus('Cactus Tall', 9, 3, 15),
        boulder('Boulder', 'stone', 3, 16, 'moss'), boulder('Rock Heap', 'stone', 4, 17), boulder('Sand Rock', 'sandstone', 3, 19),
        standing_stones(), ruin('Ruin', 'stone', 'moss', 20), ruin('Desert Ruin', 'sandstone', 'sand', 21),
        hut(), stilt_hut(), watchtower(), mountain_shrine(),
        mesa_arch(), rock_pillar(22), pyramid(), temple(), obelisk(), desert_camp(), skeleton(),
        igloo(), ice_spire('Ice Spire', 18, 23), ice_spire('Ice Shard', 9, 24), frozen_wreck(),
        basalt_vent(), basalt_columns(25), alien_pod(), alien_spire(), crystal_spire(),
        mnemo_tendril(26), mnemo_nest(27),
        giant_mushroom('Giant Mushroom', 12, 7, 28), giant_mushroom('Tall Mushroom', 18, 5, 29), mushroom_grove(30),
        crystal_cluster('Crystal Cluster', 31), crystal_cluster('Hanging Crystals', 32, up=False), hanging_roots(33),
        mine_support(), mine_shaft(), deep_shrine(), lost_camp(), bone_pile(34), mnemo_hive(35),
    ]


def rule(structure, chance, attempts, slope, ground=(), variants=()):
    text = f'        (\n            structure: "{structure}",\n            chance_per_chunk: {chance},\n            attempts: {attempts},\n            max_slope: {slope},\n'
    text += '            ground: [' + ', '.join(f'"{g}"' for g in ground) + '],\n'
    if variants:
        text += '            variants: [' + ', '.join(f'(structure: "{name}", weight: {weight})' for name, weight in variants) + '],\n'
    return text + '        ),\n'


GREEN = ('grass', 'grass_dark')
OAKS = (('Oak Tree', 5.0), ('Oak Tree Wide', 2.0), ('Oak Tree Tall', 3.0))
BIOME_RULES = {
    'Plains': [rule('Oak Tree', .18, 1, 3, GREEN, OAKS), rule('Bush', .5, 2, 2, GREEN), rule('Boulder', .08, 1, 3, GREEN), rule('Standing Stones', .012, 1, 2, GREEN), rule('Ruin', .03, 1, 2, GREEN), rule('Hut', .02, 1, 2, GREEN)],
    'Forest': [rule('Oak Tree', .9, 4, 3, GREEN, OAKS), rule('Bush', .7, 2, 3, GREEN), rule('Fallen Log', .2, 1, 2, GREEN), rule('Old Stump', .2, 1, 2, GREEN)],
    'Great Forest': [rule('Giant Tree', .45, 1, 3, GREEN, (('Giant Tree', 1.0), ('Giant Tree Old', 1.0))), rule('Oak Tree Tall', .8, 2, 3, GREEN), rule('Bush', .8, 3, 3, GREEN), rule('Fallen Log', .25, 1, 2, GREEN)],
    'Pine Forest': [rule('Pine Tree', .95, 4, 4, ('grass_dark', 'snow'), (('Pine Tree', 3.0), ('Pine Tree Tall', 2.0), ('Dead Pine', .4))), rule('Boulder', .1, 1, 4, ('grass_dark', 'snow'))],
    'Mud': [rule('Stilt Hut', .04, 1, 3, ('mud',)), rule('Old Stump', .3, 1, 2, ('mud',)), rule('Bone Pile', .03, 1, 2, ('mud',))],
    'Swamp': [rule('Swamp Tree', .7, 2, 3, ('grass_dark', 'mud')), rule('Stilt Hut', .03, 1, 3, ('grass_dark', 'mud')), rule('Old Stump', .25, 1, 2, ('grass_dark', 'mud'))],
    'Mountain': [rule('Pine Tree', .7, 3, 5, GREEN), rule('Rock Heap', .25, 1, 6, ('stone',)), rule('Crystal Spire', .05, 1, 5, ('stone',)), rule('Watchtower', .015, 1, 3), rule('Mountain Shrine', .012, 1, 3, ('stone', 'snow'))],
    'Canyon': [rule('Rock Pillar', .14, 1, 4, ('red_sand',)), rule('Mesa Arch', .05, 1, 3, ('red_sand',)), rule('Desert Ruin', .04, 1, 2, ('red_sand',))],
    'Desert': [rule('Cactus', .5, 3, 3, ('sand',), (('Cactus', 3.0), ('Cactus Tall', 1.0))), rule('Sand Rock', .12, 1, 3, ('sand',)), rule('Giant Bones', .03, 1, 3, ('sand',)), rule('Desert Camp', .02, 1, 2, ('sand',)), rule('Desert Ruin', .03, 1, 2, ('sand',))],
    'Oasis': [rule('Palm Tree', .9, 3, 3, ('sand',), (('Palm Tree', 2.0), ('Palm Tree Leaning', 1.0))), rule('Desert Camp', .1, 1, 2, ('sand',))],
    'Pyramid and Temple': [rule('Pyramid', .1, 1, 3, ('sand',)), rule('Temple', .08, 1, 3, ('sand',)), rule('Obelisk', .12, 1, 2, ('sand',)), rule('Desert Ruin', .08, 1, 2, ('sand',))],
    'Volcano': [rule('Basalt Vent', .2, 1, 4, ('basalt',)), rule('Basalt Columns', .16, 1, 5, ('basalt',))],
    'Frozen Lake': [rule('Igloo', .03, 1, 2, ('snow',)), rule('Frozen Wreck', .02, 1, 3, ('snow', 'ice')), rule('Ice Shard', .2, 1, 3, ('snow',))],
    'Ice Floe': [rule('Ice Spire', .22, 1, 4, ('snow',)), rule('Ice Shard', .4, 2, 4, ('snow',)), rule('Igloo', .03, 1, 2, ('snow',)), rule('Frozen Wreck', .02, 1, 3, ('snow',))],
    'Crater': [rule('Alien Pod', .28, 1, 3, ('alien_crust',)), rule('Alien Spire', .2, 2, 3, ('alien_crust',))],
    'Burnt Forest': [rule('Burnt Tree', .85, 3, 3, ('burnt_ground',), (('Burnt Tree', 2.0), ('Burnt Tree Tall', 1.0))), rule('Ember Stump', .4, 2, 2, ('burnt_ground',))],
    'Mnemo Plain': [rule('Mnemo Tendril', .55, 2, 4), rule('Mnemo Nest', .06, 1, 3), rule('Crystal Spire', .04, 1, 3)],
}
BIOME_FIELDS = {
    'Mountain': dict(peaks=1.0, base_height=30.0, height_scale=24.0, surface_block='"grass_dark"', subsurface_block='"dirt"'),
    'Pine Forest': dict(peaks=.45),
    'Ice Floe': dict(peaks=.6),
    'Volcano': dict(peaks=.3),
    'Canyon': dict(peaks=.18),
    'Burnt Forest': dict(peaks=.12),
    'Mnemo Plain': dict(sky_color='(0.5, 0.26, 0.28)', surface_block='"mnemo_rot"', subsurface_block='"mnemo_rot"'),
}

CAVES = [
    ('Giant Mushroom', 0.6, 6, -70, -14, False, ()), ('Tall Mushroom', 0.42, 6, -70, -14, False, ()), ('Mushroom Grove', 0.9, 6, -74, -8, False, ()),
    ('Crystal Cluster', 0.9, 6, -90, -20, False, ()), ('Hanging Crystals', 0.9, 6, -80, -10, True, ()), ('Hanging Roots', 0.9, 6, -40, 0, True, ('Forest', 'Great Forest', 'Swamp')),
    ('Mine Support', 0.3, 6, -60, 0, False, ()), ('Mine Shaft', 0.15, 3, -60, 0, False, ()), ('Lost Camp', 0.15, 3, -60, -5, False, ()),
    ('Deep Shrine', 0.15, 3, -74, -20, False, ()), ('Bone Pile', 0.3, 3, -80, -10, False, ()), ('Mnemo Hive', 1., 6, -74, -10, False, ('Mnemo Plain',)),
]


def write_blocks():
    for block, (label, color, material, cells, hardness, drop, accent, amount, spread, glow) in NEW_BLOCKS.items():
        path = os.path.join(BLOCKS, block + '.ron')
        if os.path.exists(path):
            continue
        with open(path, 'w', encoding='utf-8', newline='\n') as out:
            out.write(f'''(
    id: "{block}",
    label: "{label}",
    shape: Cube,
    color: {color},
    roughness: 0.7,
    metallic: 0.0,
    emissive: 0.0,
    voronoi: {cells},
    material: {material},
    hardness: {hardness},
    drop: {drop},
    function: r#None,
    script: None,
    power_radius: 8.0,
    accent: Some((color: {accent}, amount: {amount}, cells: {spread}, glow: {glow})),
)
''')


def write_biomes():
    for name in sorted(os.listdir(BIOMES)):
        if name.startswith('_'):
            continue
        path = os.path.join(BIOMES, name)
        text = open(path, encoding='utf-8', newline='').read().replace('\r\n', '\n')
        biome = re.search(r'name: "([^"]+)"', text).group(1)
        start = text.index('structures: [')
        depth, i = 0, start + len('structures: ')
        while True:
            depth += text[i] == '['
            depth -= text[i] == ']'
            i += 1
            if depth == 0:
                break
        text = text[:start] + 'structures: [\n' + ''.join(BIOME_RULES.get(biome, ())) + '    ]' + text[i:]
        fields = dict(BIOME_FIELDS.get(biome, ()))
        for key in ('base_height', 'height_scale', 'sky_color', 'surface_block', 'subsurface_block'):
            if key in fields:
                text = re.sub(key + r': [^\n]*(\([^\n]*\))?,\n', f'{key}: {fields.pop(key)},\n', text, count=1)
        text = re.sub(r'    peaks: [^\n]*\n', '', text)
        if 'peaks' in fields:
            text = text.replace('    continentalness:', f'    peaks: {fields["peaks"]},\n    continentalness:', 1)
        with open(path, 'w', encoding='utf-8', newline='\n') as out:
            out.write(text)

    path = os.path.join(BIOMES, '_world.ron')
    text = open(path, encoding='utf-8').read()
    text = re.sub(r'\s*cave_structures: \[.*?\n    \],', '', text, flags=re.S)
    cave = '    cave_structures: [\n'
    for structure, chance, attempts, low, high, ceiling, biomes in CAVES:
        listed = '[' + ', '.join(f'"{b}"' for b in biomes) + ']'
        cave += f'        (structure: "{structure}", chance_per_chunk: {chance}, attempts: {attempts}, min_y: {low}, max_y: {high}, ceiling: {str(ceiling).lower()}, biomes: {listed}),\n'
    cave += '    ],\n'
    end = text.rindex(')')
    body = text[:end].rstrip()
    with open(path, 'w', encoding='utf-8', newline='\n') as out:
        out.write(body + ('\n' if body.endswith(',') else ',\n') + cave + ')\n')


if __name__ == '__main__':
    write_blocks()
    structures = all_structures()
    for s in structures:
        assert s.size[0] <= 16 and s.size[2] <= 16, s.name
        path = os.path.join(STRUCTURES, s.name.replace(' ', '_') + '.ron')
        with open(path, 'w', encoding='utf-8', newline='\n') as out:
            out.write(s.ron())
    write_biomes()
    print(f'{len(structures)} structures written.')
