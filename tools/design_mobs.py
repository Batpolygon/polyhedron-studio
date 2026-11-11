"""Mob designs of the project, written as ordinary `project/entities/mobs/*.ron` rigs.

Only the boxes and the Idle, Walk and Attack clips are replaced: stats, drops,
behaviours and biomes stay as they are in each file, and so do the Player
rigs. Every rig stays editable in the Sculptor afterwards, but running this
script again replaces the edits of the mobs listed here. Run from the root:

    python tools/design_mobs.py

A box is placed by its joint: `at` is the joint relative to the parent's joint,
and `hinge` says where that joint sits on the box, in half sizes (0, 1, 0 is
the middle of its top face). Rotations are in degrees, the head looks to -Z.
"""
import math
import os

MOBS = 'project/entities/mobs'
SIDES = ((-1, 'L'), (1, 'R'))


def rgb(hex_color):
    return tuple(int(hex_color[i:i + 2], 16) for i in (0, 2, 4))


class Rig:
    def __init__(self, idle=2.0, walk=0.6, attack=0.45):
        self.boxes = []
        self.names = set()
        self.durations = {'Idle': idle, 'Walk': walk, 'Attack': attack}
        self.layers = {'Idle': {}, 'Walk': {}, 'Attack': {}}

    def box(self, name, parent, size, color, at=(0, 0, 0), hinge=(0, 0, 0), rot=(0, 0, 0), glow=0.0):
        assert name not in self.names, name
        assert parent is None or parent in self.names, (name, parent)
        self.names.add(name)
        self.boxes.append(dict(name=name, parent=parent, size=size, color=rgb(color), offset=at, rotation=rot, emissive=glow,
                               pivot=tuple(h * s / 2 for h, s in zip(hinge, size))))
        return name

    def _layer(self, clip, bone, fn, rot, move, scale):
        assert bone in self.names, bone
        self.layers[clip].setdefault(bone, []).append((fn, rot, move, scale))

    def idle(self, bone, rot=(0, 0, 0), move=(0, 0, 0), scale=(0, 0, 0), phase=0.0, freq=1):
        self._layer('Idle', bone, lambda u: math.sin(math.tau * (freq * u + phase)), rot, move, scale)

    def walk(self, bone, rot=(0, 0, 0), move=(0, 0, 0), scale=(0, 0, 0), phase=0.0, freq=1):
        self._layer('Walk', bone, lambda u: math.sin(math.tau * (freq * u + phase)), rot, move, scale)

    def attack(self, bone, rot=(0, 0, 0), move=(0, 0, 0), scale=(0, 0, 0), windup=0.4):
        self._layer('Attack', bone, lambda u: strike(u, windup), rot, move, scale)


def strike(u, windup):
    """Wind up, hit fast, come back."""
    smooth = lambda t: t * t * (3 - 2 * t)
    if u < 0.35:
        return -windup * smooth(u / 0.35)
    if u < 0.55:
        return -windup + (1 + windup) * smooth((u - 0.35) / 0.2)
    return 1 - smooth((u - 0.55) / 0.45)


def num(value):
    text = f'{float(value):.4f}'.rstrip('0')
    return text + '0' if text.endswith('.') else text


def vec(values):
    return '(' + ', '.join(num(v) for v in values) + ')'


def box_ron(box):
    parent = f'Some("{box["parent"]}")' if box['parent'] else 'None'
    color = tuple(c / 255 for c in box['color'])
    glow = f'\n            emissive: {num(box["emissive"])},' if box['emissive'] else ''
    return f'''        (
            name: "{box['name']}",
            parent: {parent},
            size: {vec(box['size'])},
            pivot: {vec(box['pivot'])},
            offset: {vec(box['offset'])},
            rotation: {vec(box['rotation'])},
            color: {vec(color)},{glow}
            visible: true,
        ),
'''


def clip_ron(rig, name):
    duration = rig.durations[name]
    looping = name != 'Attack'
    steps = 8 if looping else 10
    out = f'        (\n            name: "{name}",\n            duration: {num(duration)},\n            looping: {str(looping).lower()},\n            tracks: [\n'
    for bone, layers in rig.layers[name].items():
        out += f'                (\n                    bone: "{bone}",\n                    keys: [\n'
        for i in range(steps + 1):
            u = i / steps
            rot, move, scale = [0, 0, 0], [0, 0, 0], [1, 1, 1]
            for fn, r, m, s in layers:
                w = fn(u)
                for k in range(3):
                    rot[k] += r[k] * w
                    move[k] += m[k] * w
                    scale[k] += s[k] * w
            out += f'                        (time: {num(u * duration)}, translation: {vec(move)}, rotation: {vec(rot)}, scale: {vec(scale)}, interpolation: Auto),\n'
        out += '                    ],\n                ),\n'
    return out + '            ],\n        ),\n'


def write_rig(name, rig):
    path = os.path.join(MOBS, name + '.ron')
    lines = open(path, encoding='utf-8').read().split('\n')
    start = lines.index('    boxes: [')
    sockets = next(i for i, line in enumerate(lines) if line.startswith('    sockets:'))
    clips = next(i for i, line in enumerate(lines) if line.startswith('    clips:'))
    end = next(i for i, line in enumerate(lines) if line.startswith('    default_clip:'))
    body = '    boxes: [\n' + ''.join(box_ron(b) for b in rig.boxes) + '    ],\n'
    body += '\n'.join(lines[sockets:clips]) + '\n'
    body += '    clips: [\n' + ''.join(clip_ron(rig, c) for c in ('Idle', 'Walk', 'Attack')) + '    ],\n'
    body += '    default_clip: "Idle",'
    with open(path, 'w', encoding='utf-8', newline='\n') as out:
        out.write('\n'.join(lines[:start]) + '\n' + body + '\n' + '\n'.join(lines[end + 1:]))


def arna():
    r = Rig(walk=0.5)
    shell, dark, mark, leg = '2a2230', '1a1520', 'd83a2e', '3a2c3e'
    r.box('Thorax', None, (.42, .26, .46), shell)
    r.box('Abdomen', 'Thorax', (.72, .54, .82), dark, at=(0, .08, .2), hinge=(0, 0, -1), rot=(8, 0, 0))
    r.box('Mark', 'Abdomen', (.18, .04, .54), mark, at=(0, .27, .4), glow=.4)
    r.box('Mark bar 1', 'Abdomen', (.46, .04, .08), mark, at=(0, .27, .24), glow=.4)
    r.box('Mark bar 2', 'Abdomen', (.34, .04, .08), mark, at=(0, .27, .52), glow=.4)
    r.box('Spinneret', 'Abdomen', (.16, .14, .1), mark, at=(0, -.06, .84))
    r.box('Head', 'Thorax', (.32, .24, .24), shell, at=(0, .03, -.23), hinge=(0, 0, 1))
    for s, t in SIDES:
        r.box(f'Eye big {t}', 'Head', (.07, .07, .03), 'ff4030', at=(s * .07, .05, -.24), glow=3)
        r.box(f'Eye mid {t}', 'Head', (.05, .05, .03), 'ff4030', at=(s * .13, .08, -.23), glow=3)
        r.box(f'Eye small {t}', 'Head', (.035, .035, .03), 'ff4030', at=(s * .03, .1, -.24), glow=3)
        r.box(f'Eye low {t}', 'Head', (.035, .035, .03), 'ff4030', at=(s * .12, .01, -.24), glow=3)
        fang = r.box(f'Fang {t}', 'Head', (.07, .17, .07), 'e8dcc0', at=(s * .07, -.08, -.22), hinge=(0, 1, 0))
        r.idle(fang, rot=(0, 0, s * 6), phase=.25)
        r.attack(fang, rot=(-50, 0, s * 25))
        for i, yaw in enumerate((48, 18, -14, -44)):
            y = yaw if s > 0 else -yaw
            upper = r.box(f'Leg {t}{i + 1}', 'Thorax', (.44, .07, .07), leg, at=(s * .2, .02, -.15 + i * .1), hinge=(-s, 0, 0), rot=(0, y, s * 38))
            lower = r.box(f'Shin {t}{i + 1}', upper, (.5, .05, .05), dark, at=(s * .44, 0, 0), hinge=(-s, 0, 0), rot=(0, 0, -s * 104))
            r.box(f'Claw {t}{i + 1}', lower, (.04, .1, .04), mark, at=(s * .5, -.02, 0))
            phase = .5 if (i + (s > 0)) % 2 else 0
            r.walk(upper, rot=(0, 22, 0), phase=phase)
            r.walk(upper, rot=(0, 0, s * 12), phase=phase + .25)
            r.idle(lower, rot=(0, 0, s * 3), phase=i * .2)
    r.idle('Abdomen', rot=(4, 0, 0), scale=(.03, .03, .03))
    r.walk('Abdomen', rot=(0, 5, 0), freq=2)
    r.walk('Thorax', move=(0, .02, 0), freq=2)
    r.attack('Thorax', rot=(-16, 0, 0), move=(0, .1, -.3))
    return r


def clanger():
    r = Rig()
    gold, brass, dark, skin = 'd8aa3e', 'a87c2c', '30261c', 'f0d27a'
    r.box('Bell', None, (.56, .46, .56), gold)
    r.box('Skirt', 'Bell', (.72, .24, .72), brass, at=(0, -.34, 0))
    r.box('Lip', 'Bell', (.84, .1, .84), gold, at=(0, -.5, 0))
    r.box('Band', 'Bell', (.6, .06, .6), dark, at=(0, -.1, 0))
    r.box('Shoulder', 'Bell', (.42, .1, .42), brass, at=(0, .28, 0))
    r.box('Crown', 'Bell', (.26, .14, .26), gold, at=(0, .4, 0))
    r.box('Loop', 'Bell', (.08, .18, .24), dark, at=(0, .55, 0))
    r.box('Clapper rod', 'Bell', (.08, .4, .08), dark, at=(0, -.3, 0), hinge=(0, 1, 0))
    r.box('Clapper', 'Clapper rod', (.22, .2, .22), skin, at=(0, -.4, 0))
    r.box('Mouth', 'Bell', (.34, .08, .03), dark, at=(0, -.09, -.29))
    for s, t in SIDES:
        r.box(f'Eye {t}', 'Bell', (.1, .12, .03), 'fff4c0', at=(s * .13, .08, -.29), glow=2)
        r.box(f'Pupil {t}', f'Eye {t}', (.05, .06, .02), dark, at=(0, 0, -.02))
        arm = r.box(f'Arm {t}', 'Bell', (.14, .46, .14), brass, at=(s * .3, .08, 0), hinge=(0, 1, 0), rot=(0, 0, s * 22))
        hand = r.box(f'Hand {t}', arm, (.12, .12, .12), dark, at=(0, -.46, 0))
        r.box(f'Cymbal {t}', hand, (.46, .05, .46), gold, at=(0, -.06, 0), rot=(0, 0, s * 70))
        r.box(f'Cymbal boss {t}', f'Cymbal {t}', (.14, .06, .14), brass, at=(0, .05, 0))
        leg = r.box(f'Leg {t}', 'Bell', (.18, .3, .2), dark, at=(s * .17, -.5, 0), hinge=(0, 1, 0))
        r.box(f'Foot {t}', leg, (.22, .08, .32), brass, at=(0, -.3, -.05))
        r.idle(arm, rot=(0, 0, s * 4))
        r.walk(leg, rot=(26 * s, 0, 0))
        r.walk(arm, rot=(-30 * s, 0, 0))
        r.attack(arm, rot=(0, 0, -s * 66), windup=.7)
    r.idle('Clapper rod', rot=(0, 0, 10))
    r.walk('Clapper rod', rot=(0, 0, 24), freq=2)
    r.walk('Bell', rot=(0, 0, 5), move=(0, .03, 0), freq=1)
    r.attack('Clapper rod', rot=(0, 0, 45), windup=1)
    r.attack('Bell', rot=(-8, 0, 0))
    return r


def cronil():
    r = Rig(walk=.5)
    hide, dark, belly, tooth = '5e8a3c', '3e6228', 'd8d090', 'f0ead0'
    r.box('Body', None, (.7, .3, .92), hide)
    r.box('Belly', 'Body', (.62, .08, .84), belly, at=(0, -.15, 0))
    r.box('Head', 'Body', (.68, .18, .34), hide, at=(0, .05, -.44), hinge=(0, 0, 1))
    r.box('Snout', 'Head', (.56, .12, .5), dark, at=(0, -.03, -.5))
    r.box('Nostril L', 'Snout', (.06, .03, .06), '1a2010', at=(-.14, .07, -.18))
    r.box('Nostril R', 'Snout', (.06, .03, .06), '1a2010', at=(.14, .07, -.18))
    jaw = r.box('Jaw', 'Head', (.58, .08, .68), belly, at=(0, -.1, -.1), hinge=(0, 0, 1))
    for i in range(5):
        x = -.22 + i * .11
        r.box(f'Tooth up {i + 1}', 'Snout', (.05, .08, .05), tooth, at=(x, -.09, -.2))
        r.box(f'Tooth low {i + 1}', jaw, (.05, .07, .05), tooth, at=(x + .05, .07, -.3))
    for s, t in SIDES:
        bulge = r.box(f'Eye {t}', 'Head', (.2, .18, .2), hide, at=(s * .24, .15, -.1))
        r.box(f'Iris {t}', bulge, (.14, .12, .03), 'f4d040', at=(0, .01, -.11), glow=1)
        r.box(f'Pupil {t}', f'Iris {t}', (.03, .1, .02), '101008', at=(0, 0, -.02))
        fore = r.box(f'Fore {t}', 'Body', (.12, .24, .14), dark, at=(s * .36, -.08, -.28), hinge=(0, 1, 0), rot=(0, 0, s * 18))
        r.box(f'Fore foot {t}', fore, (.2, .05, .22), hide, at=(0, -.24, -.04))
        thigh = r.box(f'Thigh {t}', 'Body', (.2, .22, .44), dark, at=(s * .38, .02, .3), hinge=(0, 0, -1), rot=(-30, 0, 0))
        shank = r.box(f'Shank {t}', thigh, (.14, .12, .46), hide, at=(0, 0, .42), hinge=(0, 0, 1), rot=(-55, 0, 0))
        r.box(f'Hind foot {t}', shank, (.26, .05, .4), hide, at=(0, -.04, -.5), hinge=(0, 0, 1), rot=(85, 0, 0))
        r.walk(thigh, rot=(26, 0, 0), phase=0 if s > 0 else .5)
        r.walk(fore, rot=(24, 0, 0), phase=.5 if s > 0 else 0)
    for i in range(4):
        r.box(f'Scute {i + 1}', 'Body', (.3 - i * .04, .06, .12), dark, at=(0, .17, -.3 + i * .22))
    tail = r.box('Tail', 'Body', (.2, .12, .36), dark, at=(0, -.02, .46), hinge=(0, 0, -1))
    r.box('Tail tip', tail, (.1, .06, .26), hide, at=(0, 0, .36), hinge=(0, 0, -1))
    r.idle(jaw, rot=(5, 0, 0), freq=1)
    r.idle('Body', scale=(.02, .05, .02))
    r.idle(tail, rot=(0, 12, 0))
    r.walk(tail, rot=(0, 22, 0))
    r.walk('Body', move=(0, .03, 0), freq=2)
    r.attack(jaw, rot=(10, 0, 0), windup=4.5)
    r.attack('Head', rot=(-24, 0, 0), windup=1)
    r.attack('Body', move=(0, .12, -.4))
    return r


def galopin():
    r = Rig(walk=.5)
    coat, mane, hoof, cloth, gold = '9a6a3e', '2e2018', '1c1a18', 'a83a34', 'e0b850'
    r.box('Body', None, (.62, .58, 1.28), coat)
    r.box('Rump', 'Body', (.6, .1, .4), '8a5c34', at=(0, .3, .42))
    r.box('Blanket', 'Body', (.66, .4, .52), cloth, at=(0, .16, -.02))
    r.box('Blanket trim', 'Blanket', (.68, .05, .54), gold, at=(0, -.2, 0))
    r.box('Saddle', 'Body', (.34, .1, .46), '4a2c1a', at=(0, .37, -.02))
    r.box('Cantle', 'Saddle', (.3, .12, .08), '4a2c1a', at=(0, .08, .2))
    r.box('Pommel', 'Saddle', (.14, .12, .08), gold, at=(0, .08, -.2))
    neck = r.box('Neck', 'Body', (.28, .64, .36), coat, at=(0, .12, -.56), hinge=(0, -1, 0), rot=(-32, 0, 0))
    r.box('Mane', neck, (.08, .66, .12), mane, at=(0, .02, .2), hinge=(0, -1, 0))
    head = r.box('Head', neck, (.26, .28, .34), coat, at=(0, .62, -.04), hinge=(0, -.4, .6), rot=(-8, 0, 0))
    muzzle = r.box('Muzzle', head, (.22, .2, .28), 'b89070', at=(0, -.03, -.3))
    r.box('Nose', muzzle, (.18, .08, .04), hoof, at=(0, .02, -.14))
    r.box('Forelock', head, (.1, .16, .06), mane, at=(0, .12, -.17))
    r.box('Bridle', head, (.28, .04, .3), cloth, at=(0, -.02, -.1))
    for s, t in SIDES:
        r.box(f'Eye {t}', head, (.03, .07, .08), '100c08', at=(s * .135, .04, -.08))
        r.box(f'Ear {t}', head, (.06, .14, .06), coat, at=(s * .08, .2, .08))
        for z, end, front in ((-.46, 'F', True), (.46, 'B', False)):
            thigh = r.box(f'Thigh {end}{t}', 'Body', (.2, .4, .26), coat, at=(s * .22, -.2, z), hinge=(0, 1, 0))
            shin = r.box(f'Shin {end}{t}', thigh, (.14, .38, .16), '7c5230', at=(0, -.4, 0), hinge=(0, 1, 0))
            r.box(f'Sock {end}{t}', shin, (.16, .12, .18), 'e4dccc', at=(0, -.3, 0))
            r.box(f'Hoof {end}{t}', shin, (.2, .1, .24), hoof, at=(0, -.41, -.02))
            phase = 0 if (s > 0) == front else .5
            r.walk(thigh, rot=(34, 0, 0), phase=phase)
            r.walk(shin, rot=(22, 0, 0), phase=phase - .2)
    tail = r.box('Tail', 'Body', (.14, .2, .14), mane, at=(0, .18, .64), hinge=(0, 1, -1), rot=(-40, 0, 0))
    r.box('Tail hair', tail, (.18, .54, .16), mane, at=(0, -.16, .02), hinge=(0, 1, 0), rot=(26, 0, 0))
    r.idle(neck, rot=(5, 0, 0))
    r.idle(tail, rot=(0, 0, 12), freq=2)
    r.walk(neck, rot=(8, 0, 0), freq=2)
    r.walk(tail, rot=(16, 0, 0), freq=2)
    r.walk('Body', move=(0, .05, 0), freq=2)
    r.attack('Body', rot=(34, 0, 0), windup=.2)
    r.attack('Thigh FL', rot=(90, 0, 0), windup=0)
    r.attack('Thigh FR', rot=(80, 0, 0), windup=0)
    r.attack(neck, rot=(-24, 0, 0))
    return r


def gardener():
    r = Rig(idle=2.6, walk=.9, attack=.6)
    bark, dark, leaf, leaf2, moss = '6a4a2e', '46301e', '3c9a44', '2c7a3a', '7ab048'
    r.box('Trunk', None, (.62, .9, .5), bark)
    r.box('Bark L', 'Trunk', (.08, .7, .3), dark, at=(-.31, -.04, -.05))
    r.box('Bark R', 'Trunk', (.08, .6, .24), dark, at=(.31, .08, .08))
    r.box('Bark F', 'Trunk', (.2, .3, .06), dark, at=(.12, -.28, -.26))
    r.box('Moss', 'Trunk', (.4, .1, .54), moss, at=(-.08, .4, 0))
    r.box('Mouth', 'Trunk', (.3, .05, .05), '201408', at=(0, .08, -.26))
    for s, t in SIDES:
        r.box(f'Eye socket {t}', 'Trunk', (.15, .15, .04), '201408', at=(s * .15, .26, -.26))
        r.box(f'Eye {t}', f'Eye socket {t}', (.08, .08, .03), 'b6ff60', at=(0, 0, -.02), glow=3)
    crown = r.box('Canopy', 'Trunk', (.98, .42, .86), leaf, at=(0, .44, 0), hinge=(0, -1, 0))
    r.box('Canopy top', crown, (.68, .3, .6), leaf2, at=(.05, .34, .04))
    r.box('Canopy left', crown, (.34, .3, .5), leaf2, at=(-.48, .12, .1))
    r.box('Canopy right', crown, (.3, .26, .46), '4cae50', at=(.5, .2, -.1))
    r.box('Canopy peak', crown, (.34, .2, .32), leaf, at=(-.1, .54, 0))
    for i, (x, y, z, c) in enumerate(((-.3, .24, -.44, 'f06a8a'), (.26, .36, -.3, 'f4d040'), (.06, .14, -.44, 'f4f0e0'), (.46, .12, -.28, 'f06a8a'))):
        r.box(f'Flower {i + 1}', crown, (.08, .08, .04), c, at=(x, y, z), glow=.6)
    for s, t in SIDES:
        arm = r.box(f'Branch {t}', 'Trunk', (.14, .46, .14), bark, at=(s * .34, .26, 0), hinge=(0, 1, 0), rot=(0, 0, s * 30))
        fore = r.box(f'Fore {t}', arm, (.11, .38, .11), dark, at=(0, -.46, 0), hinge=(0, 1, 0), rot=(30, 0, -s * 12))
        r.box(f'Twig {t}', fore, (.05, .16, .05), bark, at=(s * .08, -.1, 0), hinge=(0, 1, 0), rot=(0, 0, s * 50))
        r.box(f'Sprout {t}', f'Twig {t}', (.1, .05, .1), leaf, at=(0, -.16, 0))
        leg = r.box(f'Root {t}', 'Trunk', (.24, .28, .3), dark, at=(s * .17, -.42, 0), hinge=(0, 1, 0))
        r.box(f'Toe {t}1', leg, (.09, .07, .24), bark, at=(s * .06, -.25, -.2))
        r.box(f'Toe {t}2', leg, (.09, .07, .24), bark, at=(-s * .08, -.25, -.16), rot=(0, s * 20, 0))
        r.box(f'Toe {t}3', leg, (.22, .07, .1), bark, at=(s * .1, -.25, .1))
        r.walk(leg, rot=(24 * s, 0, 0))
        r.walk(arm, rot=(-16 * s, 0, 0))
        r.idle(arm, rot=(5, 0, 0), phase=.3 * s)
    shears = 'Fore R'
    r.box('Shears grip', shears, (.1, .1, .1), 'b8bec8', at=(0, -.38, 0))
    a = r.box('Blade A', 'Shears grip', (.05, .5, .1), 'dfe4ec', at=(.01, -.02, 0), hinge=(0, 1, 0), rot=(0, 0, 8))
    b = r.box('Blade B', 'Shears grip', (.05, .5, .1), 'b8bec8', at=(-.01, -.02, 0), hinge=(0, 1, 0), rot=(0, 0, -8))
    r.box('Can', 'Fore L', (.2, .2, .26), '4a86c0', at=(0, -.46, 0))
    r.box('Spout', 'Can', (.05, .05, .24), '4a86c0', at=(0, .03, -.22))
    r.idle(crown, rot=(0, 0, 3), scale=(.02, .02, .02))
    r.walk('Trunk', rot=(0, 0, 3))
    r.attack(a, rot=(0, 0, 26), windup=-.5)
    r.attack(b, rot=(0, 0, -26), windup=-.5)
    r.attack('Branch R', rot=(110, 0, 0))
    return r


def gelanto():
    r = Rig(idle=2.4, walk=.8)
    ice, deep, pale = '9ad8f0', '5aaed8', 'e8faff'
    r.box('Dome', None, (.9, .6, .9), ice)
    r.box('Cap', 'Dome', (.66, .16, .66), pale, at=(0, .36, 0))
    r.box('Rim', 'Dome', (1.0, .12, 1.0), deep, at=(0, -.28, 0))
    core = r.box('Core', 'Dome', (.28, .28, .28), '5af0ff', at=(0, .02, 0), rot=(45, 0, 45), glow=3)
    r.box('Mouth', 'Dome', (.26, .06, .03), '245070', at=(0, -.1, -.46))
    for s, t in SIDES:
        r.box(f'Eye {t}', 'Dome', (.16, .18, .03), '10304a', at=(s * .2, .08, -.46))
        r.box(f'Shine {t}', f'Eye {t}', (.06, .06, .02), 'ffffff', at=(s * .03, .04, -.02), glow=2)
    spikes = ((0, .44, 0, 0, 0, .5), (-.3, .38, -.2, -14, 0, 18), (.3, .38, -.16, -14, 0, -20), (-.24, .38, .28, 18, 0, 16), (.26, .38, .26, 18, 0, -16))
    for i, (x, y, z, a, _, c, *size) in enumerate(spikes):
        spike = r.box(f'Spike {i + 1}', 'Dome', (.14, .36 if i else .5, .14), pale, at=(x, y, z), hinge=(0, -1, 0), rot=(a, 0, c))
        r.box(f'Spike tip {i + 1}', spike, (.07, .16, .07), 'ffffff', at=(0, .36 if i else .5, 0), hinge=(0, -1, 0), glow=.8)
    for i, (x, z) in enumerate(((-.36, -.36), (.36, -.36), (-.36, .36), (.36, .36), (0, -.42), (0, .42), (-.42, 0), (.42, 0))):
        frill = r.box(f'Frill {i + 1}', 'Dome', (.16, .22, .16), deep, at=(x, -.3, z), hinge=(0, 1, 0))
        r.idle(frill, rot=(9, 0, 9), phase=i / 8)
        r.walk(frill, rot=(16, 0, 16), phase=i / 8)
    r.idle('Dome', scale=(.03, -.04, .03))
    r.idle(core, rot=(0, 30, 0), scale=(.1, .1, .1), freq=2)
    r.walk('Dome', move=(0, .16, 0), scale=(-.06, .1, -.06))
    r.attack('Dome', scale=(.28, -.22, .28), windup=.2)
    r.attack(core, scale=(.8, .8, .8), windup=0)
    return r


def gimgim():
    r = Rig(walk=.5)
    fur, dark, skin, red = '8a4e7a', '5e3058', 'e0b0a0', 'e8483c'
    r.box('Torso', None, (.38, .42, .3), fur)
    r.box('Tummy', 'Torso', (.26, .28, .04), skin, at=(0, -.04, -.15))
    head = r.box('Head', 'Torso', (.46, .38, .4), fur, at=(0, .2, -.02), hinge=(0, -1, 0))
    r.box('Face', head, (.34, .26, .04), skin, at=(0, .15, -.2))
    r.box('Muzzle', head, (.22, .13, .1), skin, at=(0, .08, -.23))
    r.box('Grin', 'Muzzle', (.18, .03, .02), dark, at=(0, -.02, -.06))
    r.box('Fang', 'Muzzle', (.03, .05, .02), 'ffffff', at=(.05, -.05, -.05))
    r.box('Crest', head, (.1, .14, .24), red, at=(0, .42, .02))
    for s, t in SIDES:
        r.box(f'Eye {t}', head, (.1, .11, .03), 'ffe040', at=(s * .09, .21, -.22), glow=2)
        r.box(f'Pupil {t}', f'Eye {t}', (.04, .08, .02), '100810', at=(0, 0, -.02))
        ear = r.box(f'Ear {t}', head, (.32, .26, .05), fur, at=(s * .22, .24, 0), hinge=(-s, 0, 0), rot=(0, 0, s * 14))
        r.box(f'Ear in {t}', ear, (.2, .16, .02), skin, at=(s * .15, 0, -.03))
        arm = r.box(f'Arm {t}', 'Torso', (.12, .46, .12), fur, at=(s * .24, .15, 0), hinge=(0, 1, 0), rot=(0, 0, s * 12))
        fore = r.box(f'Fore {t}', arm, (.1, .44, .1), dark, at=(0, -.46, 0), hinge=(0, 1, 0), rot=(0, 0, -s * 8))
        r.box(f'Hand {t}', fore, (.18, .1, .2), skin, at=(0, -.46, -.03))
        leg = r.box(f'Leg {t}', 'Torso', (.14, .2, .14), dark, at=(s * .11, -.2, 0), hinge=(0, 1, 0))
        r.box(f'Foot {t}', leg, (.16, .07, .26), skin, at=(0, -.2, -.06))
        r.walk(arm, rot=(36 * s, 0, 0))
        r.walk(leg, rot=(-30 * s, 0, 0))
        r.walk(fore, rot=(18, 0, 0), phase=.25)
        r.idle(ear, rot=(0, 10 * s, 0), freq=2, phase=.2)
        r.attack(arm, rot=(120, 0, 0), windup=.5)
    tail = r.box('Tail', 'Torso', (.06, .06, .34), dark, at=(0, -.16, .15), hinge=(0, 0, -1), rot=(40, 0, 0))
    tip = r.box('Tail curl', tail, (.06, .3, .06), dark, at=(0, 0, .32), hinge=(0, -1, 0))
    r.box('Tail tuft', tip, (.13, .13, .13), red, at=(0, .32, 0))
    r.idle(head, rot=(0, 10, 4))
    r.idle('Tail', rot=(0, 14, 0))
    r.walk('Tail', rot=(14, 0, 0), freq=2)
    r.walk('Torso', move=(0, .05, 0), rot=(0, 0, 4), freq=2)
    r.attack('Torso', rot=(-22, 0, 0), move=(0, .08, -.12))
    return r


def grom():
    r = Rig(walk=.5)
    shell, dark, belly, leg = '3a3a44', '24242c', 'ff8a2a', '2c2020'
    r.box('Shell', None, (.82, .52, .92), shell)
    r.box('Shell top', 'Shell', (.62, .14, .74), dark, at=(0, .32, 0))
    r.box('Seam', 'Shell', (.04, .05, .94), belly, at=(0, .28, 0), glow=2)
    belly_box = r.box('Belly', 'Shell', (.72, .14, .82), belly, at=(0, -.3, 0), glow=2.2)
    for i in range(3):
        r.box(f'Stripe {i + 1}', 'Shell', (.84, .06, .1), 'f4c430', at=(0, -.12, -.26 + i * .26))
    for i, (x, z) in enumerate(((-.26, -.3), (.26, -.3), (-.26, .3), (.26, .3))):
        r.box(f'Rivet {i + 1}', 'Shell', (.08, .04, .08), '8a8a96', at=(x, .4, z))
    fuse = r.box('Fuse', 'Shell', (.06, .24, .06), 'c8b078', at=(0, .38, .2), hinge=(0, -1, 0), rot=(-22, 0, 0))
    fuse2 = r.box('Fuse tip', fuse, (.05, .16, .05), 'a89058', at=(0, .24, 0), hinge=(0, -1, 0), rot=(40, 0, 0))
    spark = r.box('Spark', fuse2, (.1, .1, .1), 'ffd040', at=(0, .2, 0), rot=(45, 0, 45), glow=6)
    r.box('Spark 2', spark, (.05, .05, .05), 'ffffff', at=(0, .08, 0), glow=6)
    head = r.box('Head', 'Shell', (.46, .28, .24), dark, at=(0, -.06, -.46), hinge=(0, 0, 1))
    for s, t in SIDES:
        r.box(f'Eye {t}', head, (.1, .08, .04), 'ffa030', at=(s * .13, .06, -.13), glow=3)
        m = r.box(f'Mandible {t}', head, (.08, .08, .22), '8a8a96', at=(s * .14, -.08, -.1), hinge=(0, 0, 1), rot=(0, s * 12, 0))
        r.box(f'Mandible tip {t}', m, (.1, .06, .06), 'dcdce4', at=(-s * .03, 0, -.2))
        r.idle(m, rot=(0, -s * 7, 0), freq=2)
        r.attack(m, rot=(0, -s * 24, 0), windup=1)
        for i, z in enumerate((-.3, 0, .3)):
            upper = r.box(f'Leg {t}{i + 1}', 'Shell', (.3, .09, .1), leg, at=(s * .4, -.16, z), hinge=(-s, 0, 0), rot=(0, 0, -s * 24))
            r.box(f'Foot {t}{i + 1}', upper, (.1, .2, .12), dark, at=(s * .3, -.08, 0))
            r.walk(upper, rot=(0, 22, 0), phase=(i + (s > 0)) % 2 / 2)
    r.idle(belly_box, scale=(.03, .3, .03), freq=2)
    r.idle(spark, rot=(0, 90, 0), scale=(.4, .4, .4), freq=4)
    r.idle(fuse, rot=(0, 0, 7), freq=2)
    r.walk('Shell', rot=(0, 0, 3), freq=2)
    r.walk(spark, scale=(.4, .4, .4), freq=3)
    r.attack('Shell', scale=(.14, -.1, .14), windup=.4)
    r.attack(spark, scale=(1.4, 1.4, 1.4), windup=0)
    return r


def leafer():
    r = Rig(idle=2.6, walk=.55)
    green, dark, vein, pale = '6ab04a', '3e7a30', 'a8dc70', 'd8e890'
    r.box('Thorax', None, (.22, .5, .2), green, rot=(-14, 0, 0))
    abdomen = r.box('Abdomen', 'Thorax', (.42, .08, .78), green, at=(0, -.22, .08), hinge=(0, 0, -1), rot=(24, 0, 0))
    r.box('Midrib', abdomen, (.04, .1, .8), vein, at=(0, 0, .39))
    r.box('Leaf tip', abdomen, (.2, .08, .22), dark, at=(0, 0, .86))
    for i, z in enumerate((.2, .42, .62)):
        r.box(f'Vein {i + 1}', abdomen, (.4 - i * .04, .1, .03), vein, at=(0, 0, z))
    head = r.box('Head', 'Thorax', (.46, .16, .16), pale, at=(0, .25, -.02), hinge=(0, -1, 0))
    r.box('Mouth', head, (.08, .14, .08), dark, at=(0, -.12, -.03))
    for s, t in SIDES:
        r.box(f'Eye {t}', head, (.15, .18, .17), '60e860', at=(s * .24, .03, 0), glow=1.6)
        r.box(f'Pupil {t}', f'Eye {t}', (.03, .08, .02), '102808', at=(0, 0, -.09))
        ant = r.box(f'Antenna {t}', head, (.025, .5, .025), dark, at=(s * .06, .08, -.03), hinge=(0, -1, 0), rot=(-34, 0, s * 26))
        arm = r.box(f'Arm {t}', 'Thorax', (.1, .4, .1), dark, at=(s * .13, .16, -.06), hinge=(0, 1, 0), rot=(60, 0, s * 14))
        scythe = r.box(f'Scythe {t}', arm, (.06, .48, .12), pale, at=(0, -.4, 0), hinge=(0, -1, 0), rot=(-100, 0, 0))
        for k in range(3):
            r.box(f'Serration {t}{k + 1}', scythe, (.05, .03, .08), 'ffffff', at=(0, .12 + k * .13, .09))
        wing = r.box(f'Wing {t}', 'Thorax', (.3, .03, .62), dark, at=(s * .1, .05, .1), hinge=(-s, 0, -1), rot=(6, s * 8, -s * 8))
        r.box(f'Wing rib {t}', wing, (.03, .05, .58), vein, at=(s * .14, 0, .3))
        for i, (y, z, yaw) in enumerate(((-.05, -.02, 40), (-.2, .1, -35))):
            leg = r.box(f'Leg {t}{i + 1}', 'Thorax', (.34, .04, .04), dark, at=(s * .1, y, z), hinge=(-s, 0, 0), rot=(0, yaw if s > 0 else -yaw, s * 30))
            shin = r.box(f'Shin {t}{i + 1}', leg, (.44, .03, .03), green, at=(s * .34, 0, 0), hinge=(-s, 0, 0), rot=(0, 0, -s * 100))
            r.walk(leg, rot=(0, 18, 0), phase=(i + (s > 0)) % 2 / 2)
        r.idle(arm, rot=(6, 0, 0), freq=2)
        r.idle(ant, rot=(0, 0, 8), phase=s * .2)
        r.idle(wing, rot=(4, 0, 0))
        r.walk(wing, rot=(3, 0, 0), freq=3)
        r.attack(arm, rot=(-55, 0, 0), windup=.7)
        r.attack(scythe, rot=(70, 0, 0), windup=.6)
    r.idle(abdomen, rot=(0, 7, 0))
    r.idle(head, rot=(0, 18, 0))
    r.walk(head, rot=(0, 12, 0))
    r.attack('Thorax', rot=(-22, 0, 0), move=(0, 0, -.3))
    return r


def miby():
    r = Rig(walk=.34, attack=.35)
    fur, pink, white, dark = 'f4c8dc', 'ff9ab4', 'fff4f6', '301820'
    r.box('Body', None, (.4, .34, .42), fur)
    r.box('Chest', 'Body', (.26, .2, .06), white, at=(0, -.04, -.2))
    head = r.box('Head', 'Body', (.34, .3, .3), fur, at=(0, .15, -.1), hinge=(0, -1, 0))
    r.box('Nose', head, (.05, .04, .03), pink, at=(0, .1, -.16))
    r.box('Mouth', head, (.09, .02, .02), dark, at=(0, .06, -.16))
    for s, t in SIDES:
        r.box(f'Eye {t}', head, (.075, .09, .03), dark, at=(s * .1, .17, -.15))
        r.box(f'Shine {t}', f'Eye {t}', (.03, .03, .02), 'ffffff', at=(s * .01, .02, -.02), glow=1.5)
        r.box(f'Cheek {t}', head, (.07, .04, .02), pink, at=(s * .13, .08, -.155))
        ear = r.box(f'Ear {t}', head, (.1, .28, .05), fur, at=(s * .09, .29, .03), hinge=(0, -1, 0), rot=(6, 0, s * 12))
        r.box(f'Ear in {t}', ear, (.05, .2, .02), pink, at=(0, .15, -.03))
        tip = r.box(f'Ear tip {t}', ear, (.1, .16, .05), fur, at=(0, .28, 0), hinge=(0, -1, 0), rot=(30, 0, 0))
        paw = r.box(f'Paw {t}', 'Body', (.09, .1, .12), white, at=(s * .12, -.16, -.13), hinge=(0, 1, 0))
        foot = r.box(f'Foot {t}', 'Body', (.12, .08, .22), white, at=(s * .14, -.17, .1))
        r.idle(ear, rot=(5, 0, 0), phase=s * .1)
        r.idle(tip, rot=(12, 0, 0), freq=2, phase=.3)
        r.walk(ear, rot=(24, 0, 0))
        r.walk(tip, rot=(18, 0, 0), phase=.2)
        r.walk(paw, rot=(40, 0, 0))
        r.walk(foot, rot=(-30, 0, 0))
        r.attack(ear, rot=(-40, 0, 0), windup=0)
    tail = r.box('Tail', 'Body', (.17, .17, .17), white, at=(0, .02, .22))
    r.idle(head, rot=(6, 0, 4), freq=2)
    r.idle('Body', scale=(.01, .04, .01))
    r.idle(tail, scale=(.1, .1, .1), freq=2)
    r.walk('Body', move=(0, .1, 0), rot=(12, 0, 0))
    r.attack('Body', move=(0, .12, -.14), scale=(.08, -.08, .08))
    return r


def mnemoeater():
    r = Rig(idle=1.8, walk=.6, attack=.5)
    flesh, dark, rot, tooth, tumour = 'a83c4c', '6e2632', '481a24', 'e8e2c8', 'ff4a60'
    r.box('Maw', None, (1.0, .56, 1.1), flesh)
    r.box('Hide', 'Maw', (.84, .12, .9), dark, at=(0, .32, .06))
    r.box('Spine', 'Maw', (.14, .12, .86), rot, at=(0, .42, .06))
    for i, (x, y, z, size) in enumerate(((-.36, .34, .3, .24), (.34, .3, -.12, .18), (.2, .38, .4, .2), (-.5, .08, .1, .2), (.52, -.04, .3, .16))):
        lump = r.box(f'Tumour {i + 1}', 'Maw', (size, size, size), tumour, at=(x, y, z), rot=(0, 30 * i, 0), glow=2.4)
        r.idle(lump, scale=(.14, .14, .14), phase=i * .2)
    upper = r.box('Upper jaw', 'Maw', (.96, .2, .72), flesh, at=(0, .14, -.4), hinge=(0, -.4, 1), rot=(14, 0, 0))
    lower = r.box('Lower jaw', 'Maw', (.92, .18, .72), dark, at=(0, -.18, -.4), hinge=(0, .4, 1), rot=(-12, 0, 0))
    r.box('Tongue', lower, (.5, .08, .6), 'c05068', at=(0, .1, -.34))
    r.box('Throat', 'Maw', (.7, .3, .04), '180c10', at=(0, 0, -.56))
    for i in range(6):
        x = -.4 + i * .16
        r.box(f'Fang up {i + 1}', upper, (.08, .18, .08), tooth, at=(x, -.16, -.66))
        r.box(f'Fang low {i + 1}', lower, (.08, .15, .08), tooth, at=(x + .04, .14, -.66))
    for s, t in SIDES:
        for z, k in ((-.26, 'a'), (.26, 'b')):
            r.box(f'Side fang {t}{k}', upper, (.07, .15, .07), tooth, at=(s * .44, -.15, -.42 + (z + .26) * .5))
        r.box(f'Eye {t}', upper, (.14, .14, .14), 'ffd850', at=(s * .3, .15, -.42), glow=3.5)
        r.box(f'Pupil {t}', f'Eye {t}', (.05, .1, .03), '182008', at=(0, 0, -.08))
        for i, z in enumerate((-.3, .34)):
            thigh = r.box(f'Thigh {t}{i + 1}', 'Maw', (.26, .34, .3), dark, at=(s * .46, -.2, z), hinge=(-s * .4, 1, 0), rot=(0, 0, s * 18))
            shin = r.box(f'Shin {t}{i + 1}', thigh, (.18, .32, .2), rot, at=(0, -.32, 0), hinge=(0, 1, 0), rot=(0, 0, -s * 18))
            for k, dz in enumerate((-.1, .1)):
                r.box(f'Talon {t}{i + 1}{k + 1}', shin, (.07, .08, .22), tooth, at=(dz * .9, -.32, -.1))
            phase = 0 if (s > 0) == (i == 0) else .5
            r.walk(thigh, rot=(26, 0, 0), phase=phase)
    r.idle(upper, rot=(5, 0, 0))
    r.idle(lower, rot=(-5, 0, 0))
    r.idle('Maw', scale=(.02, .03, .02))
    r.walk(upper, rot=(7, 0, 0), freq=2)
    r.walk(lower, rot=(-7, 0, 0), freq=2)
    r.walk('Maw', rot=(0, 0, 5), move=(0, .03, 0))
    r.attack(upper, rot=(-18, 0, 0), windup=2.2)
    r.attack(lower, rot=(16, 0, 0), windup=2.2)
    r.attack('Maw', move=(0, .08, -.5))
    return r


def mnemowatcher():
    r = Rig(idle=3.2)
    root, dark, flesh, sclera, iris = '5a2630', '34141c', 'a03a4c', 'f0e8dc', 'ff3a50'
    r.box('Mound', None, (.92, .26, .92), dark)
    r.box('Mound top', 'Mound', (.6, .14, .6), root, at=(0, .19, 0))
    for i, (x, z, yaw) in enumerate(((-1, -1, 45), (1, -1, -45), (-1, 1, 135), (1, 1, -135), (0, -1.3, 0), (0, 1.3, 180))):
        a = r.box(f'Root {i + 1}', 'Mound', (.16, .12, .5), root, at=(x * .34, -.06, z * .34), hinge=(0, 0, 1), rot=(0, yaw, 0))
        r.box(f'Root tip {i + 1}', a, (.09, .08, .34), dark, at=(0, 0, -.48), hinge=(0, 0, 1))
    s1 = r.box('Stalk 1', 'Mound', (.32, .5, .32), flesh, at=(0, .22, 0), hinge=(0, -1, 0))
    s2 = r.box('Stalk 2', s1, (.26, .46, .26), root, at=(0, .46, 0), hinge=(0, -1, 0), rot=(-6, 0, 0))
    s3 = r.box('Stalk 3', s2, (.2, .4, .2), flesh, at=(0, .42, 0), hinge=(0, -1, 0), rot=(-6, 0, 0))
    for i, y in enumerate((.12, .3)):
        r.box(f'Vein {i + 1}', s2, (.28, .05, .28), 'b03a5a', at=(0, y, 0))
    eye = r.box('Eye', s3, (.82, .78, .72), sclera, at=(0, .38, -.04), hinge=(0, -1, 0), rot=(12, 0, 0))
    r.box('Iris', eye, (.48, .46, .03), iris, at=(0, .39, -.37), glow=2.6)
    r.box('Pupil', eye, (.2, .28, .03), '0a0c0a', at=(0, .39, -.39))
    r.box('Glint', eye, (.07, .07, .02), 'ffffff', at=(.1, .49, -.405), glow=3)
    r.box('Lid top', eye, (.88, .2, .78), flesh, at=(0, .72, 0))
    r.box('Lid low', eye, (.88, .14, .78), flesh, at=(0, .05, 0))
    r.box('Lid back', eye, (.86, .46, .1), root, at=(0, .39, .36))
    for s, t in SIDES:
        v = r.box(f'Vessel {t}', eye, (.03, .3, .03), 'd04060', at=(s * .3, .4, -.36), rot=(0, 0, s * 30))
        for i, (y, z) in enumerate(((.08, 0), (.3, .1))):
            ten = r.box(f'Tendril {t}{i + 1}', s1 if i == 0 else s2, (.34, .07, .07), flesh, at=(s * .13, y, z), hinge=(-s, 0, 0), rot=(0, 0, s * 26))
            tip = r.box(f'Tendril tip {t}{i + 1}', ten, (.3, .05, .05), iris, at=(s * .34, 0, 0), hinge=(-s, 0, 0), rot=(0, 0, s * 40), glow=1.2)
            r.idle(ten, rot=(0, 0, 12), phase=i * .3 + s * .1)
            r.walk(ten, rot=(0, 0, 12), phase=i * .3 + s * .1, freq=1)
            r.attack(tip, rot=(0, 40, 0), windup=.2)
    r.idle(s1, rot=(3, 0, 4))
    r.idle(s2, rot=(4, 0, -5), phase=.15)
    r.idle(s3, rot=(4, 0, 6), phase=.3)
    r.idle(eye, rot=(0, 16, 0), scale=(.02, .02, .02), freq=1)
    r.walk(s2, rot=(4, 0, 4))
    r.walk(eye, rot=(0, 12, 0))
    r.attack(s2, rot=(-20, 0, 0))
    r.attack(eye, scale=(.14, .14, .14), windup=.5)
    return r


def niff():
    r = Rig(walk=.55)
    fur, dark, nose, claw = '5e4c64', '42364a', 'f09ab0', 'e8dcc4'
    r.box('Body', None, (.7, .54, .9), fur)
    r.box('Back', 'Body', (.5, .12, .7), dark, at=(0, .3, .06))
    r.box('Belly', 'Body', (.5, .06, .7), 'a08eaa', at=(0, -.28, 0))
    head = r.box('Head', 'Body', (.44, .36, .34), fur, at=(0, .02, -.45), hinge=(0, 0, 1))
    snout = r.box('Snout', head, (.24, .2, .4), 'c8a0a8', at=(0, -.05, -.34), hinge=(0, 0, 1))
    tip = r.box('Nose', snout, (.3, .26, .08), nose, at=(0, 0, -.42))
    for i, (x, y) in enumerate(((-.2, .08), (.2, .08), (-.2, -.08), (.2, -.08), (0, .19), (0, -.19))):
        r.box(f'Feeler {i + 1}', tip, (.14 if x else .06, .06 if x else .14, .06), nose, at=(x, y, -.03), rot=(0, 0, 30 if x * y > 0 else (-30 if x else 0)))
    r.box('Nostril L', tip, (.04, .05, .02), '6a2a40', at=(-.05, 0, -.05))
    r.box('Nostril R', tip, (.04, .05, .02), '6a2a40', at=(.05, 0, -.05))
    for s, t in SIDES:
        r.box(f'Lid {t}', head, (.07, .03, .03), '241c28', at=(s * .13, .06, -.17))
        ear = r.box(f'Ear {t}', head, (.1, .24, .22), dark, at=(s * .22, .12, .04), hinge=(-s, -1, 0), rot=(0, 0, -s * 22))
        r.box(f'Ear in {t}', ear, (.04, .16, .14), nose, at=(s * .03, .1, -.02))
        for i, z in enumerate((-.28, .3)):
            leg = r.box(f'Leg {t}{i + 1}', 'Body', (.2, .22, .24), dark, at=(s * .32, -.22, z), hinge=(0, 1, 0))
            paw = r.box(f'Paw {t}{i + 1}', leg, (.3 if i == 0 else .22, .08, .3), nose, at=(s * .03, -.22, -.03))
            for k in range(3):
                r.box(f'Dig claw {t}{i + 1}{k + 1}', paw, (.05, .05, .16), claw, at=(-.09 + k * .09, -.01, -.2))
            phase = 0 if (s > 0) == (i == 0) else .5
            r.walk(leg, rot=(30, 0, 0), phase=phase)
        r.idle(ear, rot=(0, 0, s * 10), freq=2)
        r.attack(f'Leg {t}1', rot=(70, 0, 0), windup=.3)
    stub = r.box('Tail', 'Body', (.09, .09, .22), nose, at=(0, 0, .45), hinge=(0, 0, -1), rot=(-20, 0, 0))
    r.idle(snout, rot=(10, 16, 0), freq=3)
    r.idle(tip, scale=(.1, .1, .1), freq=4)
    r.idle(stub, rot=(0, 16, 0), freq=2)
    r.walk(snout, rot=(0, 14, 0), freq=2)
    r.walk('Body', rot=(0, 0, 4), move=(0, .02, 0))
    r.walk(stub, rot=(0, 18, 0), freq=2)
    r.attack(head, rot=(-24, 0, 0))
    return r


def notturo():
    r = Rig(idle=1.4, walk=.42, attack=.45)
    fur, dark, wing, bone, red = '3e3248', '281f32', '5a2e54', 'b89abc', 'ff3a4a'
    r.box('Body', None, (.42, .5, .34), fur)
    r.box('Chest', 'Body', (.3, .28, .05), '6a5674', at=(0, .06, -.17))
    head = r.box('Head', 'Body', (.42, .36, .36), fur, at=(0, .25, -.02), hinge=(0, -1, 0))
    r.box('Snout', head, (.16, .12, .1), 'c08aa0', at=(0, .12, -.21))
    r.box('Nose leaf', 'Snout', (.06, .1, .03), '8a4a66', at=(0, .07, -.05))
    for s, t in SIDES:
        r.box(f'Eye {t}', head, (.09, .08, .03), red, at=(s * .12, .22, -.18), glow=4)
        r.box(f'Tooth {t}', head, (.03, .07, .03), 'fffaf0', at=(s * .05, .04, -.2))
        ear = r.box(f'Ear {t}', head, (.18, .42, .06), fur, at=(s * .14, .34, 0), hinge=(0, -1, 0), rot=(0, 0, s * 16))
        r.box(f'Ear in {t}', ear, (.1, .28, .03), 'c08aa0', at=(0, .17, -.04))
        arm = r.box(f'Wing arm {t}', 'Body', (.5, .06, .07), bone, at=(s * .2, .18, 0), hinge=(-s, 0, 0), rot=(0, 0, s * 18))
        web1 = r.box(f'Web in {t}', arm, (.5, .5, .03), wing, at=(s * .25, -.27, 0))
        fore = r.box(f'Wing hand {t}', arm, (.56, .05, .05), bone, at=(s * .5, 0, 0), hinge=(-s, 0, 0), rot=(0, 0, -s * 24))
        r.box(f'Claw {t}', arm, (.04, .1, .04), 'fffaf0', at=(s * .5, .07, 0))
        r.box(f'Web out {t}', fore, (.56, .42, .03), wing, at=(s * .28, -.23, 0))
        r.box(f'Web tip {t}', fore, (.3, .2, .03), dark, at=(s * .16, -.54, 0))
        r.box(f'Finger {t}', fore, (.04, .62, .04), bone, at=(s * .56, -.03, 0), hinge=(0, 1, 0), rot=(0, 0, s * 12))
        leg = r.box(f'Leg {t}', 'Body', (.08, .26, .08), dark, at=(s * .13, -.24, 0), hinge=(0, 1, 0))
        r.box(f'Talon {t}', leg, (.13, .06, .18), bone, at=(0, -.26, -.03))
        r.idle(arm, rot=(0, 0, s * 18))
        r.idle(fore, rot=(0, 0, s * 12), phase=-.1)
        r.walk(arm, rot=(0, 0, s * 34), phase=0)
        r.walk(fore, rot=(0, 0, s * 26), phase=-.12)
        r.walk(leg, rot=(14, 0, 0))
        r.idle(ear, rot=(0, 0, s * 5), freq=2)
        r.attack(arm, rot=(0, s * 50, 0), windup=.8)
        r.attack(leg, rot=(70, 0, 0), windup=.2)
    r.idle('Body', move=(0, .03, 0))
    r.walk('Body', move=(0, .06, 0), rot=(10, 0, 0))
    r.attack('Body', rot=(-36, 0, 0), move=(0, .1, -.3))
    r.attack(head, rot=(30, 0, 0))
    return r


def rockle():
    r = Rig(walk=.5)
    rock, dark, moss, crystal = '7a7f8a', '54585f', '5c7a4a', 'c078ff'
    r.box('Carapace', None, (.74, .3, .64), rock)
    r.box('Under', 'Carapace', (.6, .1, .52), dark, at=(0, -.19, 0))
    r.box('Boulder', 'Carapace', (.5, .2, .44), dark, at=(-.04, .24, .04), rot=(0, 12, 0))
    r.box('Boulder 2', 'Carapace', (.26, .14, .24), rock, at=(.2, .2, -.12), rot=(0, 35, 0))
    r.box('Moss', 'Carapace', (.3, .05, .3), moss, at=(-.14, .35, .1))
    for i, (x, z, a, c, h) in enumerate(((-.1, .08, 8, 14, .44), (.12, .14, -12, -20, .32), (-.22, -.1, -20, 20, .26), (.08, -.1, 10, -8, .22))):
        shard = r.box(f'Crystal {i + 1}', 'Carapace', (.13, h, .13), crystal, at=(x, .26, z), hinge=(0, -1, 0), rot=(a, 0, c), glow=1.8)
        r.box(f'Crystal tip {i + 1}', shard, (.06, .1, .06), 'f0d8ff', at=(0, h, 0), hinge=(0, -1, 0), glow=3)
        r.idle(shard, scale=(.06, .06, .06), phase=i * .25)
    for s, t in SIDES:
        stalk = r.box(f'Stalk {t}', 'Carapace', (.05, .2, .05), dark, at=(s * .13, .12, -.3), hinge=(0, -1, 0), rot=(-16, 0, s * 8))
        r.box(f'Eye {t}', stalk, (.11, .11, .11), 'ffe080', at=(0, .22, 0), glow=2.5)
        r.box(f'Pupil {t}', f'Eye {t}', (.05, .05, .02), '101008', at=(0, 0, -.06))
        arm = r.box(f'Arm {t}', 'Carapace', (.32, .12, .12), dark, at=(s * .34, -.04, -.22), hinge=(-s, 0, 0), rot=(0, s * 40, 0))
        fore = r.box(f'Fore {t}', arm, (.12, .12, .3), rock, at=(s * .3, 0, 0), hinge=(0, 0, 1))
        claw = r.box(f'Claw {t}', fore, (.24, .14, .24), rock, at=(0, .02, -.3), hinge=(0, 0, 1))
        r.box(f'Claw tip {t}', claw, (.09, .1, .16), crystal, at=(s * .05, .02, -.3), glow=1.2)
        finger = r.box(f'Pincer {t}', claw, (.09, .08, .26), dark, at=(-s * .08, -.05, -.22), hinge=(0, 0, 1))
        r.idle(finger, rot=(0, s * 14, 0), freq=2)
        r.walk(arm, rot=(0, 10, 0))
        r.attack(fore, rot=(-36, 0, 0), windup=.8)
        r.attack(finger, rot=(0, s * 26, 0), windup=-1)
        for i, z in enumerate((-.08, .08, .24)):
            leg = r.box(f'Leg {t}{i + 1}', 'Carapace', (.32, .07, .08), dark, at=(s * .34, -.08, z), hinge=(-s, 0, 0), rot=(0, (20 - i * 20) * s, s * 26))
            shin = r.box(f'Shin {t}{i + 1}', leg, (.3, .05, .06), rock, at=(s * .32, 0, 0), hinge=(-s, 0, 0), rot=(0, 0, -s * 84))
            r.walk(leg, rot=(0, 20, 0), phase=(i + (s > 0)) % 2 / 2)
    r.walk('Carapace', rot=(0, 0, 3), freq=2)
    r.attack('Carapace', move=(0, 0, -.14))
    return r


def sarcophager():
    r = Rig(idle=3.0, walk=.8, attack=.6)
    gold, lapis, dark, wrap, skin = 'd8ac3c', '2850a8', '241c14', 'd4c8a4', '6a5a40'
    r.box('Case', None, (.66, .96, .46), gold)
    r.box('Inlay', 'Case', (.46, .7, .04), lapis, at=(0, -.06, -.24))
    for i in range(5):
        r.box(f'Bar {i + 1}', 'Case', (.48, .05, .05), gold, at=(0, -.34 + i * .14, -.25))
    r.box('Scarab', 'Case', (.18, .14, .04), '40c8b0', at=(0, .22, -.27), glow=1.6)
    r.box('Case back', 'Case', (.62, .9, .06), dark, at=(0, 0, .25))
    head = r.box('Head', 'Case', (.42, .42, .4), gold, at=(0, .48, 0), hinge=(0, -1, 0))
    r.box('Face', head, (.3, .3, .03), 'e4c060', at=(0, .2, -.21))
    r.box('Beard', head, (.09, .24, .06), lapis, at=(0, -.03, -.21))
    r.box('Cobra', head, (.09, .16, .08), gold, at=(0, .46, -.2))
    r.box('Cobra hood', 'Cobra', (.14, .08, .03), lapis, at=(0, .03, -.04))
    r.box('Crown', head, (.46, .1, .44), lapis, at=(0, .4, 0))
    for s, t in SIDES:
        r.box(f'Eye {t}', head, (.1, .05, .03), '60f0ff', at=(s * .08, .25, -.225), glow=3)
        r.box(f'Brow {t}', head, (.13, .03, .03), lapis, at=(s * .08, .3, -.225))
        for i in range(3):
            r.box(f'Nemes {t}{i + 1}', head, (.14, .08, .12), lapis if i % 2 else gold, at=(s * .27, .28 - i * .16, -.09), rot=(0, 0, s * 8))
            r.box(f'Nemes back {t}{i + 1}', head, (.18, .08, .3), gold if i % 2 else lapis, at=(s * .22, .28 - i * .16, .1))
        arm = r.box(f'Arm {t}', 'Case', (.14, .52, .14), wrap, at=(s * .41, .36, 0), hinge=(0, 1, 0), rot=(82, 0, s * 6))
        for i, y in enumerate((-.1, -.26, -.4)):
            r.box(f'Wrap {t}{i + 1}', arm, (.16, .05, .16), skin, at=(0, y, 0), rot=(0, 0, 14))
        hand = r.box(f'Hand {t}', arm, (.13, .13, .14), skin, at=(0, -.56, 0))
        strip = r.box(f'Loose wrap {t}', arm, (.05, .34, .03), wrap, at=(0, -.3, .09), hinge=(0, 1, 0))
        leg = r.box(f'Leg {t}', 'Case', (.2, .3, .2), wrap, at=(s * .17, -.48, 0), hinge=(0, 1, 0))
        r.box(f'Shin wrap {t}', leg, (.22, .05, .22), skin, at=(0, -.18, 0), rot=(0, 0, s * 10))
        r.box(f'Foot {t}', leg, (.2, .08, .3), skin, at=(0, -.3, -.05))
        r.idle(arm, rot=(5, 0, 0), phase=s * .1)
        r.idle(strip, rot=(14, 0, 0), freq=2, phase=s * .15)
        r.walk(strip, rot=(24, 0, 0), freq=2)
        r.walk(leg, rot=(14 * s, 0, 0))
        r.walk(arm, rot=(0, 0, s * 7), phase=.25)
        r.attack(arm, rot=(-70, 0, 0), windup=.5)
    r.idle('Case', rot=(0, 0, 2))
    r.walk('Case', rot=(0, 7, 5), move=(0, .02, 0))
    r.attack('Case', rot=(-12, 0, 0), move=(0, 0, -.2))
    return r


def squipter():
    r = Rig(idle=2.4, walk=.8)
    skin, dark, spot, inner = 'd8607a', 'a03a58', '7a2040', 'ffc0c8'
    r.box('Mantle', None, (.56, .78, .5), skin)
    r.box('Mantle tip', 'Mantle', (.38, .24, .34), dark, at=(0, .5, 0))
    r.box('Mantle peak', 'Mantle', (.18, .16, .16), spot, at=(0, .68, 0))
    for i, (x, y) in enumerate(((-.16, .1), (.14, .22), (.02, -.02), (-.1, .3), (.18, -.1))):
        r.box(f'Spot {i + 1}', 'Mantle', (.09, .09, .03), spot, at=(x, y, .26))
    head = r.box('Head', 'Mantle', (.54, .3, .48), dark, at=(0, -.42, 0))
    r.box('Siphon', head, (.12, .1, .14), inner, at=(0, -.1, -.28))
    for s, t in SIDES:
        fin = r.box(f'Fin {t}', 'Mantle', (.28, .4, .05), dark, at=(s * .28, .36, 0), hinge=(-s, 0, 0), rot=(0, 0, s * 10))
        r.box(f'Fin rim {t}', fin, (.06, .3, .06), spot, at=(s * .26, 0, 0))
        r.box(f'Eye {t}', head, (.2, .22, .22), 'fff0d8', at=(s * .24, .03, -.14))
        r.box(f'Iris {t}', f'Eye {t}', (.04, .15, .15), '30d0e0', at=(s * .09, 0, 0), glow=2)
        r.box(f'Pupil {t}', f'Iris {t}', (.03, .1, .05), '101018', at=(s * .03, 0, 0))
        r.idle(fin, rot=(0, 16, 0), phase=s * .1)
        r.walk(fin, rot=(0, 26, 0), freq=2)
    for i in range(8):
        angle = math.tau * (i + .5) / 8
        x, z = math.cos(angle), math.sin(angle)
        name = r.box(f'Tentacle {i + 1}', 'Head', (.12, .36, .12), skin if i % 2 else dark, at=(x * .19, -.14, z * .17), hinge=(0, 1, 0), rot=(-z * 26, 0, x * 26))
        mid = r.box(f'Tentacle mid {i + 1}', name, (.09, .3, .09), dark if i % 2 else skin, at=(0, -.36, 0), hinge=(0, 1, 0), rot=(-z * 24, 0, x * 24))
        tip = r.box(f'Sucker {i + 1}', mid, (.11, .05, .11), inner, at=(0, -.28, 0))
        r.idle(name, rot=(-8 * z, 0, 8 * x), phase=i / 8)
        r.idle(mid, rot=(-14 * z, 0, 14 * x), phase=i / 8 - .12)
        r.walk(name, rot=(24, 0, 0), phase=(i % 2) / 2)
        r.walk(mid, rot=(24, 0, 0), phase=(i % 2) / 2 - .15)
        if z < 0:
            r.attack(name, rot=(60, 0, 0), windup=.5)
            r.attack(mid, rot=(40, 0, 0), windup=.5)
    r.idle('Mantle', scale=(.04, -.02, .04), move=(0, .02, 0))
    r.walk('Mantle', move=(0, .05, 0), freq=2, rot=(5, 0, 0))
    r.attack('Mantle', rot=(-14, 0, 0))
    return r


def sulfy():
    r = Rig(idle=2.2, walk=1.0)
    sulfur, dark, ember, crust = 'e8d040', 'b89a28', 'ff8020', '8a7420'
    r.box('Blob', None, (.62, .56, .62), sulfur)
    r.box('Blob top', 'Blob', (.42, .14, .42), dark, at=(0, .34, 0))
    r.box('Blob low', 'Blob', (.44, .14, .44), crust, at=(0, -.34, 0))
    r.box('Drip 1', 'Blob', (.1, .2, .1), dark, at=(-.2, -.42, .08))
    r.box('Drip 2', 'Blob', (.08, .14, .08), dark, at=(.14, -.42, -.14))
    r.box('Drip 3', 'Blob', (.07, .28, .07), crust, at=(.02, -.46, .2))
    socket = r.box('Eye socket', 'Blob', (.34, .3, .05), crust, at=(0, .08, -.31))
    r.box('Eye', socket, (.24, .2, .04), ember, at=(0, 0, -.03), glow=4)
    r.box('Pupil', 'Eye', (.08, .14, .02), '180800', at=(0, 0, -.03))
    cannon = r.box('Cannon', 'Blob', (.26, .26, .16), dark, at=(0, -.14, -.31), hinge=(0, 0, 1))
    r.box('Barrel', cannon, (.2, .2, .36), crust, at=(0, 0, -.3))
    r.box('Muzzle', cannon, (.26, .26, .06), sulfur, at=(0, 0, -.5))
    r.box('Bore', 'Muzzle', (.14, .14, .03), ember, at=(0, 0, -.03), glow=4)
    orbit = r.box('Orbit', 'Blob', (.02, .02, .02), dark, at=(0, .06, 0))
    for i, (x, z) in enumerate(((.56, 0), (-.56, 0), (0, .56), (0, -.56))):
        shard = r.box(f'Shard {i + 1}', orbit, (.15, .24, .15), sulfur, at=(x, 0, z), rot=(0, 45, 0), glow=.6 if i % 2 else 0)
        r.box(f'Shard tip {i + 1}', shard, (.08, .12, .08), ember, at=(0, .17, 0), glow=2)
        r.idle(shard, move=(0, .05, 0), phase=i * .25, freq=2)
    for s, t in SIDES:
        r.box(f'Vent {t}', 'Blob', (.05, .22, .22), ember, at=(s * .32, .06, .08), glow=2.2)
    r.idle('Blob', move=(0, .07, 0), scale=(.02, -.03, .02))
    r.idle(orbit, rot=(0, 180, 0))
    r.walk('Blob', move=(0, .09, 0), rot=(8, 0, 0))
    r.walk(orbit, rot=(0, 180, 0))
    r.attack(cannon, move=(0, 0, .14), windup=-.4, scale=(.1, .1, -.1))
    r.attack('Blob', rot=(-10, 0, 0), scale=(.06, -.06, .06))
    return r


def tolossus():
    r = Rig(idle=3.4, walk=1.2, attack=.9)
    stone, dark, pale, moss, core = '84868e', '5e6068', 'a4a6ae', '5e8a48', '60d0ff'
    r.box('Torso', None, (1.1, 1.0, .74), stone)
    r.box('Chest', 'Torso', (.84, .5, .1), pale, at=(0, .16, -.38))
    r.box('Crack', 'Chest', (.08, .4, .03), dark, at=(.16, 0, -.06), rot=(0, 0, 14))
    r.box('Rune', 'Chest', (.2, .2, .03), core, at=(0, .02, -.06), rot=(0, 0, 45), glow=3.5)
    r.box('Waist', 'Torso', (.7, .3, .54), dark, at=(0, -.62, 0))
    r.box('Hips', 'Torso', (.9, .24, .62), stone, at=(0, -.84, 0))
    r.box('Moss back', 'Torso', (.9, .12, .64), moss, at=(0, .55, .06))
    r.box('Moss hang', 'Torso', (.4, .3, .05), moss, at=(-.2, .34, .38))
    r.box('Tuft', 'Torso', (.26, .1, .24), '74a858', at=(.32, .64, -.12))
    head = r.box('Head', 'Torso', (.34, .3, .32), pale, at=(0, .55, -.1), hinge=(0, -1, 0))
    r.box('Brow', head, (.38, .07, .1), dark, at=(0, .22, -.16))
    r.box('Jaw', head, (.3, .08, .3), dark, at=(0, .03, -.03))
    for s, t in SIDES:
        r.box(f'Eye {t}', head, (.07, .05, .03), core, at=(s * .08, .15, -.17), glow=4)
        pad = r.box(f'Pauldron {t}', 'Torso', (.46, .32, .54), dark, at=(s * .68, .38, 0))
        r.box(f'Pauldron moss {t}', pad, (.4, .06, .46), moss, at=(0, .18, 0))
        arm = r.box(f'Arm {t}', 'Torso', (.34, .78, .36), stone, at=(s * .7, .26, 0), hinge=(0, 1, 0), rot=(0, 0, s * 5))
        fore = r.box(f'Fore {t}', arm, (.44, .78, .46), pale, at=(0, -.78, 0), hinge=(0, 1, 0))
        r.box(f'Band {t}', fore, (.48, .1, .5), dark, at=(0, -.16, 0))
        fist = r.box(f'Fist {t}', fore, (.58, .44, .6), stone, at=(0, -.96, 0))
        for k in range(3):
            r.box(f'Knuckle {t}{k + 1}', fist, (.14, .12, .1), dark, at=(-.18 + k * .18, .08, -.31))
        leg = r.box(f'Leg {t}', 'Torso', (.34, .64, .4), dark, at=(s * .28, -.9, 0), hinge=(0, 1, 0))
        shin = r.box(f'Shin {t}', leg, (.4, .56, .44), stone, at=(0, -.62, 0), hinge=(0, 1, 0))
        r.box(f'Foot {t}', shin, (.46, .16, .62), dark, at=(0, -.56, -.08))
        r.idle(arm, rot=(3, 0, 0), phase=s * .1)
        r.walk(leg, rot=(20 * s, 0, 0))
        r.walk(shin, rot=(14, 0, 0), phase=(.25 if s > 0 else .75))
        r.walk(arm, rot=(-18 * s, 0, 0))
        r.attack(arm, rot=(30, 0, 0), windup=-4)
        r.attack(fore, rot=(10, 0, 0), windup=-5)
    r.idle('Torso', move=(0, .02, 0))
    r.idle(head, rot=(0, 14, 0))
    r.walk('Torso', rot=(0, 5, 3))
    r.attack('Torso', rot=(-24, 0, 0), windup=.6)
    return r


def vorator():
    r = Rig(idle=2.2, walk=.8, attack=.5)
    red, dark, stem, leaf, tooth = 'c03048', '801c30', '4c8a3c', '3a7030', 'f0f0d8'
    r.box('Bulb', None, (.7, .42, .7), stem)
    r.box('Bulb top', 'Bulb', (.5, .1, .5), leaf, at=(0, .26, 0))
    s1 = r.box('Stem', 'Bulb', (.3, .5, .3), stem, at=(0, .2, 0), hinge=(0, -1, 0), rot=(-8, 0, 0))
    s2 = r.box('Neck', s1, (.24, .44, .24), leaf, at=(0, .46, 0), hinge=(0, -1, 0), rot=(-16, 0, 0))
    jaw_base = r.box('Hinge', s2, (.36, .2, .3), dark, at=(0, .44, 0), rot=(24, 0, 0))
    up = r.box('Upper trap', jaw_base, (.96, .16, .9), red, at=(0, .06, .08), hinge=(0, -.5, 1), rot=(22, 0, 0))
    low = r.box('Lower trap', jaw_base, (.96, .16, .9), dark, at=(0, -.06, .08), hinge=(0, .5, 1), rot=(-22, 0, 0))
    r.box('Inside up', up, (.8, .03, .74), 'ff9a70', at=(0, -.09, -.46), glow=1.0)
    r.box('Inside low', low, (.8, .03, .74), 'ff9a70', at=(0, .09, -.46), glow=1.0)
    r.box('Spot 1', up, (.2, .03, .2), 'f4e0a0', at=(-.2, .09, -.5))
    r.box('Spot 2', up, (.14, .03, .14), 'f4e0a0', at=(.22, .09, -.36))
    r.box('Spot 3', up, (.1, .03, .1), 'f4e0a0', at=(.1, .09, -.68))
    for i in range(6):
        x = -.4 + i * .16
        r.box(f'Spike up {i + 1}', up, (.05, .2, .05), tooth, at=(x, -.14, -.9))
        r.box(f'Spike low {i + 1}', low, (.05, .2, .05), tooth, at=(x + .03, .14, -.9))
    for s, t in SIDES:
        for i in range(3):
            r.box(f'Spike up {t}{i + 1}', up, (.05, .18, .05), tooth, at=(s * .49, -.13, -.3 - i * .2))
            r.box(f'Spike low {t}{i + 1}', low, (.05, .18, .05), tooth, at=(s * .49, .13, -.34 - i * .2))
        blade = r.box(f'Leaf {t}', s1, (.5, .05, .24), leaf, at=(s * .14, .16, 0), hinge=(-s, 0, 0), rot=(0, 0, s * 22))
        r.box(f'Leaf tip {t}', blade, (.3, .04, .16), stem, at=(s * .48, 0, 0), hinge=(-s, 0, 0), rot=(0, 0, -s * 40))
        r.idle(blade, rot=(0, 0, s * 8), phase=s * .1)
        r.walk(blade, rot=(0, 0, s * 14), freq=2)
        r.attack(blade, rot=(0, 0, s * 30), windup=.2)
        for i, (x, z, yaw) in enumerate(((.26, -.24, 40), (.34, .02, 0), (.26, .28, -40))):
            root = r.box(f'Root {t}{i + 1}', 'Bulb', (.3, .12, .14), dark, at=(s * x, -.14, z), hinge=(-s, 0, 0), rot=(0, yaw if s > 0 else -yaw, -s * 8))
            tip = r.box(f'Root tip {t}{i + 1}', root, (.3, .08, .09), leaf, at=(s * .3, 0, 0), hinge=(-s, 0, 0), rot=(0, 0, -s * 42))
            r.walk(root, rot=(0, 24, s * 12), phase=(i + (s > 0)) % 2 / 2)
    r.idle(s1, rot=(3, 0, 5))
    r.idle(s2, rot=(4, 0, -6), phase=.2)
    r.idle(up, rot=(-9, 0, 0))
    r.idle(low, rot=(9, 0, 0))
    r.walk(s1, rot=(0, 0, 8))
    r.walk(up, rot=(-10, 0, 0), freq=2)
    r.walk(low, rot=(10, 0, 0), freq=2)
    r.attack(up, rot=(-30, 0, 0), windup=1.4)
    r.attack(low, rot=(30, 0, 0), windup=1.4)
    r.attack(s2, rot=(-36, 0, 0), windup=.7)
    return r


def wise():
    r = Rig(idle=4.0, walk=1.3, attack=.8)
    shell, plate, rim, skin, dark = '5a6a3a', '74864a', 'a88a54', '8aa070', '586c48'
    r.box('Body', None, (1.0, .38, 1.28), skin)
    r.box('Shell', 'Body', (1.16, .34, 1.42), shell, at=(0, .28, .02))
    r.box('Rim', 'Body', (1.24, .08, 1.5), rim, at=(0, .12, .02))
    r.box('Dome', 'Body', (.88, .24, 1.08), shell, at=(0, .54, .02))
    r.box('Crest', 'Body', (.5, .12, .66), plate, at=(0, .7, .02))
    for i, (x, z) in enumerate(((0, -.34), (0, .38), (-.34, -.22), (.34, -.22), (-.34, .3), (.34, .3))):
        r.box(f'Scute {i + 1}', 'Body', (.32, .05, .3), plate, at=(x, .67 - (abs(x) > 0) * .1, z))
    neck = r.box('Neck', 'Body', (.3, .3, .46), skin, at=(0, .06, -.62), hinge=(0, 0, 1), rot=(18, 0, 0))
    r.box('Wrinkle', neck, (.32, .05, .32), dark, at=(0, 0, -.22))
    head = r.box('Head', neck, (.38, .32, .4), skin, at=(0, .02, -.42), hinge=(0, 0, 1), rot=(-18, 0, 0))
    r.box('Beak', head, (.24, .1, .1), rim, at=(0, -.04, -.22))
    r.box('Beard', head, (.14, .3, .1), 'ece8dc', at=(0, -.26, -.16))
    r.box('Beard tip', 'Beard', (.08, .14, .07), 'ffffff', at=(0, -.2, 0))
    r.box('Cap', head, (.3, .06, .28), dark, at=(0, .18, .02))
    for s, t in SIDES:
        r.box(f'Lid {t}', head, (.1, .05, .12), dark, at=(s * .17, .09, -.12))
        r.box(f'Eye {t}', head, (.09, .06, .1), 'f0d050', at=(s * .17, .04, -.12), glow=1.5)
        r.box(f'Brow {t}', head, (.12, .05, .12), 'ece8dc', at=(s * .13, .14, -.18), rot=(0, 0, s * 18))
        for i, z in enumerate((-.5, .5)):
            leg = r.box(f'Leg {t}{i + 1}', 'Body', (.34, .38, .36), skin, at=(s * .52, -.1, z), hinge=(0, 1, 0))
            r.box(f'Band {t}{i + 1}', leg, (.36, .05, .38), dark, at=(0, -.16, 0))
            r.box(f'Foot {t}{i + 1}', leg, (.4, .1, .44), dark, at=(0, -.36, -.03))
            for k in range(3):
                r.box(f'Nail {t}{i + 1}{k + 1}', leg, (.07, .06, .06), 'e8e0c8', at=(-.12 + k * .12, -.37, -.26))
            phase = 0 if (s > 0) == (i == 0) else .5
            r.walk(leg, rot=(18, 0, 0), phase=phase)
    tail = r.box('Tail', 'Body', (.16, .12, .26), dark, at=(0, -.04, .66), hinge=(0, 0, -1), rot=(-14, 0, 0))
    r.idle(neck, rot=(6, 8, 0))
    r.idle(head, rot=(6, 14, 0), phase=.2)
    r.idle('Body', scale=(0, .015, 0))
    r.walk(neck, rot=(0, 10, 0))
    r.walk('Body', rot=(0, 0, 3), move=(0, .02, 0))
    r.walk(tail, rot=(0, 14, 0))
    r.attack(neck, move=(0, .04, -.3), rot=(-10, 0, 0), windup=.6)
    r.attack(head, rot=(24, 0, 0), windup=.8)
    r.attack('Body', rot=(-4, 0, 0))
    return r


DESIGNS = {
    'Arna': arna, 'Clanger': clanger, 'Cronil': cronil, 'Galopin': galopin, 'Gardener': gardener, 'Gelanto': gelanto, 'Gim-Gim': gimgim,
    'Grom': grom, 'Leafer': leafer, 'Miby': miby, 'MnemoEater': mnemoeater, 'MnemoWatcher': mnemowatcher, 'Niff': niff, 'Notturo': notturo,
    'Rockle': rockle, 'Sarcophager': sarcophager, 'Squipter': squipter, 'Sulfy': sulfy, 'Tolossus': tolossus, 'Vorator': vorator, 'Wise': wise,
}

if __name__ == '__main__':
    for name, design in DESIGNS.items():
        write_rig(name, design())
    print(f'{len(DESIGNS)} mobs written.')
