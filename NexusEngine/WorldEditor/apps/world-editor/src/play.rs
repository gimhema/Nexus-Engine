//! 플레이 모드 — 편집 중인 씬을 **그 자리에서** 시뮬레이션으로 돌린다 (S7-1).
//!
//! ```text
//! 씬(편집 데이터) ──시작──▶ SimWorld (복사본) ──LocalAuthority.tick (20Hz)──▶ 화면
//!        ▲                                                   │
//!        └───────────── 정지하면 버린다 (씬은 그대로) ────────┘
//! ```
//!
//! - **씬을 바꾸지 않는다.** 시작할 때 씬을 복사해 시뮬레이션을 만들고, 정지하면 버린다.
//!   그래서 플레이 중 일어난 일(몬스터 사망 등)은 언두 기록과 무관하다.
//! - 입력은 Intent 로만 들어간다. 클릭한 대상에 **다가가는 것은 입력 쪽의 일**이다 (S6 설계) —
//!   [`PlaySession`] 의 주문(`Order`)이 매 tick `MoveTo`/`Attack`/`PickUp` 을 낸다.
//! - 에디터 `Scene` 과 `SimWorld` 는 핸들 공간이 다르다. 이름·색은 시작할 때 만든 대응표로 찾는다.
//! - 규칙 수치·아이템 이름은 [`GameData`] (`data/rules.ron` · `data/display.ron`) 에서 온다 — 코드에 수치가 없다.

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use nexus_assets::{AnimState, SpriteAnimator, SpriteSheet};
use nexus_core::{Camera2d, Entity, Vec2, Vec3};
use nexus_render::{DEPTH_LAYER, RenderCommand, SpriteAnchor, TextureId, UvRect};
use nexus_script::RhaiHost;
use nexus_sim::{
    Authority, BagKind, ClassId, Condition, CraftId, EquipSlot, Event, Intent, ItemStack,
    LocalAuthority, PortalDef, Price, Progress, QuestId, QuestState, Rejection, Relation, Service,
    ShopId, SimWorld, SkillId, TeleportId, Unit,
};

use crate::game_data::{ActorLook, GameData};
use crate::save_file::SaveData;
use crate::scene::{ActorId, ItemKind, Scene};
use crate::sprites::{Look, NO_TINT, SpriteLibrary};

/// 쫓는 대상이 마지막 경로 지점에서 이만큼(m) 벗어나야 다시 경로를 잡는다 (AI 와 같은 값).
const REPATH_DISTANCE: f32 = 0.5;
/// 단축키 스킬을 대상 없이 눌렀을 때 적을 찾는 거리 (m).
const SKILL_SEARCH_RANGE: f32 = 10.0;
/// 이벤트 기록을 몇 줄까지 보관하나.
const LOG_LINES: usize = 10;

// 겹침 순서 (깊이 편향). 에디터 표시와 같은 규칙 — 지면 표시는 z = 0 + 편향.
const BIAS_PATH: f32 = 2.0 * DEPTH_LAYER;
const BIAS_ITEM: f32 = 3.0 * DEPTH_LAYER;
const BIAS_TARGET: f32 = 4.0 * DEPTH_LAYER;
const BIAS_SPRITE: f32 = 3.0 * DEPTH_LAYER;
const BIAS_BAR: f32 = 2.0 * DEPTH_LAYER;

const PATH_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 0.55];
const TARGET_COLOR: [f32; 4] = [1.0, 0.35, 0.30, 1.0];
/// 포털 구역 (R6) — 들어갈 수 있음 / 입장 조건이 맞지 않음.
const PORTAL_COLOR: [f32; 4] = [0.70, 0.50, 1.0, 0.9];
const PORTAL_LOCKED_COLOR: [f32; 4] = [0.55, 0.55, 0.60, 0.7];
const BAR_BACK: [f32; 4] = [0.05, 0.05, 0.07, 1.0];
const BAR_HP: [f32; 4] = [0.30, 0.85, 0.40, 1.0];
const BAR_HP_LOW: [f32; 4] = [0.95, 0.30, 0.25, 1.0];
const CORPSE_TINT: [f32; 4] = [0.35, 0.35, 0.38, 1.0];
/// 맞은 유닛을 붉게 깜빡인다 — 피격 그림이 없는 시트에도 맞은 것이 보이게. 곱하는 색이다.
const HIT_TINT: [f32; 4] = [1.0, 0.35, 0.35, 1.0];
/// 땅 아이템이 사라지기 전 깜빡이는 구간과 한 번의 켜짐·꺼짐 길이.
const ITEM_BLINK_BEFORE: Duration = Duration::from_secs(5);
const ITEM_BLINK_MS: u128 = 250;
/// 부활 직후 보호 중인 유닛이 깜빡이는 한 번의 길이.
const PROTECT_BLINK_MS: u128 = 100;
/// 깜빡임 길이 (tick). 3 tick = 150ms.
const HIT_FLASH_TICKS: u8 = 3;

/// 단축키 칸 수 — 숫자키 1~9 (R3).
pub(crate) const HOTBAR_SLOTS: usize = 9;

/// 플레이어가 클릭으로 내린 주문. 매 tick 이것을 보고 Intent 를 낸다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Order {
    /// 할 일 없음 (이동은 `MoveTo` 한 번으로 끝나므로 주문으로 남기지 않는다).
    Idle,
    /// 다가가서 친다. 대상이 죽거나 거절되면 끝.
    ///
    /// `skill` 은 단축키로 예약한 스킬이다 — 그 스킬이 한 번 맞으면 비우고 기본 공격으로 이어 간다.
    Attack {
        target: Entity,
        skill: Option<SkillId>,
    },
    /// 다가가서 줍는다.
    PickUp(Entity),
    /// 다가가서 말을 건다 — 기능이 있는 NPC (R4).
    Talk(Entity),
}

/// 거래 창 조작 (R4) — UI 가 돌려주면 세션이 Intent 로 바꿔 낸다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TradeAction {
    Buy {
        shop: ShopId,
        goods: u16,
        count: u32,
    },
    Sell {
        shop: ShopId,
        bag: BagKind,
        slot: u16,
        count: u32,
    },
    /// 상점이 아닌 기능 (치료 등) — 기능 번호.
    Service(u16),
    /// 제작 (R4-2).
    Craft {
        craft: CraftId,
        recipe: u16,
        times: u32,
    },
    /// 텔레포트 (R4-2).
    Teleport {
        teleport: TeleportId,
        dest: u16,
    },
    /// 창고에 맡기기 / 찾기 (R4-2).
    Deposit {
        bag: BagKind,
        slot: u16,
        count: u32,
    },
    Withdraw {
        slot: u16,
        count: u32,
    },
    /// 퀘스트 받기·보고 (R5).
    AcceptQuest(QuestId),
    TurnInQuest(QuestId),
    Close,
}

/// NPC 창의 쪽 (R4b) — 처음은 기능 메뉴, 고르면 그 목록으로 넘어간다. 대사 분기(R9)가 생기면
/// 메뉴 앞에 끼운다.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum NpcPage {
    #[default]
    Menu,
    Quests,
    Buy(ShopId),
    Sell(ShopId),
    Craft(CraftId),
    Teleport(TeleportId),
    Deposit,
    Withdraw,
}

impl NpcPage {
    fn title(self) -> &'static str {
        match self {
            Self::Menu => "무엇을 도와줄까?",
            Self::Quests => "퀘스트",
            Self::Buy(_) => "사기",
            Self::Sell(_) => "팔기",
            Self::Craft(_) => "제작",
            Self::Teleport(_) => "이동",
            Self::Deposit => "창고에 맡기기",
            Self::Withdraw => "창고에서 찾기",
        }
    }
}

/// NPC 창 목록 한 줄 (R4b) — 화면의 목록 위젯이 그리고, 누르면 [`PlaySession::npc_pick`] 으로 온다.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NpcRow {
    /// 왼쪽 글 — "빨간 포션", "사기" …
    pub(crate) text: String,
    /// 오른쪽 글 — 값·상태 ("20 골드", "보고하기").
    pub(crate) right: String,
    /// 누를 수 있나 (돈이 모자라면·받을 수 없으면 흐리게).
    pub(crate) enabled: bool,
    act: NpcAct,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum NpcAct {
    Page(NpcPage),
    Trade(TradeAction),
    None,
}

/// NPC 창이 그리는 것 (R4b) — 대화창(이름·인사말)과 지금 쪽의 목록.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NpcScreen {
    pub(crate) name: String,
    /// 인사말 — 지금은 `display.ron` 의 프로필 글. 대사(R9)가 생기면 그 대사.
    pub(crate) profile: String,
    /// 지금 쪽의 제목.
    pub(crate) page: String,
    pub(crate) rows: Vec<NpcRow>,
}

/// 거래 창이 그리는 것 (R4).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TradeView {
    pub(crate) npc_name: String,
    /// 지갑 — 기본 재화 금액과 그 이름.
    pub(crate) purse: String,
    pub(crate) shops: Vec<ShopView>,
    /// 상점이 아닌 기능 `(기능 번호, 단추 글)`.
    pub(crate) services: Vec<(u16, String)>,
    /// NPC 프로필 글 (R4-2). 없으면 빈 문자열.
    pub(crate) profile: String,
    pub(crate) crafts: Vec<CraftView>,
    pub(crate) teleports: Vec<TeleportView>,
    /// 창고 기능이 있으면.
    pub(crate) storage: Option<StorageView>,
    /// 이 NPC 의 퀘스트 (R5).
    pub(crate) quests: Vec<QuestOffer>,
}

/// NPC 창의 퀘스트 한 줄 (R5).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct QuestOffer {
    pub(crate) quest: QuestId,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) state: QuestState,
    /// 지금 받을 수 있나 (조건·반복).
    pub(crate) can_accept: bool,
}

/// 퀘스트 목록 한 줄 (R5) — 플레이 패널.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct QuestLine {
    pub(crate) quest: QuestId,
    pub(crate) title: String,
    pub(crate) state: QuestState,
    /// 지금 단계의 목표 글 `(글, 채웠나)`.
    pub(crate) goals: Vec<(String, bool)>,
}

/// 제작 표 하나 (R4-2).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CraftView {
    pub(crate) craft: CraftId,
    /// `(레시피 번호, 결과 글, 재료 글, 만들 수 있나)`.
    pub(crate) recipes: Vec<(u16, String, String, bool)>,
}

/// 텔레포트 표 하나 (R4-2).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TeleportView {
    pub(crate) teleport: TeleportId,
    /// `(목적지 번호, 이름, 비용 글)`.
    pub(crate) destinations: Vec<(u16, String, String)>,
}

/// 창고 (R4-2).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StorageView {
    /// 맡길 수 있는 가방 칸 `(가방, 칸, 이름, 개수)`.
    pub(crate) carried: Vec<(BagKind, u16, String, u32)>,
    /// 창고 칸 `(칸, 이름, 개수)`.
    pub(crate) stored: Vec<(u16, String, u32)>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ShopView {
    pub(crate) shop: ShopId,
    /// `(물건 번호, 이름, 값 글, 살 수 있나)`.
    pub(crate) goods: Vec<(u16, String, String, bool)>,
    /// 팔 수 있는 가방 칸 `(가방, 칸, 이름, 개수, 한 개 값 글)`.
    pub(crate) sellable: Vec<(BagKind, u16, String, u32, String)>,
}

/// 시뮬레이션 유닛의 이름과 색 — 씬 마커에서 가져온다.
#[derive(Clone, Debug)]
struct Label {
    name: String,
    /// 이 유닛의 **액터 타입** — 스프라이트 시트를 고르는 기준이다.
    actor: ActorId,
    /// 액터 타입이 명시한 색 (없으면 `NO_TINT`).
    tint: [f32; 4],
    /// 무채색 시트에 쓸 마커 종류 색.
    fallback: [f32; 4],
}

/// 인벤토리 패널에서 누른 것.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InventoryAction {
    Use(u16),
    Equip(u16),
    Unequip(EquipSlot),
}

/// 플레이를 시작할 때 주는 설정.
#[derive(Debug, Default)]
pub(crate) struct PlayOptions {
    /// 여기(씬 마커)에서 시작한다 — 없으면 첫 플레이어 스폰 (P3-1).
    pub(crate) spawn: Option<Entity>,
    /// 이어서 할 저장 데이터 (P3). 없으면 새로 시작한다.
    pub(crate) save: Option<SaveData>,
    /// 새 게임에서 고른 직업 (R2). 이어 하기면 저장한 직업이 이긴다. 둘 다 없으면 기본 직업.
    pub(crate) class: Option<ClassId>,
    /// 포털이 가리키는 레벨들의 사정 (R6) — 지금 레벨과 입장 조건.
    pub(crate) links: LevelLinks,
}

/// 포털을 sim 에 놓을 때 앱이 알려 주는 레벨 사정 (R6). 레벨 파일은 에디터가 읽으므로 여기로 건넨다.
#[derive(Debug, Default)]
pub(crate) struct LevelLinks {
    /// 지금 레벨 번호 — 포털이 이 레벨을 가리키면 같은 존 안으로 옮긴다(레벨을 다시 열지 않는다).
    pub(crate) here: String,
    /// 레벨별 입장 조건 (레벨 파일의 `requires`). 없으면 누구나.
    pub(crate) requires: HashMap<String, Condition>,
    /// 지금 레벨의 던전 클리어 (R6-2). 없으면 클리어가 없는 존.
    pub(crate) clear: Option<nexus_sim::ClearDef>,
}

/// 포털 `travel` 번호의 시작 — NPC 텔레포트 번호(`GameData::travel_level`)와 겹치지 않게 띄운다.
const PORTAL_TRAVEL_BASE: u32 = 100_000;

/// NPC 창 한 쪽의 목록 (R4b). 메뉴에는 이 NPC 가 가진 기능만 나온다 — 기능이 곧 역할이다(R4).
fn npc_rows(view: &TradeView, page: NpcPage) -> Vec<NpcRow> {
    let row = |text: String, right: String, enabled: bool, act: NpcAct| NpcRow {
        text,
        right,
        enabled,
        act,
    };
    let go =
        |text: &str, page: NpcPage| row(text.to_owned(), String::new(), true, NpcAct::Page(page));
    let many = |base: &str, n: usize, i: usize| {
        if n > 1 {
            format!("{base} {}", i + 1)
        } else {
            base.to_owned()
        }
    };
    match page {
        NpcPage::Menu => {
            let mut out = Vec::new();
            if !view.quests.is_empty() {
                let ready = view
                    .quests
                    .iter()
                    .filter(|q| q.state == QuestState::Ready)
                    .count();
                let mut r = go("퀘스트", NpcPage::Quests);
                if ready > 0 {
                    r.right = format!("보고 {ready}");
                }
                out.push(r);
            }
            for (i, s) in view.shops.iter().enumerate() {
                out.push(go(&many("사기", view.shops.len(), i), NpcPage::Buy(s.shop)));
                out.push(go(
                    &many("팔기", view.shops.len(), i),
                    NpcPage::Sell(s.shop),
                ));
            }
            for (i, c) in view.crafts.iter().enumerate() {
                out.push(go(
                    &many("제작", view.crafts.len(), i),
                    NpcPage::Craft(c.craft),
                ));
            }
            for (i, t) in view.teleports.iter().enumerate() {
                out.push(go(
                    &many("이동", view.teleports.len(), i),
                    NpcPage::Teleport(t.teleport),
                ));
            }
            if view.storage.is_some() {
                out.push(go("창고에 맡기기", NpcPage::Deposit));
                out.push(go("창고에서 찾기", NpcPage::Withdraw));
            }
            for (i, label) in &view.services {
                out.push(row(
                    label.clone(),
                    String::new(),
                    true,
                    NpcAct::Trade(TradeAction::Service(*i)),
                ));
            }
            out
        }
        NpcPage::Quests => view
            .quests
            .iter()
            .map(|q| {
                let (right, act) = match q.state {
                    QuestState::Ready => ("보고하기", Some(TradeAction::TurnInQuest(q.quest))),
                    QuestState::Active => ("진행 중", None),
                    _ if q.can_accept => ("받기", Some(TradeAction::AcceptQuest(q.quest))),
                    QuestState::Completed => ("완료", None),
                    _ => ("아직 받을 수 없음", None),
                };
                row(
                    q.title.clone(),
                    right.to_owned(),
                    act.is_some(),
                    act.map_or(NpcAct::None, NpcAct::Trade),
                )
            })
            .collect(),
        NpcPage::Buy(shop) => view
            .shops
            .iter()
            .filter(|s| s.shop == shop)
            .flat_map(|s| {
                s.goods.iter().map(move |(g, name, price, ok)| {
                    row(
                        name.clone(),
                        price.clone(),
                        *ok,
                        NpcAct::Trade(TradeAction::Buy {
                            shop,
                            goods: *g,
                            count: 1,
                        }),
                    )
                })
            })
            .collect(),
        NpcPage::Sell(shop) => view
            .shops
            .iter()
            .filter(|s| s.shop == shop)
            .flat_map(|s| {
                s.sellable
                    .iter()
                    .map(move |(bag, slot, name, count, price)| {
                        row(
                            format!("{name} ×{count}"),
                            format!("개당 {price}"),
                            true,
                            NpcAct::Trade(TradeAction::Sell {
                                shop,
                                bag: *bag,
                                slot: *slot,
                                count: 1,
                            }),
                        )
                    })
            })
            .collect(),
        NpcPage::Craft(craft) => view
            .crafts
            .iter()
            .filter(|c| c.craft == craft)
            .flat_map(|c| {
                c.recipes.iter().map(move |(r, output, needs, ok)| {
                    row(
                        output.clone(),
                        needs.clone(),
                        *ok,
                        NpcAct::Trade(TradeAction::Craft {
                            craft,
                            recipe: *r,
                            times: 1,
                        }),
                    )
                })
            })
            .collect(),
        NpcPage::Teleport(teleport) => view
            .teleports
            .iter()
            .filter(|t| t.teleport == teleport)
            .flat_map(|t| {
                t.destinations.iter().map(move |(d, name, price)| {
                    row(
                        name.clone(),
                        price.clone(),
                        true,
                        NpcAct::Trade(TradeAction::Teleport { teleport, dest: *d }),
                    )
                })
            })
            .collect(),
        NpcPage::Deposit => view
            .storage
            .iter()
            .flat_map(|s| s.carried.iter())
            .map(|(bag, slot, name, count)| {
                row(
                    format!("{name} ×{count}"),
                    String::from("맡기기"),
                    true,
                    NpcAct::Trade(TradeAction::Deposit {
                        bag: *bag,
                        slot: *slot,
                        count: *count,
                    }),
                )
            })
            .collect(),
        NpcPage::Withdraw => view
            .storage
            .iter()
            .flat_map(|s| s.stored.iter())
            .map(|(slot, name, count)| {
                row(
                    format!("{name} ×{count}"),
                    String::from("찾기"),
                    true,
                    NpcAct::Trade(TradeAction::Withdraw {
                        slot: *slot,
                        count: *count,
                    }),
                )
            })
            .collect(),
    }
}

/// 조건 안에 `Cleared` 가 있는가 — 막힌 포털이 던전의 나가는 문인지 알아볼 때 (R6-2).
fn mentions_cleared(c: &Condition) -> bool {
    match c {
        Condition::Cleared => true,
        Condition::All(list) | Condition::Any(list) => list.iter().any(mentions_cleared),
        Condition::Not(c) => mentions_cleared(c),
        _ => false,
    }
}

/// 스킬 창 한 줄 (R3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SkillLine {
    pub(crate) skill: SkillId,
    pub(crate) name: String,
    pub(crate) level: u32,
    pub(crate) max_level: u32,
    /// 지금 레벨의 MP.
    pub(crate) mp_cost: u32,
    /// 놓인 단축키 칸 (0부터). 없으면 `None`.
    pub(crate) slot: Option<usize>,
}

/// 진행 중인 플레이 한 판.
#[derive(Debug)]
pub(crate) struct PlaySession {
    auth: LocalAuthority,
    player: Entity,
    labels: HashMap<Entity, Label>,
    /// 유닛마다 따로 — 걷는 유닛과 서 있는 유닛의 위상이 달라야 한다.
    animators: HashMap<Entity, SpriteAnimator>,
    /// 맞은 유닛의 붉은 깜빡임 — 남은 tick.
    hit_flash: HashMap<Entity, u8>,
    order: Order,
    /// 단축키 스킬 — 조작하는 액터 타입의 `skills`. 1번 키가 맨 앞.
    /// 단축키 칸 — [`HOTBAR_SLOTS`] 칸, 빈 칸은 `None` (R3). **플레이어가 직접 배치한다** — 새로 배운
    /// 스킬은 저절로 들어가지 않는다. 처음에는 시작 스킬을 차례로 넣어 둔다.
    hotbar: Vec<Option<SkillId>>,
    /// 플레이어가 기본 부활할 시각 (시뮬레이션 시계). 쓰러져 있는 동안만 있다.
    revive_at: Option<Duration>,
    /// 거래 창을 연 NPC (R4) — 말을 걸면 열리고, 멀어지거나 닫으면 닫힌다.
    trading: Option<Entity>,
    /// NPC 창의 지금 쪽 (R4b). 말을 걸 때마다 메뉴부터.
    npc_page: NpcPage,
    /// 다른 레벨로 가는 텔레포트 요청 `(레벨, 도착 위치)` (R4-2) — 에디터가 꺼내 레벨을 연다.
    travel: Option<(String, Vec2)>,
    /// 포털이 가는 레벨 — `PORTAL_TRAVEL_BASE + 색인` 이 `travel` 번호다 (R6).
    portal_levels: Vec<String>,
    /// 조작하는 액터의 스크립트가 `on_dead` 를 정의했다 — 부활은 스크립트가 맡고 기본 부활은 하지 않는다.
    scripted_revive: bool,
    /// 주문을 위해 마지막으로 경로를 잡은 지점.
    chase_goal: Option<Vec2>,
    log: VecDeque<String>,
    /// 상태 바에 띄울 경고 (스크립트 오류) — 편집기가 꺼내 간다.
    alerts: Vec<String>,
    /// 시작 전 에디터 카메라 — 정지하면 되돌린다.
    editor_camera: Camera2d,
    /// 이번 판의 게임 데이터 (규칙 수치 + 이름·색). 시작할 때 파일에서 읽는다.
    data: GameData,
}

impl PlaySession {
    /// 씬에서 시뮬레이션을 만든다. **첫 플레이어 스폰**에 플레이어가 서고, 나머지 플레이어
    /// 스폰은 스폰 지점일 뿐이라 유닛을 만들지 않는다.
    ///
    /// # Errors
    /// 플레이어 스폰이 없거나, 액터 스크립트에 문법 오류가 있으면 플레이할 수 없다.
    pub(crate) fn start(
        scene: &Scene,
        editor_camera: Camera2d,
        data: GameData,
        options: PlayOptions,
    ) -> Result<Self, String> {
        // 플레이용 맵은 **존 전체**를 덮는다 — 에디터 씬의 맵이 존보다 작아도(칠한 곳 주변만) 존 안은
        // 어디든 걸을 수 있게. 늘어난 칸은 기본 타일이고, 청크라 칠하지 않은 곳은 메모리를 쓰지 않는다 (R6-3).
        // 씬·존 파일은 건드리지 않는다: 화면은 씬의 맵으로 그리고, 시뮬레이션과는 월드 미터로만 이어진다.
        let mut tiles = scene.tiles.clone();
        tiles.grow_to_cover(
            scene.zone.min,
            scene.zone.max,
            nexus_sim::Tile::default(),
            crate::zone_file::MAX_TILEMAP_SIDE,
        );
        let mut world = SimWorld::new(tiles);
        data.install(&mut world);

        // 액터 스크립트 (P2) — 플레이 시작마다 새로 컴파일한다. 문법 오류면 플레이를 막는다
        // (고친 것이 적용되지 않은 채 모르고 지나가지 않게 — 데이터 파일과 같은 규칙).
        let mut scripts = RhaiHost::new(data.seed());
        let mut compiled = HashMap::new();
        for (path, source) in data.script_sources() {
            let id = scripts.add_script(path, source)?;
            compiled.insert(path.to_owned(), id);
        }

        // 플레이어 직업 (R2) — 저장한 직업 → 새 게임에서 고른 직업 → 기본 직업. 데이터에서 사라진
        // 직업은 건너뛴다 (데이터를 고쳤다고 이어 하기가 막히면 곤란하다 — 아이템과 같은 규칙).
        let player_class = [options.save.as_ref().and_then(|s| s.class), options.class]
            .into_iter()
            .flatten()
            .find(|c| data.has_class(*c))
            .or_else(|| data.default_class());

        let mut labels = HashMap::new();
        let mut player = None;
        // 조작할 스폰 — 고른 마커가 있으면 그것, 없으면 첫 플레이어 스폰 (P3-1).
        let chosen = options.spawn.filter(|e| {
            scene
                .items
                .iter()
                .any(|i| i.entity == *e && i.kind == ItemKind::PlayerSpawn)
        });
        // 포털 (R6) — 액터를 세우지 않고 구역만 놓는다. 다른 레벨이면 번호를 매겨 두고,
        // `Traveled` 가 오면 그 번호로 레벨을 찾는다.
        let mut portal_levels: Vec<String> = Vec::new();
        for item in scene.items.iter().filter(|i| i.kind == ItemKind::Portal) {
            let link = &item.portal;
            let level = link.level.trim();
            let travel = if level.is_empty() || level == options.links.here {
                0
            } else {
                let i = portal_levels
                    .iter()
                    .position(|l| l == level)
                    .unwrap_or_else(|| {
                        portal_levels.push(level.to_owned());
                        portal_levels.len() - 1
                    });
                PORTAL_TRAVEL_BASE + u32::try_from(i).unwrap_or(0)
            };
            world.add_portal(PortalDef {
                center: item.pos,
                radius: item.size * 0.5,
                travel,
                pos: link.to,
                // 도착 레벨의 입장 조건 + 이 포털만의 조건(나가는 문의 `Cleared` 등, R6-2) — 둘 다 맞아야 한다.
                requires: match (
                    options.links.requires.get(level).cloned(),
                    link.requires.as_ref().map(Condition::from),
                ) {
                    (Some(a), Some(b)) => Condition::All(vec![a, b]),
                    (Some(c), None) | (None, Some(c)) => c,
                    (None, None) => Condition::Always,
                },
            });
        }
        // 던전 클리어 (R6-2) — 존(인스턴스)의 목표. 레벨을 열 때마다 처음부터다.
        if let Some(clear) = options.links.clear.clone() {
            world.set_clear(clear);
        }
        for item in scene.items.iter().filter(|i| i.kind.spawns_actor()) {
            let is_player =
                item.kind == ItemKind::PlayerSpawn && chosen.is_none_or(|e| e == item.entity);
            if is_player && player.is_some() {
                continue;
            }
            if item.kind == ItemKind::PlayerSpawn && !is_player {
                continue; // 나머지 플레이어 스폰은 지점일 뿐이다
            }
            // 마커가 가리키는 액터 타입에서 수치·그림이 나오고, 마커별 덮어쓰기가 그 위에 얹힌다 (P1, P1-4).
            let actor = data.resolve_actor(item.actor, item.kind);
            // 스폰 영역 — 무리는 반경 안에 흩어 세운다. 걸을 수 없는 칸에 떨어진 마리는 마커 자리로.
            // 마리마다 자기 자리가 리스폰 지점이 된다 (P15).
            let points = if is_player {
                vec![item.pos]
            } else {
                item.spawn.points(item.pos)
            };
            for pos in points {
                let tiles = world.tiles();
                let pos = if tiles
                    .get(tiles.world_to_tile(pos))
                    .is_some_and(|t| t.walkable)
                {
                    pos
                } else {
                    item.pos
                };
                let mut def = item.overrides.apply(data.unit_def(actor));
                if is_player {
                    def.class = player_class;
                }
                let unit = world.spawn_unit(pos, item.orientation, def);
                let name = if is_player {
                    String::from("플레이어")
                } else {
                    item.name.clone()
                };
                labels.insert(
                    unit,
                    Label {
                        name,
                        actor,
                        // 두 색을 따로 들고 간다 — 어느 쪽을 쓸지는 시트가 무채색인지로 정해진다.
                        tint: data.actor_look(actor).map_or(NO_TINT, ActorLook::tint),
                        fallback: item.kind.color(),
                    },
                );
                if let Some(id) = data.actor_script(actor).and_then(|p| compiled.get(p)) {
                    scripts.attach(unit, *id);
                }
                if is_player {
                    // 이어 하기면 저장한 소지품이 들어오므로 시작 소지품은 주지 않는다.
                    match options.save.as_ref() {
                        Some(save) => {
                            restore(&mut world, unit, save);
                            // 재화가 생기기 전의 저장 — 시작 재화로 이어 한다 (R4).
                            if save.wallet.is_none() {
                                data.give_starting_currency(&mut world, unit);
                            }
                        }
                        None => data.give_starting_kit(&mut world, unit),
                    }
                    player = Some(unit);
                }
            }
        }
        let player = player.ok_or("플레이어 스폰이 없어 플레이할 수 없습니다")?;
        // 시작 스킬 — 직업이 정했으면 그것, 아니면 액터 타입의 것 (R3).
        let starting: Vec<SkillId> = player_class
            .and_then(|c| data.class_skills(c))
            .map(<[SkillId]>::to_vec)
            .or_else(|| {
                labels
                    .get(&player)
                    .map(|l: &Label| data.actor_skills(l.actor).to_vec())
            })
            .unwrap_or_default();
        // 배운 스킬 — 이어 하기면 저장한 것, 아니면(새 게임·스킬이 생기기 전의 저장) 시작 스킬 1레벨.
        let saved_skills = options.save.as_ref().and_then(|s| s.skills.as_ref());
        match saved_skills {
            Some(list) => {
                for &(skill, level) in list {
                    world.set_skill_level(player, skill, level);
                }
            }
            None => {
                for &skill in &starting {
                    world.set_skill_level(player, skill, 1);
                }
            }
        }
        // 단축키 — 저장한 배치가 있으면 그것, 아니면 시작 스킬을 차례로.
        let mut hotbar = match options.save.as_ref().and_then(|s| s.hotbar.as_ref()) {
            Some(saved) => saved.clone(),
            None => starting.iter().map(|s| Some(*s)).collect(),
        };
        hotbar.resize(HOTBAR_SLOTS, None);
        let scripted_revive = labels
            .get(&player)
            .is_some_and(|l: &Label| data.scripted_revive(l.actor));

        let mut auth = LocalAuthority::new(world);
        auth.set_script_host(Box::new(scripts));

        let mut session = Self {
            auth,
            player,
            labels,
            animators: HashMap::new(),
            hit_flash: HashMap::new(),
            order: Order::Idle,
            hotbar,
            revive_at: None,
            trading: None,
            npc_page: NpcPage::Menu,
            travel: None,
            portal_levels,
            scripted_revive,
            chase_goal: None,
            log: VecDeque::new(),
            alerts: Vec::new(),
            editor_camera,
            data,
        };
        session.note(String::from(
            "플레이 시작 — 클릭: 이동 · 적 클릭: 공격 · 아이템 클릭: 줍기 · 숫자키: 스킬",
        ));
        Ok(session)
    }

    /// 시작 전 에디터 카메라.
    pub(crate) fn editor_camera(&self) -> &Camera2d {
        &self.editor_camera
    }

    pub(crate) fn world(&self) -> &SimWorld {
        self.auth.world()
    }

    pub(crate) fn player(&self) -> Entity {
        self.player
    }

    /// 기본 부활까지 남은 초 — 쓰러져 있고 기본 부활을 기다릴 때만. 스크립트가 맡으면 `None`.
    pub(crate) fn revive_in(&self) -> Option<f32> {
        let at = self.revive_at?;
        Some(at.saturating_sub(self.world().now()).as_secs_f32())
    }

    pub(crate) fn ticks(&self) -> u64 {
        self.auth.ticks()
    }

    /// 플레이어 직업 이름 (R2). 직업이 없으면 빈 문자열.
    pub(crate) fn class_name(&self) -> String {
        self.world()
            .unit(self.player)
            .and_then(|u| u.def().class)
            .map_or_else(String::new, |c| self.data.class_name(c))
    }

    /// 플레이어 기본 스탯 `(화면 이름, 값)` — 데이터가 정한 순서 (R2). 직업이 없으면 빈 목록.
    pub(crate) fn stat_lines(&self) -> Vec<(String, i64)> {
        let (Some(u), Some(rules)) = (self.world().unit(self.player), self.world().stat_rules())
        else {
            return Vec::new();
        };
        let Some(sheet) = u.stat_sheet() else {
            return Vec::new();
        };
        rules
            .stats()
            .iter()
            .zip(&sheet.base)
            .map(|(key, v)| (self.data.stat_name(key).to_owned(), *v))
            .collect()
    }

    /// 다른 레벨로 가는 텔레포트 요청을 꺼낸다 (R4-2).
    pub(crate) fn take_travel(&mut self) -> Option<(String, Vec2)> {
        self.travel.take()
    }

    /// 기본 재화 금액 (R4) — HUD `{gold}`. 재화가 없는 게임이면 0.
    pub(crate) fn gold(&self) -> u64 {
        let (Some(u), Some(c)) = (self.world().unit(self.player), self.data.default_currency())
        else {
            return 0;
        };
        u.wallet().get(c)
    }

    /// 거래 창 (R4) — 말을 건 NPC 가 있으면 그 기능들.
    pub(crate) fn trade_view(&self) -> Option<TradeView> {
        let npc = self.trading?;
        let world = self.world();
        let me = world.unit(self.player)?;
        let purse = self.data.default_currency().map_or_else(String::new, |c| {
            self.data.price_text(Price {
                currency: c,
                amount: u32::try_from(me.wallet().get(c)).unwrap_or(u32::MAX),
            })
        });
        let mut shops = Vec::new();
        let mut services = Vec::new();
        let mut crafts = Vec::new();
        let mut teleports = Vec::new();
        let mut storage = None;
        let mut quests = Vec::new();
        for (i, service) in world.services_of(npc).iter().enumerate() {
            let index = u16::try_from(i).unwrap_or(u16::MAX);
            match *service {
                Service::Shop(id) => {
                    let Some(def) = world.shop(id) else { continue };
                    let goods = def
                        .goods
                        .iter()
                        .enumerate()
                        .map(|(g, goods)| {
                            (
                                u16::try_from(g).unwrap_or(u16::MAX),
                                self.data.item_name(goods.item),
                                self.data.price_text(goods.price),
                                me.wallet().get(goods.price.currency)
                                    >= u64::from(goods.price.amount),
                            )
                        })
                        .collect();
                    let sellable = [BagKind::Consumable, BagKind::Equipment]
                        .into_iter()
                        .flat_map(|bag| {
                            me.inventory()
                                .bag(bag)
                                .iter()
                                .map(move |(slot, s)| (bag, slot, s))
                                .collect::<Vec<_>>()
                        })
                        .filter_map(|(bag, slot, s)| {
                            let price = world.sell_price(id, s.item)?;
                            Some((
                                bag,
                                u16::try_from(slot).ok()?,
                                self.data.item_name(s.item),
                                s.count,
                                self.data.price_text(price),
                            ))
                        })
                        .collect();
                    shops.push(ShopView {
                        shop: id,
                        goods,
                        sellable,
                    });
                }
                Service::Heal { price } => {
                    let label = if price.amount == 0 {
                        String::from("치료 (무료)")
                    } else {
                        format!("치료 — {}", self.data.price_text(price))
                    };
                    services.push((index, label));
                }
                Service::Craft(id) => {
                    let Some(def) = world.craft_def(id) else {
                        continue;
                    };
                    let carried = |item| {
                        let inv = me.inventory();
                        inv.bag(BagKind::Consumable).count_of(item)
                            + inv.bag(BagKind::Equipment).count_of(item)
                    };
                    let recipes = def
                        .recipes
                        .iter()
                        .enumerate()
                        .map(|(r, recipe)| {
                            let mut parts: Vec<String> = recipe
                                .inputs
                                .iter()
                                .map(|(item, n)| {
                                    format!("{} {}/{n}", self.data.item_name(*item), carried(*item))
                                })
                                .collect();
                            if let Some(fee) = recipe.fee {
                                parts.push(self.data.price_text(fee));
                            }
                            let enough = recipe.inputs.iter().all(|(item, n)| carried(*item) >= *n)
                                && recipe.fee.is_none_or(|f| {
                                    me.wallet().get(f.currency) >= u64::from(f.amount)
                                })
                                && world.check(&recipe.requires, self.player);
                            (
                                u16::try_from(r).unwrap_or(u16::MAX),
                                format!("{} ×{}", self.data.item_name(recipe.output), recipe.count),
                                parts.join(" · "),
                                enough,
                            )
                        })
                        .collect();
                    crafts.push(CraftView { craft: id, recipes });
                }
                Service::Teleport(id) => {
                    let Some(def) = world.teleport_def(id) else {
                        continue;
                    };
                    let destinations = def
                        .destinations
                        .iter()
                        .enumerate()
                        .map(|(d, dest)| {
                            (
                                u16::try_from(d).unwrap_or(u16::MAX),
                                self.data.destination_name(id, d),
                                dest.price.map_or_else(
                                    || String::from("무료"),
                                    |p| self.data.price_text(p),
                                ),
                            )
                        })
                        .collect();
                    teleports.push(TeleportView {
                        teleport: id,
                        destinations,
                    });
                }
                Service::Quest(quest) => {
                    quests.push(QuestOffer {
                        quest,
                        title: self.data.quest_title(quest),
                        description: self.data.quest_description(quest).to_owned(),
                        state: world.quest_state(self.player, quest),
                        can_accept: world.can_start_quest(self.player, quest),
                    });
                }
                Service::Storage => {
                    let carried = [BagKind::Consumable, BagKind::Equipment]
                        .into_iter()
                        .flat_map(|bag| {
                            me.inventory()
                                .bag(bag)
                                .iter()
                                .filter_map(move |(slot, s)| {
                                    Some((bag, u16::try_from(slot).ok()?, s))
                                })
                                .collect::<Vec<_>>()
                        })
                        .map(|(bag, slot, s)| (bag, slot, self.data.item_name(s.item), s.count))
                        .collect();
                    let stored = me
                        .storage()
                        .iter()
                        .filter_map(|(slot, s)| {
                            Some((
                                u16::try_from(slot).ok()?,
                                self.data.item_name(s.item),
                                s.count,
                            ))
                        })
                        .collect();
                    storage = Some(StorageView { carried, stored });
                }
            }
        }
        let profile = self
            .labels
            .get(&npc)
            .and_then(|l| self.data.actor_look(l.actor))
            .map(|l| l.profile.clone())
            .unwrap_or_default();
        Some(TradeView {
            npc_name: self.name(npc).to_owned(),
            purse,
            shops,
            services,
            profile,
            crafts,
            teleports,
            storage,
            quests,
        })
    }

    /// 퀘스트 목록 (R5) — 진행 중(보고 가능 포함)인 것만, 번호 순.
    pub(crate) fn quest_lines(&self) -> Vec<QuestLine> {
        let world = self.world();
        let Some(me) = world.unit(self.player) else {
            return Vec::new();
        };
        me.quests()
            .iter()
            .filter_map(|(quest, entry)| {
                let active = entry.active.as_ref()?;
                let def = world.quest_def(quest)?;
                let goals = def
                    .stages
                    .get(usize::from(active.stage))
                    .map(|stage| {
                        stage
                            .objectives
                            .iter()
                            .enumerate()
                            .map(|(i, o)| {
                                self.goal_text(o, active.counts.get(i).copied().unwrap_or(0))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                Some(QuestLine {
                    quest,
                    title: self.data.quest_title(quest),
                    state: world.quest_state(self.player, quest),
                    goals,
                })
            })
            .collect()
    }

    /// 던전 클리어 목표 (R6-2) — `(클리어했나, 목표 글들)`. 클리어가 없는 존이면 `None`.
    pub(crate) fn clear_lines(&self) -> Option<(bool, Vec<(String, bool)>)> {
        let world = self.world();
        let def = world.clear_def()?;
        let counts = world.clear_counts().unwrap_or_default();
        let goals = def
            .objectives
            .iter()
            .enumerate()
            .map(|(i, o)| {
                let (text, _) = self.goal_text(o, counts.get(i).copied().unwrap_or(0));
                (text, world.clear_objective_done(i))
            })
            .collect();
        Some((world.is_cleared(), goals))
    }

    /// 목표 하나의 글과 채웠는지.
    fn goal_text(&self, o: &nexus_sim::Objective, count: u32) -> (String, bool) {
        use nexus_sim::Objective;
        let world = self.world();
        match o {
            Objective::Kill { actor, count: n } => {
                let who =
                    actor.map_or_else(|| String::from("적"), |a| self.data.actor_type_name(a));
                (format!("{who} 처치 {}/{n}", count.min(*n)), count >= *n)
            }
            Objective::Collect { item, count: n, .. } => {
                let have = world.unit(self.player).map_or(0, |u| {
                    let inv = u.inventory();
                    inv.bag(BagKind::Consumable).count_of(*item)
                        + inv.bag(BagKind::Equipment).count_of(*item)
                });
                (
                    format!("{} 모으기 {}/{n}", self.data.item_name(*item), have.min(*n)),
                    have >= *n,
                )
            }
            Objective::Reach { .. } => (String::from("목표 지점에 가기"), count >= 1),
            Objective::Talk { actor } => (
                format!("{} 에게 말 걸기", self.data.actor_type_name(*actor)),
                count >= 1,
            ),
            Objective::Check(c) => (String::from("조건 맞추기"), world.check(c, self.player)),
        }
    }

    /// 퀘스트 포기 (R5) — 어디서나.
    pub(crate) fn abandon_quest(&mut self, quest: QuestId) {
        self.auth.submit(Intent::AbandonQuest {
            unit: self.player,
            quest,
        });
    }

    /// 거래 창 조작 (R4) — Intent 로 바꿔 낸다. 판정은 권한자가 한다.
    pub(crate) fn trade(&mut self, action: TradeAction) {
        let (Some(npc), unit) = (self.trading, self.player) else {
            return;
        };
        let intent = match action {
            TradeAction::Close => {
                self.trading = None;
                return;
            }
            TradeAction::Buy { shop, goods, count } => Intent::Buy {
                unit,
                npc,
                shop,
                goods,
                count,
            },
            TradeAction::Sell {
                shop,
                bag,
                slot,
                count,
            } => Intent::Sell {
                unit,
                npc,
                shop,
                bag,
                slot,
                count,
            },
            TradeAction::Service(index) => Intent::UseService { unit, npc, index },
            TradeAction::Craft {
                craft,
                recipe,
                times,
            } => Intent::Craft {
                unit,
                npc,
                craft,
                recipe,
                times,
            },
            TradeAction::Teleport { teleport, dest } => Intent::Teleport {
                unit,
                npc,
                teleport,
                dest,
            },
            TradeAction::Deposit { bag, slot, count } => Intent::Deposit {
                unit,
                npc,
                bag,
                slot,
                count,
            },
            TradeAction::Withdraw { slot, count } => Intent::Withdraw {
                unit,
                npc,
                slot,
                count,
            },
            TradeAction::AcceptQuest(quest) => Intent::AcceptQuest { unit, npc, quest },
            TradeAction::TurnInQuest(quest) => Intent::TurnInQuest { unit, npc, quest },
        };
        self.auth.submit(intent);
    }

    /// NPC 창이 열려 있는가 (R4b) — 화면 스택에 NPC 화면을 띄우고 내리는 기준.
    pub(crate) fn is_trading(&self) -> bool {
        self.trading.is_some()
    }

    /// NPC 창의 지금 쪽 (R4b) — 대화창과 목록. 말을 건 NPC 가 없으면 `None`.
    pub(crate) fn npc_screen(&self) -> Option<NpcScreen> {
        let view = self.trade_view()?;
        let page = self.npc_page;
        let mut rows = npc_rows(&view, page);
        if rows.is_empty() {
            rows.push(NpcRow {
                text: String::from("(없음)"),
                right: String::new(),
                enabled: false,
                act: NpcAct::None,
            });
        }
        Some(NpcScreen {
            name: view.npc_name,
            profile: view.profile,
            page: page.title().to_owned(),
            rows,
        })
    }

    /// NPC 창 목록의 `index` 번째 줄을 누른다 (R4b) — 쪽을 넘기거나 거래 Intent 를 낸다.
    /// 흐린 줄·없는 줄은 아무 일도 없다.
    pub(crate) fn npc_pick(&mut self, index: usize) {
        let Some(row) = self
            .npc_screen()
            .and_then(|s| s.rows.into_iter().nth(index))
        else {
            return;
        };
        if !row.enabled {
            return;
        }
        match row.act {
            NpcAct::Page(page) => self.npc_page = page,
            NpcAct::Trade(action) => self.trade(action),
            NpcAct::None => {}
        }
    }

    /// NPC 창을 처음 메뉴로 (R4b).
    pub(crate) fn npc_back(&mut self) {
        self.npc_page = NpcPage::Menu;
    }

    /// 플레이어가 배운 스킬 — 스킬 창용 (R3). 직업이 없는 게임이면 단축키에 있는 스킬을 1레벨로 보인다.
    pub(crate) fn skill_lines(&self) -> Vec<SkillLine> {
        let world = self.world();
        let Some(u) = world.unit(self.player) else {
            return Vec::new();
        };
        let mut learned: Vec<(SkillId, u32)> = u.learned_skills().collect();
        if u.def().class.is_none() && learned.is_empty() {
            learned = self.hotbar.iter().flatten().map(|s| (*s, 1)).collect();
        }
        learned
            .into_iter()
            .map(|(skill, level)| {
                let def = world.effective_skill(self.player, skill);
                SkillLine {
                    skill,
                    name: self.data.skill_name(skill),
                    level,
                    max_level: world.skill_max_level(skill),
                    mp_cost: def.map_or(0, |d| d.mp_cost),
                    slot: self.hotbar.iter().position(|s| *s == Some(skill)),
                }
            })
            .collect()
    }

    /// 단축키 배치 (R3) — `slot`(0부터) 에 `skill` 을 놓는다. 다른 칸에 있던 같은 스킬은 빠지고,
    /// 그 칸에 있던 스킬은 밀려난다. `slot` 이 `None` 이면 단축키에서 뺀다.
    pub(crate) fn assign_hotbar(&mut self, skill: SkillId, slot: Option<usize>) {
        for s in &mut self.hotbar {
            if *s == Some(skill) {
                *s = None;
            }
        }
        if let Some(i) = slot.filter(|i| *i < HOTBAR_SLOTS) {
            self.hotbar[i] = Some(skill);
            self.note(format!(
                "단축키 {} ← {}",
                i + 1,
                self.data.skill_name(skill)
            ));
        }
    }

    /// 유닛 이름. 모르면 `"?"`.
    pub(crate) fn name(&self, unit: Entity) -> &str {
        self.labels.get(&unit).map_or("?", |l| l.name.as_str())
    }

    /// 지금 상태를 저장 데이터로 (P3). `zone` 은 편집기가 열어 둔 존 파일이다.
    pub(crate) fn save_data(&self, zone: String) -> SaveData {
        let u = self.world().unit(self.player);
        let progress = u.map(|u| u.progress()).unwrap_or_default();
        let inventory = u.map(Unit::inventory);
        let bags = [BagKind::Consumable, BagKind::Equipment]
            .into_iter()
            .filter_map(|kind| inventory.map(|i| i.bag(kind)))
            .flat_map(|bag| {
                bag.iter()
                    .map(|(slot, s)| (slot as u16, s.item, s.count))
                    .collect::<Vec<_>>()
            })
            .collect();
        SaveData {
            actor: self
                .labels
                .get(&self.player)
                .map_or(ActorId::DEFAULT, |l| l.actor),
            level: progress.level(),
            exp: progress.exp(),
            // 쓰러진 채로 저장하면 이어 할 수 없다 — 최소 1 로 둔다.
            hp: u.map_or(1, |u| u.hp().max(1)),
            mp: u.map(Unit::mp),
            zone,
            pos: u.map(Unit::pos),
            bags,
            equipped: EquipSlot::ALL
                .into_iter()
                .filter_map(|slot| inventory?.equipped(slot))
                .collect(),
            class: u.and_then(|u| u.def().class),
            skills: Some(u.map(|u| u.learned_skills().collect()).unwrap_or_default()),
            hotbar: Some(self.hotbar.clone()),
            flags: u.map(|u| u.flags().iter().collect()).unwrap_or_default(),
            wallet: u.map(|u| u.wallet().iter().collect()),
            quests: u
                .map(|u| {
                    u.quests()
                        .iter()
                        .map(|(q, e)| {
                            (
                                q,
                                e.active.as_ref().map(|a| (a.stage, a.counts.clone())),
                                e.completed,
                            )
                        })
                        .collect()
                })
                .unwrap_or_default(),
            storage: u
                .map(|u| {
                    u.storage()
                        .iter()
                        .map(|(slot, s)| (slot as u16, s.item, s.count))
                        .collect()
                })
                .unwrap_or_default(),
            world_flags: self.world().world_flags().iter().collect(),
        }
    }

    /// 상태 바에 띄울 경고를 꺼낸다.
    pub(crate) fn take_alerts(&mut self) -> Vec<String> {
        std::mem::take(&mut self.alerts)
    }

    /// 최근 이벤트 (오래된 것부터).
    pub(crate) fn log(&self) -> impl Iterator<Item = &str> {
        self.log.iter().map(String::as_str)
    }

    /// 플레이어가 두 tick 사이 어디쯤 그려지는가 — 카메라가 따라간다.
    pub(crate) fn player_render_pos(&self, alpha: f32) -> Option<Vec2> {
        self.world().unit(self.player).map(|u| u.render_pos(alpha))
    }

    // ── 입력 ─────────────────────────────────────────────────────────────────

    /// 뷰포트 클릭. 적이면 공격, 땅의 아이템이면 줍기, 아니면 그 자리로 이동.
    ///
    /// `px` 는 화면 1픽셀의 월드 길이 — 멀리서 봐도 작은 대상을 누를 수 있게 한다.
    pub(crate) fn click(&mut self, at: Vec2, px: f32) {
        let world = self.auth.world();
        let Some(me) = world.unit(self.player).filter(|u| u.is_alive()) else {
            return;
        };
        let my_faction = me.def().faction;

        let nearest_unit = world
            .units()
            .filter(|(e, u)| *e != self.player && u.is_alive())
            .map(|(e, u)| (e, u.pos().distance(at)))
            .filter(|&(_, d)| d <= (14.0 * px).max(0.6))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let nearest_item = world
            .ground_items()
            .map(|(e, g)| (e, g.pos.distance(at)))
            .filter(|&(_, d)| d <= (10.0 * px).max(0.5))
            .min_by(|a, b| a.1.total_cmp(&b.1));

        self.chase_goal = None;
        let hostile = |t: Entity| {
            world
                .unit(t)
                .is_some_and(|u| world.relation(my_faction, u.def().faction) == Relation::Hostile)
        };
        if let Some((target, _)) = nearest_unit
            && !world.services_of(target).is_empty()
            && !hostile(target)
        {
            // 기능이 있는 NPC 는 치지 않고 말을 건다 (R4).
            self.order = Order::Talk(target);
        } else if let Some((target, _)) = nearest_unit
            && world
                .unit(target)
                .is_some_and(|t| world.relation(my_faction, t.def().faction) != Relation::Friendly)
        {
            self.order = Order::Attack {
                target,
                skill: None,
            };
        } else if let Some((item, _)) = nearest_item {
            self.order = Order::PickUp(item);
        } else {
            self.order = Order::Idle;
            self.auth.submit(Intent::MoveTo {
                unit: self.player,
                target: at,
            });
        }
    }

    /// 하던 주문(치기·줍기·말 걸기)이 있는가 — 경제 시뮬레이터의 봇이 다음 할 일을 고를 때 (R7).
    pub(crate) fn busy(&self) -> bool {
        !matches!(self.order, Order::Idle)
    }

    /// 지금 치고 있는 대상.
    pub(crate) fn attack_target(&self) -> Option<Entity> {
        match self.order {
            Order::Attack { target, .. } => Some(target),
            _ => None,
        }
    }

    /// HUD 단축키 칸 — 키 번호·남은 쿨타임 비율·MP 충분 여부.
    pub(crate) fn skill_slots(&self) -> Vec<crate::screen::SkillSlot> {
        let world = self.world();
        let me = world.unit(self.player);
        let now = world.now();
        // 마지막으로 채운 칸까지만 그린다 — 빈 칸은 흐리게.
        let shown = self
            .hotbar
            .iter()
            .rposition(Option::is_some)
            .map_or(0, |i| i + 1);
        self.hotbar[..shown]
            .iter()
            .enumerate()
            .map(|(i, slot)| {
                let key = u8::try_from(i + 1).unwrap_or(u8::MAX);
                let Some(skill) = *slot else {
                    return crate::screen::SkillSlot {
                        key,
                        cooldown: 0.0,
                        usable: false,
                    };
                };
                // 레벨 수식이 반영된 값 (R3) — MP·쿨타임이 레벨마다 다르다.
                let def = world.effective_skill(self.player, skill);
                let total = def.map_or(0.0, |d| d.cooldown().as_secs_f32());
                let left = me.map_or(0.0, |u| u.cooldown_left(skill, now).as_secs_f32());
                crate::screen::SkillSlot {
                    key,
                    cooldown: if total > 0.0 { left / total } else { 0.0 },
                    usable: me
                        .is_some_and(|u| u.is_alive() && def.is_some_and(|d| u.has_mp(d.mp_cost))),
                }
            })
            .collect()
    }

    /// 숫자키 `key`(1부터)의 스킬을 쓴다 — 지금 치는 대상에게, 없으면 가까운 적에게.
    /// 사거리 밖이면 다가가서 쓴다. 한 번 맞으면 기본 공격으로 이어 간다.
    ///
    /// 쿨타임·MP 는 **누르는 순간** 확인해 이유를 기록에 남긴다 — 예약해 두고 조용히 기다리면
    /// 눌렀는데 왜 안 나가는지 알 수 없다. 이때 하던 주문은 그대로 둔다.
    pub(crate) fn use_skill(&mut self, key: usize) {
        match self.plan_skill(key) {
            // 회복 스킬은 자신에게 곧바로 — 하던 주문(공격·이동)은 그대로 둔다.
            Ok((target, skill)) if target == self.player => {
                self.auth.submit(Intent::Attack {
                    unit: self.player,
                    target,
                    skill,
                });
                self.note(format!("{} → 자신", self.data.skill_name(skill)));
            }
            Ok((target, skill)) => {
                let line = format!("{} → {}", self.data.skill_name(skill), self.name(target));
                self.order = Order::Attack {
                    target,
                    skill: Some(skill),
                };
                self.chase_goal = None;
                self.note(line);
            }
            Err(why) => self.note(why),
        }
    }

    fn plan_skill(&self, key: usize) -> Result<(Entity, SkillId), String> {
        let skill = key
            .checked_sub(1)
            .and_then(|i| self.hotbar.get(i).copied().flatten())
            .ok_or_else(|| format!("단축키 {key} 에 스킬이 없습니다"))?;
        let name = self.data.skill_name(skill);
        let world = self.auth.world();
        let me = world
            .unit(self.player)
            .filter(|u| u.is_alive())
            .ok_or_else(|| String::from("쓰러진 상태입니다"))?;
        // 레벨 수식이 반영된 값 (R3).
        let def = world
            .effective_skill(self.player, skill)
            .ok_or_else(|| format!("{name}: 모르는 스킬입니다"))?;
        if me.def().class.is_some()
            && me.def().basic_attack != Some(skill)
            && me.skill_level(skill) == 0
        {
            return Err(format!("{name}: 배우지 않은 스킬입니다"));
        }
        let now = world.now();
        if !me.is_ready(skill, now) {
            return Err(format!(
                "{name}: 아직 쓸 수 없습니다 ({:.1}초)",
                me.cooldown_left(skill, now).as_secs_f32()
            ));
        }
        if !me.has_mp(def.mp_cost) {
            return Err(format!(
                "{name}: MP 가 부족합니다 ({}/{})",
                me.mp(),
                def.mp_cost
            ));
        }
        if def.is_heal() {
            return Ok((self.player, skill));
        }
        let current = self
            .attack_target()
            .filter(|t| world.unit(*t).is_some_and(Unit::is_alive));
        let target = current
            .or_else(|| {
                // 클릭으로 고른 대상이 없으면 가까운 적 — 우호·중립·불사는 고르지 않는다.
                let faction = me.def().faction;
                world
                    .units()
                    .filter(|(e, u)| {
                        *e != self.player
                            && u.is_alive()
                            && !u.def().immortal
                            && world.relation(faction, u.def().faction) == Relation::Hostile
                    })
                    .map(|(e, u)| (e, u.pos().distance(me.pos())))
                    .filter(|&(_, d)| d <= SKILL_SEARCH_RANGE)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(e, _)| e)
            })
            .ok_or_else(|| format!("{name}: 주변에 적이 없습니다"))?;
        Ok((target, skill))
    }

    /// 인벤토리 패널 조작.
    pub(crate) fn inventory(&mut self, action: InventoryAction) {
        let unit = self.player;
        self.auth.submit(match action {
            InventoryAction::Use(slot) => Intent::UseItem { unit, slot },
            InventoryAction::Equip(slot) => Intent::Equip { unit, slot },
            InventoryAction::Unequip(slot) => Intent::Unequip { unit, slot },
        });
    }

    // ── 진행 ─────────────────────────────────────────────────────────────────

    /// 한 tick. **고정 timestep 에서만** 부른다 — 애니메이션도 여기서 진행한다.
    pub(crate) fn tick(&mut self, dt: Duration, sprites: Option<&SpriteLibrary>) {
        // 기본 부활 — 시각이 되면 저장 지점(처음 선 자리, 이어 하기면 저장한 자리)에서 일으킨다.
        // 부활도 Intent 다: 단계 3 에서는 서버가 받아들일지 정한다.
        if let Some(at) = self.revive_at
            && self.world().now() >= at
        {
            self.revive_at = None;
            if let Some(u) = self.world().unit(self.player).filter(|u| !u.is_alive()) {
                let policy = self.data.revive();
                let intent = Intent::Revive {
                    unit: self.player,
                    at: u.spawn_pos(),
                    hp_per_mille: policy.hp_per_mille,
                    exp_loss_per_mille: policy.exp_loss_per_mille,
                    protect_ms: policy.protect_ms,
                };
                self.auth.submit(intent);
            }
        }
        for intent in self.follow_order() {
            self.auth.submit(intent);
        }
        let events = self.auth.tick(dt);
        let cues = AnimCue::collect(&events);
        for event in events {
            // 다른 레벨로 가는 텔레포트 (R4-2) — 레벨을 여는 것은 에디터의 일.
            if let Event::Traveled { unit, travel, pos } = event
                && unit == self.player
            {
                let level = travel
                    .checked_sub(PORTAL_TRAVEL_BASE)
                    .and_then(|i| self.portal_levels.get(usize::try_from(i).ok()?))
                    .map(String::as_str)
                    .or_else(|| self.data.travel_level(travel));
                match level {
                    Some(level) => self.travel = Some((level.to_owned(), pos)),
                    None => self
                        .alerts
                        .push(format!("텔레포트 목적지 {travel} 의 레벨을 모릅니다")),
                }
            }
            // 말을 걸면 거래 창이 열린다 (R4).
            if let Event::Interacted { unit, npc } = event
                && unit == self.player
            {
                self.trading = Some(npc);
                self.npc_page = NpcPage::Menu;
            }
            // 잊은 스킬은 단축키에서도 뺀다 (R3).
            if let Event::SkillForgotten { unit, skill } = event
                && unit == self.player
            {
                for slot in &mut self.hotbar {
                    if *slot == Some(skill) {
                        *slot = None;
                    }
                }
            }
            self.on_event(event);
        }
        // 거래 창은 멀어지거나 상대·내가 쓰러지면 닫는다 (R4).
        if let Some(npc) = self.trading {
            let world = self.world();
            let near = match (world.unit(self.player), world.unit(npc)) {
                (Some(me), Some(n)) => {
                    me.is_alive()
                        && n.is_alive()
                        && me.pos().distance(n.pos()) <= world.interact_range() + 0.5
                }
                _ => false,
            };
            if !near {
                self.trading = None;
            }
        }
        // 깜빡임은 tick 으로 센다 — 렌더 프레임으로 세면 fps 에 따라 길이가 달라진다.
        self.hit_flash.retain(|_, left| {
            *left -= 1;
            *left > 0
        });
        for (unit, cue) in &cues {
            if cue.hit {
                self.hit_flash.insert(*unit, HIT_FLASH_TICKS);
            }
        }
        // 스크립트의 print 는 이벤트 기록으로, 오류는 상태 바로도 (그 스크립트는 꺼졌다).
        let lines = self
            .auth
            .script_host_mut()
            .map(|host| host.drain_log())
            .unwrap_or_default();
        for line in lines {
            if line.error {
                self.alerts
                    .push(format!("스크립트 오류 — {}", line.message));
            }
            self.note(line.message);
        }

        // 유닛마다 자기 액터 타입의 시트로 진행한다 — 시트마다 클립 길이가 다르다.
        let actors: Vec<(Entity, ActorId)> = self
            .labels
            .iter()
            .map(|(unit, label)| (*unit, label.actor))
            .collect();
        let world = self.auth.world();
        for (unit, actor) in actors {
            let Some(u) = world.unit(unit) else { continue };
            next_anim(
                self.animators.entry(unit).or_default(),
                sprites.map(|s| s.sheet(actor).anim()),
                UnitPose {
                    alive: u.is_alive(),
                    moving: u.is_moving(),
                },
                cues.get(&unit).copied().unwrap_or_default(),
                dt,
            );
        }
    }

    /// 주문을 이번 tick 의 Intent 로 바꾼다. 다가가기는 여기서 — 규칙(사거리·줍기 거리)은 월드에 묻는다.
    fn follow_order(&mut self) -> Vec<Intent> {
        let attack_skill = self.data.player_attack();
        let world = self.auth.world();
        let me = self.player;
        let Some(u) = world.unit(me).filter(|u| u.is_alive()) else {
            self.order = Order::Idle;
            return Vec::new();
        };

        let (goal, reach) = match self.order {
            Order::Idle => return Vec::new(),
            Order::Attack { target, skill } => {
                let Some(t) = world.unit(target).filter(|t| t.is_alive()) else {
                    self.order = Order::Idle;
                    return Vec::new();
                };
                // 예약한 스킬이 있으면 그 스킬의 사거리까지 다가간다.
                let range = world
                    .skill(skill.unwrap_or(attack_skill))
                    .map_or(0.0, |s| s.range);
                (t.pos(), range)
            }
            Order::PickUp(item) => {
                let Some(g) = world.ground_item(item) else {
                    self.order = Order::Idle;
                    return Vec::new();
                };
                (g.pos, world.pickup_range())
            }
            Order::Talk(npc) => {
                let Some(n) = world.unit(npc).filter(|n| n.is_alive()) else {
                    self.order = Order::Idle;
                    return Vec::new();
                };
                // 거리 끝에서 멈추면 한 걸음 차이로 거절될 수 있어 조금 안쪽까지.
                (n.pos(), world.interact_range() * 0.8)
            }
        };

        let mut out = Vec::new();
        if u.pos().distance(goal) <= reach {
            if u.is_moving() {
                out.push(Intent::Stop { unit: me });
            }
            self.chase_goal = None;
            match self.order {
                Order::Attack { target, skill } => {
                    let skill = skill.unwrap_or(attack_skill);
                    if u.is_ready(skill, world.now()) {
                        out.push(Intent::Attack {
                            unit: me,
                            target,
                            skill,
                        });
                    }
                }
                Order::PickUp(item) => {
                    out.push(Intent::PickUp { unit: me, item });
                    self.order = Order::Idle;
                }
                Order::Talk(npc) => {
                    out.push(Intent::Interact { unit: me, npc });
                    self.order = Order::Idle;
                }
                Order::Idle => {}
            }
        } else if self
            .chase_goal
            .is_none_or(|g| g.distance(goal) > REPATH_DISTANCE)
            || !u.is_moving()
        {
            self.chase_goal = Some(goal);
            out.push(Intent::MoveTo {
                unit: me,
                target: goal,
            });
        }
        out
    }

    fn on_event(&mut self, event: Event) {
        // 포털·텔레포트로 옮겨졌으면 하던 주문을 버린다 — 옛 자리의 대상을 향해 되돌아 걷지 않게 (R6).
        if let Event::Teleported { unit, .. } | Event::Traveled { unit, .. } = event
            && unit == self.player
        {
            self.order = Order::Idle;
            self.chase_goal = None;
        }
        // 노리던 대상이 쓰러지면 그 tick 안에 주문을 끝낸다 — 다음 tick 까지 남겨 두면
        // 쓰러진 대상 쪽으로 한 걸음 더 내디딘다.
        if let Event::Died { unit, .. } = event
            && self.attack_target() == Some(unit)
        {
            self.order = Order::Idle;
            self.chase_goal = None;
        }
        // 리스폰 — 핸들이 새로 나왔다. 이름표를 옮기고, 옛 핸들에 걸린 재생기·깜빡임은 버린다
        // (새 유닛은 대기 동작부터). 플레이어였다면 조작 대상도 옮긴다.
        if let Event::Respawned { unit, replaces } = event {
            if let Some(label) = self.labels.remove(&replaces) {
                self.labels.insert(unit, label);
            }
            self.animators.remove(&replaces);
            self.hit_flash.remove(&replaces);
            if self.player == replaces {
                self.player = unit;
            }
        }
        // 예약한 스킬이 맞았으면 비운다 — 이후로는 기본 공격으로 이어 간다.
        if let Event::Damaged {
            attacker, skill, ..
        } = event
            && attacker == self.player
            && let Order::Attack { skill: queued, .. } = &mut self.order
            && *queued == Some(skill)
        {
            *queued = None;
        }
        let line = match event {
            Event::Damaged {
                attacker,
                target,
                skill,
                amount,
                remaining_hp,
            } => {
                // 기본 공격이 아니면 스킬 이름을 붙인다 — 단축키 스킬·스크립트 스킬이 기록에서 보이게.
                let basic = if attacker == self.player {
                    Some(self.data.player_attack())
                } else {
                    self.world()
                        .unit(attacker)
                        .and_then(|u| u.def().basic_attack)
                };
                let what = if basic == Some(skill) {
                    String::new()
                } else {
                    format!(" {}", self.data.skill_name(skill))
                };
                format!(
                    "{} →{what} {} {amount} 피해 (HP {remaining_hp})",
                    self.name(attacker),
                    self.name(target)
                )
            }
            Event::Died { unit, .. } if unit == self.player => {
                self.order = Order::Idle;
                self.chase_goal = None;
                if self.scripted_revive {
                    String::from(
                        "플레이어가 쓰러졌습니다 — 부활은 액터 스크립트(on_dead)가 정합니다",
                    )
                } else {
                    let delay = self.data.revive().delay;
                    self.revive_at = Some(self.world().now() + delay);
                    format!(
                        "플레이어가 쓰러졌습니다 — {:.1}초 뒤 저장 지점에서 일어납니다",
                        delay.as_secs_f32()
                    )
                }
            }
            Event::Died { unit, .. } => format!("{} 쓰러짐", self.name(unit)),
            Event::Revived {
                unit, hp, exp_lost, ..
            } => {
                let mut line = format!("{} 부활 — HP {hp}", self.name(unit));
                if exp_lost > 0 {
                    line.push_str(&format!(" · 경험치 -{exp_lost}"));
                }
                let now = self.world().now();
                if let Some(left) = self
                    .world()
                    .unit(unit)
                    .map(|u| u.protection_left(now))
                    .filter(|d| !d.is_zero())
                {
                    line.push_str(&format!(" · {:.1}초 보호", left.as_secs_f32()));
                }
                line
            }
            Event::Respawned { unit, .. } => format!("{} 다시 나타남", self.name(unit)),
            Event::ItemSpawned { stack, .. } => {
                format!(
                    "{} ×{} 떨어짐",
                    self.data.item_name(stack.item),
                    stack.count
                )
            }
            Event::PickedUp { stack, .. } => {
                format!("{} ×{} 획득", self.data.item_name(stack.item), stack.count)
            }
            Event::ExperienceGained {
                unit,
                amount,
                progress,
            } if unit == self.player => format!(
                "경험치 +{amount} (레벨 {} · {}/{})",
                progress.level(),
                progress.exp(),
                progress.exp_to_next()
            ),
            Event::LeveledUp { unit, level, .. } if unit == self.player => {
                format!("레벨 {level} 달성 — HP 가 가득 찼다")
            }
            Event::SkillLearned { unit, skill, level } if unit == self.player => {
                let max = self.world().skill_max_level(skill);
                if level == 1 {
                    format!(
                        "스킬을 배웠다: {} — 스킬 목록에서 단축키에 놓으세요",
                        self.data.skill_name(skill)
                    )
                } else {
                    format!("{} 레벨 {level}/{max}", self.data.skill_name(skill))
                }
            }
            Event::SkillForgotten { unit, skill } if unit == self.player => {
                format!("스킬을 잊었다: {}", self.data.skill_name(skill))
            }
            Event::Interacted { unit, npc } if unit == self.player => {
                format!("{} 에게 말을 걸었다", self.name(npc))
            }
            Event::Crafted { unit, item, count } if unit == self.player => {
                format!("만들었다: {} ×{count}", self.data.item_name(item))
            }
            Event::Teleported { unit, .. } if unit == self.player => String::from("순간이동했다"),
            Event::PortalBlocked { unit, portal } if unit == self.player => {
                // 던전의 나가는 문이면 무엇이 남았는지 말해 준다 (R6-2).
                let waits_clear = self
                    .world()
                    .portals()
                    .get(usize::from(portal))
                    .is_some_and(|p| mentions_cleared(&p.requires))
                    && !self.world().is_cleared();
                if waits_clear {
                    String::from("아직 나갈 수 없다 — 던전 목표를 먼저 채워야 한다")
                } else {
                    String::from("아직 들어갈 수 없다 — 입장 조건이 맞지 않는다")
                }
            }
            Event::ClearProgressed { objective, count } => {
                let goal = self
                    .world()
                    .clear_def()
                    .and_then(|d| d.objectives.get(usize::from(objective)))
                    .map(|o| self.goal_text(o, count).0)
                    .unwrap_or_default();
                format!("던전 목표 — {goal}")
            }
            Event::DungeonCleared => String::from("던전 클리어! — 보상을 받았다"),
            Event::Traveled { unit, .. } if unit == self.player => {
                String::from("다른 곳으로 떠난다…")
            }
            Event::Deposited { unit, item, count } if unit == self.player => {
                format!("맡겼다: {} ×{count}", self.data.item_name(item))
            }
            Event::QuestAccepted { unit, quest } if unit == self.player => {
                format!("퀘스트를 받았다: {}", self.data.quest_title(quest))
            }
            Event::QuestStageDone { unit, quest, .. } if unit == self.player => {
                format!("{} — 다음 단계로", self.data.quest_title(quest))
            }
            Event::QuestReady { unit, quest } if unit == self.player => {
                format!("{} — 보고할 수 있다", self.data.quest_title(quest))
            }
            Event::QuestCompleted { unit, quest } if unit == self.player => {
                format!("퀘스트 완료: {}", self.data.quest_title(quest))
            }
            Event::QuestAbandoned { unit, quest } if unit == self.player => {
                format!("퀘스트를 포기했다: {}", self.data.quest_title(quest))
            }
            Event::Withdrew { unit, item, count } if unit == self.player => {
                format!("찾았다: {} ×{count}", self.data.item_name(item))
            }
            Event::Bought { unit, item, count } if unit == self.player => {
                format!("샀다: {} ×{count}", self.data.item_name(item))
            }
            Event::Sold { unit, item, count } if unit == self.player => {
                format!("팔았다: {} ×{count}", self.data.item_name(item))
            }
            Event::CurrencyChanged {
                unit,
                currency,
                delta,
                total,
            } if unit == self.player => {
                let name = self.data.currency_name(currency);
                format!("{name} {delta:+} ({name} {total})")
            }
            // 플레이어는 이름 없이 (포션·회복 스킬), 다른 유닛은 이름을 붙인다 (범위 회복).
            Event::Healed {
                unit,
                amount,
                remaining_hp,
            } if unit == self.player => format!("HP +{amount} (HP {remaining_hp})"),
            Event::Healed {
                unit,
                amount,
                remaining_hp,
            } => format!("{} HP +{amount} (HP {remaining_hp})", self.name(unit)),
            Event::ManaRestored {
                amount,
                remaining_mp,
                ..
            } => format!("MP +{amount} (MP {remaining_mp})"),
            Event::EquipmentChanged { unit, slot } => {
                let now = self
                    .world()
                    .unit(unit)
                    .and_then(|u| u.inventory().equipped(slot));
                match now {
                    Some(item) => format!("{} 장착", self.data.item_name(item)),
                    None => format!("{} 해제", text::slot_name(slot)),
                }
            }
            Event::Engaged { unit, target } => {
                format!(
                    "{} 이(가) {} 을(를) 노린다",
                    self.name(unit),
                    self.name(target)
                )
            }
            Event::Evading { unit } => format!("{} 추격 포기", self.name(unit)),
            Event::Blocked { unit } if unit == self.player => String::from("길이 막혔습니다"),
            Event::Rejected { intent, reason } if intent.unit() == self.player => {
                // 쿨타임은 주문이 계속 기다리면 되는 일이다. 그 밖의 거절은 주문을 끝낸다
                // — 그대로 두면 매 tick 같은 거절이 반복된다.
                if reason == Rejection::OnCooldown {
                    return;
                }
                self.order = Order::Idle;
                self.chase_goal = None;
                format!("할 수 없음: {}", text::rejection(reason))
            }
            _ => return,
        };
        self.note(line);
    }

    fn note(&mut self, line: String) {
        if self.log.len() == LOG_LINES {
            self.log.pop_front();
        }
        self.log.push_back(line);
    }

    // ── 패널 ─────────────────────────────────────────────────────────────────

    /// 플레이어 가방 내용 — UI 가 그대로 그린다.
    pub(crate) fn bag_lines(&self, kind: BagKind) -> Vec<BagLine> {
        let Some(me) = self.world().unit(self.player) else {
            return Vec::new();
        };
        me.inventory()
            .bag(kind)
            .iter()
            .filter_map(|(slot, s)| {
                Some(BagLine {
                    slot: u16::try_from(slot).ok()?,
                    name: self.data.item_name(s.item),
                    count: s.count,
                })
            })
            .collect()
    }

    /// HUD 인벤토리 칸 — `(아이템 색, 개수)`, 소모품 가방 다음 장비 가방 (P5).
    ///
    /// HUD 는 글자 대신 색으로 아이템을 보여 준다 — 폰트에 한글이 없기 때문이다.
    pub(crate) fn hud_slots(&self) -> Vec<([f32; 4], u32)> {
        let Some(me) = self.world().unit(self.player) else {
            return Vec::new();
        };
        [BagKind::Consumable, BagKind::Equipment]
            .into_iter()
            .flat_map(|kind| {
                me.inventory()
                    .bag(kind)
                    .iter()
                    .map(|(_, s)| (self.data.item_color(s.item), s.count))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// 소모품 가방 — `(아이템 번호, 개수)`, 칸 순서. UI 스크립트의 `item_count` 재료.
    pub(crate) fn consumable_counts(&self) -> Vec<(u32, u32)> {
        self.world().unit(self.player).map_or_else(Vec::new, |me| {
            me.inventory()
                .bag(BagKind::Consumable)
                .iter()
                .map(|(_, s)| (s.item.0, s.count))
                .collect()
        })
    }

    /// 그 소모품을 하나 쓴다 — 앞쪽 칸부터. 가방에 없으면 `false`.
    pub(crate) fn use_item_by_id(&mut self, item: u32) -> bool {
        let slot = self.world().unit(self.player).and_then(|me| {
            me.inventory()
                .bag(BagKind::Consumable)
                .iter()
                .find(|(_, s)| s.item.0 == item)
                .and_then(|(slot, _)| u16::try_from(slot).ok())
        });
        match slot {
            Some(slot) => {
                self.inventory(InventoryAction::Use(slot));
                true
            }
            None => false,
        }
    }

    /// 장착 자리마다 (자리, 자리 이름, 장착한 아이템 이름).
    pub(crate) fn equipped_lines(&self) -> Vec<(EquipSlot, &'static str, Option<String>)> {
        let me = self.world().unit(self.player);
        EquipSlot::ALL
            .into_iter()
            .map(|slot| {
                let item = me
                    .and_then(|u| u.inventory().equipped(slot))
                    .map(|i| self.data.item_name(i));
                (slot, text::slot_name(slot), item)
            })
            .collect()
    }

    // ── 그리기 ───────────────────────────────────────────────────────────────

    /// 지면 층 — 땅의 아이템, 이동 경로, 공격 대상 표시.
    pub(crate) fn build_ground(
        &self,
        camera: &Camera2d,
        alpha: f32,
        px: f32,
        out: &mut Vec<RenderCommand>,
    ) {
        let world = self.world();

        for (_, g) in world.ground_items() {
            // 사라지기 전 마지막 몇 초는 깜빡인다 — 시뮬레이션 시계 기준이라 fps 와 무관하다.
            if let Some(left) = world.ground_item_time_left(g)
                && left < ITEM_BLINK_BEFORE
                && (left.as_millis() / ITEM_BLINK_MS) % 2 == 1
            {
                continue;
            }
            let size = (10.0 * px).max(0.4);
            let diamond = std::f32::consts::FRAC_PI_4;
            for (grow, bias, color) in [
                (2.0 * px, BIAS_ITEM, BAR_BACK),
                (
                    0.0,
                    BIAS_ITEM + DEPTH_LAYER,
                    self.data.item_color(g.stack.item),
                ),
            ] {
                out.push(RenderCommand::DrawRect {
                    center: g.pos,
                    size: Vec2::splat(size + grow),
                    rotation: diamond,
                    z: 0.0,
                    depth_bias: bias,
                    color,
                    uv: UvRect::FULL,
                    texture: TextureId::WHITE,
                });
            }
        }

        // 포털 (R6) — 들어갈 수 있으면 보라, 입장 조건이 맞지 않으면 회색.
        for p in world.portals() {
            let color = if world.check(&p.requires, self.player) {
                PORTAL_COLOR
            } else {
                PORTAL_LOCKED_COLOR
            };
            crate::push_ring(p.center, p.radius, 3.0 * px, BIAS_PATH, color, out);
            crate::push_ring(
                p.center,
                (p.radius - 4.0 * px).max(0.0),
                px,
                BIAS_PATH,
                color,
                out,
            );
        }

        if let Some(me) = world.unit(self.player) {
            // 남은 경로 — 지금 그려지는 위치에서 시작한다.
            let mut from = me.render_pos(alpha);
            for to in me.waypoints() {
                crate::push_segment(from, to, 2.0 * px, BIAS_PATH, PATH_COLOR, out);
                from = to;
            }
        }

        if let Order::Attack { target, .. } = self.order
            && let Some(t) = world.unit(target)
        {
            let half = Vec2::splat((12.0 * px).max(0.5));
            let at = t.render_pos(alpha);
            crate::grid::build_outline(
                camera,
                at - half,
                at + half,
                2.0,
                BIAS_TARGET,
                TARGET_COLOR,
                out,
            );
        }
    }

    /// 오브젝트 층 — 유닛 스프라이트.
    pub(crate) fn build_objects(
        &self,
        alpha: f32,
        px: f32,
        sprites: &SpriteLibrary,
        out: &mut Vec<RenderCommand>,
    ) {
        let idle_animator = SpriteAnimator::default();
        let now = self.world().now();
        for (unit, u) in self.world().units() {
            // 부활 직후 보호 중이면 깜빡인다 — 스프라이트는 컷아웃이라 반투명 대신 껐다 켠다.
            if u.is_protected(now)
                && (u.protection_left(now).as_millis() / PROTECT_BLINK_MS) % 2 == 1
            {
                continue;
            }
            let actor = self.actor_of(unit);
            // 시체는 어떤 시트든 회색 — 명시한 색으로 넘겨 컬러 아트에도 곱해지게 한다.
            let (tint, fallback) = match self.labels.get(&unit) {
                _ if !u.is_alive() => (CORPSE_TINT, CORPSE_TINT),
                _ if self.hit_flash.contains_key(&unit) => (HIT_TINT, HIT_TINT),
                Some(l) => (l.tint, l.fallback),
                None => (NO_TINT, NO_TINT),
            };
            let look = Look {
                heading: u.heading(),
                tint,
                fallback,
            };
            let animator = self.animators.get(&unit).unwrap_or(&idle_animator);
            sprites
                .sheet(actor)
                .push(u.render_pos(alpha), look, animator, px, BIAS_SPRITE, out);
        }
    }

    /// 이 유닛의 액터 타입. 모르면 기본값 — 내장 시트로 그려진다.
    fn actor_of(&self, unit: Entity) -> ActorId {
        self.labels.get(&unit).map_or(ActorId::DEFAULT, |l| l.actor)
    }

    /// 표시 층 — 살아 있는 유닛 머리 위 HP 막대.
    ///
    /// 막대는 **빌보드**(흰 텍스처 스프라이트)다 — 화면을 향해 서므로 쿼터뷰에서도 눌리지 않는다.
    /// 막대가 뜨는 높이는 **유닛이 쓰는 시트**의 스프라이트 높이다 — 종류마다 칸 크기가 다르다.
    pub(crate) fn build_overlay(
        &self,
        alpha: f32,
        px: f32,
        sprites: Option<&SpriteLibrary>,
        out: &mut Vec<RenderCommand>,
    ) {
        const WIDTH_PX: f32 = 30.0;
        const HEIGHT_PX: f32 = 4.0;
        const GAP_PX: f32 = 6.0;
        /// 시트가 없을 때(로드 실패) 쓰는 머리 높이 (m).
        const NO_SHEET_HEIGHT: f32 = 1.5;

        for (unit, u) in self.world().units().filter(|(_, u)| u.is_alive()) {
            let height =
                sprites.map_or(NO_SHEET_HEIGHT, |s| s.sheet(self.actor_of(unit)).height(px));
            let above = height + GAP_PX * px;
            let at = u.render_pos(alpha);
            let ratio = u.hp() as f32 / u.max_hp() as f32;
            let full = WIDTH_PX * px;
            let color = if ratio <= 0.3 { BAR_HP_LOW } else { BAR_HP };

            push_bar(
                Vec3::new(at.x, at.y, above),
                full + 2.0 * px,
                (HEIGHT_PX + 2.0) * px,
                0.0,
                BAR_BACK,
                out,
            );
            // 채움은 왼쪽 정렬 — 가운데에서 모자란 만큼 왼쪽으로 민다 (카메라 right 가 +X).
            let fill = full * ratio;
            let shift = (full - fill) * 0.5;
            push_bar(
                Vec3::new(at.x - shift, at.y, above),
                fill,
                HEIGHT_PX * px,
                DEPTH_LAYER,
                color,
                out,
            );
        }
    }
}

/// 흰 텍스처 빌보드 하나 — 막대의 한 층.
fn push_bar(
    center: Vec3,
    width: f32,
    height: f32,
    bias: f32,
    color: [f32; 4],
    out: &mut Vec<RenderCommand>,
) {
    if width <= 0.0 {
        return;
    }
    out.push(RenderCommand::DrawSprite {
        pos: center,
        size: Vec2::new(width, height),
        anchor: SpriteAnchor::Center,
        depth_bias: BIAS_BAR + bias,
        uv: UvRect::FULL,
        texture: TextureId::WHITE,
        tint: color,
    });
}

/// 인벤토리 패널이 보여 줄 한 줄.
#[derive(Clone, Debug)]
pub(crate) struct BagLine {
    pub(crate) slot: u16,
    pub(crate) name: String,
    pub(crate) count: u32,
}

/// 화면 문구 — 데이터가 아니라 UI 의 일부라 코드에 둔다.
/// 한 tick 동안 유닛에게 일어난 일 중 **동작을 바꾸는 것** — 전투 이벤트에서 모은다.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct AnimCue {
    /// 공격이 맞았다 (때린 쪽).
    attacked: bool,
    /// 공격에 맞았다.
    hit: bool,
    died: bool,
}

impl AnimCue {
    fn collect(events: &[Event]) -> HashMap<Entity, Self> {
        let mut cues: HashMap<Entity, Self> = HashMap::new();
        for event in events {
            match *event {
                Event::Damaged {
                    attacker, target, ..
                } => {
                    cues.entry(attacker).or_default().attacked = true;
                    cues.entry(target).or_default().hit = true;
                }
                Event::Died { unit, .. } => cues.entry(unit).or_default().died = true,
                _ => {}
            }
        }
        cues
    }
}

/// 동작을 고르는 데 필요한 유닛 상태.
#[derive(Clone, Copy, Debug)]
struct UnitPose {
    alive: bool,
    moving: bool,
}

/// 유닛 하나의 재생기를 한 tick 진행한다.
///
/// 우선순위: **사망 > 공격 > 피격 > 걷기·대기.** 공격·피격·사망은 한 번짜리 동작이라 끝날 때까지
/// 걷기·대기로 덮지 않는다. 다만 **시트에 그 클립이 있을 때만** 들어간다 — 없는 클립은 대기로
/// 대체되는데(`clip_or_fallback`), 대기는 루프라 끝나지 않아 유닛이 그 자리에 얼어붙는다.
/// 피격 그림이 없는 시트는 대신 붉게 깜빡인다 ([`HIT_TINT`]).
///
/// 시트가 없으면(`None` — 시험이나 로드 실패) 상태만 바꾸고 진행하지 않는다.
fn next_anim(
    animator: &mut SpriteAnimator,
    sheet: Option<&SpriteSheet>,
    pose: UnitPose,
    cue: AnimCue,
    dt: Duration,
) {
    let has = |state| sheet.is_some_and(|s| s.clip(state).is_some());
    if !pose.alive {
        if cue.died && has(AnimState::Die) {
            animator.play(AnimState::Die);
        }
        // 사망 동작이 없으면 시체는 쓰러진 순간의 프레임에 멈춘다.
        if animator.state() != AnimState::Die {
            return;
        }
    } else {
        if cue.attacked && has(AnimState::Attack) {
            // 연속 공격은 처음부터 다시 — 같은 상태여도 리셋한다.
            animator.play(AnimState::Attack);
        } else if cue.hit && has(AnimState::Hit) && animator.state() != AnimState::Attack {
            // 휘두르는 중에 맞아도 공격 동작은 끊지 않는다.
            animator.play(AnimState::Hit);
        }
        let busy =
            matches!(animator.state(), AnimState::Attack | AnimState::Hit) && !animator.finished();
        if !busy {
            animator.set_state(if pose.moving {
                AnimState::Walk
            } else {
                AnimState::Idle
            });
        }
    }
    if let Some(sheet) = sheet {
        animator.advance(sheet, dt);
    }
}

mod text {
    use nexus_sim::{EquipSlot, Rejection};

    pub(super) fn slot_name(slot: EquipSlot) -> &'static str {
        match slot {
            EquipSlot::Weapon => "무기",
            EquipSlot::Head => "머리",
            EquipSlot::Body => "몸",
            EquipSlot::Hand => "손",
            EquipSlot::Shoes => "신발",
        }
    }

    pub(super) fn rejection(reason: Rejection) -> &'static str {
        match reason {
            Rejection::UnknownEntity | Rejection::UnknownItem => "대상이 없습니다",
            Rejection::NoPath => "갈 수 없는 곳입니다",
            Rejection::Immobile => "움직일 수 없습니다",
            Rejection::Dead => "쓰러진 상태입니다",
            Rejection::TargetDead => "대상이 이미 쓰러졌습니다",
            Rejection::UnknownSkill => "모르는 스킬입니다",
            Rejection::OnCooldown => "아직 쓸 수 없습니다",
            Rejection::NotEnoughMp => "MP 가 부족합니다",
            Rejection::OutOfRange => "너무 멉니다",
            Rejection::InvalidTarget => "그 대상은 고를 수 없습니다",
            Rejection::Invulnerable => "공격할 수 없는 대상입니다",
            Rejection::Friendly => "우호 대상입니다",
            Rejection::EmptySlot => "빈 칸입니다",
            Rejection::InventoryFull => "가방이 가득 찼습니다",
            Rejection::NotUsable => "쓸 수 없는 아이템입니다",
            Rejection::NotDead => "쓰러진 상태가 아닙니다",
            Rejection::ClassRestricted => "이 직업은 쓸 수 없는 장비입니다",
            Rejection::SkillNotLearned => "배우지 않은 스킬입니다",
            Rejection::SkillMaxed => "이미 최대 레벨인 스킬입니다",
            Rejection::SkillRequirement => "배울 수 없는 스킬입니다 (직업·조건)",
            Rejection::NoService => "그런 일은 하지 않는 NPC 입니다",
            Rejection::NotEnoughCurrency => "돈이 모자랍니다",
            Rejection::NotSellable => "그 상점은 이 물건을 사지 않습니다",
            Rejection::MissingMaterials => "재료가 모자랍니다",
            Rejection::RequirementNotMet => "조건이 맞지 않습니다",
            Rejection::QuestUnavailable => "받을 수 없는 퀘스트입니다",
            Rejection::QuestNotReady => "아직 보고할 수 없습니다",
        }
    }
}

/// 저장 데이터를 스폰한 플레이어에 얹는다 (P3).
///
/// **수치는 되살리지 않는다** — HP·MP 상한·공격력은 `rules.ron` 의 지금 값에서 다시 계산된다.
/// 데이터에서 사라진 아이템은 조용히 버린다 (규칙이 바뀌었다고 이어 하기가 막히면 곤란하다).
fn restore(world: &mut SimWorld, player: Entity, save: &SaveData) {
    world.set_progress(player, Progress::new(save.level, save.exp));
    for &(slot, item, count) in &save.bags {
        world.place_item(player, slot, ItemStack { item, count });
    }
    for &item in &save.equipped {
        world.force_equip(player, item);
    }
    // 위치는 같은 존일 때만 남아 있다 — 다른 존이면 스폰 지점 그대로다.
    if let Some(pos) = save.pos {
        world.set_position(player, pos);
    }
    world.set_hp(player, save.hp);
    if let Some(mp) = save.mp {
        world.set_mp(player, mp);
    }
    for &(flag, value) in &save.flags {
        world.set_unit_flag(player, flag, value);
    }
    for &(flag, value) in &save.world_flags {
        world.set_world_flag(flag, value);
    }
    // 퀘스트 기록 (R5).
    for (quest, active, completed) in &save.quests {
        world.set_quest(player, *quest, active.clone(), *completed);
    }
    // 창고 (R4-2) — 칸 번호 그대로.
    for &(slot, item, count) in &save.storage {
        world.place_in_storage(player, slot, ItemStack { item, count });
    }
    // 지갑 (R4) — 재화가 생기기 전의 저장이면 시작 재화를 그대로 둔다.
    if let Some(wallet) = &save.wallet {
        for &(currency, amount) in wallet {
            world.set_currency(player, currency, amount);
        }
    }
}
#[cfg(test)]
mod tests {
    use nexus_sim::ItemId;

    use super::*;

    const DT: Duration = Duration::from_millis(50);

    fn session() -> PlaySession {
        PlaySession::start(
            &Scene::server_default(),
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions::default(),
        )
        .unwrap()
    }

    fn find(s: &PlaySession, name: &str) -> Entity {
        s.labels
            .iter()
            .find(|(_, l)| l.name == name)
            .map(|(e, _)| *e)
            .unwrap()
    }

    #[test]
    fn scene_becomes_one_player_and_all_npcs_and_monsters() {
        let s = session();
        // 기본 씬: 플레이어 스폰 2 · NPC 2 · 몬스터 1 → 플레이어 1명 + 3
        assert_eq!(s.world().unit_count(), 4);
        assert_eq!(s.name(s.player()), "플레이어");
        let me = s.world().unit(s.player()).unwrap();
        assert_eq!(me.pos(), Vec2::ZERO, "첫 플레이어 스폰에 선다");
        assert_eq!(
            me.inventory()
                .bag(BagKind::Consumable)
                .count_of(ItemId(501)),
            3
        );
    }

    #[test]
    fn starting_does_not_touch_the_scene() {
        let scene = Scene::server_default();
        let before = scene.items.clone();
        let mut s = PlaySession::start(
            &scene,
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions::default(),
        )
        .unwrap();
        s.click(Vec2::new(5.0, 0.0), 0.01);
        for _ in 0..40 {
            s.tick(DT, None);
        }
        assert_eq!(scene.items, before);
    }

    #[test]
    fn a_scripted_goblin_spots_the_player_and_charges() {
        // P2: 행동은 전부 data/scripts/goblin.rhai 가 정한다 (AI 는 Passive).
        let mut scene = Scene::server_default();
        let slime = scene.items.iter_mut().find(|i| i.name == "슬라임").unwrap();
        slime.actor = ActorId::new(102);
        slime.pos = Vec2::new(6.0, 0.0);
        let mut s = PlaySession::start(
            &scene,
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions::default(),
        )
        .unwrap();
        let goblin = find(&s, "슬라임");
        let full = s.world().unit(s.player()).unwrap().hp();

        for _ in 0..80 {
            s.tick(DT, None);
        }
        assert!(
            s.log().any(|l| l == "scripts/goblin.rhai: 적 발견 — 돌진!"),
            "{:?}",
            s.log
        );
        let me = s.world().unit(s.player()).unwrap();
        assert!(me.hp() < full, "고블린이 물었어야 한다");
        assert!(s.world().unit(goblin).unwrap().pos().distance(me.pos()) <= 1.5);
        assert!(s.take_alerts().is_empty());
    }

    // ── 이어 하기 (P3) ──────────────────────────────────────────────────────

    #[test]
    fn the_chosen_player_spawn_is_the_one_that_walks() {
        // P3-1: 여러 플레이어 스폰 중 고른 마커에서 시작한다.
        let scene = Scene::server_default();
        let spawns: Vec<_> = scene
            .items
            .iter()
            .filter(|i| i.kind == ItemKind::PlayerSpawn)
            .collect();
        assert!(spawns.len() >= 2, "시험 전제: 플레이어 스폰이 둘 이상");
        let (second, at) = (spawns[1].entity, spawns[1].pos);

        let s = PlaySession::start(
            &scene,
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions {
                spawn: Some(second),
                save: None,
                class: None,
                links: Default::default(),
            },
        )
        .unwrap();
        assert_eq!(s.world().unit(s.player()).unwrap().pos(), at);
        // 스폰 지점은 여전히 하나만 유닛이 된다.
        assert_eq!(s.world().unit_count(), 4);
    }

    #[test]
    fn a_saved_character_comes_back_with_level_items_and_place() {
        let mut s = session();
        // 슬라임을 잡아 경험치를 얻고, 떨어진 것을 줍는다.
        let slime = find(&s, "슬라임");
        s.click(s.world().unit(slime).unwrap().pos(), 0.01);
        for _ in 0..900 {
            s.tick(DT, None);
            // 쓰러지면 멈춘다 — 더 돌리면 리스폰해서(10초) 옛 핸들이 사라진다.
            if !s.world().unit(slime).unwrap().is_alive() {
                break;
            }
        }
        assert!(!s.world().unit(slime).unwrap().is_alive(), "{:?}", s.log);
        let before = s.world().unit(s.player()).unwrap().progress();
        assert!(
            before.level() >= 1 && before.exp() > 0,
            "경험치를 받았어야 한다"
        );

        let save = s.save_data(String::from("zones/sample.zone.ron"));
        assert_eq!((save.level, save.exp), (before.level(), before.exp()));
        assert!(!save.bags.is_empty(), "시작 소지품이 저장에 담긴다");

        // 같은 존에서 이어 하기 — 레벨·소지품·위치가 그대로다.
        let again = PlaySession::start(
            &Scene::server_default(),
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions {
                spawn: None,
                save: Some(save.clone()),
                class: None,
                links: Default::default(),
            },
        )
        .unwrap();
        let me = again.world().unit(again.player()).unwrap();
        assert_eq!(me.progress(), before);
        assert_eq!(me.pos(), save.pos.unwrap(), "저장한 자리에서 이어 한다");
        assert_eq!(me.hp(), save.hp);
        assert_eq!(
            me.inventory()
                .bag(BagKind::Consumable)
                .count_of(ItemId(501)),
            save.bags
                .iter()
                .filter(|(_, item, _)| *item == ItemId(501))
                .map(|(_, _, n)| n)
                .sum::<u32>()
        );
    }

    // ── 부활 ─────────────────────────────────────────────────────────────────

    fn start_with(data: GameData) -> PlaySession {
        PlaySession::start(
            &Scene::server_default(),
            Camera2d::default(),
            data,
            PlayOptions::default(),
        )
        .unwrap()
    }

    /// 슬라임을 플레이어 옆으로 옮기고 HP 를 1 로 — 한 번 물리면 쓰러진다. 쓰러질 때까지 돌린다.
    fn knock_out(s: &mut PlaySession) {
        let me = s.player();
        let slime = find(s, "슬라임");
        let at = s.world().unit(me).unwrap().pos() + Vec2::new(1.0, 0.0);
        s.auth.world_mut().set_position(slime, at);
        s.auth.world_mut().set_hp(me, 1);
        for _ in 0..200 {
            s.tick(DT, None);
            if !s.world().unit(me).unwrap().is_alive() {
                return;
            }
        }
        panic!("쓰러지지 않았다: {:?}", s.log);
    }

    #[test]
    fn the_player_gets_up_at_the_save_point_after_the_default_delay() {
        // rules.ron: revive (delay_ms: 3000, hp_percent: 100, exp_loss_percent: 0).
        let mut s = session();
        let me = s.player();
        let save_point = s.world().unit(me).unwrap().spawn_pos();
        knock_out(&mut s);
        assert_eq!(
            last_log(&s),
            "플레이어가 쓰러졌습니다 — 3.0초 뒤 저장 지점에서 일어납니다"
        );
        // 쓰러져 있는 동안 화면 값 — 어둡게 덮기와 남은 초 (`{revive}`).
        let v = crate::screen::Values::from_play(&s, false).unwrap();
        assert!(v.down);
        assert!(
            v.revive_in.is_some_and(|t| (2.9..=3.0).contains(&t)),
            "{v:?}"
        );
        for _ in 0..55 {
            s.tick(DT, None);
        }
        assert!(
            !s.world().unit(me).unwrap().is_alive(),
            "3초 전에는 쓰러져 있다"
        );
        for _ in 0..10 {
            s.tick(DT, None);
        }
        let u = s.world().unit(me).expect("같은 핸들 — 리스폰이 아니다");
        assert!(u.is_alive());
        assert_eq!(u.pos(), save_point);
        assert!(
            s.log.iter().any(|l| l.starts_with("플레이어 부활 — HP")),
            "{:?}",
            s.log
        );
        // 소지품은 그대로 — 새 유닛이 아니다.
        assert!(!s.bag_lines(BagKind::Consumable).is_empty());
        // 일어난 뒤 3초 보호 (protect_ms) — 화면 값도 "쓰러짐" 이 풀린다.
        assert!(u.is_protected(s.world().now()));
        assert!(s.log.iter().any(|l| l.ends_with("초 보호")), "{:?}", s.log);
        let v = crate::screen::Values::from_play(&s, false).unwrap();
        assert!(!v.down && v.revive_in.is_none());
    }

    #[test]
    fn the_default_revive_takes_its_numbers_from_the_rules() {
        let rules = GameData::embedded_rules().replacen(
            "revive: (delay_ms: 3000, hp_percent: 100, exp_loss_percent: 0, protect_ms: 3000)",
            "revive: (delay_ms: 500, hp_percent: 25, exp_loss_percent: 50)",
            1,
        );
        assert_ne!(rules, GameData::embedded_rules(), "시험 전제");
        let mut s = start_with(GameData::parse_with_scripts(&rules, &[]).unwrap());
        let me = s.player();
        s.auth.world_mut().set_progress(me, Progress::new(1, 80));
        knock_out(&mut s);
        for _ in 0..15 {
            s.tick(DT, None);
        }
        let u = s.world().unit(me).unwrap();
        assert!(u.is_alive(), "0.5초 뒤");
        assert_eq!(u.progress().exp(), 40, "경험치 절반");
        assert!(
            s.log.iter().any(|l| l.contains("경험치 -40")),
            "{:?}",
            s.log
        );
    }

    #[test]
    fn an_on_dead_script_replaces_the_default_revive() {
        // 플레이어 액터에 스크립트를 붙인다 — 1초 뒤 쓰러진 그 자리에서 HP 30% 로.
        const HERO: &str = r#"
            fn on_death(me, killer) { this.down = 0.0; }
            fn on_dead(me, dt) {
                this.down += dt;
                if this.down >= 1.0 { revive(me, me.x, me.y, 30); }
            }
        "#;
        let rules = GameData::embedded_rules().replacen(
            "            skills: [5, 6, 7, 8],\n",
            "            skills: [5, 6, 7, 8],\n            script: Some(\"scripts/hero.rhai\"),\n",
            1,
        );
        assert_ne!(rules, GameData::embedded_rules(), "시험 전제");
        let data = GameData::parse_with_scripts(&rules, &[("scripts/hero.rhai", HERO)]).unwrap();
        let mut s = start_with(data);
        let me = s.player();
        assert!(s.scripted_revive);
        knock_out(&mut s);
        let fell_at = s.world().unit(me).unwrap().pos();
        assert!(
            last_log(&s).contains("액터 스크립트(on_dead)"),
            "{}",
            last_log(&s)
        );
        assert_eq!(s.revive_at, None, "기본 부활은 예약하지 않는다");
        for _ in 0..30 {
            s.tick(DT, None);
        }
        let u = s.world().unit(me).unwrap();
        assert!(u.is_alive(), "{:?}", s.log);
        assert_eq!(u.pos(), fell_at, "그 자리에서");
        assert!(
            u.hp() <= u.max_hp() * 3 / 10,
            "HP 30% (그 뒤 맞았을 수도 있다)"
        );
    }

    // ── 리스폰 ───────────────────────────────────────────────────────────────

    #[test]
    fn a_slain_slime_respawns_at_its_marker_with_its_name() {
        // rules.ron: 슬라임 respawn_ms 10000.
        let mut s = session();
        let slime = find(&s, "슬라임");
        let spawn = s.world().unit(slime).unwrap().pos();
        s.click(spawn, 0.01);
        for _ in 0..900 {
            s.tick(DT, None);
            if !s.world().unit(slime).unwrap().is_alive() {
                break;
            }
        }
        assert!(!s.world().unit(slime).unwrap().is_alive(), "{:?}", s.log);
        // 플레이어를 멀리 치워 둔다 — 되살아난 슬라임이 곧바로 덤벼 죽지 않게.
        let me = s.player();
        s.auth.world_mut().set_position(me, Vec2::new(-30.0, 30.0));

        // 쓰러진 tick 의 시작 시각부터 10초 = 그 뒤 199 tick 째.
        for _ in 0..198 {
            s.tick(DT, None);
        }
        assert!(s.world().unit(slime).is_some(), "10초 전에는 시체로 남는다");
        s.tick(DT, None);
        assert!(s.world().unit(slime).is_none(), "시체는 치워졌다");
        let again = find(&s, "슬라임");
        let u = s.world().unit(again).unwrap();
        assert!(u.is_alive() && u.hp() == u.max_hp());
        assert_eq!(u.pos(), spawn, "마커 자리");
        assert!(
            s.log.iter().any(|l| l == "슬라임 다시 나타남"),
            "{:?}",
            s.log
        );
    }

    // ── 단축키 스킬 ──────────────────────────────────────────────────────────

    const SMASH: SkillId = SkillId(5);

    fn last_log(s: &PlaySession) -> String {
        s.log.back().cloned().unwrap_or_default()
    }

    #[test]
    fn the_hotbar_comes_from_the_controlled_actor() {
        let s = session();
        assert_eq!(
            s.hotbar[..4],
            [
                Some(SkillId(5)),
                Some(SkillId(6)),
                Some(SkillId(7)),
                Some(SkillId(8))
            ],
            "rules.ron: classes.1.skills (시작 스킬이 처음 단축키에 차례로)"
        );
        assert_eq!(s.hotbar.len(), HOTBAR_SLOTS);
        let slots = s.skill_slots();
        assert_eq!(slots.len(), 4);
        assert!(slots.iter().all(|x| x.usable && x.cooldown == 0.0));
    }

    #[test]
    fn a_heal_skill_key_mends_the_player_and_keeps_the_current_order() {
        // rules.ron: 단축키 4 = 치유 (#8, 회복 60, MP 15).
        let mut s = session();
        let me = s.player();
        s.auth.world_mut().set_hp(me, 100);
        let slime = find(&s, "슬라임");
        s.order = Order::Attack {
            target: slime,
            skill: None,
        };
        s.use_skill(4);
        assert_eq!(last_log(&s), "치유 → 자신");
        assert!(
            matches!(s.order, Order::Attack { target, skill: None } if target == slime),
            "하던 주문은 그대로"
        );
        s.tick(DT, None);
        let u = s.world().unit(me).unwrap();
        assert_eq!((u.hp(), u.mp()), (160, 45));
        assert!(s.log.iter().any(|l| l == "HP +60 (HP 160)"), "{:?}", s.log);
    }

    #[test]
    fn a_skill_key_casts_once_on_the_target_then_basic_attacks_resume() {
        let mut s = session();
        let slime = find(&s, "슬라임");
        s.click(s.world().unit(slime).unwrap().pos(), 0.01);
        s.use_skill(1);
        assert_eq!(
            s.order,
            Order::Attack {
                target: slime,
                skill: Some(SMASH)
            }
        );
        let mut cast = false;
        for _ in 0..400 {
            s.tick(DT, None);
            if s.log.iter().any(|l| l.contains("강베기 슬라임")) {
                cast = true;
                break;
            }
        }
        assert!(cast, "{:?}", s.log);
        assert_eq!(
            s.order,
            Order::Attack {
                target: slime,
                skill: None
            },
            "한 번 맞으면 기본 공격으로"
        );
        let me = s.world().unit(s.player()).unwrap();
        assert!(me.mp() < 60, "MP 15 를 썼다 (지금 {})", me.mp());
        assert!(s.skill_slots()[0].cooldown > 0.0, "칸에 쿨타임이 보인다");

        // 쿨타임 중에 다시 누르면 이유를 알리고 주문은 그대로.
        s.use_skill(1);
        assert!(
            last_log(&s).contains("아직 쓸 수 없습니다"),
            "{}",
            last_log(&s)
        );
        assert_eq!(s.attack_target(), Some(slime));
    }

    #[test]
    fn a_skill_key_without_a_target_picks_the_nearest_enemy() {
        let mut s = session();
        let slime = find(&s, "슬라임");
        let near = s.world().unit(slime).unwrap().pos() + Vec2::new(3.0, 0.0);
        let me = s.player();
        s.auth.world_mut().set_position(me, near);
        s.use_skill(2);
        assert_eq!(s.attack_target(), Some(slime), "{:?}", s.log);
        assert_eq!(last_log(&s), "돌 던지기 → 슬라임");

        // 아무도 없으면 알린다.
        let mut far = session();
        let me = far.player();
        far.auth
            .world_mut()
            .set_position(me, Vec2::new(-30.0, 30.0));
        far.use_skill(2);
        assert!(
            last_log(&far).contains("주변에 적이 없습니다"),
            "{:?}",
            far.log
        );
    }

    #[test]
    fn short_mp_and_empty_keys_are_reported_at_once() {
        let mut s = session();
        let me = s.player();
        s.auth.world_mut().set_mp(me, 3);
        let slime = find(&s, "슬라임");
        s.click(s.world().unit(slime).unwrap().pos(), 0.01);
        s.use_skill(1);
        assert_eq!(last_log(&s), "강베기: MP 가 부족합니다 (3/15)");
        assert!(!s.skill_slots()[0].usable, "칸이 흐려진다");
        assert_eq!(
            s.order,
            Order::Attack {
                target: slime,
                skill: None
            },
            "하던 주문은 그대로"
        );
        s.use_skill(7);
        assert_eq!(last_log(&s), "단축키 7 에 스킬이 없습니다");
    }

    // ── 전투 동작 ────────────────────────────────────────────────────────────

    /// 공격 그림은 있고 사망 그림은 없는 시트 (내장 그림의 칸을 빌려 쓴다 — 재생 규칙만 본다).
    const PLAYER_SHEET: &str = "(version: 1, cell: (32, 48), directions: 4, clips: [
        (state: Idle, row: 0, frames: 4, frame_ms: 250, looping: true),
        (state: Walk, row: 4, frames: 4, frame_ms: 150, looping: true),
        (state: Attack, row: 0, frames: 4, frame_ms: 90, looping: false),
    ])";
    /// 사망 그림만 있고 공격·피격 그림은 없는 시트.
    const SLIME_SHEET: &str = "(version: 1, cell: (32, 48), directions: 4, clips: [
        (state: Idle, row: 0, frames: 4, frame_ms: 250, looping: true),
        (state: Walk, row: 4, frames: 4, frame_ms: 150, looping: true),
        (state: Die, row: 0, frames: 3, frame_ms: 150, looping: false),
    ])";
    const STANDING: UnitPose = UnitPose {
        alive: true,
        moving: false,
    };
    const WALKING: UnitPose = UnitPose {
        alive: true,
        moving: true,
    };
    const DEAD: UnitPose = UnitPose {
        alive: false,
        moving: false,
    };
    const NONE: AnimCue = AnimCue {
        attacked: false,
        hit: false,
        died: false,
    };
    const ATTACKED: AnimCue = AnimCue {
        attacked: true,
        ..NONE
    };

    #[test]
    fn an_attack_plays_once_then_walking_resumes() {
        // 플레이어 시트의 공격 = 4프레임 × 90ms = 360ms → 8 tick 째에 끝난다.
        let sheet = crate::sprites::sheet_for_test(PLAYER_SHEET);
        let mut a = SpriteAnimator::default();
        next_anim(&mut a, Some(&sheet), WALKING, ATTACKED, DT);
        assert_eq!(a.state(), AnimState::Attack, "걷는 중이어도 휘두른다");
        for _ in 0..6 {
            next_anim(&mut a, Some(&sheet), WALKING, NONE, DT);
            assert_eq!(
                a.state(),
                AnimState::Attack,
                "끝날 때까지 걷기로 덮지 않는다"
            );
        }
        assert_eq!(a.frame(), 3, "마지막 프레임까지 보여 준다");
        next_anim(&mut a, Some(&sheet), WALKING, NONE, DT); // 400ms — 끝
        next_anim(&mut a, Some(&sheet), WALKING, NONE, DT);
        assert_eq!(a.state(), AnimState::Walk);
    }

    #[test]
    fn a_second_hit_restarts_the_swing() {
        let sheet = crate::sprites::sheet_for_test(PLAYER_SHEET);
        let mut a = SpriteAnimator::default();
        next_anim(&mut a, Some(&sheet), STANDING, ATTACKED, DT);
        for _ in 0..4 {
            next_anim(&mut a, Some(&sheet), STANDING, NONE, DT);
        }
        assert!(a.frame() > 0);
        next_anim(&mut a, Some(&sheet), STANDING, ATTACKED, DT);
        assert_eq!((a.state(), a.frame()), (AnimState::Attack, 0));
    }

    #[test]
    fn a_missing_clip_is_never_entered() {
        // 슬라임 시트에는 공격·피격 그림이 없다 — 들어가면 대기(루프)로 대체되어 끝나지 않는다.
        let sheet = crate::sprites::sheet_for_test(SLIME_SHEET);
        let mut a = SpriteAnimator::default();
        let both = AnimCue {
            attacked: true,
            hit: true,
            died: false,
        };
        next_anim(&mut a, Some(&sheet), WALKING, both, DT);
        assert_eq!(a.state(), AnimState::Walk);
    }

    #[test]
    fn death_plays_its_clip_and_stays_on_the_last_frame() {
        let sheet = crate::sprites::sheet_for_test(SLIME_SHEET);
        let mut a = SpriteAnimator::default();
        let died = AnimCue { died: true, ..NONE };
        next_anim(&mut a, Some(&sheet), DEAD, died, DT);
        assert_eq!(a.state(), AnimState::Die);
        for _ in 0..40 {
            next_anim(&mut a, Some(&sheet), DEAD, NONE, DT);
        }
        assert_eq!(
            (a.state(), a.frame(), a.finished()),
            (AnimState::Die, 2, true)
        );

        // 사망 그림이 없는 시트는 쓰러진 순간의 프레임에 멈춘다 — 대기로 돌아가지 않는다.
        let player = crate::sprites::sheet_for_test(PLAYER_SHEET);
        let mut b = SpriteAnimator::default();
        next_anim(&mut b, Some(&player), WALKING, NONE, DT * 3);
        let frozen = (b.state(), b.frame());
        for _ in 0..10 {
            next_anim(&mut b, Some(&player), DEAD, died, DT);
        }
        assert_eq!((b.state(), b.frame()), frozen);
    }

    #[test]
    fn hits_flash_for_a_few_ticks_in_play() {
        // 슬라임을 끝까지 잡는 판 — 맞은 쪽이 깜빡이고, 깜빡임은 tick 으로 끝난다.
        let mut s = session();
        let slime = find(&s, "슬라임");
        s.click(s.world().unit(slime).unwrap().pos(), 0.01);
        let mut flashed = false;
        for _ in 0..900 {
            s.tick(DT, None);
            flashed |= s.hit_flash.contains_key(&slime);
            if !s.world().unit(slime).unwrap().is_alive() {
                break;
            }
        }
        assert!(flashed, "맞은 슬라임이 깜빡였어야 한다");
        assert!(!s.world().unit(slime).unwrap().is_alive(), "{:?}", s.log);
        for _ in 0..HIT_FLASH_TICKS {
            s.tick(DT, None);
        }
        assert!(
            !s.hit_flash.contains_key(&slime),
            "더 맞지 않으면 깜빡임은 tick 으로 끝난다"
        );
    }

    #[test]
    fn the_player_has_mp_that_a_blue_potion_restores_and_a_save_keeps() {
        const BLUE_POTION: ItemId = ItemId(502);
        let mut s = session();
        let me = s.player();
        let u = s.world().unit(me).unwrap();
        assert_eq!((u.mp(), u.max_mp()), (60, 60), "rules.ron: max_mp 60");
        let slot = u
            .inventory()
            .bag(BagKind::Consumable)
            .iter()
            .find(|(_, st)| st.item == BLUE_POTION)
            .map(|(slot, _)| u16::try_from(slot).unwrap())
            .expect("시작 소지품에 파란 포션");

        s.auth.world_mut().set_mp(me, 5);
        s.inventory(InventoryAction::Use(slot));
        s.tick(DT, None);
        // 5 + 40. 초당 2 회복은 50ms 로는 아직 1 이 되지 않는다.
        assert_eq!(s.world().unit(me).unwrap().mp(), 45);
        assert!(s.log.iter().any(|l| l == "MP +40 (MP 45)"), "{:?}", s.log);

        let save = s.save_data(String::new());
        assert_eq!(save.mp, Some(45));
        let s = PlaySession::start(
            &Scene::server_default(),
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions {
                spawn: None,
                save: Some(save),
                class: None,
                links: Default::default(),
            },
        )
        .unwrap();
        assert_eq!(s.world().unit(s.player()).unwrap().mp(), 45, "이어 하기");
    }

    #[test]
    fn continuing_in_another_zone_keeps_the_character_but_not_the_place() {
        let mut save = session().save_data(String::from("zones/other.zone.ron"));
        save.level = 4;
        save.hp = 7;
        save.pos = None; // 편집기가 다른 존이라 지운 상태
        let s = PlaySession::start(
            &Scene::server_default(),
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions {
                spawn: None,
                save: Some(save),
                class: None,
                links: Default::default(),
            },
        )
        .unwrap();
        let me = s.world().unit(s.player()).unwrap();
        assert_eq!(me.progress().level(), 4);
        assert_eq!(me.hp(), 7);
        assert_eq!(me.pos(), Vec2::ZERO, "스폰 지점에서 시작한다");
        // 레벨 4 = 성장치 ×3 (rules.ron: HP +40/레벨).
        assert_eq!(me.max_hp(), 200 + 40 * 3);
    }

    #[test]
    fn the_slime_hunt_trigger_rewards_the_fifth_kill_and_the_flags_are_saved() {
        // rules.ron 트리거 1·2 (R1): 플래그 1 = 슬라임 처치 수, 다섯 마리째에 포션 3개·플래그 2.
        use nexus_sim::FlagId;
        const RED_POTION: ItemId = ItemId(501);
        let mut save = session().save_data(String::new());
        save.flags = vec![(FlagId(1), 4)];
        save.world_flags = vec![(FlagId(40), 9)];
        let mut s = PlaySession::start(
            &Scene::server_default(),
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions {
                spawn: None,
                save: Some(save),
                class: None,
                links: Default::default(),
            },
        )
        .unwrap();
        let me = s.player();
        let potions = |s: &PlaySession| {
            let u = s.world().unit(me).unwrap();
            u.inventory().bag(BagKind::Consumable).count_of(RED_POTION)
        };
        let before = potions(&s);
        let slime = find(&s, "슬라임");
        s.click(s.world().unit(slime).unwrap().pos(), 0.01);
        for _ in 0..900 {
            s.tick(DT, None);
            if !s.world().unit(slime).unwrap().is_alive() {
                break;
            }
        }
        assert!(!s.world().unit(slime).unwrap().is_alive(), "{:?}", s.log);
        let u = s.world().unit(me).unwrap();
        assert_eq!((u.flag(FlagId(1)), u.flag(FlagId(2))), (5, 1));
        assert_eq!(potions(&s), before + 3, "보상 포션");

        let again = s.save_data(String::new());
        assert_eq!(again.flags, [(FlagId(1), 5), (FlagId(2), 1)]);
        assert_eq!(
            again.world_flags,
            [(FlagId(40), 9)],
            "월드 플래그도 이어진다"
        );
    }

    #[test]
    fn items_that_no_longer_exist_are_dropped_instead_of_blocking_play() {
        let mut save = session().save_data(String::new());
        save.bags.push((5, ItemId(9_999), 1));
        save.equipped.push(ItemId(9_998));
        let s = PlaySession::start(
            &Scene::server_default(),
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions {
                spawn: None,
                save: Some(save),
                class: None,
                links: Default::default(),
            },
        )
        .expect("없는 아이템 때문에 플레이가 막히면 안 된다");
        let inv = s.world().unit(s.player()).unwrap().inventory();
        assert_eq!(inv.bag(BagKind::Equipment).get(5), None);
    }

    #[test]
    fn marker_overrides_reach_the_spawned_unit() {
        // P1-4: 같은 타입이라도 이 마커의 슬라임만 HP 가 다르다. 나머지 수치는 타입 그대로.
        let mut scene = Scene::server_default();
        let slime = scene.items.iter_mut().find(|i| i.name == "슬라임").unwrap();
        slime.overrides.max_hp = Some(333);
        let s = PlaySession::start(
            &scene,
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions::default(),
        )
        .unwrap();
        let plain = PlaySession::start(
            &Scene::server_default(),
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions::default(),
        )
        .unwrap();

        let unit = s.world().unit(find(&s, "슬라임")).unwrap();
        let base = plain.world().unit(find(&plain, "슬라임")).unwrap();
        assert_eq!((unit.hp(), unit.def().max_hp), (333, 333));
        assert_eq!(unit.def().attack, base.def().attack);
        assert_ne!(base.def().max_hp, 333);
    }

    #[test]
    fn a_spawn_area_raises_a_scattered_pack_that_respawns_in_place() {
        let mut scene = Scene::server_default();
        let slime = scene.items.iter_mut().find(|i| i.name == "슬라임").unwrap();
        slime.spawn = crate::scene::SpawnArea {
            count: 3,
            radius: 2.0,
        };
        let center = slime.pos;
        let s = PlaySession::start(
            &scene,
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions::default(),
        )
        .unwrap();
        let pack: Vec<Vec2> = s
            .world()
            .units()
            .filter(|(e, _)| s.labels[e].name == "슬라임")
            .map(|(_, u)| u.spawn_pos())
            .collect();
        assert_eq!(pack.len(), 3, "마커 하나에 세 마리");
        for (i, p) in pack.iter().enumerate() {
            assert!(p.distance(center) <= 2.0 + 1e-3, "{i}: 반경 밖");
            for q in &pack[i + 1..] {
                assert!(p.distance(*q) > 0.1, "같은 자리에 겹쳤다");
            }
        }
    }

    #[test]
    fn no_player_spawn_means_no_play() {
        let mut scene = Scene::server_default();
        scene.items.retain(|i| i.kind != ItemKind::PlayerSpawn);
        assert!(
            PlaySession::start(
                &scene,
                Camera2d::default(),
                GameData::embedded(),
                PlayOptions::default()
            )
            .is_err()
        );
    }

    #[test]
    fn clicking_the_ground_walks_there() {
        let mut s = session();
        let goal = Vec2::new(-4.0, 3.0);
        s.click(goal, 0.01);
        for _ in 0..60 {
            s.tick(DT, None);
        }
        assert_eq!(s.world().unit(s.player()).unwrap().pos(), goal);
    }

    #[test]
    fn clicking_a_monster_walks_up_and_fights_it_to_the_death() {
        let mut s = session();
        let slime = find(&s, "슬라임");
        let at = s.world().unit(slime).unwrap().pos();
        s.click(at, 0.01);
        assert_eq!(s.attack_target(), Some(slime));

        let mut dead = false;
        for _ in 0..(20 * 60) {
            s.tick(DT, None);
            if !s.world().unit(slime).unwrap().is_alive() {
                dead = true;
                break;
            }
        }
        assert!(dead, "슬라임을 쓰러뜨리지 못했다: {:?}", s.log);
        assert!(s.world().unit(s.player()).unwrap().is_alive());
        assert_eq!(s.order, Order::Idle, "대상이 죽으면 주문이 끝난다");
    }

    #[test]
    fn loot_can_be_clicked_and_picked_up() {
        let mut s = session();
        let slime = find(&s, "슬라임");
        let at = s.world().unit(slime).unwrap().pos();
        s.click(at, 0.01);
        for _ in 0..(20 * 60) {
            s.tick(DT, None);
            if s.world().ground_items().count() > 0 {
                break;
            }
        }
        let (_, drop) = s
            .world()
            .ground_items()
            .next()
            .expect("시드 1 이면 젤리가 나온다");
        let (pos, stack) = (drop.pos, drop.stack);

        s.click(pos, 0.01);
        for _ in 0..100 {
            s.tick(DT, None);
        }
        assert_eq!(
            s.world().ground_items().count(),
            0,
            "줍지 않았다: {:?}",
            s.log
        );
        let me = s.world().unit(s.player()).unwrap();
        assert!(me.inventory().bag(BagKind::Consumable).count_of(stack.item) >= stack.count);
    }

    #[test]
    fn clicking_an_immortal_npc_is_refused_once_not_every_tick() {
        let mut s = session();
        let guard = find(&s, "마을 경비병");
        // 경비병 옆으로 붙인다 — 사거리 안에서 거절이 나야 한다.
        let at = s.world().unit(guard).unwrap().pos();
        s.click(at + Vec2::new(1.0, 0.0), 0.01);
        for _ in 0..200 {
            s.tick(DT, None);
        }
        s.click(at, 0.01);
        for _ in 0..40 {
            s.tick(DT, None);
        }
        let refusals = s.log().filter(|l| l.starts_with("할 수 없음")).count();
        assert_eq!(refusals, 1, "{:?}", s.log);
        assert_eq!(s.order, Order::Idle);
    }

    #[test]
    fn inventory_panel_equips_the_starting_sword() {
        let mut s = session();
        let base = s.world().unit(s.player()).unwrap().attack();
        s.inventory(InventoryAction::Equip(0));
        s.tick(DT, None);
        assert_eq!(s.world().unit(s.player()).unwrap().attack(), base + 12);
        assert!(s.log().any(|l| l == "숏소드 장착"));
    }

    // ── 직업 (R2) ────────────────────────────────────────────────────────────

    fn new_game(class: Option<ClassId>, save: Option<SaveData>) -> PlaySession {
        PlaySession::start(
            &Scene::server_default(),
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions {
                spawn: None,
                save,
                class,
                links: Default::default(),
            },
        )
        .unwrap()
    }

    #[test]
    fn the_default_class_is_the_old_player() {
        // 직업 1(전사)은 직업이 생기기 전의 플레이어와 수치가 같다 — 밸런스가 흔들리지 않게.
        let s = new_game(None, None);
        let u = s.world().unit(s.player()).unwrap();
        assert_eq!(u.def().class, Some(ClassId(1)));
        assert_eq!(
            (
                u.max_hp(),
                u.max_mp(),
                u.mp_regen(),
                u.attack(),
                u.defense()
            ),
            (200, 60, 2, 20, 5)
        );
        assert_eq!(s.class_name(), "전사");
        assert_eq!(
            s.stat_lines(),
            [
                (String::from("힘"), 10),
                (String::from("체력"), 9),
                (String::from("지능"), 5),
                (String::from("민첩"), 5)
            ]
        );
    }

    #[test]
    fn a_mage_has_its_own_numbers_hotbar_and_cannot_hold_the_sword() {
        let mut s = new_game(Some(ClassId(2)), None);
        let u = s.world().unit(s.player()).unwrap();
        // rules.ron 마법사: vit 6 → HP 140, int 12 → MP 12×12+20 = 164, 회복 6, 공격 str 5 × 2.
        assert_eq!(
            (u.max_hp(), u.max_mp(), u.mp_regen(), u.attack()),
            (140, 164, 6, 10)
        );
        assert_eq!(
            s.hotbar[..3],
            [Some(SkillId(8)), Some(SkillId(6)), None],
            "직업의 시작 스킬"
        );
        s.inventory(InventoryAction::Equip(0));
        s.tick(DT, None);
        assert!(
            s.log().any(|l| l.contains("이 직업은 쓸 수 없는 장비")),
            "{:?}",
            s.log
        );
        assert_eq!(s.world().unit(s.player()).unwrap().attack(), 10);
    }

    #[test]
    fn the_class_is_saved_and_beats_a_new_game_choice_when_continuing() {
        let save = new_game(Some(ClassId(3)), None).save_data(String::new());
        assert_eq!(save.class, Some(ClassId(3)));
        let s = new_game(Some(ClassId(2)), Some(save.clone()));
        assert_eq!(s.class_name(), "궁수", "이어 하기면 저장한 직업");
        // 데이터에서 사라진 직업 → 기본 직업으로 이어 한다 (막지 않는다).
        let gone = SaveData {
            class: Some(ClassId(77)),
            ..save
        };
        assert_eq!(new_game(None, Some(gone)).class_name(), "전사");
    }

    // ── 스킬 (R3) ────────────────────────────────────────────────────────────

    fn give(s: &mut PlaySession, item: u32, count: u32) -> u16 {
        let me = s.player();
        assert!(s.auth.world_mut().give_item(me, ItemId(item), count));
        let bag = s
            .world()
            .unit(me)
            .unwrap()
            .inventory()
            .bag(BagKind::Consumable);
        bag.iter()
            .find(|(_, st)| st.item == ItemId(item))
            .unwrap()
            .0 as u16
    }

    #[test]
    fn skill_formulas_scale_with_level_and_stats() {
        // 치유 = 40 + int × 2 + skill_level × 10 — 전사(int 5) 60, 마법사(int 12) 74.
        let warrior = new_game(Some(ClassId(1)), None);
        let w = warrior.world();
        assert_eq!(
            w.effective_skill(warrior.player(), SkillId(8))
                .unwrap()
                .heal,
            60
        );
        let mage = new_game(Some(ClassId(2)), None);
        assert_eq!(
            mage.world()
                .effective_skill(mage.player(), SkillId(8))
                .unwrap()
                .heal,
            74
        );

        // 강베기 비급으로 2레벨 → 위력 1.9배, MP 17.
        let mut s = new_game(Some(ClassId(1)), None);
        let me = s.player();
        let before = s.world().effective_skill(me, SkillId(5)).unwrap();
        assert_eq!((before.damage_mult, before.mp_cost), (1.8, 15));
        let slot = give(&mut s, 3001, 1);
        s.inventory(InventoryAction::Use(slot));
        s.tick(DT, None);
        assert_eq!(s.world().unit(me).unwrap().skill_level(SkillId(5)), 2);
        let after = s.world().effective_skill(me, SkillId(5)).unwrap();
        assert_eq!((after.damage_mult, after.mp_cost), (1.9, 17));
        assert!(s.log().any(|l| l == "강베기 레벨 2/5"), "{:?}", s.log);
    }

    #[test]
    fn a_mage_gets_fireball_at_level_3_and_places_it_by_hand() {
        let mut s = new_game(Some(ClassId(2)), None);
        let me = s.player();
        let fireball = SkillId(9);
        // 레벨 2, 다음 레벨까지 1 남김 (exp_curve: 레벨 2 → 3 은 400) — 경험치 1 짜리를 쓰러뜨리면 레벨 3.
        assert_eq!(s.world().exp_needed(2), 400);
        s.auth.world_mut().set_progress(me, Progress::new(2, 399));
        let near = s.world().unit(me).unwrap().pos() + Vec2::new(1.0, 0.0);
        let dummy = s.auth.world_mut().spawn_unit(
            near,
            0.0,
            nexus_sim::UnitDef {
                exp_reward: 1,
                faction: nexus_sim::FactionId(201),
                ..nexus_sim::UnitDef::default()
            },
        );
        s.auth.submit(Intent::Attack {
            unit: me,
            target: dummy,
            skill: SkillId(1),
        });
        s.tick(DT, None);
        assert_eq!(s.world().unit(me).unwrap().progress().level(), 3);
        assert_eq!(
            s.world().unit(me).unwrap().skill_level(fireball),
            1,
            "{:?}",
            s.log
        );
        assert!(
            !s.hotbar.contains(&Some(fireball)),
            "새로 배운 스킬은 저절로 놓이지 않는다"
        );
        assert!(s.log().any(|l| l.starts_with("스킬을 배웠다: 화염구")));

        s.assign_hotbar(fireball, Some(0));
        assert_eq!(s.hotbar[0], Some(fireball), "그 칸에 있던 치유는 밀려난다");
        assert!(!s.hotbar.contains(&Some(SkillId(8))));
        s.assign_hotbar(SkillId(8), Some(4));

        // 저장 → 이어 하기: 배운 스킬·레벨·배치가 그대로.
        let save = s.save_data(String::new());
        let back = new_game(None, Some(save));
        let u = back.world().unit(back.player()).unwrap();
        assert_eq!(u.skill_level(fireball), 1);
        assert_eq!(back.hotbar[0], Some(fireball));
        assert_eq!(back.hotbar[4], Some(SkillId(8)));
    }

    #[test]
    fn an_old_save_without_skills_gets_the_starting_skills() {
        let mut save = new_game(Some(ClassId(3)), None).save_data(String::new());
        save.skills = None;
        save.hotbar = None;
        let s = new_game(None, Some(save));
        let u = s.world().unit(s.player()).unwrap();
        assert_eq!(
            u.learned_skills().collect::<Vec<_>>(),
            [(SkillId(5), 1), (SkillId(6), 1)],
            "궁수의 시작 스킬"
        );
        assert_eq!(s.hotbar[..2], [Some(SkillId(6)), Some(SkillId(5))]);
    }

    #[test]
    fn a_warrior_cannot_read_the_fireball_tome() {
        let mut s = new_game(Some(ClassId(1)), None);
        let slot = give(&mut s, 3002, 1);
        s.inventory(InventoryAction::Use(slot));
        s.tick(DT, None);
        assert!(
            s.log().any(|l| l.contains("배울 수 없는 스킬")),
            "{:?}",
            s.log
        );
        let lines = s.skill_lines();
        assert_eq!(lines.len(), 4, "전사의 시작 스킬 넷");
        assert_eq!(
            (lines[0].name.as_str(), lines[0].level, lines[0].max_level),
            ("강베기", 1, 5)
        );
        assert_eq!(lines[0].slot, Some(0));
    }

    // ── 상점·재화 (R4) ───────────────────────────────────────────────────────

    /// 상인에게 걸어가 말을 걸 때까지 돌린다.
    fn talk_to_merchant(s: &mut PlaySession) -> Entity {
        let merchant = find(s, "상인 NPC");
        let at = s.world().unit(merchant).unwrap().pos();
        s.click(at, 0.01);
        for _ in 0..600 {
            s.tick(DT, None);
            if s.trade_view().is_some() {
                return merchant;
            }
        }
        panic!("상인에게 닿지 못했다: {:?}", s.log);
    }

    #[test]
    fn clicking_the_merchant_talks_instead_of_attacking() {
        let mut s = new_game(None, None);
        assert_eq!(s.gold(), 100, "rules.ron: starting_currency");
        let merchant = talk_to_merchant(&mut s);
        assert!(s.attack_target().is_none());
        assert_eq!(s.world().unit(merchant).unwrap().hp(), 300, "치지 않았다");
        let view = s.trade_view().unwrap();
        assert_eq!(view.npc_name, "상인 NPC");
        assert_eq!(view.purse, "100 골드");
        assert_eq!(view.services, [(1, String::from("치료 — 20 골드"))]);
        let shop = &view.shops[0];
        assert_eq!(
            shop.goods[0],
            (0, String::from("빨간 포션"), String::from("50 골드"), true)
        );
        assert!(!shop.goods[3].3, "마도서 1000 골드는 살 수 없다");
        assert!(s.log().any(|l| l == "상인 NPC 에게 말을 걸었다"));
    }

    /// NPC 창(R4b) — 메뉴에는 그 NPC 의 기능만, 고르면 그 쪽, 줄을 누르면 거래.
    #[test]
    fn the_npc_window_pages_from_the_menu_to_buying() {
        let mut s = new_game(None, None);
        assert!(s.npc_screen().is_none(), "말을 걸기 전에는 없다");
        talk_to_merchant(&mut s);
        assert!(s.is_trading());
        let menu = s.npc_screen().unwrap();
        assert_eq!(menu.name, "상인 NPC");
        assert!(menu.profile.starts_with("마을 잡화점 주인"));
        let texts: Vec<&str> = menu.rows.iter().map(|r| r.text.as_str()).collect();
        for want in ["퀘스트", "사기", "팔기", "치료 — 20 골드"] {
            assert!(texts.contains(&want), "{want} 없음 — {texts:?}");
        }
        assert!(
            !texts.contains(&"제작") && !texts.contains(&"창고에 맡기기"),
            "상인에게 없는 기능은 메뉴에 없다 — {texts:?}"
        );

        // 사기 쪽으로 — 빨간 포션 줄을 누르면 산다.
        let buy = texts.iter().position(|t| *t == "사기").unwrap();
        s.npc_pick(buy);
        let page = s.npc_screen().unwrap();
        assert_eq!(page.page, "사기");
        assert_eq!(page.rows[0].text, "빨간 포션");
        assert_eq!(page.rows[0].right, "50 골드");
        let book = page.rows.iter().position(|r| !r.enabled).unwrap();
        let gold = s.gold();
        s.npc_pick(book); // 흐린 줄 — 아무 일도 없다
        s.tick(DT, None);
        assert_eq!(s.gold(), gold);
        s.npc_pick(0);
        s.tick(DT, None);
        assert_eq!(s.gold(), gold - 50);
        assert_eq!(s.npc_screen().unwrap().page, "사기", "산 뒤에도 같은 쪽");

        s.npc_back();
        assert_eq!(s.npc_screen().unwrap().page, "무엇을 도와줄까?");
        s.trade(TradeAction::Close);
        assert!(!s.is_trading() && s.npc_screen().is_none());
    }

    #[test]
    fn buying_selling_and_healing_at_the_merchant() {
        let mut s = new_game(None, None);
        talk_to_merchant(&mut s);
        let me = s.player();
        let potions = |s: &PlaySession| {
            let u = s.world().unit(me).unwrap();
            u.inventory().bag(BagKind::Consumable).count_of(ItemId(501))
        };
        let before = potions(&s);
        s.trade(TradeAction::Buy {
            shop: ShopId(1),
            goods: 0,
            count: 1,
        });
        s.tick(DT, None);
        assert_eq!((potions(&s), s.gold()), (before + 1, 50));
        assert!(s.log().any(|l| l == "샀다: 빨간 포션 ×1"));
        // 물약 2개를 판다 — 기본 값 40 의 반값 20 × 2.
        let slot = s.trade_view().unwrap().shops[0]
            .sellable
            .iter()
            .find(|line| line.2 == "빨간 포션")
            .unwrap()
            .1;
        s.trade(TradeAction::Sell {
            shop: ShopId(1),
            bag: BagKind::Consumable,
            slot,
            count: 2,
        });
        s.tick(DT, None);
        assert_eq!((potions(&s), s.gold()), (before - 1, 90));
        // 치료 20 골드.
        s.auth.world_mut().set_hp(me, 10);
        s.trade(TradeAction::Service(1));
        s.tick(DT, None);
        let u = s.world().unit(me).unwrap();
        assert_eq!((u.hp(), s.gold()), (u.max_hp(), 70));
        // 저장 → 이어 하기에 지갑이 남는다.
        let back = new_game(None, Some(s.save_data(String::new())));
        assert_eq!(back.gold(), 70);
    }

    #[test]
    fn slimes_pay_a_bounty_and_old_saves_start_with_the_starting_gold() {
        let mut s = session();
        let slime = find(&s, "슬라임");
        s.click(s.world().unit(slime).unwrap().pos(), 0.01);
        for _ in 0..900 {
            s.tick(DT, None);
            if !s.world().unit(slime).unwrap().is_alive() {
                break;
            }
        }
        assert_eq!(s.gold(), 115, "시작 100 + 슬라임 15");
        let mut old = s.save_data(String::new());
        old.wallet = None;
        assert_eq!(new_game(None, Some(old)).gold(), 100);
    }

    #[test]
    fn walking_away_closes_the_trade_window() {
        let mut s = new_game(None, None);
        let merchant = talk_to_merchant(&mut s);
        let away = s.world().unit(merchant).unwrap().pos() + Vec2::new(12.0, 0.0);
        s.click(away, 0.01);
        for _ in 0..200 {
            s.tick(DT, None);
        }
        assert!(s.trade_view().is_none());
    }

    // ── 제작·텔레포트·창고 (R4-2) ────────────────────────────────────────────

    /// 마을 존으로 새 게임.
    fn village() -> PlaySession {
        let scene = crate::zone_file::from_ron(include_str!("../../../zones/village.zone.ron"))
            .expect("마을 존");
        PlaySession::start(
            &scene,
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions::default(),
        )
        .unwrap()
    }

    /// 플레이용 맵은 존 전체를 덮는다 — 씬의 맵(마을은 64×64)은 그대로다 (R6-3, A안).
    #[test]
    fn the_play_map_covers_the_whole_zone_but_the_scene_does_not_change() {
        let scene = crate::zone_file::from_ron(include_str!("../../../zones/village.zone.ron"))
            .expect("마을 존");
        let before = (
            scene.tiles.width(),
            scene.tiles.origin(),
            scene.tiles.runs(),
        );
        let mut s = PlaySession::start(
            &scene,
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions::default(),
        )
        .unwrap();
        assert_eq!(
            (
                scene.tiles.width(),
                scene.tiles.origin(),
                scene.tiles.runs()
            ),
            before,
            "씬은 그대로"
        );
        let tiles = s.world().tiles();
        let (lo, hi) = tiles.world_bounds();
        assert!(lo.cmple(scene.zone.min).all() && hi.cmpge(scene.zone.max).all());
        // 같은 월드 자리는 같은 타일 — 칠한 지형이 따라온다.
        for (y, x, _, tile) in scene.tiles.runs() {
            let at = scene
                .tiles
                .tile_center(nexus_sim::TileCoord::new(x as i32, y as i32));
            assert_eq!(tiles.get(tiles.world_to_tile(at)), Some(tile));
        }
        // 씬의 맵(-32~32m) 밖으로 걸어 나간다.
        let goal = Vec2::new(0.0, 60.0);
        s.click(goal, 0.01);
        let arrived = (0..1500).any(|_| {
            s.tick(DT, None);
            s.world().unit(s.player()).unwrap().pos().distance(goal) < 0.1
        });
        assert!(arrived, "{:?}", s.log);
    }

    // ── 포털 (R6) ─────────────────────────────────────────────────────────

    /// 이 존 파일로 새 게임 — 지금 레벨과 레벨별 입장 조건을 함께 준다.
    fn in_level(zone: &str, here: &str, requires: &[(&str, Condition)]) -> PlaySession {
        let scene = crate::zone_file::from_ron(zone).expect("존");
        PlaySession::start(
            &scene,
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions {
                links: LevelLinks {
                    here: here.to_owned(),
                    clear: None,
                    requires: requires
                        .iter()
                        .map(|(l, c)| ((*l).to_owned(), c.clone()))
                        .collect(),
                },
                ..PlayOptions::default()
            },
        )
        .unwrap()
    }

    /// `at` 으로 걸어가며 다른 레벨로 가는 요청이 나올 때까지 돈다.
    fn walk_until_travel(s: &mut PlaySession, at: Vec2) -> Option<(String, Vec2)> {
        s.click(at, 0.01);
        for _ in 0..400 {
            s.tick(DT, None);
            if let Some(t) = s.take_travel() {
                return Some(t);
            }
        }
        None
    }

    const VILLAGE: &str = include_str!("../../../zones/village.zone.ron");
    const HOUSE: &str = include_str!("../../../zones/house.zone.ron");

    #[test]
    fn the_house_door_leads_inside_and_the_inside_door_leads_back() {
        let mut s = in_level(VILLAGE, "village", &[]);
        assert_eq!(
            walk_until_travel(&mut s, Vec2::new(-6.5, 4.5)),
            Some((String::from("house"), Vec2::new(0.0, -2.0)))
        );
        // 집 안에서 문을 밟으면 마을의 집 앞으로.
        let mut s = in_level(HOUSE, "house", &[]);
        assert_eq!(
            walk_until_travel(&mut s, Vec2::new(0.0, -3.5)),
            Some((String::from("village"), Vec2::new(-6.5, 3.0)))
        );
    }

    #[test]
    fn a_level_entry_condition_keeps_the_door_shut() {
        let lv5 = Condition::Level {
            cmp: nexus_sim::Cmp::Ge,
            value: 5,
        };
        let mut s = in_level(VILLAGE, "village", &[("house", lv5)]);
        s.click(Vec2::new(-6.5, 4.5), 0.01);
        let blocked = (0..400).any(|_| {
            s.tick(DT, None);
            assert_eq!(s.take_travel(), None);
            s.log.iter().any(|l| l.contains("아직 들어갈 수 없다"))
        });
        assert!(blocked, "{:?}", s.log);
    }

    #[test]
    fn a_portal_to_the_level_you_are_in_moves_you_inside_it() {
        // 지금 레벨을 가리키는 포털은 레벨을 다시 열지 않고 그 자리로 옮긴다.
        let mut s = in_level(VILLAGE, "house", &[]);
        assert_eq!(walk_until_travel(&mut s, Vec2::new(-6.5, 4.5)), None);
        let pos = s.world().unit(s.player()).unwrap().pos();
        assert!(pos.distance(Vec2::new(0.0, -2.0)) < 0.01, "{pos}");
    }

    /// R6-2 끝나는 조건: 던전 입장 → 나가는 문이 닫혀 있음 → 보스 처치 → 클리어·보상 → 나가는 문 열림.
    #[test]
    fn the_dungeon_exit_opens_when_the_boss_falls() {
        let (levels, warnings) = crate::level::Levels::load();
        assert!(warnings.is_empty(), "{warnings:?}");
        let clear = levels.clear_of("dungeon").expect("던전 레벨의 클리어");
        let data = GameData::embedded();
        let file = levels
            .level("dungeon")
            .and_then(|l| l.clear.as_ref())
            .unwrap();
        assert!(
            data.check_clear(file).is_empty(),
            "{:?}",
            data.check_clear(file)
        );
        let scene = crate::zone_file::from_ron(include_str!("../../../zones/dungeon.zone.ron"))
            .expect("던전 존");
        let mut s = PlaySession::start(
            &scene,
            Camera2d::default(),
            data,
            PlayOptions {
                links: LevelLinks {
                    here: String::from("dungeon"),
                    requires: levels.entry_conditions(),
                    clear: Some(clear),
                },
                ..PlayOptions::default()
            },
        )
        .unwrap();
        let (cleared, goals) = s.clear_lines().unwrap();
        assert!(!cleared);
        assert_eq!(goals, [(String::from("슬라임 왕 처치 0/1"), false)]);

        // 깨기 전에는 나가는 문이 닫혀 있다.
        let exit = Vec2::new(0.0, -5.5);
        s.click(exit, 0.01);
        let blocked = (0..200).any(|_| {
            s.tick(DT, None);
            assert_eq!(s.take_travel(), None);
            s.log.iter().any(|l| l.contains("던전 목표를 먼저 채워야"))
        });
        assert!(blocked, "{:?}", s.log);

        // 보스를 한 방 거리로 — 수치 싸움은 이 시험의 일이 아니다.
        let boss = find(&s, "슬라임 왕");
        s.auth.world_mut().set_hp(boss, 1);
        let at = s.world().unit(boss).unwrap().pos();
        s.click(at, 0.01);
        let won = (0..600).any(|_| {
            s.tick(DT, None);
            s.log.iter().any(|l| l.contains("던전 클리어"))
        });
        assert!(won, "{:?}", s.log);
        assert!(
            s.log
                .iter()
                .any(|l| l.contains("던전 목표 — 슬라임 왕 처치 1/1"))
        );
        let (cleared, goals) = s.clear_lines().unwrap();
        assert!(cleared && goals.iter().all(|(_, done)| *done));
        assert!(
            s.world()
                .unit(s.player())
                .unwrap()
                .wallet()
                .get(nexus_sim::CurrencyId(1))
                >= 150
        );

        // 이제 열린다 — 마을로.
        let back = walk_until_travel(&mut s, exit);
        assert_eq!(back, Some((String::from("village"), Vec2::new(8.0, -8.0))));
    }

    /// 이름으로 NPC 에게 걸어가 말을 건다.
    fn talk_to(s: &mut PlaySession, name: &str) {
        let npc = find(s, name);
        let at = s.world().unit(npc).unwrap().pos();
        s.click(at, 0.01);
        for _ in 0..600 {
            s.tick(DT, None);
            if s.trade_view().is_some_and(|v| v.npc_name == name) {
                return;
            }
        }
        panic!("{name} 에게 닿지 못했다: {:?}", s.log);
    }

    #[test]
    fn the_blacksmith_crafts_stores_and_teleports() {
        let mut s = village();
        let me = s.player();
        assert!(s.auth.world_mut().give_item(me, ItemId(909), 7));
        talk_to(&mut s, "대장장이");
        let view = s.trade_view().unwrap();
        assert!(view.profile.starts_with("젤리만 넉넉하면"));
        let craft = &view.crafts[0];
        assert_eq!(
            craft.recipes[0],
            (
                0,
                String::from("빨간 포션 ×1"),
                String::from("젤리 7/5 · 10 골드"),
                true
            )
        );
        assert!(!craft.recipes[1].3, "숏소드는 레벨 3 부터");
        assert_eq!(view.teleports[0].destinations[0].1, "마을 광장");

        // 제작: 젤리 5 + 10 골드 → 빨간 포션.
        let potions = |s: &PlaySession| {
            s.world()
                .unit(me)
                .unwrap()
                .inventory()
                .bag(BagKind::Consumable)
                .count_of(ItemId(501))
        };
        let before = potions(&s);
        s.trade(TradeAction::Craft {
            craft: CraftId(1),
            recipe: 0,
            times: 1,
        });
        s.tick(DT, None);
        assert_eq!((potions(&s), s.gold()), (before + 1, 90));

        // 창고: 빨간 포션을 맡기고 저장 → 이어 하기에 남는다.
        let slot = s.trade_view().unwrap().storage.unwrap().carried[0].1;
        s.trade(TradeAction::Deposit {
            bag: BagKind::Consumable,
            slot,
            count: 2,
        });
        s.tick(DT, None);
        let stored = s.trade_view().unwrap().storage.unwrap().stored;
        assert_eq!(stored, [(0, String::from("빨간 포션"), 2)]);
        let save = s.save_data(String::new());
        assert_eq!(save.storage, [(0, ItemId(501), 2)]);
        let scene =
            crate::zone_file::from_ron(include_str!("../../../zones/village.zone.ron")).unwrap();
        let back = PlaySession::start(
            &scene,
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions {
                spawn: None,
                save: Some(save),
                class: None,
                links: Default::default(),
            },
        )
        .unwrap();
        assert_eq!(
            back.world()
                .unit(back.player())
                .unwrap()
                .storage()
                .count_of(ItemId(501)),
            2
        );

        // 마을 안 텔레포트: 10 골드 → 광장(0, 0). 멀어지면 창이 닫힌다.
        s.trade(TradeAction::Teleport {
            teleport: TeleportId(1),
            dest: 0,
        });
        s.tick(DT, None);
        assert_eq!(s.world().unit(me).unwrap().pos(), Vec2::ZERO);
        assert_eq!(s.gold(), 80);
        assert!(s.trade_view().is_none());
    }

    #[test]
    fn the_merchant_can_send_you_to_another_level() {
        let mut s = new_game(None, None);
        talk_to(&mut s, "상인 NPC");
        let tp = &s.trade_view().unwrap().teleports[0];
        assert_eq!(
            tp.destinations,
            [
                (0, String::from("마을"), String::from("무료")),
                (1, String::from("샘플 존"), String::from("30 골드"))
            ]
        );
        s.trade(TradeAction::Teleport {
            teleport: TeleportId(2),
            dest: 0,
        });
        s.tick(DT, None);
        assert_eq!(
            s.take_travel(),
            Some((String::from("village"), Vec2::new(-6.0, 10.0))),
            "다른 레벨은 에디터가 연다"
        );
        assert!(s.log().any(|l| l == "다른 곳으로 떠난다…"));
    }

    // ── 퀘스트 (R5) ──────────────────────────────────────────────────────────

    /// 플레이어 옆에 슬라임(액터 100)을 세우고 그 tick 에 쓰러뜨린다.
    fn slay_slime(s: &mut PlaySession) {
        let me = s.player();
        let near = s.world().unit(me).unwrap().pos() + Vec2::new(1.0, 0.0);
        let def = nexus_sim::UnitDef {
            actor: nexus_sim::ActorType(100),
            faction: nexus_sim::FactionId(201),
            exp_reward: 60,
            ..nexus_sim::UnitDef::default()
        };
        let slime = s.auth.world_mut().spawn_unit(near, 0.0, def);
        // 기본 공격 쿨타임이 있으니 쓰러질 때까지 다시 낸다.
        for _ in 0..40 {
            s.auth.submit(Intent::Attack {
                unit: me,
                target: slime,
                skill: SkillId(1),
            });
            s.tick(DT, None);
            if !s.world().unit(slime).unwrap().is_alive() {
                return;
            }
        }
        panic!("슬라임을 쓰러뜨리지 못했다: {:?}", s.log);
    }

    #[test]
    fn accept_slay_and_report_the_slime_quest() {
        // R5 끝나는 조건: 받기 → 처치 → 보고 → 보상.
        let mut s = new_game(None, None);
        talk_to(&mut s, "상인 NPC");
        let offer = &s.trade_view().unwrap().quests[0];
        assert_eq!(
            (offer.title.as_str(), offer.can_accept),
            ("슬라임 소탕", true)
        );
        s.trade(TradeAction::AcceptQuest(QuestId(1)));
        s.tick(DT, None);
        assert!(s.log().any(|l| l == "퀘스트를 받았다: 슬라임 소탕"));
        assert_eq!(
            s.quest_lines()[0].goals,
            [(String::from("슬라임 처치 0/3"), false)]
        );
        for _ in 0..3 {
            slay_slime(&mut s);
        }
        assert_eq!(s.quest_lines()[0].state, QuestState::Ready);
        assert!(s.log().any(|l| l == "슬라임 소탕 — 보고할 수 있다"));

        // 보고 — 상인에게 다시 말을 걸어 (멀어졌을 수 있다).
        talk_to(&mut s, "상인 NPC");
        let gold = s.gold();
        let exp_before = s.world().unit(s.player()).unwrap().progress();
        s.trade(TradeAction::TurnInQuest(QuestId(1)));
        s.tick(DT, None);
        assert!(s.log().any(|l| l == "퀘스트 완료: 슬라임 소탕"));
        assert_eq!(s.gold(), gold + 100);
        assert_ne!(
            s.world().unit(s.player()).unwrap().progress(),
            exp_before,
            "경험치 80"
        );
        assert!(
            s.quest_lines().iter().all(|q| q.quest != QuestId(1)),
            "끝난 퀘스트는 목록에서 빠진다"
        );
        // 보상 경험치로 레벨 2 — 트리거 4 가 월드 퀘스트 '광장 구경' 을 준다 (데이터가 맞물린다).
        assert!(s.quest_lines().iter().any(|q| q.quest == QuestId(3)));
        let view = s.trade_view().unwrap();
        assert_eq!(view.quests[0].state, QuestState::Completed);
        assert!(!view.quests[0].can_accept, "반복 불가");
    }

    #[test]
    fn quest_progress_is_saved_and_the_sequel_needs_the_first() {
        let mut s = village();
        talk_to(&mut s, "대장장이");
        let sequel = &s.trade_view().unwrap().quests[0];
        assert_eq!(sequel.title, "대장장이의 부탁");
        assert!(!sequel.can_accept, "슬라임 소탕을 끝내야 받는다");

        talk_to(&mut s, "상인 NPC");
        s.trade(TradeAction::AcceptQuest(QuestId(1)));
        s.tick(DT, None);
        slay_slime(&mut s);
        let save = s.save_data(String::new());
        assert_eq!(save.quests, [(QuestId(1), Some((0, vec![1])), 0)]);
        let scene =
            crate::zone_file::from_ron(include_str!("../../../zones/village.zone.ron")).unwrap();
        let back = PlaySession::start(
            &scene,
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions {
                spawn: None,
                save: Some(save),
                class: None,
                links: Default::default(),
            },
        )
        .unwrap();
        assert_eq!(
            back.quest_lines()[0].goals,
            [(String::from("슬라임 처치 1/3"), false)],
            "이어 하기에 센 수가 남는다"
        );
    }

    #[test]
    fn reaching_level_two_starts_the_plaza_world_quest() {
        let mut s = new_game(None, None);
        let me = s.player();
        // 레벨 1 → 2 (100) 직전까지 채우고 슬라임 하나(60)로 넘긴다.
        s.auth.world_mut().set_progress(me, Progress::new(1, 99));
        slay_slime(&mut s);
        assert!(
            s.log().any(|l| l == "퀘스트를 받았다: 광장 구경"),
            "{:?}",
            s.log
        );
        assert_eq!(
            s.quest_lines()[0].goals,
            [(String::from("목표 지점에 가기"), false)]
        );
        // 광장(0, 0)으로 걸어가면 보고 없이 끝난다.
        s.auth.world_mut().set_position(me, Vec2::new(6.0, 0.0));
        let gold = s.gold();
        s.click(Vec2::ZERO, 0.01);
        for _ in 0..200 {
            s.tick(DT, None);
        }
        assert!(
            s.log().any(|l| l == "퀘스트 완료: 광장 구경"),
            "{:?}",
            s.log
        );
        assert_eq!(s.gold(), gold + 20);
    }
}
