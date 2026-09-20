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

use crate::scene::{ActorId, ItemKind};

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
    /// 액터 타입별 수치. 마커가 이 번호를 가리킨다 (이름·시트는 `display.ron`).
    actors: BTreeMap<u32, UnitFile>,
    /// 마커 종류별 기본 액터 — 마커가 타입을 정하지 않았을 때.
    default_actors: BTreeMap<MarkerKindFile, u32>,
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

impl MarkerKindFile {
    fn of(kind: ItemKind) -> Self {
        match kind {
            ItemKind::PlayerSpawn => Self::Player,
            ItemKind::Npc => Self::Npc,
            ItemKind::Monster => Self::Monster,
        }
    }

    fn to_marker(self) -> ItemKind {
        match self {
            Self::Player => ItemKind::PlayerSpawn,
            Self::Npc => ItemKind::Npc,
            Self::Monster => ItemKind::Monster,
        }
    }
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
    /// 액터 타입의 표시 이름과 시트. 번호는 `rules.ron` 의 `actors` 와 같다.
    #[serde(default)]
    actors: BTreeMap<u32, ActorLookFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActorLookFile {
    /// 인스펙터 드롭다운에 나오는 이름.
    name: String,
    /// 스프라이트 시트 정의 파일 경로. **비우면** 내장 플레이스홀더를 쓴다.
    ///
    /// `Option` 이 아니라 빈 문자열인 이유: 손으로 쓰는 파일에서 `Some("...")` 은 번거롭고,
    /// RON 은 `Option` 에 반드시 `Some` 을 요구한다 (시트 정의의 `image` 와 같은 규칙).
    #[serde(default)]
    sheet: String,
    /// 시트에 곱할 색 (sRGB). 같은 시트를 색만 바꿔 재사용할 때 쓴다. 기본은 흰색(그림 그대로).
    #[serde(default = "white")]
    tint: (f32, f32, f32),
}

/// 색을 적지 않았을 때 — 그림 그대로.
fn white() -> (f32, f32, f32) {
    (1.0, 1.0, 1.0)
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

/// 인스펙터에 보여 줄 액터 수치 — **읽기 전용 요약**이다.
///
/// 값을 고치는 곳은 `data/rules.ron` 이다. 여기서 편집하게 만들면 수치가 두 곳에 생긴다.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ActorStats {
    move_speed: f32,
    max_hp: u32,
    attack: u32,
    defense: u32,
    faction: u32,
    ai: AiKind,
    aggro_range: f32,
    immortal: bool,
}

impl ActorStats {
    fn of(def: &UnitDef) -> Self {
        Self {
            move_speed: def.move_speed,
            max_hp: def.max_hp,
            attack: def.attack,
            defense: def.defense,
            faction: def.faction.0,
            ai: def.ai,
            aggro_range: def.aggro_range,
            immortal: def.immortal,
        }
    }

    /// 패널에 뿌릴 `(항목, 값)` 줄.
    pub(crate) fn rows(&self) -> Vec<(&'static str, String)> {
        let mut rows = vec![
            ("HP", self.max_hp.to_string()),
            ("공격", self.attack.to_string()),
            ("방어", self.defense.to_string()),
            ("이동", format!("{:.1} m/s", self.move_speed)),
            ("진영", self.faction.to_string()),
            (
                "AI",
                match self.ai {
                    AiKind::Passive => "수동",
                    AiKind::Defensive => "방어",
                    AiKind::Aggressive => "공격",
                }
                .to_string(),
            ),
        ];
        if self.ai == AiKind::Aggressive {
            rows.push(("어그로", format!("{:.1} m", self.aggro_range)));
        }
        if self.immortal {
            rows.push(("불사", String::from("예")));
        }
        rows
    }
}

/// 액터 타입 하나의 표시 정보.
#[derive(Clone, Debug)]
pub(crate) struct ActorLook {
    pub(crate) name: String,
    /// 시트 정의 파일 경로. 없으면 내장 플레이스홀더.
    pub(crate) sheet: Option<String>,
    /// 시트에 곱할 색 (sRGB). 흰색이면 **정하지 않음** — 무채색 시트는 마커 종류 색을 쓴다.
    tint: [f32; 4],
}

impl ActorLook {
    /// 이 타입을 그릴 때 쓸 색. 타입이 색을 정하지 않았으면 `fallback`(마커 종류 색).
    ///
    /// 내장 플레이스홀더는 무채색이라 색이 없으면 형체만 남는다 — 그래서 기본값이 필요하다.
    pub(crate) fn tint_or(&self, fallback: [f32; 4]) -> [f32; 4] {
        if self.tint == crate::sprites::NO_TINT {
            fallback
        } else {
            self.tint
        }
    }
}

/// 검증을 마친 게임 데이터.
///
/// 플레이 한 판이 소유하고, **에디터도 하나 들고 있다** — 인스펙터가 액터 타입 목록을
/// 보여 줘야 하기 때문이다. 플레이를 시작할 때마다 새로 읽는다.
#[derive(Debug)]
pub(crate) struct GameData {
    seed: u64,
    relations: Vec<(FactionId, FactionId, Relation)>,
    skills: Vec<(SkillId, SkillDef)>,
    items: Vec<(ItemId, ItemDef)>,
    loot: Vec<(LootTableId, Vec<LootEntry>)>,
    /// 액터 타입별 수치.
    actors: BTreeMap<ActorId, UnitDef>,
    /// 마커 종류별 기본 액터.
    default_actors: BTreeMap<MarkerKindFile, ActorId>,
    player_attack: SkillId,
    starting_kit: Vec<(ItemId, u32)>,
    looks: BTreeMap<u32, ItemLook>,
    actor_looks: BTreeMap<ActorId, ActorLook>,
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
        for (id, u) in &r.actors {
            check(*id != 0, String::from("액터 번호 0 은 '기본값' 예약"));
            let finite = [u.move_speed, u.aggro_range, u.leash_range]
                .iter()
                .all(|v| v.is_finite() && *v >= 0.0);
            check(finite, format!("액터 {id}: 속도·거리는 0 이상의 수"));
            if let Some(skill) = u.basic_attack {
                check(
                    r.skills.contains_key(&skill),
                    format!("액터 {id}: 없는 스킬 {skill}"),
                );
            }
            if let Some(table) = u.loot {
                check(
                    r.loot.contains_key(&table),
                    format!("액터 {id}: 없는 드롭 테이블 {table}"),
                );
            }
            // 이름이 없으면 인스펙터 드롭다운에 번호만 나온다 — 저작에 쓸 수 없다.
            check(
                d.actors.contains_key(id),
                format!("액터 {id}: display.ron 에 이름이 없음"),
            );
        }
        for kind in [
            MarkerKindFile::Player,
            MarkerKindFile::Npc,
            MarkerKindFile::Monster,
        ] {
            match r.default_actors.get(&kind) {
                Some(id) => check(
                    r.actors.contains_key(id),
                    format!("default_actors 의 {kind:?}: 없는 액터 {id}"),
                ),
                None => check(false, format!("default_actors 에 {kind:?} 가 없음")),
            }
        }
        for (id, look) in &d.actors {
            check(
                r.actors.contains_key(id),
                format!("표시 액터 {id}: rules.ron 에 수치가 없음"),
            );
            check(
                !look.name.trim().is_empty(),
                format!("표시 액터 {id}: 이름이 비어 있음"),
            );
            let (r_, g, b) = look.tint;
            check(
                [r_, g, b].iter().all(|c| (0.0..=1.0).contains(c)),
                format!("표시 액터 {id}: 색은 0~1"),
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
            actors: r
                .actors
                .iter()
                .map(|(id, u)| (ActorId::new(*id), (*u).into()))
                .collect(),
            default_actors: r
                .default_actors
                .iter()
                .map(|(k, id)| (*k, ActorId::new(*id)))
                .collect(),
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
            actor_looks: d
                .actors
                .into_iter()
                .map(|(id, l)| {
                    let (r_, g, b) = l.tint;
                    (
                        ActorId::new(id),
                        ActorLook {
                            name: l.name,
                            sheet: (!l.sheet.is_empty()).then_some(l.sheet),
                            tint: [r_, g, b, 1.0],
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

    /// 마커가 실제로 쓸 액터 타입.
    ///
    /// 마커가 타입을 정하지 않았으면(`ActorId::DEFAULT` — 예전 존 파일도 그렇다) 종류별 기본값,
    /// 정했는데 데이터에 **없는 번호**면 역시 기본값으로 떨어진다 — 데이터를 고치는 동안
    /// 존 파일이 못 열리면 곤란하기 때문이다.
    pub(crate) fn resolve_actor(&self, actor: ActorId, kind: ItemKind) -> ActorId {
        if !actor.is_default() && self.actors.contains_key(&actor) {
            return actor;
        }
        // parse() 가 세 종류의 기본값이 다 있는지 확인했다.
        self.default_actors
            .get(&MarkerKindFile::of(kind))
            .copied()
            .unwrap_or(ActorId::DEFAULT)
    }

    /// 액터 타입의 유닛 수치.
    pub(crate) fn unit_def(&self, actor: ActorId) -> UnitDef {
        self.actors.get(&actor).copied().unwrap_or_default()
    }

    /// 액터 타입의 표시 정보 (이름·시트·색).
    pub(crate) fn actor_look(&self, actor: ActorId) -> Option<&ActorLook> {
        self.actor_looks.get(&actor)
    }

    /// 인스펙터 드롭다운에 쓸 목록 — `(번호, 이름, 수치)`, 번호 오름차순.
    pub(crate) fn actor_catalog(&self) -> Vec<(ActorId, &str, ActorStats)> {
        self.actors
            .iter()
            .map(|(&id, def)| {
                let name = self
                    .actor_looks
                    .get(&id)
                    .map_or("이름 없음", |l| l.name.as_str());
                (id, name, ActorStats::of(def))
            })
            .collect()
    }

    /// 마커 종류의 기본 액터 — 새 마커를 만들 때 쓴다.
    pub(crate) fn default_actor(&self, kind: ItemKind) -> ActorId {
        self.resolve_actor(ActorId::DEFAULT, kind)
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

    /// 액터 타입별 스프라이트 시트 정의 파일 — `(번호, 경로)`. 시트를 안 적은 타입은 빠진다.
    pub(crate) fn sprite_sheets(&self) -> Vec<(ActorId, String)> {
        self.actor_looks
            .iter()
            .filter_map(|(id, look)| look.sheet.clone().map(|path| (*id, path)))
            .collect()
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
        let monster = data.default_actor(ItemKind::Monster);
        assert_eq!(data.unit_def(monster).loot, Some(LootTableId(1)));
        let npc = data.default_actor(ItemKind::Npc);
        assert!(data.unit_def(npc).immortal);
    }

    #[test]
    fn a_marker_uses_its_actor_type_not_its_kind() {
        // P1 의 요점 — 같은 몬스터 마커라도 타입이 다르면 수치가 다르다.
        let data = GameData::embedded();
        let default_monster = data.default_actor(ItemKind::Monster);
        let other = data
            .actor_catalog()
            .into_iter()
            .map(|(id, _, _)| id)
            .find(|&id| id != default_monster && data.unit_def(id).max_hp > 0)
            .expect("액터 타입이 둘 이상이어야 이 기능에 의미가 있다");

        let resolved = data.resolve_actor(other, ItemKind::Monster);
        assert_eq!(resolved, other, "마커가 정한 타입이 무시됐다");
        assert_ne!(
            data.unit_def(resolved).max_hp,
            data.unit_def(default_monster).max_hp,
            "타입이 달라도 수치가 같으면 시험이 되지 않는다"
        );
    }

    #[test]
    fn an_unset_or_unknown_actor_falls_back_to_the_kind_default() {
        // 존 파일이 예전 것이거나(0), 데이터를 고치는 중에 번호가 사라져도 열려야 한다.
        let data = GameData::embedded();
        for kind in ItemKind::ALL {
            let fallback = data.default_actor(kind);
            assert_eq!(data.resolve_actor(ActorId::DEFAULT, kind), fallback);
            assert_eq!(data.resolve_actor(ActorId::new(9999), kind), fallback);
            assert!(!fallback.is_default(), "{kind:?} 의 기본 액터가 없다");
        }
    }

    #[test]
    fn every_actor_has_a_name_and_every_name_has_stats() {
        // 이름 없는 타입은 드롭다운에서 고를 수 없고, 수치 없는 이름은 스폰할 수 없다.
        let data = GameData::embedded();
        for (id, name, _) in data.actor_catalog() {
            assert!(!name.trim().is_empty(), "액터 {}", id.raw());
            assert!(data.actor_look(id).is_some(), "액터 {}", id.raw());
        }
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
    fn a_missing_kind_default_is_reported() {
        // 종류별 기본 액터가 없으면 그 종류의 마커를 스폰할 수 없다.
        let rules = EMBEDDED_RULES.replacen("        Player: 1,\n", "", 1);
        let err = GameData::parse(&rules, EMBEDDED_DISPLAY).unwrap_err();
        assert!(err.contains("Player"), "{err}");
    }

    #[test]
    fn a_kind_default_pointing_at_nothing_is_reported() {
        let rules = EMBEDDED_RULES.replacen("        Monster: 100,", "        Monster: 777,", 1);
        let err = GameData::parse(&rules, EMBEDDED_DISPLAY).unwrap_err();
        assert!(err.contains("777"), "{err}");
    }

    #[test]
    fn an_actor_without_a_display_entry_is_reported() {
        // 이름이 없으면 인스펙터에서 고를 수 없다 — 데이터 누락이다.
        let display = EMBEDDED_DISPLAY.replacen("        101: (", "        999: (", 1);
        let err = GameData::parse(EMBEDDED_RULES, &display).unwrap_err();
        assert!(err.contains("101") && err.contains("999"), "{err}");
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
