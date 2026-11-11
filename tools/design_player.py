"""Player body and first-person hand, written as ordinary rigs of the project.

Writes the boxes, the sockets and every clip of `Player.ron` and
`Player_Hand.ron`; they stay editable in the Sculptor (Mobs workspace), but
running this script again replaces those edits. Run from the root:

    python tools/design_player.py

Conventions: the body looks to -Z. A positive X rotation swings a hanging limb
forward, so it bends an elbow; a negative X rotation bends a knee. A positive X
rotation tips the torso backward. A positive Y rotation turns the front to the
left, so it brings the right shoulder forward.
"""
import math
import os

from design_mobs import MOBS, SIDES, Rig, box_ron, num, vec

TAU = math.tau
SKIN, BODY, LEGS = 'f6e220', 'e2d01d', 'c9b91a'
SHADE, DARK, METAL = 'a8990f', '33333c', 'b8c0cc'
LOOPING = {'Idle': 12, 'Walk': 16, 'Run': 16, 'Fall': 10, 'Swim': 16, 'Fly': 12}


def s(u, freq=1, phase=0.0):
    return math.sin(TAU * (freq * u + phase))


def c(u, freq=1, phase=0.0):
    return math.cos(TAU * (freq * u + phase))


def up(value):
    return max(0.0, value)


class Clip:
    """Keys of a clip: `pose(u)` gives {bone: rotation or (rotation, move)}."""

    def __init__(self, name, duration, looping, steps, pose):
        times = [i / steps for i in range(steps + 1)]
        self.name, self.duration, self.looping = name, duration, looping
        self.frames = [(u, pose(u)) for u in times]

    @classmethod
    def keys(cls, name, duration, looping, keys):
        """Hand-placed poses: `keys` is a list of (u, {bone: ...})."""
        clip = cls.__new__(cls)
        clip.name, clip.duration, clip.looping, clip.frames = name, duration, looping, keys
        return clip

    def ron(self):
        bones = []
        for _, pose in self.frames:
            bones += [bone for bone in pose if bone not in bones]
        out = f'        (\n            name: "{self.name}",\n            duration: {num(self.duration)},\n            looping: {str(self.looping).lower()},\n            tracks: [\n'
        for bone in bones:
            out += f'                (\n                    bone: "{bone}",\n                    keys: [\n'
            for u, pose in self.frames:
                value = pose.get(bone, (0, 0, 0))
                rot, move = value if isinstance(value[0], (tuple, list)) else (value, (0, 0, 0))
                out += f'                        (time: {num(u * self.duration)}, translation: {vec(move)}, rotation: {vec(rot)}, scale: (1.0, 1.0, 1.0), interpolation: Auto),\n'
            out += '                    ],\n                ),\n'
        return out + '            ],\n        ),\n'


def write(name, rig, sockets, clips):
    path = os.path.join(MOBS, name + '.ron')
    lines = open(path, encoding='utf-8').read().split('\n')
    start = lines.index('    boxes: [')
    end = next(i for i, line in enumerate(lines) if line.startswith('    default_clip:'))
    body = '    boxes: [\n' + ''.join(box_ron(b) for b in rig.boxes) + '    ],\n    sockets: [\n'
    for socket, bone, offset, rotation in sockets:
        body += f'        (\n            name: "{socket}",\n            bone: "{bone}",\n            offset: {vec(offset)},\n            rotation: {vec(rotation)},\n        ),\n'
    body += '    ],\n    clips: [\n' + ''.join(clip.ron() for clip in clips) + '    ],\n    default_clip: "Idle",'
    with open(path, 'w', encoding='utf-8', newline='\n') as out:
        out.write('\n'.join(lines[:start]) + '\n' + body + '\n' + '\n'.join(lines[end + 1:]))


def body():
    r = Rig()
    r.box('Hips', None, (.46, .16, .26), BODY, at=(0, -.04, 0))
    r.box('Belt', 'Hips', (.48, .05, .28), DARK, at=(0, .02, 0))
    r.box('Buckle', 'Belt', (.1, .07, .02), METAL, at=(0, 0, -.145))
    r.box('Torso', 'Hips', (.5, .46, .28), BODY, at=(0, .08, 0), hinge=(0, -1, 0))
    r.box('Chest', 'Torso', (.34, .2, .02), SHADE, at=(0, .29, -.145))
    r.box('Core', 'Chest', (.1, .1, .02), '6fe8ff', at=(0, 0, -.015), glow=1.5)
    r.box('Collar', 'Torso', (.3, .04, .2), DARK, at=(0, .47, 0))
    r.box('Head', 'Torso', (.44, .44, .44), SKIN, at=(0, .46, 0), hinge=(0, -1, 0))
    for side, name in SIDES:
        r.box(f'Eye {name}', 'Head', (.1, .21, .01), '000000', at=(side * .11, .1825 + .04, -.222))
        arm = r.box(f'Arm {name}', 'Torso', (.18, .33, .18), BODY, at=(side * .36, .4, 0), hinge=(0, .85, 0))
        r.box(f'Shoulder {name}', arm, (.22, .1, .22), SHADE, at=(0, 0, 0))
        fore = r.box(f'Forearm {name}', arm, (.165, .33, .165), BODY, at=(0, -.3, 0), hinge=(0, 1, 0))
        r.box(f'Cuff {name}', fore, (.18, .05, .18), DARK, at=(0, -.3, 0))
        hand = r.box(f'Hand {name}', fore, (.18, .18, .18), SKIN, at=(0, -.33, 0), hinge=(0, 1, 0))
        r.box(f'Thumb {name}', hand, (.05, .11, .06), SKIN, at=(-side * .11, -.08, -.03))
        leg = r.box(f'Leg {name}', 'Hips', (.2, .31, .2), LEGS, at=(side * .12, -.08, 0), hinge=(0, 1, 0))
        r.box(f'Knee {name}', leg, (.16, .1, .03), SHADE, at=(0, -.28, -.105))
        shin = r.box(f'Shin {name}', leg, (.18, .31, .18), LEGS, at=(0, -.3, 0), hinge=(0, 1, 0))
        foot = r.box(f'Foot {name}', shin, (.2, .15, .3), SKIN, at=(0, -.3, 0), hinge=(0, 1, .33))
        r.box(f'Toe {name}', foot, (.2, .06, .06), DARK, at=(0, -.12, -.17))
    return r


def gait(u, thigh, lean, knee_rest, knee_swing, arm, elbow, elbow_swing, foot, twist, hip_turn, bob, sway, head):
    """One cycle: the left leg leads when sin > 0, arms go against the legs."""
    a, b = s(u), c(u)
    pose = {
        'Hips': ((lean[0], -hip_turn * a, sway * a), (0, -bob * c(u, 2), 0)),
        'Torso': (lean[1], twist * a, -sway * .6 * a),
        'Head': (lean[2], -twist * .8 * a, 0),
    }
    for sign, name in ((1, 'L'), (-1, 'R')):
        wave, lift = sign * a, up(sign * b)
        pose[f'Leg {name}'] = (thigh[0] * wave + thigh[1], 0, 0)
        pose[f'Shin {name}'] = (-(knee_rest + knee_swing * lift ** 1.3), 0, 0)
        pose[f'Foot {name}'] = (foot * up(-wave), 0, 0)
        pose[f'Arm {name}'] = (-arm * wave, 0, -sign * 4)
        pose[f'Forearm {name}'] = (elbow + elbow_swing * up(-wave), 0, 0)
    return pose


def body_clips():
    def idle(u):
        pose = {'Hips': ((0, 0, 0), (0, -.006 * c(u), 0)), 'Torso': (.8 * s(u), 0, 0), 'Head': (1.2 * s(u, 1, .1), 4 * s(u), 0)}
        for sign, name in SIDES:
            pose[f'Arm {name}'] = (-1.5 * s(u), 0, sign * (3.5 + 1 * s(u)))
            pose[f'Forearm {name}'] = (7 + 1.5 * s(u), 0, 0)
        return pose

    walk = lambda u: gait(u, (27, 2), (0, -3, 2), 8, 50, 25, 14, 22, 18, 6, 5, .018, 1.5, 0)
    run = lambda u: gait(u, (48, 12), (-5, -9, 12), 22, 82, 52, 72, 18, 24, 11, 8, .04, 2.5, 0)

    def fall(u):
        pose = {'Torso': (3, 0, 0), 'Head': (-8, 0, 0)}
        for sign, name in SIDES:
            pose[f'Arm {name}'] = (6 * s(u, 1, .25 * sign), 0, sign * (58 + 9 * s(u)))
            pose[f'Forearm {name}'] = (24 + 8 * s(u, 1, .3), 0, 0)
        pose['Leg L'], pose['Shin L'] = (14 + 5 * s(u), 0, 0), (-26, 0, 0)
        pose['Leg R'], pose['Shin R'] = (-6 - 5 * s(u), 0, 0), (-38, 0, 0)
        return pose

    def swim(u):
        a, b = s(u), c(u)
        pose = {'Hips': (-76, 7 * a, 0), 'Torso': (-3, 6 * a, 0), 'Head': (62, 0, 0)}
        for sign, name in ((1, 'L'), (-1, 'R')):
            pose[f'Arm {name}'] = (132 + 48 * sign * a, 0, -sign * 9)
            pose[f'Forearm {name}'] = (14 + 34 * up(sign * b), 0, 0)
            pose[f'Leg {name}'] = (5 + 15 * sign * s(u, 2), 0, 0)
            pose[f'Shin {name}'] = (-10 - 9 * up(-sign * s(u, 2)), 0, 0)
            pose[f'Foot {name}'] = (-26, 0, 0)
        return pose

    def fly(u):
        pose = {'Hips': (-30, 0, 2 * s(u)), 'Torso': (-3, 0, 0), 'Head': (26, 0, 0)}
        for sign, name in SIDES:
            pose[f'Arm {name}'] = (-8, 0, sign * (70 + 5 * s(u)))
            pose[f'Forearm {name}'] = (8 + 3 * s(u), 0, 0)
        for name, angle in (('L', -6), ('R', -13)):
            pose[f'Leg {name}'] = (angle + 2 * s(u), 0, 0)
            pose[f'Shin {name}'] = (-14 - 3 * s(u, 1, .2), 0, 0)
            pose[f'Foot {name}'] = (-22, 0, 0)
        return pose

    jump = [(0, {'Torso': (2, 0, 0)}), (.6, {
        'Torso': (-7, 0, 0), 'Head': (6, 0, 0),
        'Leg L': (58, 0, 0), 'Shin L': (-76, 0, 0), 'Leg R': (-10, 0, 0), 'Shin R': (-22, 0, 0), 'Foot R': (-14, 0, 0),
        'Arm L': (-30, 0, -20), 'Forearm L': (34, 0, 0), 'Arm R': (42, 0, 12), 'Forearm R': (52, 0, 0),
    })]
    jump.append((1, jump[1][1]))

    def land(depth):
        pose = {'Hips': ((-10 * depth, 0, 0), (0, -.13 * depth, 0)), 'Torso': (-18 * depth, 0, 0), 'Head': (20 * depth, 0, 0)}
        for sign, name in SIDES:
            pose[f'Leg {name}'] = (46 * depth, 0, sign * 3 * depth)
            pose[f'Shin {name}'] = (-72 * depth, 0, 0)
            pose[f'Foot {name}'] = (26 * depth, 0, 0)
            pose[f'Arm {name}'] = (-12 * depth, 0, sign * 24 * depth)
            pose[f'Forearm {name}'] = (28 * depth, 0, 0)
        return pose

    def sit():
        pose = {}
        for sign, name in SIDES:
            pose[f'Leg {name}'] = (84, 0, sign * 4)
            pose[f'Shin {name}'] = (-62, 0, 0)
            pose[f'Foot {name}'] = (-10, 0, 0)
            pose[f'Arm {name}'] = (32, 0, sign * 5)
            pose[f'Forearm {name}'] = (40, 0, 0)
        return pose

    hold = {'Arm R': (26, 0, 3), 'Forearm R': (44, 0, 0)}
    block = {
        'Arm R': (60, 14, 4), 'Forearm R': (64, 0, 0), 'Hand R': (-8, 0, 0),
        'Arm L': (56, -24, -6), 'Forearm L': (78, 0, 0), 'Torso': (-5, 6, 0), 'Head': (3, 0, 0),
    }

    def blow(arm, fore, hand, torso, head, other, other_fore):
        return {'Arm R': arm, 'Forearm R': fore, 'Hand R': hand, 'Torso': torso, 'Head': head, 'Arm L': other, 'Forearm L': other_fore}

    attack = [
        (0, blow((26, 0, 3), (44, 0, 0), (0, 0, 0), (0, 0, 0), (0, 0, 0), (0, 0, -4), (12, 0, 0))),
        (.26, blow((152, 0, 18), (58, 0, 0), (22, 0, 0), (5, -24, 0), (0, 16, 0), (-22, 0, -14), (26, 0, 0))),
        (.44, blow((66, 20, 4), (12, 0, 0), (-16, 0, 0), (-14, 28, 0), (6, -18, 0), (34, 0, -8), (46, 0, 0))),
        (.64, blow((44, 12, 3), (26, 0, 0), (-6, 0, 0), (-9, 16, 0), (4, -10, 0), (20, 0, -6), (32, 0, 0))),
        (1, blow((26, 0, 3), (44, 0, 0), (0, 0, 0), (0, 0, 0), (0, 0, 0), (0, 0, -4), (12, 0, 0))),
    ]
    use = [
        (0, {'Arm R': (26, 0, 3), 'Forearm R': (44, 0, 0), 'Torso': (0, 0, 0)}),
        (.38, {'Arm R': (82, 8, 3), 'Forearm R': (8, 0, 0), 'Torso': (-5, 10, 0)}),
        (1, {'Arm R': (26, 0, 3), 'Forearm R': (44, 0, 0), 'Torso': (0, 0, 0)}),
    ]
    return [
        Clip('Idle', 4.0, True, LOOPING['Idle'], idle),
        Clip('Walk', 1.3, True, LOOPING['Walk'], walk),
        Clip('Run', 1.6, True, LOOPING['Run'], run),
        Clip.keys('Jump', .35, False, jump),
        Clip('Fall', .9, True, LOOPING['Fall'], fall),
        Clip.keys('Land', .3, False, [(0, land(0)), (.3, land(1)), (1, land(0))]),
        Clip('Swim', 1.8, True, LOOPING['Swim'], swim),
        Clip('Fly', 2.0, True, LOOPING['Fly'], fly),
        Clip.keys('Sit', 1.0, True, [(0, sit())]),
        Clip.keys('Hold', 1.0, True, [(0, hold)]),
        Clip.keys('Block', 1.0, True, [(0, block)]),
        Clip.keys('Attack', .42, False, attack),
        Clip.keys('Use', .26, False, use),
    ]


def hand():
    r = Rig()
    r.box('Arm', None, (.15, .15, .52), BODY, hinge=(0, 0, -.5))
    r.box('Sleeve', 'Arm', (.17, .17, .1), SHADE, at=(0, 0, .28))
    r.box('Cuff', 'Arm', (.175, .175, .05), DARK, at=(0, 0, -.1))
    r.box('Hand', 'Arm', (.17, .12, .17), SKIN, at=(0, 0, -.13), hinge=(0, 0, 1))
    r.box('Thumb', 'Hand', (.05, .06, .11), SKIN, at=(-.105, .01, -.07), rot=(0, -18, 0))
    for index in range(4):
        r.box(f'Finger {index + 1}', 'Hand', (.036, .055, .085), SKIN, at=(-.063 + index * .042, -.012, -.205), rot=(-24, 0, 0))
    return r


def hand_clips():
    def still(arm, wrist=(0, 0, 0)):
        return {'Arm': arm, 'Hand': wrist}

    def idle(u):
        return {'Arm': ((.9 * s(u), .5 * s(u, 1, .2), 0), (0, -.005 * c(u), 0))}

    def step(amount, turn):
        def pose(u):
            return {'Arm': ((turn * c(u, 2), turn * .7 * s(u), turn * .5 * s(u)), (amount * s(u), -amount * .7 * c(u, 2), amount * .4 * c(u, 2)))}
        return pose

    def swim(u):
        return {'Arm': ((-8 + 7 * s(u), 14 * s(u), -6 * s(u)), (-.08 - .1 * s(u), .03, -.07 - .05 * c(u)))}

    def fall(u):
        return {'Arm': ((13 + 2 * s(u), 0, -5), (.01, .05 + .006 * s(u), 0))}

    jump = [(0, {'Arm': ((0, 0, 0), (0, 0, 0))}), (1, {'Arm': ((10, 0, -3), (0, .035, 0))})]
    land = [(0, {'Arm': ((0, 0, 0), (0, 0, 0))}), (.3, {'Arm': ((-9, 0, 0), (0, -.07, 0))}), (1, {'Arm': ((0, 0, 0), (0, 0, 0))})]
    block = {'Arm': ((16, 36, -14), (-.16, .08, .06)), 'Hand': (-8, 0, 0)}

    def blow(arm, wrist):
        return {'Arm': arm, 'Hand': wrist}

    attack = [
        (0, blow(((0, 0, 0), (0, 0, 0)), (0, 0, 0))),
        (.24, blow(((30, -10, 8), (.05, .11, .11)), (16, 0, 0))),
        (.46, blow(((-44, 36, -10), (-.15, -.13, -.23)), (-22, 0, 0))),
        (.68, blow(((-20, 18, -4), (-.08, -.06, -.1)), (-8, 0, 0))),
        (1, blow(((0, 0, 0), (0, 0, 0)), (0, 0, 0))),
    ]
    use = [
        (0, blow(((0, 0, 0), (0, 0, 0)), (0, 0, 0))),
        (.36, blow(((-14, 7, 0), (-.01, -.03, -.17)), (-12, 0, 0))),
        (1, blow(((0, 0, 0), (0, 0, 0)), (0, 0, 0))),
    ]
    return [
        Clip('Idle', 4.0, True, 8, idle),
        Clip('Walk', 1.3, True, 12, step(.012, 1.2)),
        Clip('Run', 1.6, True, 12, step(.026, 2.8)),
        Clip.keys('Jump', .35, False, jump),
        Clip('Fall', .9, True, 8, fall),
        Clip.keys('Land', .3, False, land),
        Clip('Swim', 1.8, True, 12, swim),
        Clip.keys('Block', 1.0, True, [(0, block)]),
        Clip.keys('Attack', .42, False, attack),
        Clip.keys('Use', .26, False, use),
    ]


if __name__ == '__main__':
    write('Player', body(), [('Hand', 'Hand R', (0, -.13, -.02), (-90, 0, 0))], body_clips())
    write('Player_Hand', hand(), [('Hand', 'Hand', (-.01, .06, -.05), (-76.5, 0, 8.6))], hand_clips())
    print('player rigs written')
