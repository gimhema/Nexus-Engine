//! 게임 데이터 파일 — 규칙 수치(`data/rules.ron`)와 표시 데이터(`data/display.ron`) (S7-3).
//!
//! # 두 파일로 나누는 이유
//!
//! 패킷은 번호만 실어 나르므로 번호 → 내용 대응표가 필요하다. 이것이 서버와의 세 번째 결합선이
//! 되지 않도록 **절반씩** 나눈다 (CLAUDE.md 「정적 테이블은 반으로 쪼갠다」):
//!
//! | | 규칙 (`rules.ron`) | 표시 (`display.ron`) |
//! |---|---|---|
//! | 아이템 1101 | 공격력 12, 무기 자리 | "숏소드", 땅에 떨어진 색 |
//! | 대응하는 쪽 | 서버 테이블 | 클라이언트만 |
//!
//! 규칙 파일에는 이름·색이 없고, 표시 파일에는 수치가 없다.
//!
//! # 어디서 읽나
//!
//! 두 파일은 실행 파일에 **들어 있다** (`include_str!`) — 작업 디렉터리가 어디든 플레이할 수 있다.
//! 같은 경로(`data/…`)에 파일이 있으면 **그것을 먼저** 읽는다. 그래서 수치를 고치고 F5 만 다시 누르면
//! 적용되고, 다시 빌드할 필요가 없다. 디스크 파일이 잘못됐으면 조용히 내장본으로 넘어가지 않고
//! 오류를 낸다 — 고친 것이 적용되지 않은 채 모르고 지나가지 않게.
//!
//! `nexus-sim` 은 이 파일들을 모른다 — 여기서 `nexus-sim` 타입으로 바꿔 넘긴다.

use std::collections::BTreeMap;
use std::path::Path;

use nexus_core::Entity;
use nexus_sim::{
    AiKind, EquipSlot, FactionId, ItemDef, ItemId, ItemKind as SimItemKind, LootEntry, LootTableId,
    Relation, SimWorld, SkillDef, SkillId, UnitDef,
};
use serde::Deserialize;

use crate::scene::ItemKind;

/// 지금 읽을 수 있는 형식 번호.
const FORMAT_VERSION: u32 = 1;

pub(crate) const RULES_PATH: &str = "data/rules.ron";
pub(crate) const DISPLAY_PATH: &str = "data/display.ron";

const EMBEDDED_RULES: &str = include_str!("../../../data/rules.ron");
const EMBEDDED_DISPLAY: &str = include_str!("../../../data/display.ron");

// ─────────────────────────────────────────────────────────────────────────────
// 파일 형식
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RulesFile {
    version: u32,
    seed: u64,
    #[serde(default)]
    relations: Vec<RelationFile>,
    skills: BTreeMap<u32, SkillFile>,
    items: BTreeMap<u32, ItemFile>,
    #[serde(default)]
    loot: BTreeMap<u32, Vec<LootFile>>,
    units: BTreeMap<MarkerKindFile, UnitFile>,
    player_attack: u32,
    #[serde(default)]
    starting_kit: Vec<StackFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RelationFile {
    a: u32,
    b: u32,
    relation: RelationKindFile,
}

#[derive(Clone, Copy, Debug, Deserialize)]
enum RelationKindFile {
    Hostile,
    Neutral,
    Friendly,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillFile {
    range: f32,
    cooldown_ms: u32,
    damage_mult: f32,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
enum ItemFile {
    Consumable {
        heal: u32,
        max_stack: u32,
    },
    Equipment {
        slot: SlotFile,
        attack: u32,
        defense: u32,
    },
}

#[derive(Clone, Copy, Debug, Deserialize)]
enum SlotFile {
    Weapon,
    Head,
    Body,
    Hand,
    Shoes,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LootFile {
    item: u32,
    count: u32,
    chance_per_mille: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
enum MarkerKindFile {
    Player,
    Npc,
    Monster,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UnitFile {
    move_speed: f32,
    max_hp: u32,
    attack: u32,
    defense: u32,
    #[serde(default)]
    immortal: bool,
    #[serde(default)]
    faction: u32,
    #[serde(default)]
    ai: AiFile,
    #[serde(default)]
    aggro_range: f32,
    #[serde(default)]
    leash_range: f32,
    #[serde(default)]
    basic_attack: Option<u32>,
    #[serde(default)]
    loot: Option<u32>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
enum AiFile {
    #[default]
    Passive,
    Defensive,
    Aggressive,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StackFile {
    item: u32,
    count: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DisplayFile {
    version: u32,
    items: BTreeMap<u32, ItemLookFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemLookFile {
    name: String,
    /// sRGB.
    color: (f32, f32, f32),
}

// ─────────────────────────────────────────────────────────────────────────────
// 읽은 결과
// ─────────────────────────────────────────────────────────────────────────────

/// 아이템 하나의 표시 정보.
#[derive(Clone, Debug)]
struct ItemLook {
    name: String,
    color: [f32; 4],
}

/// 검증을 마친 게임 데이터. 플레이 한 판이 소유한다.
#[derive(Debug)]
pub(crate) struct GameData {
    seed: u64,
    relations: Vec<(FactionId, FactionId, Relation)>,
    skills: Vec<(SkillId, SkillDef)>,
    items: Vec<(ItemId, ItemDef)>,
    loot: Vec<(LootTableId, Vec<LootEntry>)>,
    units: BTreeMap<MarkerKindFile, UnitDef>,
    player_attack: SkillId,
    starting_kit: Vec<(ItemId, u32)>,
    looks: BTreeMap<u32, ItemLook>,
}

impl GameData {
    /// `data/` 에 파일이 있으면 그것을, 없으면 실행 파일에 든 것을 읽는다.
    pub(crate) fn load() -> Result<Self, String> {
        let rules = read_or_embedded(Path::new(RULES_PATH), EMBEDDED_RULES)?;
        let display = read_or_embedded(Path::new(DISPLAY_PATH), EMBEDDED_DISPLAY)?;
        Self::parse(&rules, &display)
    }

    /// 실행 파일에 든 데이터 — 테스트와 비교 기준.
    #[cfg(test)]
    pub(crate) fn embedded() -> Self {
        Self::parse(EMBEDDED_RULES, EMBEDDED_DISPLAY).expect("내장 게임 데이터가 잘못됐다")
    }

    /// 두 파일을 읽고 **서로 가리키는 번호가 다 있는지**까지 확인한다.
    pub(crate) fn parse(rules: &str, display: &str) -> Result<Self, String> {
        let r: RulesFile = ron::from_str(rules).map_err(|e| format!("{RULES_PATH}: {e}"))?;
        let d: DisplayFile = ron::from_str(display).map_err(|e| format!("{DISPLAY_PATH}: {e}"))?;
        if r.version != FORMAT_VERSION || d.version != FORMAT_VERSION {
            return Err(format!(
                "형식 번호가 맞지 않음 (규칙 {}, 표시 {} — 이 에디터는 {FORMAT_VERSION})",
                r.version, d.version
            ));
        }

        let mut errors = Vec::new();
        let mut check = |ok: bool, msg: String| {
            if !ok {
                errors.push(msg);
            }
        };

        for (id, s) in &r.skills {
            check(
                s.range.is_finite() && s.range >= 0.0 && s.damage_mult.is_finite(),
                format!("스킬 {id}: 사거리·배율은 유한한 수, 사거리는 0 이상"),
            );
        }
        for (table, entries) in &r.loot {
            for e in entries {
                check(
                    r.items.contains_key(&e.item),
                    format!("드롭 테이블 {table}: 없는 아이템 {}", e.item),
                );
            }
        }
        for (kind, u) in &r.units {
            let finite = [u.move_speed, u.aggro_range, u.leash_range]
                .iter()
                .all(|v| v.is_finite() && *v >= 0.0);
            check(finite, format!("{kind:?}: 속도·거리는 0 이상의 수"));
            if let Some(skill) = u.basic_attack {
                check(
                    r.skills.contains_key(&skill),
                    format!("{kind:?}: 없는 스킬 {skill}"),
                );
            }
            if let Some(table) = u.loot {
                check(
                    r.loot.contains_key(&table),
                    format!("{kind:?}: 없는 드롭 테이블 {table}"),
                );
            }
        }
        for kind in [
            MarkerKindFile::Player,
            MarkerKindFile::Npc,
            MarkerKindFile::Monster,
        ] {
            check(
                r.units.contains_key(&kind),
                format!("units 에 {kind:?} 가 없음"),
            );
        }
        check(
            r.skills.contains_key(&r.player_attack),
            format!("player_attack: 없는 스킬 {}", r.player_attack),
        );
        for s in &r.starting_kit {
            check(
                r.items.contains_key(&s.item),
                format!("starting_kit: 없는 아이템 {}", s.item),
            );
        }
        for rel in &r.relations {
            check(
                rel.a != rel.b,
                format!("관계 ({}, {}): 같은 진영끼리는 정할 수 없음", rel.a, rel.b),
            );
        }
        for (id, look) in &d.items {
            let (r_, g, b) = look.color;
            check(
                [r_, g, b].iter().all(|c| (0.0..=1.0).contains(c)),
                format!("표시 아이템 {id}: 색은 0~1"),
            );
        }
        if !errors.is_empty() {
            return Err(format!("게임 데이터 오류 — {}", errors.join(" / ")));
        }

        Ok(Self {
            seed: r.seed,
            relations: r
                .relations
                .iter()
                .map(|x| (FactionId(x.a), FactionId(x.b), x.relation.into()))
                .collect(),
            skills: r
                .skills
                .iter()
                .map(|(id, s)| {
                    (
                        SkillId(*id),
                        SkillDef {
                            range: s.range,
                            cooldown_ms: s.cooldown_ms,
                            damage_mult: s.damage_mult,
                        },
                    )
                })
                .collect(),
            items: r
                .items
                .iter()
                .map(|(id, i)| (ItemId(*id), (*i).into()))
                .collect(),
            loot: r
                .loot
                .iter()
                .map(|(id, entries)| {
                    (
                        LootTableId(*id),
                        entries
                            .iter()
                            .map(|e| LootEntry {
                                item: ItemId(e.item),
                                count: e.count,
                                chance_per_mille: e.chance_per_mille,
                            })
                            .collect(),
                    )
                })
                .collect(),
            units: r.units.iter().map(|(k, u)| (*k, (*u).into())).collect(),
            player_attack: SkillId(r.player_attack),
            starting_kit: r
                .starting_kit
                .iter()
                .map(|s| (ItemId(s.item), s.count))
                .collect(),
            looks: d
                .items
                .into_iter()
                .map(|(id, l)| {
                    let (r_, g, b) = l.color;
                    (
                        id,
                        ItemLook {
                            name: l.name,
                            color: [r_, g, b, 1.0],
                        },
                    )
                })
                .collect(),
        })
    }

    /// 규칙을 시뮬레이션 월드에 등록한다 (설정 작업).
    pub(crate) fn install(&self, world: &mut SimWorld) {
        world.set_seed(self.seed);
        for &(a, b, relation) in &self.relations {
            world.set_relation(a, b, relation);
        }
        for &(id, def) in &self.skills {
            world.define_skill(id, def);
        }
        for &(id, def) in &self.items {
            world.define_item(id, def);
        }
        for (id, entries) in &self.loot {
            world.define_loot(*id, entries.clone());
        }
    }

    /// 마커 종류의 유닛 수치.
    pub(crate) fn unit_def(&self, kind: ItemKind) -> UnitDef {
        let key = match kind {
            ItemKind::PlayerSpawn => MarkerKindFile::Player,
            ItemKind::Npc => MarkerKindFile::Npc,
            ItemKind::Monster => MarkerKindFile::Monster,
        };
        // parse() 가 세 종류가 다 있는지 확인했다.
        self.units.get(&key).copied().unwrap_or_default()
    }

    /// 플레이어에게 시작 소지품을 준다.
    pub(crate) fn give_starting_kit(&self, world: &mut SimWorld, player: Entity) {
        for &(item, count) in &self.starting_kit {
            if !world.give_item(player, item, count) {
                eprintln!("시작 소지품 {} ×{count} 가 가방에 들어가지 않음", item.0);
            }
        }
    }

    pub(crate) fn player_attack(&self) -> SkillId {
        self.player_attack
    }

    /// 아이템 이름. 표시 파일에 없으면 번호로.
    pub(crate) fn item_name(&self, id: ItemId) -> String {
        self.looks
            .get(&id.0)
            .map_or_else(|| format!("아이템 #{}", id.0), |l| l.name.clone())
    }

    /// 땅에 떨어졌을 때의 색 (sRGB).
    pub(crate) fn item_color(&self, id: ItemId) -> [f32; 4] {
        self.looks
            .get(&id.0)
            .map_or([0.8, 0.8, 0.8, 1.0], |l| l.color)
    }
}

/// 디스크에 있으면 그것을, 없으면 내장본을. 있는데 못 읽으면 오류 — 조용히 넘어가지 않는다.
fn read_or_embedded(path: &Path, embedded: &str) -> Result<String, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(embedded.to_owned()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

impl From<RelationKindFile> for Relation {
    fn from(r: RelationKindFile) -> Self {
        match r {
            RelationKindFile::Hostile => Self::Hostile,
            RelationKindFile::Neutral => Self::Neutral,
            RelationKindFile::Friendly => Self::Friendly,
        }
    }
}

impl From<SlotFile> for EquipSlot {
    fn from(s: SlotFile) -> Self {
        match s {
            SlotFile::Weapon => Self::Weapon,
            SlotFile::Head => Self::Head,
            SlotFile::Body => Self::Body,
            SlotFile::Hand => Self::Hand,
            SlotFile::Shoes => Self::Shoes,
        }
    }
}

impl From<ItemFile> for ItemDef {
    fn from(i: ItemFile) -> Self {
        match i {
            ItemFile::Consumable { heal, max_stack } => Self {
                kind: SimItemKind::Consumable { heal },
                max_stack,
            },
            ItemFile::Equipment {
                slot,
                attack,
                defense,
            } => Self {
                kind: SimItemKind::Equipment {
                    slot: slot.into(),
                    attack,
                    defense,
                },
                max_stack: 1,
            },
        }
    }
}

impl From<UnitFile> for UnitDef {
    fn from(u: UnitFile) -> Self {
        Self {
            move_speed: u.move_speed,
            max_hp: u.max_hp,
            attack: u.attack,
            defense: u.defense,
            immortal: u.immortal,
            faction: FactionId(u.faction),
            ai: match u.ai {
                AiFile::Passive => AiKind::Passive,
                AiFile::Defensive => AiKind::Defensive,
                AiFile::Aggressive => AiKind::Aggressive,
            },
            aggro_range: u.aggro_range,
            leash_range: u.leash_range,
            basic_attack: u.basic_attack.map(SkillId),
            loot: u.loot.map(LootTableId),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_data_is_valid() {
        let data = GameData::embedded();
        assert_eq!(data.player_attack(), SkillId(1));
        assert_eq!(data.unit_def(ItemKind::Monster).loot, Some(LootTableId(1)));
        assert!(data.unit_def(ItemKind::Npc).immortal);
    }

    #[test]
    fn every_rule_item_has_a_display_entry() {
        // 규칙에만 있고 이름이 없는 아이템은 화면에 "아이템 #번호" 로 나온다 — 데이터 누락이다.
        let data = GameData::embedded();
        for (id, _) in &data.items {
            assert!(
                data.looks.contains_key(&id.0),
                "아이템 {} 의 표시 정보가 없다",
                id.0
            );
        }
    }

    #[test]
    fn rules_file_has_no_display_fields_and_display_has_no_numbers() {
        // 두 파일이 서로의 일을 하지 않는지 — deny_unknown_fields 가 필드를 막고, 여기서는 반대쪽을 본다.
        assert!(
            !EMBEDDED_RULES.contains("name:"),
            "규칙 파일에 이름이 들어갔다"
        );
        for field in ["heal", "attack", "range", "chance"] {
            assert!(
                !EMBEDDED_DISPLAY.contains(field),
                "표시 파일에 수치({field})가 들어갔다"
            );
        }
    }

    #[test]
    fn broken_references_are_reported_together() {
        let rules = EMBEDDED_RULES
            .replacen("basic_attack: Some(2)", "basic_attack: Some(77)", 1)
            .replacen("(item: 909, count: 1", "(item: 4040, count: 1", 1)
            .replacen("player_attack: 1", "player_attack: 9", 1);
        let err = GameData::parse(&rules, EMBEDDED_DISPLAY).unwrap_err();
        for expected in [
            "없는 스킬 77",
            "없는 아이템 4040",
            "player_attack: 없는 스킬 9",
        ] {
            assert!(err.contains(expected), "'{expected}' 기대, 실제: {err}");
        }
    }

    #[test]
    fn typos_are_errors_not_silent_defaults() {
        // 필드 이름을 틀리면 기본값으로 조용히 넘어가지 않는다.
        let rules = EMBEDDED_RULES.replacen("aggro_range: 6.0", "agro_range: 6.0", 1);
        assert!(GameData::parse(&rules, EMBEDDED_DISPLAY).is_err());
    }

    #[test]
    fn missing_unit_kind_is_reported() {
        let start = EMBEDDED_RULES.find("        Player: (").unwrap();
        let end =
            start + EMBEDDED_RULES[start..].find("        ),\n").unwrap() + "        ),\n".len();
        let rules = format!("{}{}", &EMBEDDED_RULES[..start], &EMBEDDED_RULES[end..]);
        let err = GameData::parse(&rules, EMBEDDED_DISPLAY).unwrap_err();
        assert!(err.contains("Player"), "{err}");
    }

    #[test]
    fn unknown_items_fall_back_to_a_number() {
        let data = GameData::embedded();
        assert_eq!(data.item_name(ItemId(501)), "빨간 포션");
        assert_eq!(data.item_name(ItemId(4242)), "아이템 #4242");
    }

    #[test]
    fn a_missing_file_uses_the_embedded_copy_but_a_broken_one_does_not() {
        let dir = std::env::temp_dir().join(format!("nexus-data-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let missing = dir.join("nope.ron");
        assert_eq!(read_or_embedded(&missing, "내장").unwrap(), "내장");

        // 디렉터리를 파일처럼 읽으면 "없음" 이 아닌 오류다 — 내장본으로 넘어가지 않는다.
        assert!(read_or_embedded(&dir, "내장").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
