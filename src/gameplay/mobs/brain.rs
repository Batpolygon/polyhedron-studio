//! Mob brains: an AI assembled in the editor.
//!
//! A brain is a list of rules, read from the top: the first rule whose
//! conditions all hold runs all its actions. It is the selector of a behaviour
//! tree with sequences for children, and needs no code.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BrainCondition {
    PlayerWithin(f32),
    PlayerBeyond(f32),
    /// Below this share of its health (0 to 1).
    HealthBelow(f32),
    HitByPlayer,
    Angry,
    Afraid,
    Day,
    Night,
    /// At least `count` mobs of its kind within `radius`.
    AlliesAtLeast { count: u32, radius: f32 },
    /// Chance per second.
    Chance(f32),
    /// Its reload is over.
    Ready,
}

impl BrainCondition {
    pub fn palette() -> Vec<(&'static str, Self)> {
        vec![
            ("Player within", Self::PlayerWithin(10.0)),
            ("Player beyond", Self::PlayerBeyond(10.0)),
            ("Health below", Self::HealthBelow(0.3)),
            ("Hit by the player", Self::HitByPlayer),
            ("Angry", Self::Angry),
            ("Afraid", Self::Afraid),
            ("By day", Self::Day),
            ("By night", Self::Night),
            ("Allies around", Self::AlliesAtLeast { count: 2, radius: 8.0 }),
            ("Chance per second", Self::Chance(0.2)),
            ("Reload over", Self::Ready),
        ]
    }

    pub fn label(&self) -> &'static str {
        Self::palette().iter().find(|entry| std::mem::discriminant(&entry.1) == std::mem::discriminant(self)).map_or("", |entry| entry.0)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BrainAction {
    Wander(f32),
    Chase(f32),
    Flee(f32),
    KeepDistance { distance: f32, speed: f32 },
    GoHome(f32),
    FacePlayer,
    Melee(f32),
    Shoot { damage: f32, range: f32, reload: f32 },
    Leap(f32),
    /// Calls the mobs around to this spot.
    CallAllies,
    PlayClip(String),
    /// Sends a Weave message.
    Broadcast(String),
}

impl BrainAction {
    pub fn palette() -> Vec<(&'static str, Self)> {
        vec![
            ("Wander", Self::Wander(1.2)),
            ("Chase the player", Self::Chase(3.0)),
            ("Flee the player", Self::Flee(4.0)),
            ("Keep a distance", Self::KeepDistance { distance: 7.0, speed: 2.6 }),
            ("Go home", Self::GoHome(2.0)),
            ("Face the player", Self::FacePlayer),
            ("Melee strike", Self::Melee(2.0)),
            ("Shoot", Self::Shoot { damage: 1.5, range: 18.0, reload: 2.2 }),
            ("Leap at the player", Self::Leap(8.0)),
            ("Call allies", Self::CallAllies),
            ("Play clip", Self::PlayClip("Attack".to_string())),
            ("Weave message", Self::Broadcast("alert".to_string())),
        ]
    }

    pub fn label(&self) -> &'static str {
        Self::palette().iter().find(|entry| std::mem::discriminant(&entry.1) == std::mem::discriminant(self)).map_or("", |entry| entry.0)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BrainRule {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub when: Vec<BrainCondition>,
    #[serde(default)]
    pub actions: Vec<BrainAction>,
}

/// What a mob knows this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct BrainFacts {
    pub to_player: f32,
    pub health: f32,
    pub hit: bool,
    pub angry: bool,
    pub afraid: bool,
    pub day: bool,
    pub ready: bool,
    /// Random number of this frame, in `[0, 1)`.
    pub roll: f32,
    pub delta: f32,
}

impl BrainFacts {
    pub fn holds(&self, condition: &BrainCondition, allies: &dyn Fn(f32) -> u32) -> bool {
        match condition {
            BrainCondition::PlayerWithin(distance) => self.to_player <= *distance,
            BrainCondition::PlayerBeyond(distance) => self.to_player > *distance,
            BrainCondition::HealthBelow(share) => self.health < *share,
            BrainCondition::HitByPlayer => self.hit,
            BrainCondition::Angry => self.angry,
            BrainCondition::Afraid => self.afraid,
            BrainCondition::Day => self.day,
            BrainCondition::Night => !self.day,
            BrainCondition::AlliesAtLeast { count, radius } => allies(*radius) >= *count,
            BrainCondition::Chance(per_second) => self.roll < per_second * self.delta,
            BrainCondition::Ready => self.ready,
        }
    }

    /// The first rule whose conditions all hold.
    pub fn choose<'a>(&self, rules: &'a [BrainRule], allies: &dyn Fn(f32) -> u32) -> Option<&'a BrainRule> {
        rules.iter().find(|rule| rule.when.iter().all(|condition| self.holds(condition, allies)))
    }
}

fn rule(label: &str, when: Vec<BrainCondition>, actions: Vec<BrainAction>) -> BrainRule {
    BrainRule { label: label.to_string(), when, actions }
}

/// Ready-made brains offered by the editor.
pub fn templates() -> Vec<(&'static str, Vec<BrainRule>)> {
    use BrainAction as A;
    use BrainCondition as C;
    let shot = || A::Shoot { damage: 1.5, range: 18.0, reload: 2.2 };
    vec![
        (
            "Hunter",
            vec![
                rule("Strike", vec![C::PlayerWithin(2.5)], vec![A::FacePlayer, A::Melee(2.0)]),
                rule("Chase", vec![C::PlayerWithin(18.0)], vec![A::Chase(3.2)]),
                rule("Idle", Vec::new(), vec![A::Wander(1.2)]),
            ],
        ),
        (
            "Shooter",
            vec![
                rule("Back off", vec![C::PlayerWithin(7.0)], vec![A::FacePlayer, A::KeepDistance { distance: 7.0, speed: 2.6 }, shot()]),
                rule("Fire", vec![C::PlayerWithin(18.0)], vec![A::FacePlayer, A::Chase(2.6), shot()]),
                rule("Idle", Vec::new(), vec![A::Wander(1.2)]),
            ],
        ),
        (
            "Coward",
            vec![
                rule("Run", vec![C::PlayerWithin(8.0)], vec![A::Flee(4.5)]),
                rule("Idle", Vec::new(), vec![A::Wander(1.0)]),
            ],
        ),
        (
            "Guard",
            vec![
                rule("Hit back", vec![C::Angry, C::PlayerWithin(2.5)], vec![A::FacePlayer, A::Melee(3.0)]),
                rule("Pursue", vec![C::Angry], vec![A::Chase(3.5), A::CallAllies]),
                rule("Back home", vec![C::PlayerBeyond(20.0)], vec![A::GoHome(2.0)]),
                rule("Idle", Vec::new(), vec![A::Wander(0.8)]),
            ],
        ),
        (
            "Pack",
            vec![
                rule("Flee alone", vec![C::HealthBelow(0.3)], vec![A::Flee(4.0)]),
                rule("Strike", vec![C::PlayerWithin(2.5)], vec![A::FacePlayer, A::Melee(1.5)]),
                rule("Rush", vec![C::PlayerWithin(14.0), C::AlliesAtLeast { count: 1, radius: 10.0 }], vec![A::Chase(3.6)]),
                rule("Idle", Vec::new(), vec![A::Wander(1.2)]),
            ],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_rule_that_holds_wins() {
        let (_, rules) = templates().into_iter().find(|(name, _)| *name == "Hunter").unwrap();
        let facts = |to_player| BrainFacts { to_player, ..BrainFacts::default() };
        let none = |_: f32| 0;
        assert_eq!(facts(2.0).choose(&rules, &none).unwrap().label, "Strike");
        assert_eq!(facts(9.0).choose(&rules, &none).unwrap().label, "Chase");
        assert_eq!(facts(99.0).choose(&rules, &none).unwrap().label, "Idle");
    }

    #[test]
    fn allies_gate_a_rule() {
        let (_, rules) = templates().into_iter().find(|(name, _)| *name == "Pack").unwrap();
        let facts = BrainFacts { to_player: 9.0, health: 1.0, ..BrainFacts::default() };
        assert_eq!(facts.choose(&rules, &|_| 0).unwrap().label, "Idle");
        assert_eq!(facts.choose(&rules, &|_| 2).unwrap().label, "Rush");
    }
}
