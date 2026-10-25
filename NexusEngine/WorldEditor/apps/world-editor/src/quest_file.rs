//! `rules.ron` 의 `quests` 블록 — 파일 형식·검증·시뮬레이션 타입으로 옮기기 (R5).
//!
//! ```ron
//! quests: {
//!     1: (
//!         stages: [
//!             (objectives: [Kill(actor: 100, count: 3)]),
//!         ],
//!         rewards: [GiveCurrency(currency: 1, amount: 100), GiveExp(50), GiveLoot(1)],
//!     ),
//!     2: (
//!         requires: Quest(quest: 1, state: Completed),     // 선행 퀘스트
//!         stages: [
//!             (objectives: [Talk(actor: 12)], then: [SetFlag(scope: Character, flag: 3, value: 1)]),
//!             (objectives: [Collect(item: 909, count: 5, take: true)]),
//!         ],
//!         rewards: [GiveItem(item: 3001, count: 1)],
//!         repeatable: false,
//!         auto_complete: false,      // true 면 담당 NPC 에게 보고하지 않고 그 자리에서 끝난다
//!     ),
//! },
//! ```
//!
//! 목표: `Kill(actor, count)`(actor 0 = 아무나) · `Collect(item, count, take)` · `Reach(center, radius)` ·
//! `Talk(actor)` · `Check(조건)`. 단계 효과(`then`)·보상(`rewards`)·받을 조건(`requires`)은 트리거와 **같은 형식**이다.
//! 제목·설명은 `display.ron` 의 `quests: { 1: (title: "…", description: "…") }`.
//!
//! 규칙(진행·판정)은 `nexus_sim::quest` 에 있다 — 여기는 파일 모양과 검증뿐이다.

use std::collections::BTreeMap;

use nexus_core::Vec2;
use nexus_sim::{ActorType, ItemId, Objective, QuestDef, QuestId, Stage};
use serde::{Deserialize, Serialize};

use crate::trigger_file::{self, ConditionFile, EffectFile, Known};

/// 기본값인 칸은 적지 않는다 — 디자이너가 다시 써도 손으로 쓴 것과 같은 모양이 되게.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QuestFile {
    #[serde(default = "trigger_file::always", skip_serializing_if = "is_always")]
    pub(crate) requires: ConditionFile,
    pub(crate) stages: Vec<StageFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) rewards: Vec<EffectFile>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub(crate) repeatable: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub(crate) auto_complete: bool,
}

fn is_always(c: &ConditionFile) -> bool {
    *c == ConditionFile::Always
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StageFile {
    pub(crate) objectives: Vec<ObjectiveFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) then: Vec<EffectFile>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum ObjectiveFile {
    /// `actor` 0 = 아무나.
    Kill {
        #[serde(default)]
        actor: u32,
        #[serde(default = "one")]
        count: u32,
    },
    Collect {
        item: u32,
        #[serde(default = "one")]
        count: u32,
        #[serde(default)]
        take: bool,
    },
    Reach {
        center: (f32, f32),
        radius: f32,
    },
    Talk {
        actor: u32,
    },
    Check(ConditionFile),
}

fn one() -> u32 {
    1
}

/// 퀘스트 제목·설명 (`display.ron`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QuestLookFile {
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) description: String,
}

/// 검증. 오류는 `퀘스트 N: …` 꼴.
pub(crate) fn validate(quests: &BTreeMap<u32, QuestFile>, known: &Known<'_>) -> Vec<String> {
    let mut errors = Vec::new();
    for (id, q) in quests {
        let mut err = |msg: String| errors.push(format!("퀘스트 {id}: {msg}"));
        if *id == 0 {
            err(String::from("0 은 예약 번호"));
        }
        if q.stages.is_empty() {
            err(String::from("단계가 없음"));
        }
        trigger_file::check_condition(&q.requires, 0, known, &mut err);
        for (s, stage) in q.stages.iter().enumerate() {
            if stage.objectives.is_empty() {
                err(format!("단계 {s}: 목표가 없음"));
            }
            for o in &stage.objectives {
                match o {
                    ObjectiveFile::Kill { actor, count } => {
                        if *actor != 0 && !(known.actor)(*actor) {
                            err(format!("단계 {s}: 없는 액터 {actor}"));
                        }
                        if *count == 0 {
                            err(format!("단계 {s}: 처치 수가 0"));
                        }
                    }
                    ObjectiveFile::Collect { item, count, .. } => {
                        if !(known.item)(*item) {
                            err(format!("단계 {s}: 없는 아이템 {item}"));
                        }
                        if *count == 0 {
                            err(format!("단계 {s}: 수집 수가 0"));
                        }
                    }
                    ObjectiveFile::Reach { center, radius } => {
                        if !(center.0.is_finite()
                            && center.1.is_finite()
                            && radius.is_finite()
                            && *radius > 0.0)
                        {
                            err(format!(
                                "단계 {s}: Reach 의 중심은 수, 반경은 0 보다 커야 함"
                            ));
                        }
                    }
                    ObjectiveFile::Talk { actor } if !(known.actor)(*actor) => {
                        err(format!("단계 {s}: 없는 액터 {actor}"));
                    }
                    ObjectiveFile::Check(c) => trigger_file::check_condition(c, 0, known, &mut err),
                    ObjectiveFile::Talk { .. } => {}
                }
            }
            for e in &stage.then {
                trigger_file::check_effect(e, known, &mut err);
            }
        }
        for e in &q.rewards {
            trigger_file::check_effect(e, known, &mut err);
        }
    }
    errors
}

/// 퀘스트 하나가 가리키는 다른 표의 번호 — 지우기 거부·번호 바꾸기용 (R5).
pub(crate) fn refs(q: &QuestFile) -> trigger_file::Refs {
    let mut r = trigger_file::Refs::default();
    trigger_file::add_condition_refs(&q.requires, &mut r);
    for stage in &q.stages {
        for o in &stage.objectives {
            match o {
                ObjectiveFile::Kill { actor, .. } if *actor != 0 => {
                    r.actors.insert(*actor);
                }
                ObjectiveFile::Talk { actor } => {
                    r.actors.insert(*actor);
                }
                ObjectiveFile::Collect { item, .. } => {
                    r.items.insert(*item);
                }
                ObjectiveFile::Check(c) => trigger_file::add_condition_refs(c, &mut r),
                _ => {}
            }
        }
        for e in &stage.then {
            trigger_file::add_effect_refs(e, &mut r);
        }
    }
    for e in &q.rewards {
        trigger_file::add_effect_refs(e, &mut r);
    }
    r
}

/// 검증을 마친 표를 시뮬레이션 타입으로.
pub(crate) fn to_sim(quests: &BTreeMap<u32, QuestFile>) -> Vec<(QuestId, QuestDef)> {
    quests
        .iter()
        .map(|(id, q)| {
            (
                QuestId(*id),
                QuestDef {
                    stages: q
                        .stages
                        .iter()
                        .map(|s| Stage {
                            objectives: s.objectives.iter().map(objective).collect(),
                            then: s.then.iter().map(|e| (*e).into()).collect(),
                        })
                        .collect(),
                    requires: (&q.requires).into(),
                    rewards: q.rewards.iter().map(|e| (*e).into()).collect(),
                    repeatable: q.repeatable,
                    auto_complete: q.auto_complete,
                },
            )
        })
        .collect()
}

fn objective(o: &ObjectiveFile) -> Objective {
    match o {
        ObjectiveFile::Kill { actor, count } => Objective::Kill {
            actor: (*actor != 0).then_some(ActorType(*actor)),
            count: *count,
        },
        ObjectiveFile::Collect { item, count, take } => Objective::Collect {
            item: ItemId(*item),
            count: *count,
            take: *take,
        },
        ObjectiveFile::Reach { center, radius } => Objective::Reach {
            center: Vec2::new(center.0, center.1),
            radius: *radius,
        },
        ObjectiveFile::Talk { actor } => Objective::Talk {
            actor: ActorType(*actor),
        },
        ObjectiveFile::Check(c) => Objective::Check(c.into()),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 퀘스트 디자이너용 폼 (R5)
// ─────────────────────────────────────────────────────────────────────────────

/// 목표 하나 — 폼. 조건 목표는 RON 글로 고친다.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ObjectiveForm {
    Kill { actor: u32, count: u32 },
    Collect { item: u32, count: u32, take: bool },
    Reach { center: (f32, f32), radius: f32 },
    Talk { actor: u32 },
    Check(String),
}

impl ObjectiveForm {
    pub(crate) const KINDS: [(&'static str, Self); 5] = [
        ("처치", Self::Kill { actor: 0, count: 1 }),
        (
            "수집",
            Self::Collect {
                item: 0,
                count: 1,
                take: true,
            },
        ),
        (
            "도달",
            Self::Reach {
                center: (0.0, 0.0),
                radius: 2.0,
            },
        ),
        ("대화", Self::Talk { actor: 0 }),
        ("조건", Self::Check(String::new())),
    ];

    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Kill { .. } => "처치",
            Self::Collect { .. } => "수집",
            Self::Reach { .. } => "도달",
            Self::Talk { .. } => "대화",
            Self::Check(_) => "조건",
        }
    }
}

/// 단계 하나 — 폼. 효과는 한 줄에 하나씩 RON 글.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct StageForm {
    pub(crate) objectives: Vec<ObjectiveForm>,
    pub(crate) then: Vec<String>,
}

/// 퀘스트 하나 — 규칙 반쪽과 표시 반쪽(제목·설명)을 함께.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct QuestForm {
    pub(crate) title: String,
    pub(crate) description: String,
    /// 받을 조건 — RON 글. 비우면 늘 참.
    pub(crate) requires: String,
    pub(crate) stages: Vec<StageForm>,
    pub(crate) rewards: Vec<String>,
    pub(crate) repeatable: bool,
    pub(crate) auto_complete: bool,
}

impl QuestForm {
    pub(crate) fn new(title: &str) -> Self {
        Self {
            title: title.to_owned(),
            description: String::new(),
            requires: String::new(),
            stages: vec![StageForm {
                objectives: vec![ObjectiveForm::Kill { actor: 0, count: 1 }],
                then: Vec::new(),
            }],
            rewards: Vec::new(),
            repeatable: false,
            auto_complete: false,
        }
    }

    /// 파일 항목에서 만든다.
    pub(crate) fn from_file(q: &QuestFile, look: Option<&QuestLookFile>) -> Self {
        let text = |v: &dyn erased::Ron| v.ron();
        Self {
            title: look.map(|l| l.title.clone()).unwrap_or_default(),
            description: look.map(|l| l.description.clone()).unwrap_or_default(),
            requires: if is_always(&q.requires) {
                String::new()
            } else {
                text(&q.requires)
            },
            stages: q
                .stages
                .iter()
                .map(|s| StageForm {
                    objectives: s
                        .objectives
                        .iter()
                        .map(|o| match o {
                            ObjectiveFile::Kill { actor, count } => ObjectiveForm::Kill {
                                actor: *actor,
                                count: *count,
                            },
                            ObjectiveFile::Collect { item, count, take } => {
                                ObjectiveForm::Collect {
                                    item: *item,
                                    count: *count,
                                    take: *take,
                                }
                            }
                            ObjectiveFile::Reach { center, radius } => ObjectiveForm::Reach {
                                center: *center,
                                radius: *radius,
                            },
                            ObjectiveFile::Talk { actor } => ObjectiveForm::Talk { actor: *actor },
                            ObjectiveFile::Check(c) => ObjectiveForm::Check(text(c)),
                        })
                        .collect(),
                    then: s.then.iter().map(|e| text(e)).collect(),
                })
                .collect(),
            rewards: q.rewards.iter().map(|e| text(e)).collect(),
            repeatable: q.repeatable,
            auto_complete: q.auto_complete,
        }
    }

    /// 파일 항목으로 — 조건·효과 글을 읽지 못하면 그 위치와 이유.
    pub(crate) fn to_file(&self) -> Result<QuestFile, String> {
        let effect = |src: &str, at: &str| -> Result<EffectFile, String> {
            ron::from_str(src.trim()).map_err(|e| format!("{at} \"{}\" — {e}", src.trim()))
        };
        let condition = |src: &str, at: &str| -> Result<ConditionFile, String> {
            if src.trim().is_empty() {
                return Ok(ConditionFile::Always);
            }
            ron::from_str(src.trim()).map_err(|e| format!("{at} \"{}\" — {e}", src.trim()))
        };
        let mut stages = Vec::new();
        for (s, stage) in self.stages.iter().enumerate() {
            let mut objectives = Vec::new();
            for o in &stage.objectives {
                objectives.push(match o {
                    ObjectiveForm::Kill { actor, count } => ObjectiveFile::Kill {
                        actor: *actor,
                        count: *count,
                    },
                    ObjectiveForm::Collect { item, count, take } => ObjectiveFile::Collect {
                        item: *item,
                        count: *count,
                        take: *take,
                    },
                    ObjectiveForm::Reach { center, radius } => ObjectiveFile::Reach {
                        center: *center,
                        radius: *radius,
                    },
                    ObjectiveForm::Talk { actor } => ObjectiveFile::Talk { actor: *actor },
                    ObjectiveForm::Check(src) => {
                        ObjectiveFile::Check(condition(src, &format!("단계 {s} 조건 목표"))?)
                    }
                });
            }
            let mut then = Vec::new();
            for e in stage.then.iter().filter(|e| !e.trim().is_empty()) {
                then.push(effect(e, &format!("단계 {s} 효과"))?);
            }
            stages.push(StageFile { objectives, then });
        }
        let mut rewards = Vec::new();
        for e in self.rewards.iter().filter(|e| !e.trim().is_empty()) {
            rewards.push(effect(e, "보상")?);
        }
        Ok(QuestFile {
            requires: condition(&self.requires, "받을 조건")?,
            stages,
            rewards,
            repeatable: self.repeatable,
            auto_complete: self.auto_complete,
        })
    }

    /// `rules.ron` 의 `quests` 항목 텍스트.
    pub(crate) fn rules_entry(&self, id: u32) -> Result<String, String> {
        let file = self.to_file()?;
        let config = ron::ser::PrettyConfig::new()
            .new_line("\n")
            .indentor("    ")
            .struct_names(false);
        let body = ron::ser::to_string_pretty(&file, config).map_err(|e| e.to_string())?;
        Ok(format!("{id}: {body},"))
    }

    /// `display.ron` 의 `quests` 항목 텍스트.
    pub(crate) fn display_entry(&self, id: u32) -> String {
        let q = crate::ron_patch::quote;
        let desc = if self.description.trim().is_empty() {
            String::new()
        } else {
            format!(", description: {}", q(self.description.trim()))
        };
        format!("{id}: (title: {}{desc}),", q(self.title.trim()))
    }
}

/// 조건·효과를 한 줄 RON 글로 — 폼에 보일 모양.
mod erased {
    pub(super) trait Ron {
        fn ron(&self) -> String;
    }

    impl<T: serde::Serialize> Ron for T {
        fn ron(&self) -> String {
            ron::to_string(self).unwrap_or_default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known() -> Known<'static> {
        Known {
            item: &|i| i == 909,
            actor: &|a| a == 100 || a == 12,
            skill: &|_| true,
            quest: &|q| q == 1,
            loot: &|t| t == 1,
        }
    }

    #[test]
    fn the_module_example_reads_and_converts() {
        let text = r#"{
            1: (stages: [(objectives: [Kill(actor: 100, count: 3)])],
                rewards: [GiveCurrency(currency: 1, amount: 100), GiveExp(50), GiveLoot(1)]),
            2: (requires: Quest(quest: 1, state: Completed),
                stages: [
                    (objectives: [Talk(actor: 12)], then: [SetFlag(scope: Character, flag: 3, value: 1)]),
                    (objectives: [Collect(item: 909, count: 5, take: true)]),
                ],
                repeatable: true),
        }"#;
        let quests: BTreeMap<u32, QuestFile> = ron::from_str(text).unwrap();
        assert!(
            validate(&quests, &known()).is_empty(),
            "{:?}",
            validate(&quests, &known())
        );
        let sim = to_sim(&quests);
        assert_eq!(
            sim[0].1.stages[0].objectives[0],
            Objective::Kill {
                actor: Some(ActorType(100)),
                count: 3
            }
        );
        assert!(sim[1].1.repeatable);
        assert_eq!(sim[1].1.stages.len(), 2);
    }

    #[test]
    fn dangling_numbers_and_empty_parts_are_named() {
        let text = r#"{
            0: (stages: []),
            3: (requires: Quest(quest: 9, state: Active),
                stages: [
                    (objectives: []),
                    (objectives: [Kill(actor: 7), Collect(item: 5, count: 0), Talk(actor: 8),
                                  Reach(center: (0.0, 0.0), radius: 0.0)],
                     then: [GiveLoot(4), StartQuest(6)]),
                ]),
        }"#;
        let quests: BTreeMap<u32, QuestFile> = ron::from_str(text).unwrap();
        let e = validate(&quests, &known());
        for want in [
            "퀘스트 0: 0 은 예약",
            "퀘스트 0: 단계가 없음",
            "퀘스트 3: when: 없는 퀘스트 9",
            "퀘스트 3: 단계 0: 목표가 없음",
            "퀘스트 3: 단계 1: 없는 액터 7",
            "퀘스트 3: 단계 1: 없는 아이템 5",
            "퀘스트 3: 단계 1: 수집 수가 0",
            "퀘스트 3: 단계 1: 없는 액터 8",
            "퀘스트 3: 단계 1: Reach",
            "퀘스트 3: then: 없는 드롭 표 4",
            "퀘스트 3: then: 없는 퀘스트 6",
        ] {
            assert!(
                e.iter().any(|m| m.starts_with(want)),
                "{want} 없음 — {e:#?}"
            );
        }
    }
}
