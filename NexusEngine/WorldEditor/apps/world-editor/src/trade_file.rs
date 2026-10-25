//! `rules.ron` 의 재화·상점·NPC 기능 — 파일 형식·검증·시뮬레이션 타입으로 옮기기 (R4).
//!
//! ```ron
//! currencies: [1],            // 쓰는 재화 번호 — 이름은 display.ron 의 currencies
//! default_currency: 1,        // 값만 적은 곳(아이템 value · 상점 price · 처치 bounty)의 재화
//! starting_currency: 100,     // 새 게임의 기본 재화
//! items: { 501: Consumable(heal: 60, max_stack: 20, value: 40) },   // value = 상점이 사들일 때의 기준
//! shops: {
//!     1: (goods: [(item: 501, price: 30), (item: 3002, price: 500)], buy_rate: 500),
//! },
//! actors: { 11: (…, services: [Shop(1), Heal(price: 20)]) },          // 이 NPC 가 하는 일
//! // R4-2
//! crafts: {
//!     1: (recipes: [(inputs: [(item: 909, count: 3)], fee: 10, output: 501)]),
//! },
//! teleports: {
//!     // level 을 비우면 지금 존 안, 적으면 그 레벨을 열고 pos 에 선다.
//!     1: (destinations: [(pos: (20.0, 0.0), price: 30), (level: "sample", pos: (0.0, 0.0))]),
//! },
//! actors: { 12: (…, services: [Craft(1), Teleport(1), Storage]) },
//! ```
//!
//! 목적지 이름은 표시 데이터라 `display.ron` 의 `teleports: { 1: ["광장", "샘플 존"] }`(순서대로)에 있다.
//!
//! 규칙은 `nexus_sim::trade` 에 있다 — 여기는 파일 모양과 검증뿐이다.

use std::collections::BTreeMap;

use nexus_core::Vec2;
use nexus_sim::{
    CraftDef, CraftId, CurrencyId, Destination, Goods, ItemId, Price, Recipe, Service, ShopDef,
    ShopId, TeleportDef, TeleportId,
};

use crate::trigger_file::ConditionFile;
use serde::Deserialize;

/// 상점 하나.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ShopFile {
    /// 파는 물건 — 목록 순서가 물건 번호다.
    #[serde(default)]
    pub(crate) goods: Vec<GoodsFile>,
    /// 사들이는 비율 (아이템 value 의 천분율). 0 이면 사지 않는다. 적지 않으면 500(반값).
    #[serde(default = "half")]
    pub(crate) buy_rate: u32,
}

fn half() -> u32 {
    500
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GoodsFile {
    pub(crate) item: u32,
    pub(crate) price: u32,
    /// 재화 번호 — 적지 않으면(0) 기본 재화.
    #[serde(default)]
    pub(crate) currency: u32,
}

/// NPC 기능 하나.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) enum ServiceFile {
    Shop(u32),
    /// HP·MP 를 가득 — `price` 는 기본 재화.
    Heal {
        #[serde(default)]
        price: u32,
    },
    /// 제작 표 (R4-2).
    Craft(u32),
    /// 텔레포트 목적지 표 (R4-2).
    Teleport(u32),
    /// 창고 (R4-2).
    Storage,
}

/// 제작 표 (R4-2).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CraftFile {
    #[serde(default)]
    pub(crate) recipes: Vec<RecipeFile>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecipeFile {
    pub(crate) inputs: Vec<InputFile>,
    /// 수수료 (기본 재화). 0 이면 무료.
    #[serde(default)]
    pub(crate) fee: u32,
    pub(crate) output: u32,
    #[serde(default = "one")]
    pub(crate) count: u32,
    /// 만들 수 있는 조건 — 트리거 `when` 과 같은 형식.
    #[serde(default = "crate::trigger_file::always")]
    pub(crate) requires: ConditionFile,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InputFile {
    pub(crate) item: u32,
    #[serde(default = "one")]
    pub(crate) count: u32,
}

fn one() -> u32 {
    1
}

/// 텔레포트 목적지 표 (R4-2).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TeleportFile {
    #[serde(default)]
    pub(crate) destinations: Vec<DestFile>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DestFile {
    /// 비우면 지금 존 안. 적으면 그 레벨(`levels/<번호>.level.ron`)을 열고 도착한다.
    #[serde(default)]
    pub(crate) level: String,
    pub(crate) pos: (f32, f32),
    /// 비용 (기본 재화). 0 이면 무료.
    #[serde(default)]
    pub(crate) price: u32,
}

impl ServiceFile {
    /// 파일에 적는 모양 — 편집기가 액터 항목을 다시 쓸 때.
    pub(crate) fn ron(&self) -> String {
        match self {
            Self::Shop(id) => format!("Shop({id})"),
            Self::Heal { price: 0 } => String::from("Heal()"),
            Self::Heal { price } => format!("Heal(price: {price})"),
            Self::Craft(id) => format!("Craft({id})"),
            Self::Teleport(id) => format!("Teleport({id})"),
            Self::Storage => String::from("Storage"),
        }
    }
}

/// 검증에 필요한 것만 빌려 온 표.
pub(crate) struct TradeRefs<'a> {
    pub(crate) currencies: &'a [u32],
    pub(crate) default_currency: Option<u32>,
    pub(crate) shops: &'a BTreeMap<u32, ShopFile>,
    pub(crate) crafts: &'a BTreeMap<u32, CraftFile>,
    pub(crate) teleports: &'a BTreeMap<u32, TeleportFile>,
}

impl TradeRefs<'_> {
    fn currency_ok(&self, c: u32) -> bool {
        self.currencies.contains(&c)
    }
}

/// 재화·상점 검증. `uses_default` 는 기본 재화를 쓰는 곳(값·처치 보상·시작 재화)이 있는가.
pub(crate) fn validate(
    t: &TradeRefs<'_>,
    uses_default: bool,
    item_exists: impl Fn(u32) -> bool,
) -> Vec<String> {
    let mut errors = Vec::new();
    for (i, c) in t.currencies.iter().enumerate() {
        if *c == 0 {
            errors.push(String::from("currencies: 0 은 '재화 없음' 예약 번호"));
        }
        if t.currencies[..i].contains(c) {
            errors.push(format!("currencies: {c} 이(가) 두 번 있음"));
        }
    }
    match t.default_currency {
        Some(d) if !t.currency_ok(d) => {
            errors.push(format!("default_currency: {d} 은(는) currencies 에 없음"));
        }
        None if uses_default => errors.push(String::from(
            "값(value·price·bounty·starting_currency)을 쓰려면 default_currency 가 있어야 함",
        )),
        _ => {}
    }
    for (id, shop) in t.shops {
        if shop.buy_rate > 1000 {
            errors.push(format!("상점 {id}: buy_rate 는 0~1000 (천분율)"));
        }
        for (i, g) in shop.goods.iter().enumerate() {
            if !item_exists(g.item) {
                errors.push(format!("상점 {id}: 물건 {i}: 없는 아이템 {}", g.item));
            }
            if g.currency != 0 && !t.currency_ok(g.currency) {
                errors.push(format!("상점 {id}: 물건 {i}: 없는 재화 {}", g.currency));
            }
            if g.currency == 0 && t.default_currency.is_none() {
                errors.push(format!(
                    "상점 {id}: 물건 {i}: 재화를 적거나 default_currency 를 정할 것"
                ));
            }
        }
    }
    let mut fee_needs_currency = false;
    for (id, craft) in t.crafts {
        for (i, r) in craft.recipes.iter().enumerate() {
            if r.inputs.is_empty() {
                errors.push(format!("제작 {id}: 레시피 {i}: 재료가 없음"));
            }
            for input in &r.inputs {
                if !item_exists(input.item) {
                    errors.push(format!("제작 {id}: 레시피 {i}: 없는 아이템 {}", input.item));
                }
            }
            if !item_exists(r.output) {
                errors.push(format!("제작 {id}: 레시피 {i}: 없는 결과물 {}", r.output));
            }
            fee_needs_currency |= r.fee > 0;
        }
    }
    for (id, tp) in t.teleports {
        for (i, d) in tp.destinations.iter().enumerate() {
            if !(d.pos.0.is_finite() && d.pos.1.is_finite()) {
                errors.push(format!("텔레포트 {id}: 목적지 {i}: pos 가 수가 아님"));
            }
            fee_needs_currency |= d.price > 0;
        }
    }
    if fee_needs_currency && t.default_currency.is_none() {
        errors.push(String::from(
            "제작 수수료·텔레포트 비용을 쓰려면 default_currency 가 있어야 함",
        ));
    }
    errors
}

/// 제작 표 하나를 시뮬레이션 타입으로 (R4-2). 조건은 검증을 마쳤다.
pub(crate) fn craft(def: &CraftFile, default_currency: Option<u32>) -> CraftDef {
    CraftDef {
        recipes: def
            .recipes
            .iter()
            .map(|r| Recipe {
                inputs: r.inputs.iter().map(|i| (ItemId(i.item), i.count)).collect(),
                fee: (r.fee > 0).then(|| price(default_currency, r.fee)),
                output: ItemId(r.output),
                count: r.count,
                requires: (&r.requires).into(),
            })
            .collect(),
    }
}

/// 텔레포트 표 하나를 시뮬레이션 타입으로 (R4-2). `travel` 은 레벨 이름 → 번호(1부터).
pub(crate) fn teleport(
    def: &TeleportFile,
    default_currency: Option<u32>,
    travel: impl Fn(&str) -> u32,
) -> TeleportDef {
    TeleportDef {
        destinations: def
            .destinations
            .iter()
            .map(|d| Destination {
                pos: Vec2::new(d.pos.0, d.pos.1),
                travel: if d.level.trim().is_empty() {
                    0
                } else {
                    travel(d.level.trim())
                },
                price: (d.price > 0).then(|| price(default_currency, d.price)),
            })
            .collect(),
    }
}

/// 액터 하나의 기능 목록 검증.
pub(crate) fn validate_services(
    actor: u32,
    services: &[ServiceFile],
    t: &TradeRefs<'_>,
) -> Vec<String> {
    let mut errors = Vec::new();
    for (i, s) in services.iter().enumerate() {
        match *s {
            ServiceFile::Shop(shop) if !t.shops.contains_key(&shop) => {
                errors.push(format!("액터 {actor}: 기능 {i}: 없는 상점 {shop}"));
            }
            ServiceFile::Heal { price } if price > 0 && t.default_currency.is_none() => {
                errors.push(format!(
                    "액터 {actor}: 기능 {i}: 유료 치료는 default_currency 가 있어야 함"
                ));
            }
            ServiceFile::Craft(id) if !t.crafts.contains_key(&id) => {
                errors.push(format!("액터 {actor}: 기능 {i}: 없는 제작 표 {id}"));
            }
            ServiceFile::Teleport(id) if !t.teleports.contains_key(&id) => {
                errors.push(format!("액터 {actor}: 기능 {i}: 없는 텔레포트 표 {id}"));
            }
            _ => {}
        }
        if services[..i].contains(s) {
            errors.push(format!("액터 {actor}: 기능 {i}: 같은 기능이 두 번 있음"));
        }
    }
    errors
}

/// 기본 재화로 적은 값.
pub(crate) fn price(default_currency: Option<u32>, amount: u32) -> Price {
    Price {
        currency: CurrencyId(default_currency.unwrap_or(0)),
        amount,
    }
}

pub(crate) fn shop(def: &ShopFile, default_currency: Option<u32>) -> ShopDef {
    ShopDef {
        goods: def
            .goods
            .iter()
            .map(|g| Goods {
                item: ItemId(g.item),
                price: Price {
                    currency: CurrencyId(if g.currency == 0 {
                        default_currency.unwrap_or(0)
                    } else {
                        g.currency
                    }),
                    amount: g.price,
                },
            })
            .collect(),
        buy_rate_per_mille: def.buy_rate,
    }
}

pub(crate) fn services(list: &[ServiceFile], default_currency: Option<u32>) -> Vec<Service> {
    list.iter()
        .map(|s| match *s {
            ServiceFile::Shop(id) => Service::Shop(ShopId(id)),
            ServiceFile::Heal { price: p } => Service::Heal {
                price: price(default_currency, p),
            },
            ServiceFile::Craft(id) => Service::Craft(CraftId(id)),
            ServiceFile::Teleport(id) => Service::Teleport(TeleportId(id)),
            ServiceFile::Storage => Service::Storage,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mistakes_are_named() {
        let shops: BTreeMap<u32, ShopFile> = ron::from_str(
            "{ 1: (goods: [(item: 7, price: 3), (item: 501, price: 3, currency: 9)], buy_rate: 2000) }",
        )
        .unwrap();
        let none = BTreeMap::new();
        let t = TradeRefs {
            currencies: &[1, 1, 0],
            default_currency: Some(4),
            shops: &shops,
            crafts: &none,
            teleports: &BTreeMap::new(),
        };
        let mut e = validate(&t, true, |i| i == 501);
        e.extend(validate_services(
            11,
            &[ServiceFile::Shop(2), ServiceFile::Shop(2)],
            &t,
        ));
        for want in [
            "currencies: 0 은",
            "currencies: 1 이(가) 두 번",
            "default_currency: 4 은(는)",
            "상점 1: buy_rate",
            "상점 1: 물건 0: 없는 아이템 7",
            "상점 1: 물건 1: 없는 재화 9",
            "액터 11: 기능 0: 없는 상점 2",
            "액터 11: 기능 1: 같은 기능이 두 번",
        ] {
            assert!(
                e.iter().any(|m| m.starts_with(want)),
                "{want} 없음 — {e:#?}"
            );
        }
    }

    #[test]
    fn plain_numbers_use_the_default_currency() {
        let shops: BTreeMap<u32, ShopFile> =
            ron::from_str("{ 1: (goods: [(item: 501, price: 30)]) }").unwrap();
        let def = shop(&shops[&1], Some(1));
        assert_eq!(def.goods[0].price, price(Some(1), 30));
        assert_eq!(def.buy_rate_per_mille, 500, "적지 않으면 반값");
        let list: Vec<ServiceFile> = ron::from_str("[Shop(1), Heal(price: 20), Heal()]").unwrap();
        assert_eq!(
            services(&list, Some(1))[1],
            Service::Heal {
                price: price(Some(1), 20)
            }
        );
    }
}
