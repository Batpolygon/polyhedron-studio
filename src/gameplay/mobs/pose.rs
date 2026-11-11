//! Pose evaluation: keyframe interpolation and hierarchy composition.
//!
//! Sampling is pure: given a rig, a clip and a time it returns a pose. The
//! viewport and the future runtime both consume the same function, so what the
//! animator sees is exactly what the game will play.

use std::collections::BTreeMap;

use bevy::math::{EulerRot, Mat4, Quat, Vec3};

use crate::gameplay::mobs::model::{ActionClip, BoneTrack, Interpolation, Keyframe, MobRig};

/// Local transform of one bone at a given time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BonePose {
    pub translation: Vec3,
    pub rotation: Vec3,
    pub scale: Vec3,
}

impl Default for BonePose {
    fn default() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
        }
    }
}

impl BonePose {
    pub fn local_matrix(&self) -> Mat4 {
        let rotation = Quat::from_euler(
            EulerRot::XYZ,
            self.rotation.x.to_radians(),
            self.rotation.y.to_radians(),
            self.rotation.z.to_radians(),
        );
        Mat4::from_scale_rotation_translation(self.scale.max(Vec3::splat(1e-4)), rotation, self.translation)
    }
}

/// Sampled pose of a whole rig.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RigPose {
    pub bones: BTreeMap<String, BonePose>,
}

impl RigPose {
    pub fn get(&self, bone: &str) -> BonePose {
        self.bones.get(bone).copied().unwrap_or_default()
    }

    /// Blend-tree step: interpolate every bone from `self` to `other`.
    /// Bones missing on one side use the rest pose.
    pub fn blend(&self, other: &RigPose, weight: f32) -> RigPose {
        let weight = weight.clamp(0.0, 1.0);
        let mut bones = BTreeMap::new();
        for name in self.bones.keys().chain(other.bones.keys()) {
            if bones.contains_key(name) {
                continue;
            }
            let a = self.get(name);
            let b = other.get(name);
            bones.insert(
                name.clone(),
                BonePose {
                    translation: a.translation.lerp(b.translation, weight),
                    rotation: a.rotation.lerp(b.rotation, weight),
                    scale: a.scale.lerp(b.scale, weight),
                },
            );
        }
        RigPose { bones }
    }
}

/// World transform of one drawn box.
#[derive(Clone, Debug)]
pub struct DrawnBone {
    pub name: String,
    pub center: Vec3,
    pub rotation: Quat,
    pub size: Vec3,
    pub color: [f32; 3],
}

/// Evaluate `clip` at `time` and return the local pose of every animated bone.
pub fn sample_clip(rig: &MobRig, clip: &ActionClip, time: f32) -> RigPose {
    let mut pose = RigPose::default();

    for bone in &rig.boxes {
        let bone_pose = match clip.track(&bone.name) {
            Some(track) if !track.keys.is_empty() => sample_track(track, time),
            _ => BonePose::default(),
        };
        pose.bones.insert(bone.name.clone(), bone_pose);
    }

    pose
}

/// Evaluate a single track.
pub fn sample_track(track: &BoneTrack, time: f32) -> BonePose {
    let keys = &track.keys;
    if keys.is_empty() {
        return BonePose::default();
    }
    if keys.len() == 1 {
        return key_pose(&keys[0]);
    }

    let time = time.max(0.0);
    if time <= keys[0].time {
        return key_pose(&keys[0]);
    }
    if time >= keys[keys.len() - 1].time {
        return key_pose(&keys[keys.len() - 1]);
    }

    let mut segment = 0;
    for index in 0..keys.len() - 1 {
        if time >= keys[index].time && time <= keys[index + 1].time {
            segment = index;
            break;
        }
    }

    let start = &keys[segment];
    let end = &keys[segment + 1];
    let span = (end.time - start.time).max(1e-5);
    let raw = ((time - start.time) / span).clamp(0.0, 1.0);
    let before = if segment > 0 {
        Some(&keys[segment - 1])
    } else {
        None
    };
    let after = keys.get(segment + 2);

    let start_pose = key_pose(start);
    let end_pose = key_pose(end);

    match start.interpolation {
        Interpolation::Step => start_pose,
        Interpolation::Linear => lerp_pose(start_pose, end_pose, raw),
        Interpolation::Bezier => {
            lerp_pose(start_pose, end_pose, ease_in_out(raw))
        }
        Interpolation::Auto => {
            let before_pose = before.map(key_pose).unwrap_or(start_pose);
            let after_pose = after.map(key_pose).unwrap_or(end_pose);
            catmull_rom(start_pose, end_pose, before_pose, after_pose, raw)
        }
    }
}

fn key_pose(key: &Keyframe) -> BonePose {
    BonePose {
        translation: Vec3::from_array(key.translation),
        rotation: Vec3::from_array(key.rotation),
        scale: Vec3::from_array(key.scale),
    }
}

fn lerp_pose(left: BonePose, right: BonePose, t: f32) -> BonePose {
    BonePose {
        translation: left.translation.lerp(right.translation, t),
        rotation: Vec3::new(
            lerp_angle(left.rotation.x, right.rotation.x, t),
            lerp_angle(left.rotation.y, right.rotation.y, t),
            lerp_angle(left.rotation.z, right.rotation.z, t),
        ),
        scale: left.scale.lerp(right.scale, t),
    }
}

/// Cubic ease-in/ease-out used by [`Interpolation::Bezier`].
fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        let inverse = -2.0 * t + 2.0;
        1.0 - inverse * inverse * inverse / 2.0
    }
}

/// Uniform Catmull-Rom through four poses.
fn catmull_rom(
    start: BonePose,
    end: BonePose,
    before: BonePose,
    after: BonePose,
    t: f32,
) -> BonePose {
    BonePose {
        translation: catmull_vec3(
            before.translation,
            start.translation,
            end.translation,
            after.translation,
            t,
        ),
        rotation: Vec3::new(
            catmull_scalar(
                before.rotation.x,
                start.rotation.x,
                end.rotation.x,
                after.rotation.x,
                t,
            ),
            catmull_scalar(
                before.rotation.y,
                start.rotation.y,
                end.rotation.y,
                after.rotation.y,
                t,
            ),
            catmull_scalar(
                before.rotation.z,
                start.rotation.z,
                end.rotation.z,
                after.rotation.z,
                t,
            ),
        ),
        scale: catmull_vec3(before.scale, start.scale, end.scale, after.scale, t),
    }
}

fn catmull_vec3(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    Vec3::new(
        catmull_scalar(p0.x, p1.x, p2.x, p3.x, t),
        catmull_scalar(p0.y, p1.y, p2.y, p3.y, t),
        catmull_scalar(p0.z, p1.z, p2.z, p3.z, t),
    )
}

fn catmull_scalar(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    let m1 = (p2 - p0) * 0.5;
    let m2 = (p3 - p1) * 0.5;

    (2.0 * t3 - 3.0 * t2 + 1.0) * p1
        + (t3 - 2.0 * t2 + t) * m1
        + (-2.0 * t3 + 3.0 * t2) * p2
        + (t3 - t2) * m2
}

/// Interpolate an angle along the shortest arc.
fn lerp_angle(from: f32, to: f32, t: f32) -> f32 {
    let delta = wrap_degrees(to - from);
    from + delta * t
}

fn wrap_degrees(angle: f32) -> f32 {
    let mut wrapped = angle % 360.0;
    if wrapped > 180.0 {
        wrapped -= 360.0;
    } else if wrapped < -180.0 {
        wrapped += 360.0;
    }
    wrapped
}

/// Compose local poses into world matrices, parents first.
pub fn world_matrices(rig: &MobRig, pose: &RigPose) -> BTreeMap<String, Mat4> {
    let mut matrices = BTreeMap::new();

    for index in rig.hierarchy_order() {
        let bone = &rig.boxes[index];
        let local = pose.get(&bone.name).local_matrix();

        let rest = Mat4::from_translation(Vec3::from_array(bone.offset))
            * Mat4::from_euler(
                EulerRot::XYZ,
                bone.rotation[0].to_radians(),
                bone.rotation[1].to_radians(),
                bone.rotation[2].to_radians(),
            );

        let parent = bone
            .parent
            .as_deref()
            .and_then(|parent| matrices.get(parent).copied())
            .unwrap_or(Mat4::IDENTITY);

        matrices.insert(bone.name.clone(), parent * rest * local);
    }

    matrices
}

/// Resolve the boxes to draw, in world space.
pub fn drawn_bones(rig: &MobRig, pose: &RigPose) -> Vec<DrawnBone> {
    let matrices = world_matrices(rig, pose);
    let scale = Vec3::from_array(rig.scale).max(Vec3::splat(1e-4));
    let mut drawn = Vec::with_capacity(rig.boxes.len());

    for bone in &rig.boxes {
        if !bone.visible {
            continue;
        }
        let Some(matrix) = matrices.get(&bone.name).copied() else {
            continue;
        };

        let scaled = Mat4::from_scale(scale) * matrix;
        let (world_scale, rotation, translation) = scaled.to_scale_rotation_translation();
        let pivot = Vec3::from_array(bone.pivot);
        let center = translation - rotation * (pivot * world_scale);

        drawn.push(DrawnBone {
            name: bone.name.clone(),
            center,
            rotation,
            size: Vec3::from_array(bone.size).abs().max(Vec3::splat(0.01)) * world_scale.abs(),
            color: bone.color,
        });
    }

    drawn
}

/// World position of every socket, for gizmos and gameplay attachments.
pub fn socket_positions(rig: &MobRig, pose: &RigPose) -> Vec<(String, Vec3)> {
    let matrices = world_matrices(rig, pose);
    let scale = Vec3::from_array(rig.scale).max(Vec3::splat(1e-4));

    rig.sockets
        .iter()
        .filter_map(|socket| {
            let matrix = matrices.get(&socket.bone)?;
            let position = (Mat4::from_scale(scale) * *matrix)
                .transform_point3(Vec3::from_array(socket.offset));
            Some((socket.name.clone(), position))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gameplay::mobs::model::{ActionClip, Keyframe, MobBox, MobRig, Socket};

    fn rig_with_track() -> (MobRig, ActionClip) {
        let mut rig = MobRig::new("test");
        rig.boxes.push(MobBox::new("Root", None));

        let mut clip = ActionClip::new("clip");
        clip.duration = 2.0;
        let track = clip.ensure_track("Root");
        track.upsert(
            Keyframe::new(0.0)
                .with_translation([0.0, 0.0, 0.0])
                .with_interpolation(Interpolation::Linear),
        );
        track.upsert(
            Keyframe::new(2.0)
                .with_translation([0.0, 2.0, 0.0])
                .with_interpolation(Interpolation::Linear),
        );
        (rig, clip)
    }

    #[test]
    fn linear_segment_hits_the_midpoint() {
        let (rig, clip) = rig_with_track();
        let pose = sample_clip(&rig, &clip, 1.0);
        assert!((pose.get("Root").translation.y - 1.0).abs() < 1e-4);
    }

    #[test]
    fn sampling_clamps_outside_the_key_range() {
        let (rig, clip) = rig_with_track();
        assert_eq!(sample_clip(&rig, &clip, -5.0).get("Root").translation.y, 0.0);
        assert_eq!(sample_clip(&rig, &clip, 99.0).get("Root").translation.y, 2.0);
    }

    #[test]
    fn step_holds_the_previous_value() {
        let mut clip = ActionClip::new("step");
        let track = clip.ensure_track("Root");
        track.upsert(
            Keyframe::new(0.0)
                .with_translation([0.0, 0.0, 0.0])
                .with_interpolation(Interpolation::Step),
        );
        track.upsert(
            Keyframe::new(1.0)
                .with_translation([0.0, 4.0, 0.0])
                .with_interpolation(Interpolation::Step),
        );

        let pose = sample_track(&clip.track("Root").unwrap().clone(), 0.99);
        assert_eq!(pose.translation.y, 0.0);
    }

    #[test]
    fn bezier_eases_in_and_out() {
        let mut clip = ActionClip::new("bezier");
        let track = clip.ensure_track("Root");
        track.upsert(
            Keyframe::new(0.0)
                .with_translation([0.0, 0.0, 0.0])
                .with_interpolation(Interpolation::Bezier),
        );
        track.upsert(
            Keyframe::new(1.0)
                .with_translation([0.0, 1.0, 0.0])
                .with_interpolation(Interpolation::Bezier),
        );

        let early = sample_track(clip.track("Root").unwrap(), 0.1).translation.y;
        let middle = sample_track(clip.track("Root").unwrap(), 0.5).translation.y;
        let late = sample_track(clip.track("Root").unwrap(), 0.9).translation.y;

        assert!(early < 0.1);
        assert!((middle - 0.5).abs() < 1e-4);
        assert!(late > 0.9);
    }

    #[test]
    fn auto_interpolation_is_continuous_across_keys() {
        let mut clip = ActionClip::new("auto");
        let track = clip.ensure_track("Root");
        for (time, y) in [(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)] {
            track.upsert(
                Keyframe::new(time)
                    .with_translation([0.0, y, 0.0])
                    .with_interpolation(Interpolation::Auto),
            );
        }

        let before = sample_track(clip.track("Root").unwrap(), 0.999).translation.y;
        let at = sample_track(clip.track("Root").unwrap(), 1.0).translation.y;
        let after = sample_track(clip.track("Root").unwrap(), 1.001).translation.y;

        assert!((before - at).abs() < 0.01);
        assert!((after - at).abs() < 0.01);
        assert!(at > 0.99);
    }

    #[test]
    fn rotation_takes_the_short_way_around() {
        let mut clip = ActionClip::new("spin");
        let track = clip.ensure_track("Root");
        track.upsert(
            Keyframe::new(0.0)
                .with_rotation([0.0, 0.0, 350.0])
                .with_interpolation(Interpolation::Linear),
        );
        track.upsert(
            Keyframe::new(1.0)
                .with_rotation([0.0, 0.0, 10.0])
                .with_interpolation(Interpolation::Linear),
        );

        let middle = sample_track(clip.track("Root").unwrap(), 0.5).rotation.z;
        assert!((middle - 360.0).abs() < 1e-3 || middle.abs() < 1e-3);
    }

    #[test]
    fn children_follow_their_parent_transform() {
        let mut rig = MobRig::new("hierarchy");
        let mut root = MobBox::new("Root", None);
        root.offset = [0.0, 1.0, 0.0];
        rig.boxes.push(root);

        let mut child = MobBox::new("Child", Some("Root".to_string()));
        child.offset = [0.0, 1.0, 0.0];
        rig.boxes.push(child);

        let pose = RigPose::default();
        let drawn = drawn_bones(&rig, &pose);
        let child = drawn
            .iter()
            .find(|bone| (bone.center - Vec3::new(0.0, 2.0, 0.0)).length() < 1e-4);
        assert!(child.is_some());
    }

    #[test]
    fn sockets_resolve_through_the_hierarchy() {
        let mut rig = MobRig::new("socket");
        let mut root = MobBox::new("Root", None);
        root.offset = [0.0, 1.0, 0.0];
        rig.boxes.push(root);
        rig.sockets
            .push(Socket::new("Tip", "Root").with_offset([0.0, 0.5, 0.0]));

        let positions = socket_positions(&rig, &RigPose::default());
        assert_eq!(positions.len(), 1);
        assert!((positions[0].1 - Vec3::new(0.0, 1.5, 0.0)).length() < 1e-4);
    }
}
