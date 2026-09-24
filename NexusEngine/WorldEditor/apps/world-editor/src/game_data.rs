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

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use nexus_core::Entity;
use nexus_sim::{
    AiKind, EquipSlot, FactionId, Growth, ItemDef, ItemId, ItemKind as SimItemKind, LootEntry,
    LootTableId, Relation, SimWorld, SkillDef, SkillId, UnitDef,
};
use serde::Deserialize;

use crate::scene::{ActorId, ItemKind};

/// 지금 읽을 수 있는 형식 번호.
const FORMAT_VERSION: u32 = 1;

pub(crate) const RULES_PATH: &str = "data/rules.ron";
pub(crate) const DISPLAY_PATH: &str = "data/display.ron";

const EMBEDDED_RULES: &str = include_str!("../../../data/rules.ron");
const EMBEDDED_DISPLAY: &str = include_str!("../../../data/display.ron");

/// 스크립트 경로의 기준 폴더.
pub(crate) const DATA_DIR: &str = "data";

/// 내장 스크립트 — 작업 디렉터리 밖에서 실행해도 기본 데이터가 돌게. 경로는 `data/` 기준.
/// 디스크에 같은 경로의 파일이 있으면 그것이 우선한다 (다른 데이터 파일과 같은 규칙).
const EMBEDDED_SCRIPTS: &[(&str, &str)] = &[(
    "scripts/goblin.rhai",
    include_str!("../../../data/scripts/goblin.rhai"),
)];

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
}

#[derive(Clone, Debug, Deserialize)]
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
    /// 액터 스크립트 (P2) — `data/` 기준 경로. 예: `scripts/goblin.rhai`.
    #[serde(default)]
    script: Option<String>,
    /// 이 액터를 죽인 쪽이 받는 경험치 (P3).
    #[serde(default)]
    exp_reward: u32,
    /// 레벨이 오를 때마다 더해지는 수치 (P3). 적지 않으면 성장하지 않는다.
    #[serde(default)]
    growth: GrowthFile,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrowthFile {
    #[serde(default)]
    max_hp: u32,
    #[serde(default)]
    attack: u32,
    #[serde(default)]
    defense: u32,
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
    /// 스킬의 표시 이름 (P10). 없으면 번호로 보인다.
    #[serde(default)]
    skills: BTreeMap<u32, SkillLookFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillLookFile {
    name: String,
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
    /// 이 타입이 **명시한** 색. 정하지 않았으면 `NO_TINT`(흰색).
    ///
    /// 여기서 마커 종류 색으로 채우지 않는다 — 그 판단은 시트가 무채색인지 아는 쪽
    /// (`sprites::tint_of`)이 한다.
    pub(crate) fn tint(&self) -> [f32; 4] {
        self.tint
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
    /// 액터 타입별 스크립트 경로 (`data/` 기준).
    actor_scripts: BTreeMap<ActorId, String>,
    /// 경로 → 스크립트 본문. 컴파일은 플레이를 시작할 때 한다.
    script_sources: BTreeMap<String, String>,
}

impl GameData {
    /// `data/` 에 파일이 있으면 그것을, 없으면 실행 파일에 든 것을 읽는다.
    pub(crate) fn load() -> Result<Self, String> {
        let rules = read_or_embedded(Path::new(RULES_PATH), EMBEDDED_RULES)?;
        let display = read_or_embedded(Path::new(DISPLAY_PATH), EMBEDDED_DISPLAY)?;
        Self::parse(&rules, &display)?.with_scripts(|path| {
            match std::fs::read_to_string(Path::new(DATA_DIR).join(path)) {
                Ok(text) => Ok(Some(text)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    Ok(embedded_script(path).map(str::to_owned))
                }
                Err(e) => Err(format!("{DATA_DIR}/{path}: {e}")),
            }
        })
    }

    /// 실행 파일에 든 데이터 — 테스트와 비교 기준.
    #[cfg(test)]
    pub(crate) fn embedded() -> Self {
        Self::parse(EMBEDDED_RULES, EMBEDDED_DISPLAY)
            .and_then(|d| d.with_scripts(|path| Ok(embedded_script(path).map(str::to_owned))))
            .expect("내장 게임 데이터가 잘못됐다")
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
            if let Some(path) = &u.script {
                check(
                    script_path_ok(path),
                    format!(
                        "액터 {id}: 스크립트 경로 '{path}' — data/ 기준 상대 경로, 소문자, '/' 구분, .rhai"
                    ),
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
        for (id, look) in &d.skills {
            check(
                r.skills.contains_key(id),
                format!("표시 스킬 {id}: rules.ron 에 없는 스킬"),
            );
            check(
                !look.name.trim().is_empty(),
                format!("표시 스킬 {id}: 이름이 비어 있음"),
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
                .map(|(id, u)| (ActorId::new(*id), u.into()))
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
            actor_scripts: r
                .actors
                .iter()
                .filter_map(|(id, u)| Some((ActorId::new(*id), u.script.clone()?)))
                .collect(),
            script_sources: BTreeMap::new(),
        })
    }

    /// 액터들이 가리키는 스크립트 본문을 읽어 둔다. `read` 는 경로(`data/` 기준)를 받아
    /// 본문을, 없으면 `None` 을 돌려준다 — 파일을 읽는 방법은 호출한 쪽이 정한다.
    fn with_scripts(
        mut self,
        read: impl Fn(&str) -> Result<Option<String>, String>,
    ) -> Result<Self, String> {
        let paths: BTreeSet<&String> = self.actor_scripts.values().collect();
        let mut errors = Vec::new();
        let mut sources = BTreeMap::new();
        for path in paths {
            match read(path) {
                Ok(Some(text)) => {
                    sources.insert(path.clone(), text);
                }
                Ok(None) => errors.push(format!("스크립트 {DATA_DIR}/{path} 가 없음")),
                Err(e) => errors.push(e),
            }
        }
        if !errors.is_empty() {
            return Err(format!("게임 데이터 오류 — {}", errors.join(" / ")));
        }
        self.script_sources = sources;
        Ok(self)
    }

    /// 액터 타입의 스크립트 경로 (`data/` 기준). 없으면 스크립트 없는 액터.
    pub(crate) fn actor_script(&self, actor: ActorId) -> Option<&str> {
        self.actor_scripts.get(&actor).map(String::as_str)
    }

    /// 읽어 둔 스크립트 `(경로, 본문)` — 경로 순.
    pub(crate) fn script_sources(&self) -> impl Iterator<Item = (&str, &str)> {
        self.script_sources
            .iter()
            .map(|(p, s)| (p.as_str(), s.as_str()))
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
    ///
    /// 수치는 인스펙터가 **타입 값**으로 보여 주고, 덮어쓰기(P1-4)의 시작값으로도 쓴다.
    pub(crate) fn actor_catalog(&self) -> Vec<(ActorId, &str, UnitDef)> {
        self.actors
            .iter()
            .map(|(&id, def)| {
                let name = self
                    .actor_looks
                    .get(&id)
                    .map_or("이름 없음", |l| l.name.as_str());
                (id, name, *def)
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

    /// 난수 시드 — 드롭과 스크립트 난수가 같은 값에서 출발한다 (흐름은 따로).
    pub(crate) fn seed(&self) -> u64 {
        self.seed
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

/// 내장 스크립트 본문.
fn embedded_script(path: &str) -> Option<&'static str> {
    EMBEDDED_SCRIPTS
        .iter()
        .find(|(p, _)| *p == path)
        .map(|(_, s)| *s)
}

/// 스크립트 경로 규칙 — `data/` 기준 상대 경로, 소문자, `/` 구분, `.rhai`.
///
/// 대소문자·구분자를 강제하는 이유는 에셋 이름과 같다: Windows 에서 통하던 `Goblin.rhai` 나
/// `scripts\goblin.rhai` 가 리눅스에서는 "파일 없음" 이 된다.
pub(crate) fn script_path_ok(path: &str) -> bool {
    path.ends_with(".rhai")
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && !path.split('/').any(|part| part.is_empty() || part == "..")
        && !path.chars().any(char::is_uppercase)
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

impl From<&UnitFile> for UnitDef {
    fn from(u: &UnitFile) -> Self {
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
            exp_reward: u.exp_reward,
            growth: Growth {
                max_hp: u.growth.max_hp,
                attack: u.growth.attack,
                defense: u.growth.defense,
            },
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 액터 편집기용 — 표의 항목 하나를 폼으로 (P8)
// ─────────────────────────────────────────────────────────────────────────────

/// 액터 타입 하나의 **파일에 적힌 그대로의** 값 — 규칙 반쪽(`rules.ron`)과 표시 반쪽
/// (`display.ron`)을 한 폼에 모은다. 액터 편집기가 고치고 다시 두 파일에 나눠 쓴다.
///
/// `UnitDef` 가 아니라 이 타입을 쓰는 이유: `UnitDef` 는 **보정을 거친** 값이다
/// (귀환 거리를 어그로 범위까지 올리는 등). 그걸 다시 쓰면 파일이 조용히 바뀐다.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ActorForm {
    // 규칙 반쪽 (rules.ron)
    pub(crate) move_speed: f32,
    pub(crate) max_hp: u32,
    pub(crate) attack: u32,
    pub(crate) defense: u32,
    pub(crate) immortal: bool,
    pub(crate) faction: u32,
    pub(crate) ai: AiChoice,
    pub(crate) aggro_range: f32,
    pub(crate) leash_range: f32,
    pub(crate) basic_attack: Option<u32>,
    pub(crate) loot: Option<u32>,
    pub(crate) script: Option<String>,
    pub(crate) exp_reward: u32,
    pub(crate) growth: (u32, u32, u32),
    // 표시 반쪽 (display.ron)
    pub(crate) name: String,
    pub(crate) sheet: String,
    pub(crate) tint: (f32, f32, f32),
}

/// AI 종류 — 편집기 드롭다운용 (파일 형식 `AiFile` 과 같은 세 가지).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum AiChoice {
    #[default]
    Passive,
    Defensive,
    Aggressive,
}

impl AiChoice {
    pub(crate) const ALL: [Self; 3] = [Self::Passive, Self::Defensive, Self::Aggressive];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Passive => "수동 (먼저 덤비지도 반격하지도 않음)",
            Self::Defensive => "방어 (맞으면 반격)",
            Self::Aggressive => "공격 (보이는 적에게 먼저 덤빔)",
        }
    }

    fn file_name(self) -> &'static str {
        match self {
            Self::Passive => "Passive",
            Self::Defensive => "Defensive",
            Self::Aggressive => "Aggressive",
        }
    }
}

impl From<AiFile> for AiChoice {
    fn from(a: AiFile) -> Self {
        match a {
            AiFile::Passive => Self::Passive,
            AiFile::Defensive => Self::Defensive,
            AiFile::Aggressive => Self::Aggressive,
        }
    }
}

impl ActorForm {
    /// 새 액터의 처음 값 — 걷고 치는 최소 유닛.
    pub(crate) fn new(name: &str) -> Self {
        Self {
            move_speed: 2.0,
            max_hp: 50,
            attack: 5,
            defense: 0,
            immortal: false,
            faction: 0,
            ai: AiChoice::Passive,
            aggro_range: 0.0,
            leash_range: 0.0,
            basic_attack: None,
            loot: None,
            script: None,
            exp_reward: 0,
            growth: (0, 0, 0),
            name: name.to_owned(),
            sheet: String::new(),
            tint: (1.0, 1.0, 1.0),
        }
    }

    /// `rules.ron` 의 `actors` 항목 텍스트 (들여쓰기 없음). **기본값인 필드는 적지 않는다** —
    /// 손으로 쓴 항목과 같은 모양이 되게.
    pub(crate) fn rules_entry(&self, id: u32) -> String {
        let mut s = format!("{id}: (\n");
        let mut line = |text: String| {
            s.push_str("    ");
            s.push_str(&text);
            s.push_str(",\n");
        };
        line(format!("move_speed: {:?}", self.move_speed));
        line(format!("max_hp: {}", self.max_hp));
        line(format!("attack: {}", self.attack));
        line(format!("defense: {}", self.defense));
        if self.immortal {
            line(String::from("immortal: true"));
        }
        if self.faction != 0 {
            line(format!("faction: {}", self.faction));
        }
        if self.ai != AiChoice::Passive {
            line(format!("ai: {}", self.ai.file_name()));
        }
        if self.aggro_range != 0.0 {
            line(format!("aggro_range: {:?}", self.aggro_range));
        }
        if self.leash_range != 0.0 {
            line(format!("leash_range: {:?}", self.leash_range));
        }
        if let Some(skill) = self.basic_attack {
            line(format!("basic_attack: Some({skill})"));
        }
        if let Some(table) = self.loot {
            line(format!("loot: Some({table})"));
        }
        if let Some(script) = &self.script {
            line(format!("script: Some({})", crate::ron_patch::quote(script)));
        }
        if self.exp_reward != 0 {
            line(format!("exp_reward: {}", self.exp_reward));
        }
        let (hp, atk, def) = self.growth;
        if (hp, atk, def) != (0, 0, 0) {
            line(format!(
                "growth: (max_hp: {hp}, attack: {atk}, defense: {def})"
            ));
        }
        s.push_str("),");
        s
    }

    /// `display.ron` 의 `actors` 항목 텍스트 (들여쓰기 없음).
    pub(crate) fn display_entry(&self, id: u32) -> String {
        let mut s = format!("{id}: (\n");
        s.push_str(&format!(
            "    name: {},\n",
            crate::ron_patch::quote(&self.name)
        ));
        if !self.sheet.is_empty() {
            s.push_str(&format!(
                "    sheet: {},\n",
                crate::ron_patch::quote(&self.sheet)
            ));
        }
        if self.tint != white() {
            let (r, g, b) = self.tint;
            s.push_str(&format!("    tint: ({r:?}, {g:?}, {b:?}),\n"));
        }
        s.push_str("),");
        s
    }
}

/// 두 파일에서 액터 항목을 **파일에 적힌 그대로** 읽는다 — 번호 → 폼.
///
/// 한쪽에만 있는 번호도 폼을 만든다 (빈 이름 / 기본 수치로) — 저장할 때 `GameData::parse`
/// 가 짝이 맞는지 검사하므로, 편집기에서 채워 넣고 저장하면 고쳐진다.
pub(crate) fn actor_forms(rules: &str, display: &str) -> Result<BTreeMap<u32, ActorForm>, String> {
    let r: RulesFile = ron::from_str(rules).map_err(|e| format!("{RULES_PATH}: {e}"))?;
    let d: DisplayFile = ron::from_str(display).map_err(|e| format!("{DISPLAY_PATH}: {e}"))?;
    let mut out: BTreeMap<u32, ActorForm> = BTreeMap::new();
    for (id, u) in &r.actors {
        out.insert(
            *id,
            ActorForm {
                move_speed: u.move_speed,
                max_hp: u.max_hp,
                attack: u.attack,
                defense: u.defense,
                immortal: u.immortal,
                faction: u.faction,
                ai: u.ai.into(),
                aggro_range: u.aggro_range,
                leash_range: u.leash_range,
                basic_attack: u.basic_attack,
                loot: u.loot,
                script: u.script.clone(),
                exp_reward: u.exp_reward,
                growth: (u.growth.max_hp, u.growth.attack, u.growth.defense),
                ..ActorForm::new("")
            },
        );
    }
    for (id, look) in d.actors {
        let form = out.entry(id).or_insert_with(|| ActorForm::new(""));
        form.name = look.name;
        form.sheet = look.sheet;
        form.tint = look.tint;
    }
    Ok(out)
}

/// 실행 파일에 내장된 스크립트인가 (`data/` 기준 경로) — 지워도 내장본으로 되살아난다 (P9).
pub(crate) fn is_builtin_script(path: &str) -> bool {
    embedded_script(path).is_some()
}

/// 마커 종류의 기본 액터 번호들 — 이 번호의 액터는 지울 수 없다 (P9).
pub(crate) fn default_actor_ids(rules: &str) -> Result<Vec<u32>, String> {
    let r: RulesFile = ron::from_str(rules).map_err(|e| format!("{RULES_PATH}: {e}"))?;
    Ok(r.default_actors.values().copied().collect())
}

// ─────────────────────────────────────────────────────────────────────────────
// 데이터 표 편집기용 — 아이템·스킬·드롭 표 (P10)
// ─────────────────────────────────────────────────────────────────────────────

/// 아이템 하나 — 규칙 반쪽(종류·수치)과 표시 반쪽(이름·색).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ItemForm {
    pub(crate) kind: ItemKindForm,
    pub(crate) name: String,
    /// 땅에 떨어졌을 때의 색 (sRGB).
    pub(crate) color: (f32, f32, f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ItemKindForm {
    Consumable {
        heal: u32,
        max_stack: u32,
    },
    Equipment {
        slot: SlotChoice,
        attack: u32,
        defense: u32,
    },
}

/// 장착 자리 — 편집기 드롭다운용 (파일 형식 `SlotFile` 과 같은 다섯 가지, 서버 순서).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SlotChoice {
    Weapon,
    Head,
    Body,
    Hand,
    Shoes,
}

impl SlotChoice {
    pub(crate) const ALL: [Self; 5] = [
        Self::Weapon,
        Self::Head,
        Self::Body,
        Self::Hand,
        Self::Shoes,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Weapon => "무기",
            Self::Head => "머리",
            Self::Body => "몸",
            Self::Hand => "손",
            Self::Shoes => "신발",
        }
    }

    fn file_name(self) -> &'static str {
        match self {
            Self::Weapon => "Weapon",
            Self::Head => "Head",
            Self::Body => "Body",
            Self::Hand => "Hand",
            Self::Shoes => "Shoes",
        }
    }
}

impl From<SlotFile> for SlotChoice {
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

impl ItemForm {
    pub(crate) fn new(name: &str) -> Self {
        Self {
            kind: ItemKindForm::Consumable {
                heal: 10,
                max_stack: 20,
            },
            name: name.to_owned(),
            color: (0.9, 0.9, 0.9),
        }
    }

    /// `rules.ron` 의 `items` 항목 — 손으로 쓴 것과 같은 한 줄.
    pub(crate) fn rules_entry(&self, id: u32) -> String {
        match self.kind {
            ItemKindForm::Consumable { heal, max_stack } => {
                format!("{id}: Consumable(heal: {heal}, max_stack: {max_stack}),")
            }
            ItemKindForm::Equipment {
                slot,
                attack,
                defense,
            } => format!(
                "{id}: Equipment(slot: {}, attack: {attack}, defense: {defense}),",
                slot.file_name()
            ),
        }
    }

    /// `display.ron` 의 `items` 항목.
    pub(crate) fn display_entry(&self, id: u32) -> String {
        let (r, g, b) = self.color;
        format!(
            "{id}: (name: {}, color: ({r:?}, {g:?}, {b:?})),",
            crate::ron_patch::quote(&self.name)
        )
    }
}

/// 스킬 하나 — 규칙 반쪽(사거리·쿨타임·배율)과 표시 반쪽(이름).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SkillForm {
    pub(crate) range: f32,
    pub(crate) cooldown_ms: u32,
    pub(crate) damage_mult: f32,
    pub(crate) name: String,
}

impl SkillForm {
    pub(crate) fn new(name: &str) -> Self {
        Self {
            range: 2.0,
            cooldown_ms: 1000,
            damage_mult: 1.0,
            name: name.to_owned(),
        }
    }

    pub(crate) fn rules_entry(&self, id: u32) -> String {
        format!(
            "{id}: (range: {:?}, cooldown_ms: {}, damage_mult: {:?}),",
            self.range, self.cooldown_ms, self.damage_mult
        )
    }

    pub(crate) fn display_entry(&self, id: u32) -> String {
        format!("{id}: (name: {}),", crate::ron_patch::quote(&self.name))
    }
}

/// 드롭 표 한 줄.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LootRow {
    pub(crate) item: u32,
    pub(crate) count: u32,
    /// 천분율 (1000 = 반드시).
    pub(crate) chance_per_mille: u32,
}

/// 드롭 표의 `rules.ron` 항목 — 목록이라 여러 줄이다. 드롭 표에는 표시 반쪽이 없다.
pub(crate) fn loot_entry(id: u32, rows: &[LootRow]) -> String {
    if rows.is_empty() {
        return format!("{id}: [],");
    }
    let mut s = format!("{id}: [\n");
    for r in rows {
        s.push_str(&format!(
            "    (item: {}, count: {}, chance_per_mille: {}),\n",
            r.item, r.count, r.chance_per_mille
        ));
    }
    s.push_str("],");
    s
}

/// 아이템·스킬·드롭 표의 **파일에 적힌 그대로의** 값 — 데이터 표 편집기와 브라우저가 쓴다.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TableForms {
    pub(crate) items: BTreeMap<u32, ItemForm>,
    pub(crate) skills: BTreeMap<u32, SkillForm>,
    pub(crate) loot: BTreeMap<u32, Vec<LootRow>>,
    /// 참조 찾기용 — 시작 소지품의 아이템, 플레이어 공격 스킬.
    pub(crate) starting_kit: Vec<u32>,
    pub(crate) player_attack: u32,
}

/// 두 파일에서 아이템·스킬·드롭 표를 읽는다. 표시 반쪽이 없는 번호는 빈 이름으로 만든다.
pub(crate) fn table_forms(rules: &str, display: &str) -> Result<TableForms, String> {
    let r: RulesFile = ron::from_str(rules).map_err(|e| format!("{RULES_PATH}: {e}"))?;
    let d: DisplayFile = ron::from_str(display).map_err(|e| format!("{DISPLAY_PATH}: {e}"))?;
    let items = r
        .items
        .iter()
        .map(|(id, it)| {
            let kind = match *it {
                ItemFile::Consumable { heal, max_stack } => {
                    ItemKindForm::Consumable { heal, max_stack }
                }
                ItemFile::Equipment {
                    slot,
                    attack,
                    defense,
                } => ItemKindForm::Equipment {
                    slot: slot.into(),
                    attack,
                    defense,
                },
            };
            let look = d.items.get(id);
            (
                *id,
                ItemForm {
                    kind,
                    name: look.map(|l| l.name.clone()).unwrap_or_default(),
                    color: look.map_or((0.9, 0.9, 0.9), |l| l.color),
                },
            )
        })
        .collect();
    let skills = r
        .skills
        .iter()
        .map(|(id, s)| {
            (
                *id,
                SkillForm {
                    range: s.range,
                    cooldown_ms: s.cooldown_ms,
                    damage_mult: s.damage_mult,
                    name: d.skills.get(id).map(|l| l.name.clone()).unwrap_or_default(),
                },
            )
        })
        .collect();
    let loot = r
        .loot
        .iter()
        .map(|(id, rows)| {
            (
                *id,
                rows.iter()
                    .map(|l| LootRow {
                        item: l.item,
                        count: l.count,
                        chance_per_mille: l.chance_per_mille,
                    })
                    .collect(),
            )
        })
        .collect();
    Ok(TableForms {
        items,
        skills,
        loot,
        starting_kit: r.starting_kit.iter().map(|s| s.item).collect(),
        player_attack: r.player_attack,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// 게임 규칙 설정 — 번호 표가 아닌 한 벌짜리 값들 (시드·플레이어 공격·기본 액터·진영·시작 소지품)
// ─────────────────────────────────────────────────────────────────────────────

/// 진영 관계 — 편집기용 (파일 형식 타입을 밖에 내보내지 않으려고 따로 둔다).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum RelationChoice {
    #[default]
    Hostile,
    Neutral,
    Friendly,
}

impl RelationChoice {
    pub(crate) const ALL: [Self; 3] = [Self::Hostile, Self::Neutral, Self::Friendly];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Hostile => "적대",
            Self::Neutral => "중립",
            Self::Friendly => "우호",
        }
    }

    /// 파일에 적는 이름.
    fn ron(self) -> &'static str {
        match self {
            Self::Hostile => "Hostile",
            Self::Neutral => "Neutral",
            Self::Friendly => "Friendly",
        }
    }
}

/// 진영 관계 한 줄 (대칭).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RelationRow {
    pub(crate) a: u32,
    pub(crate) b: u32,
    pub(crate) relation: RelationChoice,
}

/// 마커 종류 — 기본 액터 칸의 순서 (파일의 `default_actors` 키와 같다).
pub(crate) const MARKER_KINDS: [(&str, &str); 3] = [
    ("Player", "플레이어 스폰"),
    ("Npc", "NPC"),
    ("Monster", "몬스터"),
];

/// `rules.ron` 의 한 벌짜리 값들 — 파일에 적힌 그대로.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SettingsForm {
    pub(crate) seed: u64,
    /// 플레이어가 클릭으로 공격할 때 쓰는 스킬.
    pub(crate) player_attack: u32,
    /// [`MARKER_KINDS`] 순서의 기본 액터 번호.
    pub(crate) default_actors: [u32; 3],
    pub(crate) relations: Vec<RelationRow>,
    /// `(아이템, 개수)`.
    pub(crate) starting_kit: Vec<(u32, u32)>,
}

pub(crate) fn settings_form(rules: &str) -> Result<SettingsForm, String> {
    let r: RulesFile = ron::from_str(rules).map_err(|e| format!("{RULES_PATH}: {e}"))?;
    let actor = |k: MarkerKindFile| r.default_actors.get(&k).copied().unwrap_or(0);
    Ok(SettingsForm {
        seed: r.seed,
        player_attack: r.player_attack,
        default_actors: [
            actor(MarkerKindFile::Player),
            actor(MarkerKindFile::Npc),
            actor(MarkerKindFile::Monster),
        ],
        relations: r
            .relations
            .iter()
            .map(|x| RelationRow {
                a: x.a,
                b: x.b,
                relation: match x.relation {
                    RelationKindFile::Hostile => RelationChoice::Hostile,
                    RelationKindFile::Neutral => RelationChoice::Neutral,
                    RelationKindFile::Friendly => RelationChoice::Friendly,
                },
            })
            .collect(),
        starting_kit: r.starting_kit.iter().map(|s| (s.item, s.count)).collect(),
    })
}

/// 설정을 `rules.ron` 텍스트에 끼운다 — 그 줄·그 블록만 바꾸고 나머지 주석은 둔다.
/// 결과는 **두 파일을 함께 다시 검증**한 뒤에만 돌려준다 (없는 스킬·액터·아이템을 가리키면 거부).
///
/// ⚠ `relations`·`starting_kit`·`default_actors` 블록 **안**의 주석은 남지 않는다 (블록을 다시 쓴다).
pub(crate) fn patch_settings(
    rules: &str,
    display: &str,
    f: &SettingsForm,
) -> Result<String, String> {
    use crate::ron_patch::{replace_block, replace_field};
    let mut text = replace_field(rules, "seed", &f.seed.to_string())?;
    text = replace_field(&text, "player_attack", &f.player_attack.to_string())?;
    let actors: Vec<String> = MARKER_KINDS
        .iter()
        .zip(f.default_actors)
        .map(|((key, _), id)| format!("{key}: {id},"))
        .collect();
    text = replace_block(&text, "default_actors", '{', &actors)?;
    let relations: Vec<String> = f
        .relations
        .iter()
        .map(|r| format!("(a: {}, b: {}, relation: {}),", r.a, r.b, r.relation.ron()))
        .collect();
    text = replace_block(&text, "relations", '[', &relations)?;
    let kit: Vec<String> = f
        .starting_kit
        .iter()
        .map(|(item, count)| format!("(item: {item}, count: {count}),"))
        .collect();
    text = replace_block(&text, "starting_kit", '[', &kit)?;
    GameData::parse(&text, display)?;
    Ok(text)
}

/// 규칙·표시 두 파일을 쓴다 — **둘 다 임시 파일에 먼저** 쓴 뒤 이름을 바꾼다 (한쪽만 바뀐 채
/// 멈추는 창을 좁힌다). 부르는 쪽이 `GameData::parse` 로 검증을 마친 텍스트를 넘긴다.
pub(crate) fn write_tables(root: &Path, rules: &str, display: &str) -> Result<(), String> {
    let rules_path = root.join(RULES_PATH);
    let display_path = root.join(DISPLAY_PATH);
    let tmp_rules = rules_path.with_extension("ron.tmp");
    let tmp_display = display_path.with_extension("ron.tmp");
    let io = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    std::fs::write(&tmp_rules, rules).map_err(|e| io(&tmp_rules, e))?;
    if let Err(e) = std::fs::write(&tmp_display, display) {
        let _ = std::fs::remove_file(&tmp_rules);
        return Err(io(&tmp_display, e));
    }
    std::fs::rename(&tmp_rules, &rules_path).map_err(|e| io(&rules_path, e))?;
    std::fs::rename(&tmp_display, &display_path).map_err(|e| io(&display_path, e))
}

/// 규칙·표시 파일의 지금 텍스트 — 디스크가 있으면 그것, 없으면 내장본 (읽기 규칙과 같다).
pub(crate) fn data_texts() -> Result<(String, String), String> {
    Ok((
        read_or_embedded(Path::new(RULES_PATH), EMBEDDED_RULES)?,
        read_or_embedded(Path::new(DISPLAY_PATH), EMBEDDED_DISPLAY)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_goblin_carries_its_script() {
        let data = GameData::embedded();
        assert_eq!(
            data.actor_script(ActorId::new(102)),
            Some("scripts/goblin.rhai")
        );
        assert_eq!(data.actor_script(ActorId::new(100)), None);
        let (path, source) = data.script_sources().next().unwrap();
        assert_eq!(path, "scripts/goblin.rhai");
        assert!(source.contains("fn on_tick(me, dt)"));
    }

    #[test]
    fn script_paths_must_work_on_both_operating_systems() {
        assert!(script_path_ok("scripts/goblin.rhai"));
        assert!(script_path_ok("boss.rhai"));
        for bad in [
            "scripts/Goblin.rhai",  // 리눅스에서는 다른 파일
            "scripts\\goblin.rhai", // 리눅스에서는 이름의 일부
            "/scripts/goblin.rhai", // 절대 경로
            "C:/goblin.rhai",
            "../goblin.rhai", // data/ 밖
            "scripts//goblin.rhai",
            "scripts/goblin.lua",
        ] {
            assert!(!script_path_ok(bad), "{bad} 를 받아들였다");
        }
    }

    #[test]
    fn a_script_that_is_nowhere_stops_play_with_its_path() {
        let rules = EMBEDDED_RULES.replace("scripts/goblin.rhai", "scripts/none.rhai");
        let err = GameData::parse(&rules, EMBEDDED_DISPLAY)
            .unwrap()
            .with_scripts(|_| Ok(None))
            .unwrap_err();
        assert!(err.contains("data/scripts/none.rhai 가 없음"), "{err}");

        let rules = EMBEDDED_RULES.replace("scripts/goblin.rhai", "Scripts/Goblin.rhai");
        let err = GameData::parse(&rules, EMBEDDED_DISPLAY).unwrap_err();
        assert!(err.contains("액터 102: 스크립트 경로"), "{err}");
    }

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
