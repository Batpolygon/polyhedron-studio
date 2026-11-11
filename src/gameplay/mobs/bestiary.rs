//! The entities of the game (spec "Entitees"), built as ordinary rigs:
//! they are written to `project/entities/mobs` when missing and stay fully
//! editable in the Mobs workspace (model, clips, behaviour, biomes, drops).

use crate::gameplay::mobs::model::{ActionClip, Interpolation, Keyframe, MobBehavior, MobBox, MobRig, SpawnTime};

/// Body plans of the generated models.
#[derive(Clone, Copy)]
enum Plan {
    /// Body, head and four legs.
    Quad,
    /// Body, head, two arms and two legs.
    Biped,
    /// A round body with eyes (hops).
    Blob,
}

struct Spec {
    name: &'static str,
    description: &'static str,
    plan: Plan,
    /// Body size (x, y, z) in blocks.
    size: [f32; 3],
    body: [u8; 3],
    detail: [u8; 3],
    behavior: MobBehavior,
    hostile: bool,
    health: f32,
    speed: f32,
    biomes: &'static [&'static str],
    time: SpawnTime,
    drops: &'static [(&'static str, u32)],
    ethereal: f32,
}

fn rgb(color: [u8; 3]) -> [f32; 3] {
    color.map(|c| c as f32 / 255.0)
}

fn key(time: f32, rotation: [f32; 3]) -> Keyframe {
    Keyframe::new(time).with_rotation(rotation).with_interpolation(Interpolation::Auto)
}

fn swing(clip: &mut ActionClip, bone: &str, amplitude: f32, phase: bool, duration: f32) {
    let sign = if phase { 1.0 } else { -1.0 };
    let track = clip.ensure_track(bone);
    track.upsert(key(0.0, [amplitude * sign, 0.0, 0.0]));
    track.upsert(key(duration * 0.5, [-amplitude * sign, 0.0, 0.0]));
    track.upsert(key(duration, [amplitude * sign, 0.0, 0.0]));
}

fn build(spec: &Spec) -> MobRig {
    let mut rig = MobRig::new(spec.name);
    rig.description = spec.description.to_string();
    rig.health = spec.health;
    rig.hostile = spec.hostile;
    rig.behavior = spec.behavior;
    rig.script = None;
    rig.spawn_count = 0;
    rig.speed = spec.speed;
    rig.biomes = spec.biomes.iter().map(|b| b.to_string()).collect();
    rig.spawn_time = spec.time;
    rig.drops = spec.drops.iter().map(|(r, n)| (r.to_string(), *n)).collect();
    rig.ethereal = spec.ethereal;
    rig.loot = Some("faunal".to_string());
    let [sx, sy, sz] = spec.size;
    let body_color = rgb(spec.body);
    let detail = rgb(spec.detail);
    let eye = [0.95, 0.95, 0.9];

    let mut body = MobBox::new("Body", None);
    body.size = spec.size;
    body.color = body_color;
    rig.boxes.push(body);

    let mut walk = ActionClip::new("Walk");
    walk.duration = 0.6;
    walk.looping = true;
    let mut idle = ActionClip::new("Idle");
    idle.duration = 2.0;
    idle.looping = true;
    let mut attack = ActionClip::new("Attack");
    attack.duration = 0.4;
    attack.looping = false;

    match spec.plan {
        Plan::Quad | Plan::Biped => {
            let head_size = if matches!(spec.plan, Plan::Quad) { [sx * 0.7, sy * 0.8, sx * 0.7] } else { [sx * 0.8, sx * 0.8, sx * 0.8] };
            let mut head = MobBox::new("Head", Some("Body".to_string()));
            head.size = head_size;
            head.color = detail;
            head.pivot = [0.0, -head_size[1] * 0.4, 0.0];
            head.offset = match spec.plan {
                Plan::Quad => [0.0, sy * 0.45, -sz * 0.5 - head_size[2] * 0.3],
                _ => [0.0, sy * 0.5 + head_size[1] * 0.4, 0.0],
            };
            rig.boxes.push(head);
            for (side, x) in [("L", -1.0), ("R", 1.0)] {
                let mut eye_box = MobBox::new(format!("Eye {side}"), Some("Head".to_string()));
                eye_box.size = [head_size[0] * 0.18, head_size[1] * 0.18, 0.05];
                eye_box.offset = [x * head_size[0] * 0.22, head_size[1] * 0.45, -head_size[2] * 0.5];
                eye_box.color = eye;
                rig.boxes.push(eye_box);
            }
            let leg_height = match spec.plan {
                Plan::Quad => sy * 0.9,
                _ => sy * 0.8,
            };
            let legs: Vec<(String, [f32; 3])> = match spec.plan {
                Plan::Quad => vec![
                    ("Leg FL".to_string(), [-sx * 0.3, -sy * 0.5, -sz * 0.35]),
                    ("Leg FR".to_string(), [sx * 0.3, -sy * 0.5, -sz * 0.35]),
                    ("Leg BL".to_string(), [-sx * 0.3, -sy * 0.5, sz * 0.35]),
                    ("Leg BR".to_string(), [sx * 0.3, -sy * 0.5, sz * 0.35]),
                ],
                _ => vec![("Leg L".to_string(), [-sx * 0.25, -sy * 0.5, 0.0]), ("Leg R".to_string(), [sx * 0.25, -sy * 0.5, 0.0])],
            };
            for (index, (name, offset)) in legs.iter().enumerate() {
                let mut leg = MobBox::new(name.clone(), Some("Body".to_string()));
                leg.size = [sx * 0.24, leg_height, sx * 0.24];
                leg.pivot = [0.0, leg_height * 0.5, 0.0];
                leg.offset = *offset;
                leg.color = detail;
                rig.boxes.push(leg);
                swing(&mut walk, name, 30.0, index % 2 == 0, 0.6);
            }
            if matches!(spec.plan, Plan::Quad) {
                // Diagonal pairs move together.
                walk.tracks.retain(|track| !track.bone.starts_with("Leg"));
                swing(&mut walk, "Leg FL", 30.0, true, 0.6);
                swing(&mut walk, "Leg BR", 30.0, true, 0.6);
                swing(&mut walk, "Leg FR", 30.0, false, 0.6);
                swing(&mut walk, "Leg BL", 30.0, false, 0.6);
            }
            if matches!(spec.plan, Plan::Biped) {
                for (side, x, phase) in [("L", -1.0, false), ("R", 1.0, true)] {
                    let name = format!("Arm {side}");
                    let mut arm = MobBox::new(name.clone(), Some("Body".to_string()));
                    arm.size = [sx * 0.22, sy * 0.85, sx * 0.22];
                    arm.pivot = [0.0, sy * 0.38, 0.0];
                    arm.offset = [x * (sx * 0.5 + sx * 0.12), sy * 0.3, 0.0];
                    arm.color = body_color;
                    rig.boxes.push(arm);
                    swing(&mut walk, &name, 25.0, phase, 0.6);
                    let track = attack.ensure_track(&name);
                    track.upsert(key(0.0, [0.0, 0.0, 0.0]));
                    track.upsert(key(0.15, [-80.0, 0.0, 0.0]));
                    track.upsert(key(0.4, [0.0, 0.0, 0.0]));
                }
            }
            let track = attack.ensure_track("Head");
            track.upsert(key(0.0, [0.0, 0.0, 0.0]));
            track.upsert(key(0.15, [-25.0, 0.0, 0.0]));
            track.upsert(key(0.4, [0.0, 0.0, 0.0]));
        }
        Plan::Blob => {
            for (side, x) in [("L", -1.0), ("R", 1.0)] {
                let mut eye_box = MobBox::new(format!("Eye {side}"), Some("Body".to_string()));
                eye_box.size = [sx * 0.16, sy * 0.2, 0.05];
                eye_box.offset = [x * sx * 0.2, sy * 0.15, -sz * 0.5];
                eye_box.color = eye;
                rig.boxes.push(eye_box);
            }
            let mut crest = MobBox::new("Crest", Some("Body".to_string()));
            crest.size = [sx * 0.4, sy * 0.25, sz * 0.4];
            crest.offset = [0.0, sy * 0.6, 0.0];
            crest.color = detail;
            rig.boxes.push(crest);
            let track = walk.ensure_track("Body");
            track.upsert(Keyframe::new(0.0).with_translation([0.0, 0.0, 0.0]).with_scale([1.0, 1.0, 1.0]));
            track.upsert(Keyframe::new(0.3).with_translation([0.0, sy * 0.3, 0.0]).with_scale([0.9, 1.15, 0.9]));
            track.upsert(Keyframe::new(0.6).with_translation([0.0, 0.0, 0.0]).with_scale([1.0, 1.0, 1.0]));
            let track = attack.ensure_track("Body");
            track.upsert(Keyframe::new(0.0).with_scale([1.0, 1.0, 1.0]));
            track.upsert(Keyframe::new(0.15).with_scale([1.25, 0.8, 1.25]));
            track.upsert(Keyframe::new(0.4).with_scale([1.0, 1.0, 1.0]));
        }
    }
    let track = idle.ensure_track("Body");
    track.upsert(Keyframe::new(0.0).with_translation([0.0, 0.0, 0.0]));
    track.upsert(Keyframe::new(1.0).with_translation([0.0, sy * 0.04, 0.0]));
    track.upsert(Keyframe::new(2.0).with_translation([0.0, 0.0, 0.0]));
    rig.clips = vec![idle, walk, attack];
    rig.default_clip = "Idle".to_string();
    rig
}

const FAUNA: &[(&str, u32)] = &[("faunal", 1), ("ethereal", 1)];

/// Every entity of the spec, ready to save.
pub fn spec_rigs() -> Vec<MobRig> {
    use MobBehavior as B;
    use Plan::{Biped, Blob, Quad};
    use SpawnTime::{Any, Night};
    let specs = [
        Spec { name: "Gim-Gim", description: "Hits you if you hit it; defends the entities being hit (attacks the aggressor).", plan: Biped, size: [0.5, 0.5, 0.35], body: [90, 200, 110], detail: [60, 150, 80], behavior: B::Retaliate, hostile: false, health: 10.0, speed: 1.0, biomes: &["Plains", "Forest", "Great Forest"], time: Any, drops: FAUNA, ethereal: 3.0 },
        Spec { name: "Rockle", description: "Aggressive, attacks in packs.", plan: Quad, size: [0.6, 0.45, 0.7], body: [120, 124, 136], detail: [90, 92, 104], behavior: B::Pack, hostile: true, health: 8.0, speed: 1.1, biomes: &["Mountain"], time: Any, drops: &[("faunal", 1), ("mineral", 2)], ethereal: 2.0 },
        Spec { name: "Gardener", description: "Aggressive, protects its territory; leaves you alone outside of it.", plan: Biped, size: [0.7, 0.9, 0.45], body: [110, 80, 50], detail: [50, 140, 60], behavior: B::Territorial, hostile: true, health: 16.0, speed: 0.9, biomes: &["Pine Forest"], time: Any, drops: &[("faunal", 1), ("floral", 3), ("ethereal", 1)], ethereal: 4.0 },
        Spec { name: "Miby", description: "Fearful: flees whatever touches it, then hides.", plan: Blob, size: [0.4, 0.35, 0.4], body: [250, 200, 220], detail: [240, 140, 180], behavior: B::Fearful, hostile: false, health: 4.0, speed: 1.3, biomes: &["Plains", "Forest", "Swamp"], time: Any, drops: FAUNA, ethereal: 2.0 },
        Spec { name: "Wise", description: "Harmless but very tough; only flees when its health is low.", plan: Quad, size: [1.2, 1.0, 1.5], body: [200, 170, 120], detail: [150, 120, 80], behavior: B::Tank, hostile: false, health: 40.0, speed: 0.6, biomes: &["Desert", "Oasis"], time: Any, drops: &[("faunal", 3), ("ethereal", 2)], ethereal: 8.0 },
        Spec { name: "Galopin", description: "Comes close discreetly to socialise, scared when hit. Bind it to ride it like a horse (E to mount, Jump to get off).", plan: Quad, size: [0.7, 0.75, 1.3], body: [180, 120, 70], detail: [120, 80, 40], behavior: B::Social, hostile: false, health: 14.0, speed: 1.2, biomes: &["Plains", "Forest"], time: Any, drops: FAUNA, ethereal: 3.0 },
        Spec { name: "Sulfy", description: "Aggressive, shoots laser projectiles.", plan: Blob, size: [0.6, 0.6, 0.6], body: [240, 210, 60], detail: [250, 120, 30], behavior: B::Shooter, hostile: true, health: 10.0, speed: 1.0, biomes: &["Volcano"], time: Any, drops: &[("faunal", 1), ("crystalline", 1), ("ethereal", 1)], ethereal: 3.0 },
        Spec { name: "Cronil", description: "Hides and leaps at the player to attack.", plan: Quad, size: [0.7, 0.35, 0.9], body: [96, 74, 50], detail: [70, 52, 34], behavior: B::Ambush, hostile: true, health: 10.0, speed: 1.0, biomes: &["Mud"], time: Any, drops: FAUNA, ethereal: 3.0 },
        Spec { name: "Squipter", description: "Passive unless attacked; aggressive toward every small entity.", plan: Biped, size: [0.6, 0.7, 0.4], body: [70, 120, 200], detail: [40, 80, 150], behavior: B::Bully, hostile: false, health: 12.0, speed: 1.1, biomes: &["Plains", "Forest"], time: Any, drops: FAUNA, ethereal: 3.0 },
        Spec { name: "Grom", description: "Gathers resources and places pure resource blocks to build its nest, breeds, explodes when it dies.", plan: Quad, size: [0.8, 0.6, 0.9], body: [130, 150, 90], detail: [200, 180, 80], behavior: B::Nester, hostile: false, health: 12.0, speed: 0.9, biomes: &["Forest", "Desert"], time: Any, drops: &[("faunal", 2), ("mineral", 2), ("ethereal", 1)], ethereal: 3.0 },
        Spec { name: "Tolossus", description: "Afraid of the Sulfy; when hit or afraid it strikes its target and flees.", plan: Biped, size: [1.1, 1.3, 0.7], body: [150, 110, 160], detail: [110, 80, 120], behavior: B::HitAndRun, hostile: false, health: 24.0, speed: 0.9, biomes: &["Plains"], time: Any, drops: &[("faunal", 2), ("ethereal", 2)], ethereal: 6.0 },
        Spec { name: "Niff", description: "Senses movement: walking makes it come gently, running or breaking / placing blocks makes it charge and strike with a knockback.", plan: Quad, size: [0.7, 0.6, 0.9], body: [200, 120, 80], detail: [150, 80, 50], behavior: B::Noise, hostile: false, health: 14.0, speed: 1.0, biomes: &["Canyon"], time: Any, drops: FAUNA, ethereal: 3.0 },
        Spec { name: "Gelanto", description: "Throws far away the entities that try to attack it.", plan: Blob, size: [0.9, 0.8, 0.9], body: [170, 220, 250], detail: [230, 245, 255], behavior: B::Repel, hostile: false, health: 18.0, speed: 0.7, biomes: &["Frozen Lake", "Ice Floe"], time: Any, drops: &[("faunal", 1), ("crystalline", 1), ("ethereal", 1)], ethereal: 4.0 },
        Spec { name: "Leafer", description: "Jumps and strikes entities from behind; flees when hit.", plan: Biped, size: [0.45, 0.55, 0.3], body: [70, 170, 60], detail: [40, 120, 40], behavior: B::Backstab, hostile: true, health: 8.0, speed: 1.3, biomes: &["Forest", "Great Forest", "Plains"], time: Any, drops: &[("faunal", 1), ("floral", 2), ("ethereal", 1)], ethereal: 2.0 },
        Spec { name: "Arna", description: "Attacks when hit or walked on.", plan: Quad, size: [0.9, 0.35, 0.9], body: [60, 50, 60], detail: [160, 40, 40], behavior: B::Stepped, hostile: false, health: 12.0, speed: 1.1, biomes: &["Great Forest"], time: Any, drops: FAUNA, ethereal: 3.0 },
        Spec { name: "Notturo", description: "Aggressive: attacks entities. Spawns in every biome at night.", plan: Biped, size: [0.6, 0.9, 0.4], body: [30, 30, 50], detail: [200, 40, 60], behavior: B::Hunter, hostile: true, health: 14.0, speed: 1.1, biomes: &["*"], time: Night, drops: &[("faunal", 1), ("ethereal", 2)], ethereal: 5.0 },
        Spec { name: "Vorator", description: "Slow and almost harmless by day, aggressive toward every entity at night; strikes back if attacked by day.", plan: Quad, size: [1.0, 0.8, 1.2], body: [90, 60, 130], detail: [255, 236, 150], behavior: B::Nocturnal, hostile: false, health: 22.0, speed: 0.8, biomes: &["Crater"], time: Any, drops: &[("faunal", 2), ("astral", 1), ("ethereal", 2)], ethereal: 6.0 },
        Spec { name: "Clanger", description: "Attracts the entities around it with a ringing sound, then attacks those close to it.", plan: Biped, size: [0.7, 0.9, 0.5], body: [200, 170, 60], detail: [120, 100, 40], behavior: B::Caller, hostile: true, health: 14.0, speed: 0.8, biomes: &["Plains", "Mud"], time: Night, drops: &[("faunal", 1), ("ethereal", 2)], ethereal: 4.0 },
        Spec { name: "Sarcophager", description: "Aggressive: attacks the player. Spawns near the pyramids and temples.", plan: Biped, size: [0.7, 1.0, 0.5], body: [200, 180, 130], detail: [80, 60, 40], behavior: B::PlayerHunter, hostile: true, health: 18.0, speed: 0.9, biomes: &["Pyramid and Temple"], time: Any, drops: &[("faunal", 1), ("ethereal", 2), ("mineral", 2)], ethereal: 5.0 },
        Spec { name: "MnemoEater", description: "Aggressive: attacks everyone. Spawns in the Mnemo Plains.", plan: Quad, size: [1.0, 0.8, 1.2], body: [168, 60, 76], detail: [80, 28, 40], behavior: B::Berserk, hostile: true, health: 24.0, speed: 1.1, biomes: &["Mnemo Plain"], time: Any, drops: &[("faunal", 1), ("mnemonical", 1), ("ethereal", 2)], ethereal: 6.0 },
        Spec { name: "MnemoWatcher", description: "Passive: does nothing, never moves, only watches the entities; ignores hits.", plan: Biped, size: [0.6, 1.4, 0.4], body: [160, 58, 76], detail: [255, 255, 255], behavior: B::Watcher, hostile: false, health: 30.0, speed: 0.0, biomes: &["Mnemo Plain"], time: Any, drops: &[("mnemonical", 1), ("ethereal", 3)], ethereal: 8.0 },
    ];
    specs
        .iter()
        .map(|spec| {
            let mut rig = build(spec);
            if spec.name == "Galopin" {
                rig.rideable = true;
            }
            if spec.name == "Grom" {
                rig.explodes = true;
            }
            if spec.name == "Tolossus" {
                rig.fears = vec!["Sulfy".to_string()];
            }
            if matches!(spec.behavior, MobBehavior::Watcher | MobBehavior::Repel) {
                rig.knockback_resistance = 1.0;
            }
            rig
        })
        .collect()
}

/// Rigs created as examples by earlier versions (removed on load).
pub const RETIRED_RIGS: [&str; 1] = ["Sentinel Drone"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_spec_entity_exists_with_a_biome_and_clips() {
        let rigs = spec_rigs();
        assert_eq!(rigs.len(), 21);
        for rig in &rigs {
            assert!(!rig.biomes.is_empty(), "{} has no biome", rig.name);
            assert!(rig.clip("Walk").is_some() && rig.clip("Attack").is_some());
            assert_ne!(rig.behavior, MobBehavior::Script);
            let text = ron::ser::to_string(rig).expect("serialize");
            let back: MobRig = ron::from_str(&text).expect("parse");
            assert_eq!(&back, rig);
        }
        assert!(rigs.iter().any(|rig| rig.name == "Galopin" && rig.rideable));
        assert!(rigs.iter().any(|rig| rig.name == "Grom" && rig.explodes));
    }
}
