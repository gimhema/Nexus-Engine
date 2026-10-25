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
use std::time::Duration;

use nexus_core::Entity;
use nexus_sim::{
    ActorType, AiKind, EquipSlot, FactionId, Growth, ItemDef, ItemId, ItemKind as SimItemKind,
    LootEntry, LootTableId, Relation, SimWorld, SkillDef, SkillId, TriggerDef, TriggerId, UnitDef,
};
use nexus_sim::{
    ClassDef, ClassId, CraftDef, CraftId, CurrencyId, EquipRules, Formula, Price, Service, ShopDef,
    ShopId, SkillRules, StatRules, TeleportDef, TeleportId,
};
use serde::Deserialize;

use crate::class_file::{self, ClassFile, ClassLookFile, EquipRef, StatsFile};
use crate::quest_file::{self, QuestFile, QuestLookFile};
use crate::scene::{ActorId, ItemKind};
use crate::skill_file::{self, NumOr, SkillLevelRef};
use crate::trade_file::{self, CraftFile, ServiceFile, ShopFile, TeleportFile, TradeRefs};
use crate::trigger_file::{self, ConditionFile, TriggerFile};

/// 지금 읽을 수 있는 형식 번호.
const FORMAT_VERSION: u32 = 1;

pub(crate) const RULES_PATH: &str = "data/rules.ron";
pub(crate) const DISPLAY_PATH: &str = "data/display.ron";
/// 리스폰 시간 상한 (ms) — 1시간. 오타(0 을 하나 더)로 영영 안 나오는 몬스터를 막는다.
const MAX_RESPAWN_MS: u32 = 3_600_000;
/// 범위 스킬의 반경 상한 (m).
const MAX_SKILL_RADIUS: f32 = 50.0;
/// 단축키 칸 수 — 숫자키 1~9.
pub(crate) const MAX_HOTBAR: usize = 9;

const EMBEDDED_RULES: &str = include_str!("../../../data/rules.ron");
const EMBEDDED_DISPLAY: &str = include_str!("../../../data/display.ron");

/// 스크립트 경로의 기준 폴더.
pub(crate) const DATA_DIR: &str = "data";

/// 내장 스크립트 — 작업 디렉터리 밖에서 실행해도 기본 데이터가 돌게. 경로는 `data/` 기준.
/// 디스크에 같은 경로의 파일이 있으면 그것이 우선한다 (다른 데이터 파일과 같은 규칙).
const EMBEDDED_SCRIPTS: &[(&str, &str)] = &[
    (
        "scripts/goblin.rhai",
        include_str!("../../../data/scripts/goblin.rhai"),
    ),
    // 일시정지 화면 버튼 스크립트 (P17).
    (
        "scripts/ui/pause.rhai",
        include_str!("../../../data/scripts/ui/pause.rhai"),
    ),
];

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
    /// 플레이어 기본 부활 — 적지 않으면 3초 뒤 저장 지점에서 HP 가득, 패널티 없음.
    /// 액터 스크립트가 `on_dead` 를 정의하면 그 액터에는 쓰이지 않는다 (스크립트가 맡는다).
    /// 땅에 떨어진 아이템이 사라지기까지 (ms). 0 이거나 적지 않으면 영영 남는다.
    #[serde(default)]
    ground_item_ms: u32,
    #[serde(default)]
    revive: ReviveFile,
    /// 트리거 — 언제·만약·한다 (R1). 번호 순으로 판정한다. 형식은 `trigger_file` 모듈 doc.
    #[serde(default)]
    triggers: BTreeMap<u32, TriggerFile>,
    /// 기본 스탯과 파생 수식 (R2). 적지 않으면 직업도 없다. 형식은 `class_file` 모듈 doc.
    #[serde(default)]
    stats: StatsFile,
    /// 직업 (R2) — 새 게임에서 고른다.
    #[serde(default)]
    classes: BTreeMap<u32, ClassFile>,
    /// 경험치 곡선 (R3) — 변수 `level`. 적지 않으면 서버와 같은 `level * level * 100`.
    #[serde(default)]
    exp_curve: String,
    /// 쓰는 재화 번호 (R4). 이름은 `display.ron` 의 `currencies`.
    #[serde(default)]
    currencies: Vec<u32>,
    /// 값만 적은 곳(아이템 `value`·상점 `price`·처치 `bounty`·시작 재화)의 재화 (R4). 0 = 정하지 않음.
    #[serde(default)]
    default_currency: u32,
    /// 새 게임의 기본 재화 (R4).
    #[serde(default)]
    starting_currency: u32,
    /// 상점 (R4) — 형식은 `trade_file` 모듈 doc.
    #[serde(default)]
    shops: BTreeMap<u32, ShopFile>,
    /// 제작 표 (R4-2).
    #[serde(default)]
    crafts: BTreeMap<u32, CraftFile>,
    /// 텔레포트 목적지 표 (R4-2).
    #[serde(default)]
    teleports: BTreeMap<u32, TeleportFile>,
    /// 퀘스트 (R5) — 형식은 `quest_file` 모듈 doc.
    #[serde(default)]
    quests: BTreeMap<u32, QuestFile>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviveFile {
    #[serde(default = "default_revive_delay")]
    delay_ms: u32,
    #[serde(default = "default_revive_hp")]
    hp_percent: u32,
    #[serde(default)]
    exp_loss_percent: u32,
    /// 일어난 뒤 공격받지 않는 시간 (ms). 0 이면 보호 없음.
    #[serde(default)]
    protect_ms: u32,
}

impl Default for ReviveFile {
    fn default() -> Self {
        Self {
            delay_ms: default_revive_delay(),
            hp_percent: default_revive_hp(),
            exp_loss_percent: 0,
            protect_ms: 0,
        }
    }
}

fn default_revive_delay() -> u32 {
    3000
}

fn default_revive_hp() -> u32 {
    100
}

/// 플레이어 기본 부활 규칙 (`rules.ron` 의 `revive`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RevivePolicy {
    /// 쓰러진 뒤 일어나기까지.
    pub(crate) delay: Duration,
    pub(crate) hp_per_mille: u32,
    pub(crate) exp_loss_per_mille: u32,
    /// 일어난 뒤 보호 시간 (ms).
    pub(crate) protect_ms: u32,
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

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillFile {
    range: f32,
    /// 숫자 또는 수식 (R3 — 변수에 `skill_level`).
    cooldown_ms: NumOr,
    /// 피해 배율. `power`(천분율 수식)를 적으면 그것이 이긴다.
    #[serde(default = "one_f32")]
    damage_mult: f32,
    /// 위력 천분율 수식 (R3) — `"1500 + skill_level * 100"` 이면 1.6배부터.
    #[serde(default)]
    power: String,
    /// 쓸 때 드는 MP. 적지 않으면 0 — MP 를 보지 않는다. 숫자 또는 수식.
    #[serde(default)]
    mp_cost: NumOr,
    /// 범위 반경 (m). 0 이면 대상 하나.
    #[serde(default)]
    radius: f32,
    /// 회복량. 0 보다 크면 피해 대신 회복하는 스킬이다. 숫자 또는 수식.
    #[serde(default)]
    heal: NumOr,
    /// 최대 레벨 (R3). 적지 않으면 1.
    #[serde(default = "one_u32")]
    max_level: u32,
    /// 배울 수 있는 직업 (R3). 비우면 누구나.
    #[serde(default)]
    classes: Vec<u32>,
    /// 습득 조건 (R3) — 트리거의 `when` 과 같은 형식. 적지 않으면 늘 참.
    #[serde(default = "trigger_file::always")]
    requires: ConditionFile,
}

fn one_f32() -> f32 {
    1.0
}

fn one_u32() -> u32 {
    1
}

impl SkillFile {
    fn level_ref(&self, id: u32) -> SkillLevelRef<'_> {
        SkillLevelRef {
            id,
            max_level: self.max_level,
            classes: &self.classes,
            requires: &self.requires,
            power: &self.power,
            cooldown_ms: &self.cooldown_ms,
            mp_cost: &self.mp_cost,
            heal: &self.heal,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
enum ItemFile {
    Consumable {
        heal: u32,
        /// MP 회복량. 적지 않으면 0.
        #[serde(default)]
        mana: u32,
        max_stack: u32,
        /// 기본 값 (R4, 기본 재화) — 상점이 사들일 때의 기준. 0 이면 팔 수 없다.
        #[serde(default)]
        value: u32,
    },
    Equipment {
        #[serde(default)]
        value: u32,
        slot: SlotFile,
        attack: u32,
        defense: u32,
        /// 기본 스탯 보너스 (R2) — `{"str": 2}`. 이름은 `stats.base`.
        #[serde(default)]
        stats: BTreeMap<String, i64>,
        /// 입을 수 있는 직업 (R2). 비우면 누구나.
        #[serde(default)]
        classes: Vec<u32>,
    },
    /// 쓰면 `skill` 을 배운다 / 한 레벨 올린다 (R3). 소모품 가방에 들어간다.
    SkillBook {
        skill: u32,
        #[serde(default = "one_u32")]
        max_stack: u32,
        #[serde(default)]
        value: u32,
    },
}

impl RulesFile {
    /// 스탯 체계 — 블록을 적지 않았으면 `None`.
    fn stats_block(&self) -> Option<&StatsFile> {
        (!self.stats.is_absent()).then_some(&self.stats)
    }
}

impl ItemFile {
    /// 기본 값 (R4). 0 이면 팔 수 없다.
    fn value(&self) -> u32 {
        match self {
            Self::Consumable { value, .. }
            | Self::Equipment { value, .. }
            | Self::SkillBook { value, .. } => *value,
        }
    }

    fn equip_ref(&self, item: u32) -> Option<EquipRef<'_>> {
        match self {
            Self::Equipment { stats, classes, .. } => Some(EquipRef {
                item,
                stats,
                classes,
            }),
            Self::Consumable { .. } | Self::SkillBook { .. } => None,
        }
    }
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
    /// 포털은 액터를 세우지 않으므로 기본 액터가 없다 (R6).
    fn of(kind: ItemKind) -> Option<Self> {
        match kind {
            ItemKind::PlayerSpawn => Some(Self::Player),
            ItemKind::Npc => Some(Self::Npc),
            ItemKind::Monster => Some(Self::Monster),
            ItemKind::Portal => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UnitFile {
    move_speed: f32,
    max_hp: u32,
    /// 최대 MP. 적지 않으면 0 — MP 가 없는 액터.
    #[serde(default)]
    max_mp: u32,
    /// 초당 MP 회복량.
    #[serde(default)]
    mp_regen: u32,
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
    /// 단축키 스킬 (조작하는 액터만 쓴다) — 1번 키가 첫 스킬. 최대 [`MAX_HOTBAR`] 개.
    #[serde(default)]
    skills: Vec<u32>,
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
    /// 죽은 뒤 리스폰까지 (ms). 적지 않으면 0 — 리스폰하지 않고 시체로 남는다.
    #[serde(default)]
    respawn_ms: u32,
    /// 이 액터를 쓰러뜨린 쪽이 받는 재화 (R4, 기본 재화).
    #[serde(default)]
    bounty: u32,
    /// NPC 기능 (R4) — `[Shop(1), Heal(price: 20)]`. 이것이 붙으면 상호작용할 수 있다.
    #[serde(default)]
    services: Vec<ServiceFile>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrowthFile {
    #[serde(default)]
    max_hp: u32,
    #[serde(default)]
    max_mp: u32,
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
    /// 진영의 표시 이름 (P19). 번호는 `rules.ron` 의 진영 번호(서버 `EFactionId`)와 같다. 없으면 번호로 보인다.
    #[serde(default)]
    factions: BTreeMap<u32, SkillLookFile>,
    /// 직업 이름·설명 (R2). 번호는 `rules.ron` 의 `classes`.
    #[serde(default)]
    classes: BTreeMap<u32, ClassLookFile>,
    /// 기본 스탯의 화면 이름 (R2) — `{"str": (name: "힘")}`. 없으면 규칙 이름 그대로.
    #[serde(default)]
    stats: BTreeMap<String, SkillLookFile>,
    /// 재화 이름 (R4) — `{1: (name: "골드")}`. 번호는 `rules.ron` 의 `currencies`.
    #[serde(default)]
    currencies: BTreeMap<u32, SkillLookFile>,
    /// 텔레포트 목적지 이름 (R4-2) — `{1: ["광장", "샘플 존"]}`, `rules.ron` 의 목적지 순서대로.
    #[serde(default)]
    teleports: BTreeMap<u32, Vec<String>>,
    /// 퀘스트 제목·설명 (R5).
    #[serde(default)]
    quests: BTreeMap<u32, QuestLookFile>,
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
    /// NPC 프로필 — 설정 글 (R4-2). NPC 창 머리에 보인다. 표시 데이터라 여기에 있다.
    #[serde(default)]
    profile: String,
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
    /// NPC 프로필 글 (R4-2). 없으면 빈 문자열.
    pub(crate) profile: String,
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
    /// 액터 타입별 단축키 스킬 — 비어 있는 타입은 빠진다.
    actor_skills: BTreeMap<ActorId, Vec<SkillId>>,
    /// 스킬의 표시 이름 (`display.ron`).
    skill_names: BTreeMap<SkillId, String>,
    /// 진영의 표시 이름 (`display.ron`).
    faction_names: BTreeMap<u32, String>,
    /// 땅의 아이템 소멸 시간 — `None` 이면 남는다.
    ground_item_lifetime: Option<Duration>,
    revive: RevivePolicy,
    /// 경로 → 스크립트 본문. 컴파일은 플레이를 시작할 때 한다.
    script_sources: BTreeMap<String, String>,
    /// 트리거 (R1) — 번호 순.
    triggers: Vec<(TriggerId, TriggerDef)>,
    /// 스탯 체계 (R2). 없으면 직업이 없는 게임이다.
    stat_rules: Option<StatRules>,
    /// 직업 — 번호 순.
    classes: Vec<(ClassId, ClassDef)>,
    class_skills: BTreeMap<ClassId, Vec<SkillId>>,
    /// 직업 `(이름, 설명)` (`display.ron`).
    class_looks: BTreeMap<ClassId, (String, String)>,
    /// 기본 스탯 규칙 이름 → 화면 이름.
    stat_names: BTreeMap<String, String>,
    /// 장비의 스탯 보너스·직업 제한 — 둘 다 없는 장비는 빠진다.
    equip_rules: Vec<(ItemId, EquipRules)>,
    /// 스킬 부착 규칙·레벨 수식 (R3) — 특별한 것이 없는 스킬은 빠진다.
    skill_rules: Vec<(SkillId, SkillRules)>,
    /// 경험치 곡선 (R3). `None` 이면 기본 곡선.
    exp_curve: Option<Formula>,
    /// 상점 (R4).
    shops: Vec<(ShopId, ShopDef)>,
    /// 액터 타입별 NPC 기능 (R4).
    services: BTreeMap<ActorId, Vec<Service>>,
    /// 아이템 기본 값 (R4).
    item_prices: Vec<(ItemId, Price)>,
    /// 새 게임의 시작 재화 (R4).
    starting_currency: Option<Price>,
    /// 기본 재화 (R4) — HUD `{gold}`·상점 표시.
    default_currency: Option<CurrencyId>,
    /// 재화 이름 (`display.ron`).
    currency_names: BTreeMap<u32, String>,
    /// 제작 표 (R4-2).
    crafts: Vec<(CraftId, CraftDef)>,
    /// 텔레포트 표 (R4-2).
    teleports: Vec<(TeleportId, TeleportDef)>,
    /// 다른 레벨로 가는 목적지의 레벨 이름 — `travel` 번호 - 1 이 색인이다 (R4-2).
    travel_levels: Vec<String>,
    /// 목적지 이름 (`display.ron`).
    destination_names: BTreeMap<u32, Vec<String>>,
    /// 퀘스트 (R5).
    quests: Vec<(nexus_sim::QuestId, nexus_sim::QuestDef)>,
    /// 퀘스트 제목·설명 (`display.ron`).
    quest_looks: BTreeMap<u32, QuestLookFile>,
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

    /// 시험용 — 규칙 텍스트를 고치고 스크립트 본문을 직접 넘긴다 (`(data/ 기준 경로, 본문)`).
    /// 목록에 없는 경로는 내장본에서 찾는다.
    #[cfg(test)]
    pub(crate) fn parse_with_scripts(
        rules: &str,
        scripts: &[(&str, &str)],
    ) -> Result<Self, String> {
        Self::parse(rules, EMBEDDED_DISPLAY)?.with_scripts(|path| {
            Ok(scripts
                .iter()
                .find(|(p, _)| *p == path)
                .map(|(_, s)| (*s).to_owned())
                .or_else(|| embedded_script(path).map(str::to_owned)))
        })
    }

    /// 시험용 — 내장 규칙 텍스트.
    #[cfg(test)]
    pub(crate) fn embedded_rules() -> &'static str {
        EMBEDDED_RULES
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
            check(
                s.radius.is_finite() && (0.0..=MAX_SKILL_RADIUS).contains(&s.radius),
                format!("스킬 {id}: radius 는 0~{MAX_SKILL_RADIUS}m"),
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
            check(
                u.respawn_ms <= MAX_RESPAWN_MS,
                format!("액터 {id}: respawn_ms 는 {MAX_RESPAWN_MS} 이하 (1시간)"),
            );
            if let Some(skill) = u.basic_attack {
                check(
                    r.skills.contains_key(&skill),
                    format!("액터 {id}: 없는 스킬 {skill}"),
                );
                // 레벨이 올라도 MP 가 늘지 않는데 비용이 최대 MP 보다 크면 영영 칠 수 없다 —
                // AI 가 사거리 안에서 서서 기다리기만 한다.
                if let Some(s) = r.skills.get(&skill) {
                    // 회복 스킬을 기본 공격으로 두면 AI 가 적에게 쓰려다 매번 거절된다.
                    check(
                        s.heal.is_zero(),
                        format!("액터 {id}: 기본 공격 스킬 {skill} 이(가) 회복 스킬"),
                    );
                    // MP 가 수식이면 레벨마다 달라 미리 볼 수 없다 — 숫자일 때만 본다.
                    let mp = s.mp_cost.fixed().unwrap_or(0);
                    check(
                        mp <= u.max_mp || u.growth.max_mp > 0,
                        format!(
                            "액터 {id}: 기본 공격 스킬 {skill} 의 MP {mp} 이(가) 최대 MP {} 보다 커서 쓸 수 없음",
                            u.max_mp
                        ),
                    );
                }
            }
            check(
                u.skills.len() <= MAX_HOTBAR,
                format!("액터 {id}: 단축키 스킬은 {MAX_HOTBAR}개까지 (숫자키 1~9)"),
            );
            for (i, skill) in u.skills.iter().enumerate() {
                match r.skills.get(skill) {
                    None => check(
                        false,
                        format!("액터 {id}: 단축키 {}: 없는 스킬 {skill}", i + 1),
                    ),
                    // 기본 공격과 같은 이유 — 영영 못 쓰는 칸은 저작 실수다.
                    Some(s) => {
                        let mp = s.mp_cost.fixed().unwrap_or(0);
                        check(
                            mp <= u.max_mp || u.growth.max_mp > 0,
                            format!(
                                "액터 {id}: 단축키 {} 스킬 {skill} 의 MP {mp} 이(가) 최대 MP {} 보다 커서 쓸 수 없음",
                                i + 1,
                                u.max_mp
                            ),
                        );
                    }
                }
                check(
                    !u.skills[..i].contains(skill),
                    format!("액터 {id}: 단축키에 스킬 {skill} 이(가) 두 번 있음"),
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
        check(
            r.skills
                .get(&r.player_attack)
                .is_none_or(|s| s.heal.is_zero()),
            format!(
                "player_attack: 스킬 {} 이(가) 회복 스킬 — 클릭 공격에 쓸 수 없음",
                r.player_attack
            ),
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
        for (id, look) in &d.factions {
            // 0 은 "진영 없음"(`FactionId::NONE`) — 이름을 붙이면 헷갈린다.
            check(*id != 0, String::from("표시 진영 0: '진영 없음' 예약 번호"));
            check(
                !look.name.trim().is_empty(),
                format!("표시 진영 {id}: 이름이 비어 있음"),
            );
        }
        check(
            (1..=100).contains(&r.revive.hp_percent)
                && r.revive.exp_loss_percent <= 100
                && r.revive.delay_ms <= MAX_RESPAWN_MS
                && r.revive.protect_ms <= MAX_RESPAWN_MS,
            String::from(
                "revive: hp_percent 는 1~100, exp_loss_percent 는 0~100, delay_ms·protect_ms 는 1시간 이하",
            ),
        );
        check(
            r.ground_item_ms <= MAX_RESPAWN_MS,
            String::from("ground_item_ms 는 1시간 이하"),
        );
        let equipment: Vec<EquipRef<'_>> = r
            .items
            .iter()
            .filter_map(|(id, i)| i.equip_ref(*id))
            .collect();
        errors.extend(class_file::validate(
            r.stats_block(),
            &r.classes,
            &equipment,
            |s| r.skills.contains_key(&s),
        ));
        for id in d.classes.keys().filter(|id| !r.classes.contains_key(id)) {
            errors.push(format!("표시 직업 {id}: rules.ron 에 없는 직업"));
        }
        for name in d
            .stats
            .keys()
            .filter(|n| r.stats_block().is_none_or(|s| !s.base_has(n)))
        {
            errors.push(format!(
                "표시 스탯 '{name}': rules.ron 의 stats.base 에 없음"
            ));
        }
        // 조건·효과가 가리키는 번호 — 트리거·스킬·레시피·퀘스트가 함께 본다.
        let item_exists = |i: u32| r.items.contains_key(&i);
        let actor_exists = |a: u32| r.actors.contains_key(&a);
        let skill_exists = |s: u32| r.skills.contains_key(&s);
        let quest_exists = |q: u32| r.quests.contains_key(&q);
        let loot_exists = |t: u32| r.loot.contains_key(&t);
        let known = trigger_file::Known {
            item: &item_exists,
            actor: &actor_exists,
            skill: &skill_exists,
            quest: &quest_exists,
            loot: &loot_exists,
        };
        errors.extend(trigger_file::validate(&r.triggers, &known));
        errors.extend(quest_file::validate(&r.quests, &known));
        // 스킬의 R3 칸 — 레벨 수식·부착 규칙.
        let stat_names: Vec<String> = r
            .stats_block()
            .map(|s| s.names().to_vec())
            .unwrap_or_default();
        for (id, s) in &r.skills {
            errors.extend(skill_file::validate(
                &s.level_ref(*id),
                &stat_names,
                |c| r.classes.contains_key(&c),
                &known,
            ));
        }
        for (id, item) in &r.items {
            if let ItemFile::SkillBook { skill, .. } = item
                && !r.skills.contains_key(skill)
            {
                errors.push(format!("아이템 {id}: 스킬북이 없는 스킬 {skill} 을 가리킴"));
            }
        }
        let exp_curve = match skill_file::exp_curve(&r.exp_curve) {
            Ok(curve) => curve,
            Err(e) => {
                errors.push(e);
                None
            }
        };
        // 재화·상점·NPC 기능 (R4).
        let default_currency = (r.default_currency != 0).then_some(r.default_currency);
        let trade = TradeRefs {
            currencies: &r.currencies,
            default_currency,
            shops: &r.shops,
            crafts: &r.crafts,
            teleports: &r.teleports,
            quest_exists: &|q| r.quests.contains_key(&q),
        };
        for (id, craft) in &r.crafts {
            for (i, recipe) in craft.recipes.iter().enumerate() {
                let mut err =
                    |m: String| errors.push(format!("제작 {id}: 레시피 {i}: requires — {m}"));
                trigger_file::check_condition(&recipe.requires, 0, &known, &mut err);
            }
        }
        let uses_default = r.starting_currency > 0
            || r.items.values().any(|i| i.value() > 0)
            || r.actors.values().any(|u| u.bounty > 0);
        errors.extend(trade_file::validate(&trade, uses_default, |i| {
            r.items.contains_key(&i)
        }));
        for (id, u) in &r.actors {
            errors.extend(trade_file::validate_services(*id, &u.services, &trade));
        }
        for id in d.quests.keys().filter(|q| !r.quests.contains_key(q)) {
            errors.push(format!("표시 퀘스트 {id}: rules.ron 에 없는 퀘스트"));
        }
        for id in d.currencies.keys().filter(|c| !r.currencies.contains(c)) {
            errors.push(format!("표시 재화 {id}: rules.ron 의 currencies 에 없음"));
        }
        if !errors.is_empty() {
            return Err(format!("게임 데이터 오류 — {}", errors.join(" / ")));
        }
        let built = class_file::build(r.stats_block(), &r.classes);
        // 다른 레벨로 가는 목적지의 레벨 이름 — 처음 나온 순서대로 1, 2, 3 … (R4-2).
        let mut travel_levels: Vec<String> = Vec::new();
        for t in r.teleports.values() {
            for d in &t.destinations {
                let level = d.level.trim();
                if !level.is_empty() && !travel_levels.iter().any(|l| l == level) {
                    travel_levels.push(level.to_owned());
                }
            }
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
                        // 수식 칸의 숫자 값은 자리만 채운다 — 쓸 때 `effective_skill` 이 수식으로 덮는다.
                        // 회복 수식이면 1 로 두어 "회복 스킬" 로 분류되게 한다.
                        SkillDef {
                            range: s.range,
                            cooldown_ms: s.cooldown_ms.base(0),
                            damage_mult: s.damage_mult,
                            mp_cost: s.mp_cost.base(0),
                            radius: s.radius,
                            heal: s.heal.base(1),
                        },
                    )
                })
                .collect(),
            items: r
                .items
                .iter()
                .map(|(id, i)| (ItemId(*id), i.clone().into()))
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
                .map(|(id, u)| {
                    let def = UnitDef {
                        actor: ActorType(*id),
                        bounty: (u.bounty > 0)
                            .then(|| trade_file::price(default_currency, u.bounty)),
                        ..u.into()
                    };
                    (ActorId::new(*id), def)
                })
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
                            profile: l.profile,
                        },
                    )
                })
                .collect(),
            actor_scripts: r
                .actors
                .iter()
                .filter_map(|(id, u)| Some((ActorId::new(*id), u.script.clone()?)))
                .collect(),
            actor_skills: r
                .actors
                .iter()
                .filter(|(_, u)| !u.skills.is_empty())
                .map(|(id, u)| {
                    (
                        ActorId::new(*id),
                        u.skills.iter().copied().map(SkillId).collect(),
                    )
                })
                .collect(),
            skill_names: d
                .skills
                .iter()
                .map(|(id, l)| (SkillId(*id), l.name.clone()))
                .collect(),
            faction_names: d
                .factions
                .iter()
                .map(|(id, l)| (*id, l.name.clone()))
                .collect(),
            ground_item_lifetime: (r.ground_item_ms > 0)
                .then(|| Duration::from_millis(u64::from(r.ground_item_ms))),
            revive: RevivePolicy {
                delay: Duration::from_millis(u64::from(r.revive.delay_ms)),
                hp_per_mille: r.revive.hp_percent * 10,
                exp_loss_per_mille: r.revive.exp_loss_percent * 10,
                protect_ms: r.revive.protect_ms,
            },
            script_sources: BTreeMap::new(),
            triggers: trigger_file::to_sim(&r.triggers),
            stat_rules: built.as_ref().map(|(rules, _)| rules.clone()),
            classes: built.map(|(_, c)| c).unwrap_or_default(),
            class_skills: r
                .classes
                .iter()
                .filter(|(_, c)| !c.skills.is_empty())
                .map(|(id, c)| (ClassId(*id), c.skills.iter().map(|s| SkillId(*s)).collect()))
                .collect(),
            class_looks: d
                .classes
                .iter()
                .map(|(id, l)| (ClassId(*id), (l.name.clone(), l.description.clone())))
                .collect(),
            stat_names: d
                .stats
                .iter()
                .map(|(k, l)| (k.clone(), l.name.clone()))
                .collect(),
            skill_rules: r
                .skills
                .iter()
                .filter_map(|(id, s)| {
                    Some((
                        SkillId(*id),
                        skill_file::rules(&s.level_ref(*id), &stat_names)?,
                    ))
                })
                .collect(),
            exp_curve,
            shops: r
                .shops
                .iter()
                .map(|(id, s)| (ShopId(*id), trade_file::shop(s, default_currency)))
                .collect(),
            services: r
                .actors
                .iter()
                .filter(|(_, u)| !u.services.is_empty())
                .map(|(id, u)| {
                    (
                        ActorId::new(*id),
                        trade_file::services(&u.services, default_currency),
                    )
                })
                .collect(),
            item_prices: r
                .items
                .iter()
                .filter(|(_, i)| i.value() > 0)
                .map(|(id, i)| (ItemId(*id), trade_file::price(default_currency, i.value())))
                .collect(),
            starting_currency: (r.starting_currency > 0)
                .then(|| trade_file::price(default_currency, r.starting_currency)),
            default_currency: default_currency.map(CurrencyId),
            currency_names: d
                .currencies
                .iter()
                .map(|(id, l)| (*id, l.name.clone()))
                .collect(),
            crafts: r
                .crafts
                .iter()
                .map(|(id, c)| (CraftId(*id), trade_file::craft(c, default_currency)))
                .collect(),
            teleports: r
                .teleports
                .iter()
                .map(|(id, t)| {
                    (
                        TeleportId(*id),
                        trade_file::teleport(t, default_currency, |level| {
                            travel_levels
                                .iter()
                                .position(|l| l == level)
                                .map_or(0, |i| u32::try_from(i + 1).unwrap_or(0))
                        }),
                    )
                })
                .collect(),
            travel_levels: travel_levels.clone(),
            destination_names: d.teleports.clone(),
            quests: quest_file::to_sim(&r.quests),
            quest_looks: d.quests.clone(),
            equip_rules: equipment
                .iter()
                .filter(|e| !e.stats.is_empty() || !e.classes.is_empty())
                .map(|e| (ItemId(e.item), class_file::equip_rules(r.stats_block(), e)))
                .collect(),
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
        world.set_ground_item_lifetime(self.ground_item_lifetime);
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
        for (id, def) in &self.triggers {
            world.define_trigger(*id, def.clone());
        }
        if let Some(rules) = &self.stat_rules {
            world.set_stat_rules(rules.clone());
        }
        for (id, def) in &self.classes {
            world.define_class(*id, def.clone());
        }
        for (id, rules) in &self.equip_rules {
            world.define_equip_rules(*id, rules.clone());
        }
        for (id, rules) in &self.skill_rules {
            world.define_skill_rules(*id, rules.clone());
        }
        if let Some(curve) = &self.exp_curve {
            world.set_exp_curve(curve.clone());
        }
        for (id, shop) in &self.shops {
            world.define_shop(*id, shop.clone());
        }
        for (actor, list) in &self.services {
            world.define_services(ActorType(actor.raw()), list.clone());
        }
        for &(item, price) in &self.item_prices {
            world.define_item_price(item, price);
        }
        for (id, def) in &self.crafts {
            world.define_craft(*id, def.clone());
        }
        for (id, def) in &self.teleports {
            world.define_teleport(*id, def.clone());
        }
        for (id, def) in &self.quests {
            world.define_quest(*id, def.clone());
        }
    }

    /// 퀘스트 제목 — 표시 파일에 없으면 `퀘스트 #N` (R5).
    pub(crate) fn quest_title(&self, id: nexus_sim::QuestId) -> String {
        self.quest_looks
            .get(&id.0)
            .map_or_else(|| format!("퀘스트 #{}", id.0), |l| l.title.clone())
    }

    /// 퀘스트 설명 (없으면 빈 문자열).
    pub(crate) fn quest_description(&self, id: nexus_sim::QuestId) -> &str {
        self.quest_looks
            .get(&id.0)
            .map_or("", |l| l.description.as_str())
    }

    /// 액터 타입 이름 — 목표 글("슬라임 처치")에 쓴다.
    pub(crate) fn actor_type_name(&self, actor: nexus_sim::ActorType) -> String {
        self.actor_looks
            .get(&ActorId::new(actor.0))
            .map_or_else(|| format!("액터 #{}", actor.0), |l| l.name.clone())
    }

    /// `Event::Traveled` 의 `travel` 번호가 가리키는 레벨 이름 (R4-2).
    pub(crate) fn travel_level(&self, travel: u32) -> Option<&str> {
        let i = usize::try_from(travel.checked_sub(1)?).ok()?;
        self.travel_levels.get(i).map(String::as_str)
    }

    /// 목적지 이름 — 표시 파일에 없으면 `목적지 N`.
    pub(crate) fn destination_name(&self, teleport: TeleportId, index: usize) -> String {
        self.destination_names
            .get(&teleport.0)
            .and_then(|names| names.get(index))
            .cloned()
            .unwrap_or_else(|| format!("목적지 {}", index + 1))
    }

    /// 기본 재화 (R4). 없으면 재화가 없는 게임.
    pub(crate) fn default_currency(&self) -> Option<CurrencyId> {
        self.default_currency
    }

    /// 재화 이름 — 표시 파일에 없으면 번호로.
    pub(crate) fn currency_name(&self, id: CurrencyId) -> String {
        self.currency_names
            .get(&id.0)
            .cloned()
            .unwrap_or_else(|| format!("재화 #{}", id.0))
    }

    /// 값을 사람이 읽을 글로 — `30 골드`.
    pub(crate) fn price_text(&self, price: Price) -> String {
        format!("{} {}", price.amount, self.currency_name(price.currency))
    }

    /// 직업 번호 — 번호 순 (R2). 비어 있으면 직업이 없는 게임이다.
    pub(crate) fn class_ids(&self) -> impl Iterator<Item = ClassId> + '_ {
        self.classes.iter().map(|(id, _)| *id)
    }

    /// 새 게임에서 따로 고르지 않았을 때의 직업 — 가장 작은 번호.
    pub(crate) fn default_class(&self) -> Option<ClassId> {
        self.class_ids().next()
    }

    pub(crate) fn has_class(&self, id: ClassId) -> bool {
        self.classes.iter().any(|(c, _)| *c == id)
    }

    /// 직업 이름. 표시 파일에 없으면 번호로.
    pub(crate) fn class_name(&self, id: ClassId) -> String {
        self.class_looks
            .get(&id)
            .map_or_else(|| format!("직업 #{}", id.0), |(n, _)| n.clone())
    }

    /// 직업의 단축키 스킬. 비어 있으면 액터 타입의 것을 쓴다.
    pub(crate) fn class_skills(&self, id: ClassId) -> Option<&[SkillId]> {
        self.class_skills.get(&id).map(Vec::as_slice)
    }

    /// 기본 스탯의 화면 이름 — 표시 파일에 없으면 규칙 이름 그대로.
    pub(crate) fn stat_name<'a>(&'a self, key: &'a str) -> &'a str {
        self.stat_names.get(key).map_or(key, String::as_str)
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
        MarkerKindFile::of(kind)
            .and_then(|k| self.default_actors.get(&k))
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
        self.give_starting_currency(world, player);
    }

    /// 시작 재화 (R4) — 새 게임, 그리고 재화가 생기기 전의 저장으로 이어 할 때.
    pub(crate) fn give_starting_currency(&self, world: &mut SimWorld, player: Entity) {
        if let Some(p) = self.starting_currency {
            world.set_currency(player, p.currency, u64::from(p.amount));
        }
    }

    pub(crate) fn player_attack(&self) -> SkillId {
        self.player_attack
    }

    /// 액터 타입의 단축키 스킬 — 1번 키가 맨 앞. 없으면 빈 목록.
    pub(crate) fn actor_skills(&self, actor: ActorId) -> &[SkillId] {
        self.actor_skills.get(&actor).map_or(&[], Vec::as_slice)
    }

    /// 플레이어 기본 부활 규칙.
    pub(crate) fn revive(&self) -> RevivePolicy {
        self.revive
    }

    /// 이 액터 타입이 부활을 스크립트로 맡는가 — 스크립트가 `on_dead` 를 정의했으면.
    pub(crate) fn scripted_revive(&self, actor: ActorId) -> bool {
        self.actor_script(actor)
            .and_then(|path| self.script_sources.get(path))
            .and_then(|src| nexus_script::check(src).ok())
            .is_some_and(|c| c.hooks.contains(&nexus_script::DEAD_HOOK))
    }

    /// 진영 이름 `(번호, 이름)` — 번호 순. 인스펙터가 번호 옆에 보여 준다.
    pub(crate) fn faction_list(&self) -> Vec<(u32, &str)> {
        self.faction_names
            .iter()
            .map(|(id, name)| (*id, name.as_str()))
            .collect()
    }

    /// 스킬 이름. 표시 파일에 없으면 번호로.
    pub(crate) fn skill_name(&self, id: SkillId) -> String {
        self.skill_names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("스킬 #{}", id.0))
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

/// 스크립트 본문 하나 — `data/` 기준 경로. 디스크가 우선, 없으면 내장본, 그것도 없으면 `None`.
/// 데이터 파일과 같은 규칙이다: 있는데 못 읽으면 오류 (조용히 내장본으로 넘어가지 않는다).
pub(crate) fn read_script(path: &str) -> Result<Option<String>, String> {
    match std::fs::read_to_string(Path::new(DATA_DIR).join(path)) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(embedded_script(path).map(str::to_owned))
        }
        Err(e) => Err(format!("{DATA_DIR}/{path}: {e}")),
    }
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
            ItemFile::Consumable {
                heal,
                mana,
                max_stack,
                ..
            } => Self {
                kind: SimItemKind::Consumable { heal, mana },
                max_stack,
            },
            ItemFile::Equipment {
                slot,
                attack,
                defense,
                ..
            } => Self {
                kind: SimItemKind::Equipment {
                    slot: slot.into(),
                    attack,
                    defense,
                },
                max_stack: 1,
            },
            ItemFile::SkillBook {
                skill, max_stack, ..
            } => Self {
                kind: SimItemKind::SkillBook {
                    skill: SkillId(skill),
                },
                max_stack,
            },
        }
    }
}

impl From<&UnitFile> for UnitDef {
    /// 수치만 옮긴다 — 액터 번호는 표의 키라 파일 항목 안에 없다. 표를 읽는 쪽이 채운다.
    fn from(u: &UnitFile) -> Self {
        Self {
            actor: ActorType::default(),
            class: None,
            // 처치 보상은 기본 재화가 필요해 표를 읽는 쪽이 채운다.
            bounty: None,
            move_speed: u.move_speed,
            max_hp: u.max_hp,
            max_mp: u.max_mp,
            mp_regen: u.mp_regen,
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
                max_mp: u.growth.max_mp,
                attack: u.growth.attack,
                defense: u.growth.defense,
            },
            respawn_ms: u.respawn_ms,
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
    pub(crate) max_mp: u32,
    pub(crate) mp_regen: u32,
    pub(crate) attack: u32,
    pub(crate) defense: u32,
    pub(crate) immortal: bool,
    pub(crate) faction: u32,
    pub(crate) ai: AiChoice,
    pub(crate) aggro_range: f32,
    pub(crate) leash_range: f32,
    pub(crate) basic_attack: Option<u32>,
    /// 단축키 스킬 — 1번 키가 맨 앞.
    pub(crate) skills: Vec<u32>,
    pub(crate) loot: Option<u32>,
    pub(crate) script: Option<String>,
    pub(crate) exp_reward: u32,
    /// 레벨당 성장 — (HP, 공격, 방어).
    pub(crate) growth: (u32, u32, u32),
    /// 레벨당 최대 MP 성장. 튜플에 넣지 않았다 — 기존 편집기·테스트가 튜플 순서에 기대고 있다.
    pub(crate) growth_mp: u32,
    /// 리스폰까지 (ms). 0 = 리스폰하지 않음.
    pub(crate) respawn_ms: u32,
    /// 처치 보상 재화 (R4, 기본 재화).
    pub(crate) bounty: u32,
    /// NPC 기능 (R4) — 편집기가 고치지 않아도 저장할 때 그대로 다시 쓴다.
    pub(crate) services: Vec<ServiceFile>,
    // 표시 반쪽 (display.ron)
    pub(crate) name: String,
    pub(crate) sheet: String,
    pub(crate) tint: (f32, f32, f32),
    /// NPC 프로필 글 (R4-2) — 표시 반쪽.
    pub(crate) profile: String,
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
            max_mp: 0,
            mp_regen: 0,
            attack: 5,
            defense: 0,
            immortal: false,
            faction: 0,
            ai: AiChoice::Passive,
            aggro_range: 0.0,
            leash_range: 0.0,
            basic_attack: None,
            skills: Vec::new(),
            loot: None,
            script: None,
            exp_reward: 0,
            growth: (0, 0, 0),
            growth_mp: 0,
            respawn_ms: 0,
            bounty: 0,
            services: Vec::new(),
            name: name.to_owned(),
            sheet: String::new(),
            tint: (1.0, 1.0, 1.0),
            profile: String::new(),
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
        if self.max_mp != 0 {
            line(format!("max_mp: {}", self.max_mp));
        }
        if self.mp_regen != 0 {
            line(format!("mp_regen: {}", self.mp_regen));
        }
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
        if !self.skills.is_empty() {
            let list: Vec<String> = self.skills.iter().map(u32::to_string).collect();
            line(format!("skills: [{}]", list.join(", ")));
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
        if self.respawn_ms != 0 {
            line(format!("respawn_ms: {}", self.respawn_ms));
        }
        if self.bounty != 0 {
            line(format!("bounty: {}", self.bounty));
        }
        if !self.services.is_empty() {
            let list: Vec<String> = self.services.iter().map(ServiceFile::ron).collect();
            line(format!("services: [{}]", list.join(", ")));
        }
        let (hp, atk, def) = self.growth;
        let mp = self.growth_mp;
        if (hp, atk, def, mp) != (0, 0, 0, 0) {
            // MP 성장은 있을 때만 적는다 — MP 를 안 쓰는 액터의 줄이 예전 모양 그대로 남게.
            let mp = if mp == 0 {
                String::new()
            } else {
                format!("max_mp: {mp}, ")
            };
            line(format!(
                "growth: (max_hp: {hp}, {mp}attack: {atk}, defense: {def})"
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
        if !self.profile.trim().is_empty() {
            s.push_str(&format!(
                "    profile: {},\n",
                crate::ron_patch::quote(self.profile.trim())
            ));
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
                max_mp: u.max_mp,
                mp_regen: u.mp_regen,
                attack: u.attack,
                defense: u.defense,
                immortal: u.immortal,
                faction: u.faction,
                ai: u.ai.into(),
                aggro_range: u.aggro_range,
                leash_range: u.leash_range,
                basic_attack: u.basic_attack,
                skills: u.skills.clone(),
                loot: u.loot,
                script: u.script.clone(),
                exp_reward: u.exp_reward,
                growth: (u.growth.max_hp, u.growth.attack, u.growth.defense),
                growth_mp: u.growth.max_mp,
                respawn_ms: u.respawn_ms,
                bounty: u.bounty,
                services: u.services.clone(),
                ..ActorForm::new("")
            },
        );
    }
    for (id, look) in d.actors {
        let form = out.entry(id).or_insert_with(|| ActorForm::new(""));
        form.name = look.name;
        form.sheet = look.sheet;
        form.tint = look.tint;
        form.profile = look.profile;
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
    /// 기본 값 (R4, 기본 재화) — 상점이 사들일 때의 기준. 0 이면 팔 수 없다.
    pub(crate) value: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ItemKindForm {
    Consumable {
        heal: u32,
        mana: u32,
        max_stack: u32,
    },
    Equipment {
        slot: SlotChoice,
        attack: u32,
        defense: u32,
        /// 기본 스탯 보너스·직업 제한 (R2) — 표 편집기가 고치지 않아도 저장할 때 그대로 다시 쓴다.
        bonus: EquipBonusForm,
    },
    /// 스킬북 (R3) — 쓰면 그 스킬을 배운다 / 한 레벨 올린다.
    SkillBook { skill: u32, max_stack: u32 },
}

/// 장비의 스탯 보너스·직업 제한 (R2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct EquipBonusForm {
    /// `(스탯 이름, 더하는 값)` — 이름 순.
    pub(crate) stats: Vec<(String, i64)>,
    /// 비우면 누구나.
    pub(crate) classes: Vec<u32>,
}

impl EquipBonusForm {
    /// 항목 줄에 붙일 꼬리 — 비어 있으면 아무것도 없다 (예전 항목이 바이트 그대로 남게).
    fn file_tail(&self) -> String {
        let mut tail = String::new();
        if !self.stats.is_empty() {
            let list: Vec<String> = self
                .stats
                .iter()
                .map(|(k, v)| format!("\"{k}\": {v}"))
                .collect();
            tail.push_str(&format!(", stats: {{{}}}", list.join(", ")));
        }
        if !self.classes.is_empty() {
            let list: Vec<String> = self.classes.iter().map(u32::to_string).collect();
            tail.push_str(&format!(", classes: [{}]", list.join(", ")));
        }
        tail
    }
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
                mana: 0,
                max_stack: 20,
            },
            name: name.to_owned(),
            color: (0.9, 0.9, 0.9),
            value: 0,
        }
    }

    /// `rules.ron` 의 `items` 항목 — 손으로 쓴 것과 같은 한 줄.
    pub(crate) fn rules_entry(&self, id: u32) -> String {
        let line = self.kind_entry(id);
        // 값은 있을 때만 — 예전 항목이 바이트 그대로 남게 (R4).
        match line.strip_suffix("),") {
            Some(head) if self.value > 0 => format!("{head}, value: {}),", self.value),
            _ => line,
        }
    }

    fn kind_entry(&self, id: u32) -> String {
        match &self.kind {
            ItemKindForm::Consumable {
                heal,
                mana,
                max_stack,
            } => {
                // MP 회복은 있을 때만 적는다 — 예전 항목이 바이트 그대로 남게.
                let mana = if *mana == 0 {
                    String::new()
                } else {
                    format!("mana: {mana}, ")
                };
                format!("{id}: Consumable(heal: {heal}, {mana}max_stack: {max_stack}),")
            }
            ItemKindForm::Equipment {
                slot,
                attack,
                defense,
                bonus,
            } => format!(
                "{id}: Equipment(slot: {}, attack: {attack}, defense: {defense}{}),",
                slot.file_name(),
                bonus.file_tail()
            ),
            ItemKindForm::SkillBook { skill, max_stack } => {
                format!("{id}: SkillBook(skill: {skill}, max_stack: {max_stack}),")
            }
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
    pub(crate) mp_cost: u32,
    pub(crate) radius: f32,
    pub(crate) heal: u32,
    pub(crate) name: String,
    /// 레벨·부착 규칙 (R3) — 고치지 않아도 저장할 때 그대로 다시 쓴다.
    pub(crate) level: SkillLevelForm,
}

/// 스킬의 R3 칸. **수식 칸이 비어 있지 않으면 같은 이름의 숫자 칸보다 이긴다.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SkillLevelForm {
    pub(crate) max_level: u32,
    pub(crate) classes: Vec<u32>,
    /// 습득 조건 — RON 텍스트(`Level(cmp: Ge, value: 3)`). 비우면 늘 참.
    pub(crate) requires: String,
    pub(crate) power: String,
    pub(crate) cooldown: String,
    pub(crate) mp_cost: String,
    pub(crate) heal: String,
}

impl Default for SkillLevelForm {
    fn default() -> Self {
        Self {
            max_level: 1,
            classes: Vec::new(),
            requires: String::new(),
            power: String::new(),
            cooldown: String::new(),
            mp_cost: String::new(),
            heal: String::new(),
        }
    }
}

impl SkillForm {
    pub(crate) fn new(name: &str) -> Self {
        Self {
            range: 2.0,
            cooldown_ms: 1000,
            damage_mult: 1.0,
            mp_cost: 0,
            radius: 0.0,
            heal: 0,
            name: name.to_owned(),
            level: SkillLevelForm::default(),
        }
    }

    pub(crate) fn rules_entry(&self, id: u32) -> String {
        let q = crate::ron_patch::quote;
        let l = &self.level;
        let set = |s: &str| !s.trim().is_empty();
        // 숫자 칸 또는 그 칸의 수식.
        let num_or = |n: u32, f: &str| if set(f) { q(f.trim()) } else { n.to_string() };
        // MP 비용은 있을 때만 적는다 — 예전 항목이 바이트 그대로 남게.
        let mp = if self.mp_cost == 0 && !set(&l.mp_cost) {
            String::new()
        } else {
            format!(", mp_cost: {}", num_or(self.mp_cost, &l.mp_cost))
        };
        // 범위·회복도 있을 때만.
        let mut extra = String::new();
        if self.radius > 0.0 {
            extra.push_str(&format!(", radius: {:?}", self.radius));
        }
        if self.heal > 0 || set(&l.heal) {
            extra.push_str(&format!(", heal: {}", num_or(self.heal, &l.heal)));
        }
        // R3 칸 — 기본값이면 적지 않는다.
        if l.max_level > 1 {
            extra.push_str(&format!(", max_level: {}", l.max_level));
        }
        if !l.classes.is_empty() {
            let list: Vec<String> = l.classes.iter().map(u32::to_string).collect();
            extra.push_str(&format!(", classes: [{}]", list.join(", ")));
        }
        if set(&l.requires) {
            extra.push_str(&format!(", requires: {}", l.requires.trim()));
        }
        // 위력 수식이 있으면 배율 대신 그것을 적는다.
        let power = if set(&l.power) {
            format!("power: {}", q(l.power.trim()))
        } else {
            format!("damage_mult: {:?}", self.damage_mult)
        };
        format!(
            "{id}: (range: {:?}, cooldown_ms: {}, {power}{mp}{extra}),",
            self.range,
            num_or(self.cooldown_ms, &l.cooldown)
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
    /// 상점·제작·텔레포트 표 (R4-2).
    pub(crate) shops: BTreeMap<u32, ShopForm>,
    pub(crate) crafts: BTreeMap<u32, CraftForm>,
    pub(crate) teleports: BTreeMap<u32, TeleportForm>,
    /// 참조 찾기용 — 시작 소지품의 아이템, 플레이어 공격 스킬.
    pub(crate) starting_kit: Vec<u32>,
    pub(crate) player_attack: u32,
}

/// 상점 하나 — 표 편집기 폼 (R4-2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ShopForm {
    /// `(아이템, 값, 재화 — 0 이면 기본 재화)`.
    pub(crate) goods: Vec<(u32, u32, u32)>,
    pub(crate) buy_rate: u32,
}

impl ShopForm {
    pub(crate) fn new() -> Self {
        Self {
            goods: Vec::new(),
            buy_rate: 500,
        }
    }

    pub(crate) fn rules_entry(&self, id: u32) -> String {
        let mut s = format!("{id}: (\n    goods: [\n");
        for &(item, price, currency) in &self.goods {
            let currency = if currency == 0 {
                String::new()
            } else {
                format!(", currency: {currency}")
            };
            s.push_str(&format!(
                "        (item: {item}, price: {price}{currency}),\n"
            ));
        }
        s.push_str(&format!("    ],\n    buy_rate: {},\n),", self.buy_rate));
        s
    }
}

/// 레시피 하나 — 표 편집기 폼 (R4-2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RecipeForm {
    /// `(아이템, 개수)`.
    pub(crate) inputs: Vec<(u32, u32)>,
    pub(crate) fee: u32,
    pub(crate) output: u32,
    pub(crate) count: u32,
    /// 만들 수 있는 조건 — RON 텍스트. 비우면 늘 참.
    pub(crate) requires: String,
}

/// 제작 표 하나 (R4-2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CraftForm {
    pub(crate) recipes: Vec<RecipeForm>,
}

impl CraftForm {
    pub(crate) fn rules_entry(&self, id: u32) -> String {
        let mut s = format!("{id}: (\n    recipes: [\n");
        for r in &self.recipes {
            let inputs: Vec<String> = r
                .inputs
                .iter()
                .map(|(i, n)| format!("(item: {i}, count: {n})"))
                .collect();
            let mut line = format!("        (inputs: [{}]", inputs.join(", "));
            if r.fee > 0 {
                line.push_str(&format!(", fee: {}", r.fee));
            }
            line.push_str(&format!(", output: {}", r.output));
            if r.count != 1 {
                line.push_str(&format!(", count: {}", r.count));
            }
            if !r.requires.trim().is_empty() {
                line.push_str(&format!(", requires: {}", r.requires.trim()));
            }
            line.push_str("),\n");
            s.push_str(&line);
        }
        s.push_str("    ],\n),");
        s
    }
}

/// 텔레포트 목적지 하나 (R4-2).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DestForm {
    /// 표시 이름 (`display.ron`).
    pub(crate) name: String,
    /// 비우면 지금 존 안.
    pub(crate) level: String,
    pub(crate) pos: (f32, f32),
    pub(crate) price: u32,
}

/// 텔레포트 목적지 표 하나 (R4-2).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TeleportForm {
    pub(crate) destinations: Vec<DestForm>,
}

impl TeleportForm {
    pub(crate) fn rules_entry(&self, id: u32) -> String {
        let mut s = format!("{id}: (\n    destinations: [\n");
        for d in &self.destinations {
            let mut line = String::from("        (");
            if !d.level.trim().is_empty() {
                line.push_str(&format!(
                    "level: {}, ",
                    crate::ron_patch::quote(d.level.trim())
                ));
            }
            line.push_str(&format!("pos: ({:?}, {:?})", d.pos.0, d.pos.1));
            if d.price > 0 {
                line.push_str(&format!(", price: {}", d.price));
            }
            line.push_str("),\n");
            s.push_str(&line);
        }
        s.push_str("    ],\n),");
        s
    }

    /// `display.ron` 의 `teleports` 항목 — 목적지 이름 목록.
    pub(crate) fn display_entry(&self, id: u32) -> String {
        let names: Vec<String> = self
            .destinations
            .iter()
            .map(|d| crate::ron_patch::quote(d.name.trim()))
            .collect();
        format!("{id}: [{}],", names.join(", "))
    }
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
                ItemFile::Consumable {
                    heal,
                    mana,
                    max_stack,
                    ..
                } => ItemKindForm::Consumable {
                    heal,
                    mana,
                    max_stack,
                },
                ItemFile::Equipment {
                    slot,
                    attack,
                    defense,
                    ref stats,
                    ref classes,
                    ..
                } => ItemKindForm::Equipment {
                    slot: slot.into(),
                    attack,
                    defense,
                    bonus: EquipBonusForm {
                        stats: stats.iter().map(|(k, v)| (k.clone(), *v)).collect(),
                        classes: classes.clone(),
                    },
                },
                ItemFile::SkillBook {
                    skill, max_stack, ..
                } => ItemKindForm::SkillBook { skill, max_stack },
            };
            let look = d.items.get(id);
            (
                *id,
                ItemForm {
                    kind,
                    name: look.map(|l| l.name.clone()).unwrap_or_default(),
                    color: look.map_or((0.9, 0.9, 0.9), |l| l.color),
                    value: it.value(),
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
                    cooldown_ms: s.cooldown_ms.base(0),
                    damage_mult: s.damage_mult,
                    mp_cost: s.mp_cost.base(0),
                    radius: s.radius,
                    heal: s.heal.base(0),
                    name: d.skills.get(id).map(|l| l.name.clone()).unwrap_or_default(),
                    level: SkillLevelForm {
                        max_level: s.max_level,
                        classes: s.classes.clone(),
                        requires: if s.requires == ConditionFile::Always {
                            String::new()
                        } else {
                            ron::to_string(&s.requires).unwrap_or_default()
                        },
                        power: s.power.clone(),
                        cooldown: s.cooldown_ms.formula().unwrap_or("").to_owned(),
                        mp_cost: s.mp_cost.formula().unwrap_or("").to_owned(),
                        heal: s.heal.formula().unwrap_or("").to_owned(),
                    },
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
    let shops = r
        .shops
        .iter()
        .map(|(id, s)| {
            (
                *id,
                ShopForm {
                    goods: s
                        .goods
                        .iter()
                        .map(|g| (g.item, g.price, g.currency))
                        .collect(),
                    buy_rate: s.buy_rate,
                },
            )
        })
        .collect();
    let crafts = r
        .crafts
        .iter()
        .map(|(id, c)| {
            (
                *id,
                CraftForm {
                    recipes: c
                        .recipes
                        .iter()
                        .map(|rc| RecipeForm {
                            inputs: rc.inputs.iter().map(|i| (i.item, i.count)).collect(),
                            fee: rc.fee,
                            output: rc.output,
                            count: rc.count,
                            requires: if rc.requires == ConditionFile::Always {
                                String::new()
                            } else {
                                ron::to_string(&rc.requires).unwrap_or_default()
                            },
                        })
                        .collect(),
                },
            )
        })
        .collect();
    let teleports = r
        .teleports
        .iter()
        .map(|(id, t)| {
            let names = d.teleports.get(id);
            (
                *id,
                TeleportForm {
                    destinations: t
                        .destinations
                        .iter()
                        .enumerate()
                        .map(|(i, dest)| DestForm {
                            name: names.and_then(|n| n.get(i)).cloned().unwrap_or_default(),
                            level: dest.level.clone(),
                            pos: dest.pos,
                            price: dest.price,
                        })
                        .collect(),
                },
            )
        })
        .collect();
    Ok(TableForms {
        items,
        skills,
        loot,
        shops,
        crafts,
        teleports,
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
    /// 기본 부활 — `(대기 ms, HP %, 경험치 손실 %)`.
    pub(crate) revive: (u32, u32, u32),
    /// 부활 뒤 보호 (ms).
    pub(crate) revive_protect_ms: u32,
    /// 땅의 아이템 소멸 시간 (ms, 0 = 남음).
    pub(crate) ground_item_ms: u32,
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
        revive: (
            r.revive.delay_ms,
            r.revive.hp_percent,
            r.revive.exp_loss_percent,
        ),
        ground_item_ms: r.ground_item_ms,
        revive_protect_ms: r.revive.protect_ms,
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
    let (delay, hp, loss) = f.revive;
    let protect = match f.revive_protect_ms {
        0 => String::new(),
        ms => format!(", protect_ms: {ms}"),
    };
    let revive =
        format!("(delay_ms: {delay}, hp_percent: {hp}, exp_loss_percent: {loss}{protect})");
    text = set_or_append(&text, "revive", &revive)?;
    text = set_or_append(&text, "ground_item_ms", &f.ground_item_ms.to_string())?;
    GameData::parse(&text, display)?;
    Ok(text)
}

/// 한 줄 필드를 바꾼다 — 예전 파일이라 줄이 없으면 맨 끝 `)` 앞에 넣는다.
fn set_or_append(text: &str, field: &str, value: &str) -> Result<String, String> {
    if let Ok(t) = crate::ron_patch::replace_field(text, field, value) {
        return Ok(t);
    }
    let end = text
        .rfind(')')
        .ok_or_else(|| String::from("rules.ron 의 끝 `)` 을 찾지 못함"))?;
    Ok(format!(
        "{}    {field}: {value},\n{}",
        &text[..end],
        &text[end..]
    ))
}

/// `rules.ron` 의 트리거 중 그 아이템(`item`)이나 액터(`actor`)를 가리키는 것의 번호 (R1).
/// 콘텐츠 브라우저가 지우기를 거부하거나 번호 바꾸기를 확인할 때 쓴다.
pub(crate) fn triggers_using(
    rules: &str,
    item: Option<u32>,
    actor: Option<u32>,
) -> Result<Vec<u32>, String> {
    let r: RulesFile = ron::from_str(rules).map_err(|e| format!("{RULES_PATH}: {e}"))?;
    Ok(r.triggers
        .iter()
        .filter(|(_, t)| {
            let refs = trigger_file::refs(t);
            item.is_some_and(|i| refs.items.contains(&i))
                || actor.is_some_and(|a| refs.actors.contains(&a))
        })
        .map(|(id, _)| *id)
        .collect())
}

/// `rules.ron` 의 퀘스트 중 그 아이템(`item`)이나 액터(`actor`)를 가리키는 것의 번호 (R5).
pub(crate) fn quests_using(
    rules: &str,
    item: Option<u32>,
    actor: Option<u32>,
) -> Result<Vec<u32>, String> {
    let r: RulesFile = ron::from_str(rules).map_err(|e| format!("{RULES_PATH}: {e}"))?;
    Ok(r.quests
        .iter()
        .filter(|(_, q)| {
            let refs = quest_file::refs(q);
            item.is_some_and(|i| refs.items.contains(&i))
                || actor.is_some_and(|a| refs.actors.contains(&a))
        })
        .map(|(id, _)| *id)
        .collect())
}

/// 스킬 `skill` 을 쓰는 곳 (R3) — 지우기 전에 사람이 읽을 이유로. 액터·플레이어 공격은 부르는 쪽이 따로 본다.
pub(crate) fn skill_users(rules: &str, skill: u32) -> Result<Vec<String>, String> {
    let r: RulesFile = ron::from_str(rules).map_err(|e| format!("{RULES_PATH}: {e}"))?;
    let mut out = Vec::new();
    for (id, c) in &r.classes {
        if c.skills.contains(&skill) {
            out.push(format!("직업 #{id} 시작 스킬"));
        }
    }
    for (id, item) in &r.items {
        if matches!(item, ItemFile::SkillBook { skill: s, .. } if *s == skill) {
            out.push(format!("스킬북 아이템 #{id}"));
        }
    }
    for (id, s) in &r.skills {
        if *id != skill
            && trigger_file::condition_refs_of(&s.requires)
                .skills
                .contains(&skill)
        {
            out.push(format!("스킬 #{id} 의 습득 조건"));
        }
    }
    let triggers: Vec<String> = r
        .triggers
        .iter()
        .filter(|(_, t)| trigger_file::refs(t).skills.contains(&skill))
        .map(|(id, _)| format!("#{id}"))
        .collect();
    if !triggers.is_empty() {
        out.push(format!("트리거 {}", triggers.join(", ")));
    }
    // 퀘스트의 보상·단계 효과·조건 (R5).
    for (id, q) in &r.quests {
        if quest_file::refs(q).skills.contains(&skill) {
            out.push(format!("퀘스트 #{id}"));
        }
    }
    Ok(out)
}

/// 진영 이름 표 — `display.ron` 의 `factions` (P19). 블록이 없으면 빈 표.
pub(crate) fn faction_names(display: &str) -> Result<BTreeMap<u32, String>, String> {
    let d: DisplayFile = ron::from_str(display).map_err(|e| format!("{DISPLAY_PATH}: {e}"))?;
    Ok(d.factions.into_iter().map(|(id, l)| (id, l.name)).collect())
}

/// 진영 이름 표를 `display.ron` 에 끼운다 — `factions` 블록만 통째로 바꾸고 나머지 주석은 둔다.
/// 블록이 없으면 끝 `)` 앞에 새로 넣는다. 검증은 부르는 쪽이 두 파일을 함께 한다.
///
/// ⚠ 블록 **안**의 주석은 남지 않는다 (블록 위 주석은 남는다).
pub(crate) fn patch_faction_names(
    display: &str,
    names: &BTreeMap<u32, String>,
) -> Result<String, String> {
    let entries: Vec<String> = names
        .iter()
        .map(|(id, name)| format!("{id}: (name: {}),", crate::ron_patch::quote(name)))
        .collect();
    crate::ron_patch::replace_block(display, "factions", '{', &entries)
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

/// 스탯 디자이너의 폼 (R2) — 두 파일의 `stats`·`classes` 블록에서.
pub(crate) fn stat_form(rules: &str, display: &str) -> Result<class_file::StatForm, String> {
    let r: RulesFile = ron::from_str(rules).map_err(|e| format!("{RULES_PATH}: {e}"))?;
    let d: DisplayFile = ron::from_str(display).map_err(|e| format!("{DISPLAY_PATH}: {e}"))?;
    let labels = d.stats.into_iter().map(|(k, l)| (k, l.name)).collect();
    Ok(class_file::StatForm::from_blocks(
        &r.stats, &r.classes, &d.classes, &labels,
    ))
}

/// 폼을 두 파일에 써 넣고 **게임과 같은 검사**로 다시 읽어 본다 (R2). 통과해야 결과를 돌려준다.
pub(crate) fn patch_stat_form(
    rules: &str,
    display: &str,
    form: &class_file::StatForm,
) -> Result<(String, String), String> {
    let (rules, display) = form.patch(rules, display)?;
    GameData::parse(&rules, &display)?;
    Ok((rules, display))
}

/// 퀘스트 디자이너의 폼 (R5) — 번호 → 폼.
pub(crate) fn quest_forms(
    rules: &str,
    display: &str,
) -> Result<BTreeMap<u32, quest_file::QuestForm>, String> {
    let r: RulesFile = ron::from_str(rules).map_err(|e| format!("{RULES_PATH}: {e}"))?;
    let d: DisplayFile = ron::from_str(display).map_err(|e| format!("{DISPLAY_PATH}: {e}"))?;
    Ok(r.quests
        .iter()
        .map(|(id, q)| (*id, quest_file::QuestForm::from_file(q, d.quests.get(id))))
        .collect())
}

/// 퀘스트 하나를 두 파일에 끼우고 **게임과 같은 검사**로 다시 읽어 본다 (R5).
pub(crate) fn patch_quest(
    rules: &str,
    display: &str,
    id: u32,
    form: &quest_file::QuestForm,
) -> Result<(String, String), String> {
    use crate::ron_patch::{ensure_map_block, upsert_entry};
    let rules = upsert_entry(
        &ensure_map_block(rules, "quests")?,
        "quests",
        id,
        &form.rules_entry(id)?,
    )?;
    let display = upsert_entry(
        &ensure_map_block(display, "quests")?,
        "quests",
        id,
        &form.display_entry(id),
    )?;
    GameData::parse(&rules, &display)?;
    Ok((rules, display))
}

/// 퀘스트 하나를 두 파일에서 뺀다 (R5) — 쓰는 곳(NPC 기능·조건·효과)이 있으면 검사가 막는다.
pub(crate) fn remove_quest(
    rules: &str,
    display: &str,
    id: u32,
) -> Result<(String, String), String> {
    let (rules, _) = crate::ron_patch::remove_entry(rules, "quests", id)?;
    let display = match crate::ron_patch::remove_entry(display, "quests", id) {
        Ok((text, _)) => text,
        Err(_) => display.to_owned(), // 제목이 없던 퀘스트
    };
    GameData::parse(&rules, &display)?;
    Ok((rules, display))
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
            .replacen("player_attack: 1", "player_attack: 99", 1);
        let err = GameData::parse(&rules, EMBEDDED_DISPLAY).unwrap_err();
        for expected in [
            "없는 스킬 77",
            "없는 아이템 4040",
            "player_attack: 없는 스킬 99",
        ] {
            assert!(err.contains(expected), "'{expected}' 기대, 실제: {err}");
        }
    }

    #[test]
    fn a_basic_attack_that_costs_more_mp_than_the_actor_ever_has_is_refused() {
        // 슬라임 물기에 MP 를 매기면 MP 가 없는 슬라임은 영영 칠 수 없다.
        let rules = EMBEDDED_RULES.replacen(
            "2: (range: 1.5, cooldown_ms: 1200, damage_mult: 1.0),",
            "2: (range: 1.5, cooldown_ms: 1200, damage_mult: 1.0, mp_cost: 5),",
            1,
        );
        assert_ne!(
            rules, EMBEDDED_RULES,
            "시험 전제: 스킬 2 줄을 찾았어야 한다"
        );
        let err = GameData::parse(&rules, EMBEDDED_DISPLAY).unwrap_err();
        assert!(err.contains("액터 100") && err.contains("MP 5"), "{err}");
    }

    #[test]
    fn hotbar_skills_must_exist_fit_and_not_repeat() {
        let data = GameData::embedded();
        assert_eq!(
            data.actor_skills(ActorId::new(1)),
            [SkillId(5), SkillId(6), SkillId(7), SkillId(8)]
        );
        assert!(data.actor_skills(ActorId::new(100)).is_empty());
        assert_eq!(data.skill_name(SkillId(5)), "강베기");
        assert_eq!(data.skill_name(SkillId(999)), "스킬 #999");

        for (bad, expect) in [
            ("skills: [5, 6, 7, 8],", "없는 스킬 77"),
            ("skills: [5, 6, 7, 8],", "두 번"),
            ("skills: [5, 6, 7, 8],", "9개까지"),
        ] {
            let replacement = match expect {
                "없는 스킬 77" => "skills: [5, 77],",
                "두 번" => "skills: [5, 5],",
                _ => "skills: [1, 2, 3, 4, 5, 6, 1, 2, 3, 4],",
            };
            let rules = EMBEDDED_RULES.replacen(bad, replacement, 1);
            assert_ne!(rules, EMBEDDED_RULES, "시험 전제");
            let err = GameData::parse(&rules, EMBEDDED_DISPLAY).unwrap_err();
            assert!(err.contains(expect), "'{expect}' 기대: {err}");
        }
    }

    #[test]
    fn area_and_heal_skills_are_read_and_checked() {
        let data = GameData::embedded();
        let whirl = data
            .skills
            .iter()
            .find(|(id, _)| *id == SkillId(7))
            .unwrap()
            .1;
        let mend = data
            .skills
            .iter()
            .find(|(id, _)| *id == SkillId(8))
            .unwrap()
            .1;
        assert_eq!((whirl.radius, whirl.heal), (2.5, 0));
        // 치유는 회복량이 수식이다 (R3) — 정의의 숫자는 "회복 스킬" 로 분류되게 하는 자리 값이고,
        // 실제 양은 쓸 때 수식으로 계산한다 (`play::tests::skill_formulas_scale_with_level_and_stats`).
        assert!(mend.is_heal());

        let wide = EMBEDDED_RULES.replacen("radius: 2.5", "radius: 99.0", 1);
        let err = GameData::parse(&wide, EMBEDDED_DISPLAY).unwrap_err();
        assert!(err.contains("스킬 7") && err.contains("radius"), "{err}");
        // 회복 스킬을 기본 공격·클릭 공격으로 두면 쓸 수 없다.
        let basic = EMBEDDED_RULES.replacen("basic_attack: Some(2)", "basic_attack: Some(8)", 1);
        assert_ne!(basic, EMBEDDED_RULES, "시험 전제");
        let err = GameData::parse(&basic, EMBEDDED_DISPLAY).unwrap_err();
        assert!(err.contains("회복 스킬"), "{err}");
        let click = EMBEDDED_RULES.replacen("player_attack: 1,", "player_attack: 8,", 1);
        assert_ne!(click, EMBEDDED_RULES, "시험 전제");
        let err = GameData::parse(&click, EMBEDDED_DISPLAY).unwrap_err();
        assert!(
            err.contains("player_attack") && err.contains("회복"),
            "{err}"
        );
    }

    #[test]
    fn respawn_times_come_from_the_actor_and_are_capped() {
        let data = GameData::embedded();
        assert_eq!(
            data.unit_def(ActorId::new(100)).respawn_ms,
            10_000,
            "슬라임"
        );
        assert_eq!(
            data.unit_def(ActorId::new(1)).respawn_ms,
            0,
            "플레이어는 리스폰하지 않는다"
        );
        let rules = EMBEDDED_RULES.replacen("respawn_ms: 10000,", "respawn_ms: 36000000,", 1);
        assert_ne!(rules, EMBEDDED_RULES, "시험 전제");
        let err = GameData::parse(&rules, EMBEDDED_DISPLAY).unwrap_err();
        assert!(err.contains("respawn_ms"), "{err}");
    }

    #[test]
    fn the_revive_rule_is_checked_written_and_defaulted() {
        const LINE: &str =
            "revive: (delay_ms: 3000, hp_percent: 100, exp_loss_percent: 0, protect_ms: 3000),";
        let data = GameData::embedded();
        assert_eq!(
            data.revive(),
            RevivePolicy {
                delay: Duration::from_millis(3000),
                hp_per_mille: 1000,
                exp_loss_per_mille: 0,
                protect_ms: 3000,
            }
        );
        let bad = EMBEDDED_RULES.replacen("hp_percent: 100", "hp_percent: 0", 1);
        assert!(
            GameData::parse(&bad, EMBEDDED_DISPLAY)
                .unwrap_err()
                .contains("revive")
        );

        // 줄이 없는 예전 파일도 읽히고(기본값), 설정 창이 저장하면 줄이 생긴다.
        let old = EMBEDDED_RULES.replacen(LINE, "", 1);
        assert_ne!(old, EMBEDDED_RULES, "시험 전제");
        assert_eq!(
            GameData::parse(&old, EMBEDDED_DISPLAY).unwrap().revive(),
            RevivePolicy {
                protect_ms: 0,
                ..data.revive()
            },
            "예전 파일은 보호 없음"
        );
        let mut form = settings_form(&old).unwrap();
        form.revive = (500, 40, 10);
        let text = patch_settings(&old, EMBEDDED_DISPLAY, &form).unwrap();
        let back = GameData::parse(&text, EMBEDDED_DISPLAY).unwrap().revive();
        assert_eq!((back.hp_per_mille, back.exp_loss_per_mille), (400, 100));
        // 줄이 있으면 그 줄만 바뀐다.
        let text = patch_settings(EMBEDDED_RULES, EMBEDDED_DISPLAY, &form).unwrap();
        assert!(text.contains("revive: (delay_ms: 500, hp_percent: 40, exp_loss_percent: 10),"));
        assert_eq!(
            text.matches("revive:").count(),
            EMBEDDED_RULES.matches("revive:").count()
        );
        // 보호 시간은 0 이 아닐 때만 적힌다.
        form.revive_protect_ms = 1500;
        let text = patch_settings(EMBEDDED_RULES, EMBEDDED_DISPLAY, &form).unwrap();
        assert!(text.contains("exp_loss_percent: 10, protect_ms: 1500),"));
        let back = GameData::parse(&text, EMBEDDED_DISPLAY).unwrap().revive();
        assert_eq!(back.protect_ms, 1500);
        let bad = EMBEDDED_RULES.replacen("protect_ms: 3000", "protect_ms: 9000000", 1);
        assert!(GameData::parse(&bad, EMBEDDED_DISPLAY).is_err());
    }

    #[test]
    fn ground_item_lifetime_is_checked_written_and_installed() {
        const LINE: &str = "ground_item_ms: 60000,";
        let data = GameData::embedded();
        assert_eq!(
            data.ground_item_lifetime,
            Some(Duration::from_secs(60)),
            "내장 데이터"
        );
        let too_long = EMBEDDED_RULES.replacen(LINE, "ground_item_ms: 36000000,", 1);
        assert!(
            GameData::parse(&too_long, EMBEDDED_DISPLAY)
                .unwrap_err()
                .contains("ground_item_ms")
        );
        // 0 = 영영 남는다.
        let never = EMBEDDED_RULES.replacen(LINE, "ground_item_ms: 0,", 1);
        assert_eq!(
            GameData::parse(&never, EMBEDDED_DISPLAY)
                .unwrap()
                .ground_item_lifetime,
            None
        );

        // 줄이 없는 예전 파일은 남는 쪽, 설정 창이 저장하면 줄이 생긴다.
        let old = EMBEDDED_RULES.replacen(LINE, "", 1);
        assert_ne!(old, EMBEDDED_RULES, "시험 전제");
        assert_eq!(
            GameData::parse(&old, EMBEDDED_DISPLAY)
                .unwrap()
                .ground_item_lifetime,
            None
        );
        let mut form = settings_form(&old).unwrap();
        assert_eq!(form.ground_item_ms, 0);
        form.ground_item_ms = 5000;
        let text = patch_settings(&old, EMBEDDED_DISPLAY, &form).unwrap();
        assert_eq!(text.matches("ground_item_ms:").count(), 1);
        let back = GameData::parse(&text, EMBEDDED_DISPLAY).unwrap();
        assert_eq!(back.ground_item_lifetime, Some(Duration::from_secs(5)));

        // 월드에 들어간다 — 드롭이 5초 뒤 사라진다.
        use nexus_sim::Authority as _;
        let mut world = nexus_sim::SimWorld::new(nexus_sim::TileMap::new(
            4,
            4,
            1.0,
            nexus_core::Vec2::ZERO,
            nexus_sim::Tile::default(),
        ));
        back.install(&mut world);
        let mut auth = nexus_sim::LocalAuthority::new(world);
        let g = auth.world_mut().spawn_ground_item(
            nexus_core::Vec2::ZERO,
            nexus_sim::ItemStack {
                item: nexus_sim::ItemId(501),
                count: 1,
            },
        );
        for _ in 0..99 {
            auth.tick(Duration::from_millis(50));
        }
        assert!(auth.world().ground_item(g).is_some(), "너무 일찍 사라짐");
        auth.tick(Duration::from_millis(50));
        assert!(auth.world().ground_item(g).is_none(), "5초가 지나도 남음");
    }

    #[test]
    fn faction_names_are_display_data_and_checked() {
        let data = GameData::embedded();
        assert_eq!(
            data.faction_list(),
            [(1, "플레이어"), (100, "마을"), (201, "야생 몬스터")]
        );
        let zero = EMBEDDED_DISPLAY.replacen(
            "1: (name: \"플레이어\"),\n        100",
            "0: (name: \"없음\"),\n        100",
            1,
        );
        assert_ne!(zero, EMBEDDED_DISPLAY, "시험 전제");
        assert!(
            GameData::parse(EMBEDDED_RULES, &zero)
                .unwrap_err()
                .contains("표시 진영 0")
        );
        let empty =
            EMBEDDED_DISPLAY.replacen("201: (name: \"야생 몬스터\")", "201: (name: \" \")", 1);
        assert!(
            GameData::parse(EMBEDDED_RULES, &empty)
                .unwrap_err()
                .contains("이름이 비어")
        );
        // 규칙 파일에는 이름이 들어가지 않는다 — 반으로 나눈 표 규칙.
        assert!(!EMBEDDED_RULES.contains("야생 몬스터\")"));
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
