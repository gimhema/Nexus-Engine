//! 게임 규칙 — **I/O 가 없는 순수 로직**.
//!
//! 렌더·플랫폼·파일·네트워크를 전혀 모른다. 이유는 두 가지다:
//!
//! 1. 전부 단위 테스트가 된다.
//! 2. 단계 2 에서 C++ 서버와 규칙을 대조하거나 교체할 때 **범위가 이 크레이트로 한정된다**.
//!    같은 규칙이 두 곳(여기와 서버)에 존재하는 것은 피할 수 없으므로, 갈라지는 비용을
//!    줄이려면 한곳에 모아 두어야 한다.
//!
//! **수치는 코드가 아니라 데이터로 둔다.**

#![forbid(unsafe_code)]

mod ai;
mod authority;
mod combat;
mod faction;
mod item;
mod loot;
mod script;
mod tilemap;
mod unit;
mod world;

pub use authority::{Authority, Event, Intent, LocalAuthority, Rejection};
pub use combat::{SkillDef, SkillId, damage};
pub use faction::{FactionId, FactionTable, Relation};
pub use item::{
    BAG_SLOTS, Bag, BagKind, EquipSlot, GroundItem, Inventory, ItemDef, ItemId, ItemKind, ItemStack,
};
pub use loot::{LootEntry, LootTableId};
pub use script::{ScriptHost, ScriptLog};
pub use tilemap::{Tile, TileCoord, TileMap};
pub use unit::{AiKind, Unit, UnitDef};
pub use world::{DEFAULT_PICKUP_RANGE, SimWorld};
