//! 스탯과 직업 — **기본 스탯 → 수식 → 파생 스탯** (R2).
//!
//! ```text
//! 기본 스탯 (이름·개수는 데이터)       파생 스탯 (엔진이 쓰는 다섯 가지)
//! str = 직업 기본 + 성장 × (lv-1) + 장비   max_hp  = "100 + vit * 10 + level * 5"
//! vit = …                                  attack  = "str * 2 + equip_attack"
//! int = …                       ──수식──▶  defense = "vit + equip_defense"
//!                                          max_mp  = "int * 5"
//!                                          mp_regen= "1 + int / 4"
//! ```
//!
//! 특정 게임의 스탯 체계를 따르지 않는다 *(사용자 결정 2026-10-24)*. 기본 스탯의 이름·개수와
//! 파생 스탯의 수식을 **데이터가 정한다.** 엔진이 아는 것은 파생 스탯 다섯 가지뿐이고, 그 값을
//! 서버와 같은 전투 공식(`combat`)이 그대로 쓴다.
//!
//! # 수식 문법
//!
//! 정수 연산만 한다 — 같은 입력이면 어느 플랫폼에서나 같은 값이다(서버와 갈라지지 않게).
//!
//! | | |
//! |---|---|
//! | 수 | 정수 리터럴 (`150`). 소수는 없다 — 1.5배는 `x * 3 / 2` |
//! | 연산 | `+ - * / %`, 단항 `-`, 괄호. 우선순위는 보통 산수와 같다 |
//! | 나눗셈 | 0 쪽으로 버린다 (C++ 서버와 같다). **0 으로 나누면 0** |
//! | 함수 | `min(a, b)` · `max(a, b)` · `clamp(x, lo, hi)` |
//! | 이름 | 기본 스탯 이름 + [`RESERVED`] (`level`, `equip_attack`, `equip_defense`) |
//!
//! 넘치면 끝에서 멈춘다(포화 연산). 결과는 0 이상으로 자르고, 최대 HP 는 1 이상이다.
//!
//! # 직업
//!
//! 직업은 처음에 고르고 바뀌지 않는다 *(사용자 결정)*. 직업이 정하는 것은 기본 스탯의
//! 출발값·레벨당 성장, 그리고 필요하면 **파생 수식 덮어쓰기**(마법사만 다른 MP 공식 등)다.
//! 직업이 없는 유닛(대부분의 몬스터·NPC)은 지금처럼 `UnitDef` 의 수치를 그대로 쓴다.

use std::fmt;

/// 수식이 기본 스탯 말고 더 읽을 수 있는 이름. 이 순서로 기본 스탯 **뒤에** 붙는다.
pub const RESERVED: [&str; 3] = ["level", "equip_attack", "equip_defense"];

/// 기본 스탯 개수 상한 — 손으로 쓰는 표에서 이보다 많으면 실수다.
pub const MAX_STATS: usize = 32;

/// 수식 하나 — 이름을 미리 번호로 바꿔 둔 식 나무.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Formula {
    root: Node,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Node {
    Num(i64),
    Var(usize),
    Neg(Box<Node>),
    Bin(Op, Box<Node>, Box<Node>),
    Call(Func, Vec<Node>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Func {
    Min,
    Max,
    Clamp,
}

impl Func {
    fn arity(self) -> usize {
        match self {
            Self::Min | Self::Max => 2,
            Self::Clamp => 3,
        }
    }
}

/// 수식을 읽지 못한 이유 — `at` 은 글자 위치(0부터).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormulaError {
    pub at: usize,
    pub message: String,
}

impl fmt::Display for FormulaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}번째 글자: {}", self.at + 1, self.message)
    }
}

impl Formula {
    /// 늘 같은 값.
    #[must_use]
    pub fn constant(value: i64) -> Self {
        Self {
            root: Node::Num(value),
        }
    }

    /// `vars` 는 쓸 수 있는 이름 — 보통 [`StatRules::variables`].
    ///
    /// # Errors
    /// 문법이 틀렸거나 모르는 이름을 쓰면 그 위치와 이유.
    pub fn parse(src: &str, vars: &[String]) -> Result<Self, FormulaError> {
        let tokens = lex(src)?;
        let mut p = Parser {
            tokens: &tokens,
            pos: 0,
            vars,
            end: src.chars().count(),
        };
        let root = p.expr()?;
        if let Some(t) = p.peek() {
            return Err(FormulaError {
                at: t.at,
                message: String::from("식이 여기서 끝나야 함"),
            });
        }
        Ok(Self { root })
    }

    /// 값을 계산한다. `vals[i]` 가 `i` 번 이름의 값이다 (모자라면 0).
    #[must_use]
    pub fn eval(&self, vals: &[i64]) -> i64 {
        eval(&self.root, vals)
    }
}

fn eval(n: &Node, vals: &[i64]) -> i64 {
    match n {
        Node::Num(v) => *v,
        Node::Var(i) => vals.get(*i).copied().unwrap_or(0),
        Node::Neg(a) => eval(a, vals).saturating_neg(),
        Node::Bin(op, a, b) => {
            let (a, b) = (eval(a, vals), eval(b, vals));
            match op {
                Op::Add => a.saturating_add(b),
                Op::Sub => a.saturating_sub(b),
                Op::Mul => a.saturating_mul(b),
                // i64::MIN / -1 도 넘친다 — checked 로 0 이나 끝값으로.
                Op::Div => a
                    .checked_div(b)
                    .unwrap_or(if b == 0 { 0 } else { i64::MAX }),
                Op::Rem => a.checked_rem(b).unwrap_or(0),
            }
        }
        Node::Call(f, args) => {
            let v: Vec<i64> = args.iter().map(|a| eval(a, vals)).collect();
            match f {
                Func::Min => v[0].min(v[1]),
                Func::Max => v[0].max(v[1]),
                // lo > hi 면 lo 가 이긴다 — panic 하지 않는다.
                Func::Clamp => v[0].min(v[2]).max(v[1]),
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(i64),
    Ident(String),
    Sym(char),
}

#[derive(Clone, Debug)]
struct Token {
    tok: Tok,
    at: usize,
}

fn lex(src: &str) -> Result<Vec<Token>, FormulaError> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let at = i;
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() {
            let mut v: i64 = 0;
            while i < chars.len() && chars[i].is_ascii_digit() {
                let d = i64::from(chars[i].to_digit(10).unwrap_or(0));
                v = v
                    .checked_mul(10)
                    .and_then(|v| v.checked_add(d))
                    .ok_or(FormulaError {
                        at,
                        message: String::from("수가 너무 큼"),
                    })?;
                i += 1;
            }
            out.push(Token {
                tok: Tok::Num(v),
                at,
            });
        } else if c.is_ascii_alphabetic() || c == '_' {
            let mut s = String::new();
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                s.push(chars[i]);
                i += 1;
            }
            out.push(Token {
                tok: Tok::Ident(s),
                at,
            });
        } else if "+-*/%(),".contains(c) {
            out.push(Token {
                tok: Tok::Sym(c),
                at,
            });
            i += 1;
        } else {
            return Err(FormulaError {
                at,
                message: format!("쓸 수 없는 글자 '{c}' (소수점은 없다 — 1.5배는 x * 3 / 2)"),
            });
        }
    }
    Ok(out)
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    vars: &'a [String],
    end: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn here(&self) -> usize {
        self.peek().map_or(self.end, |t| t.at)
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek().is_some_and(|t| t.tok == Tok::Sym(c)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, c: char) -> Result<(), FormulaError> {
        if self.eat(c) {
            Ok(())
        } else {
            Err(FormulaError {
                at: self.here(),
                message: format!("'{c}' 가 있어야 함"),
            })
        }
    }

    // expr := term (('+'|'-') term)*
    fn expr(&mut self) -> Result<Node, FormulaError> {
        let mut left = self.term()?;
        loop {
            let op = if self.eat('+') {
                Op::Add
            } else if self.eat('-') {
                Op::Sub
            } else {
                return Ok(left);
            };
            left = Node::Bin(op, Box::new(left), Box::new(self.term()?));
        }
    }

    // term := unary (('*'|'/'|'%') unary)*
    fn term(&mut self) -> Result<Node, FormulaError> {
        let mut left = self.unary()?;
        loop {
            let op = if self.eat('*') {
                Op::Mul
            } else if self.eat('/') {
                Op::Div
            } else if self.eat('%') {
                Op::Rem
            } else {
                return Ok(left);
            };
            left = Node::Bin(op, Box::new(left), Box::new(self.unary()?));
        }
    }

    fn unary(&mut self) -> Result<Node, FormulaError> {
        if self.eat('-') {
            return Ok(Node::Neg(Box::new(self.unary()?)));
        }
        self.atom()
    }

    fn atom(&mut self) -> Result<Node, FormulaError> {
        let at = self.here();
        let Some(t) = self.peek().cloned() else {
            return Err(FormulaError {
                at,
                message: String::from("식이 비어 있거나 중간에 끝남"),
            });
        };
        self.pos += 1;
        match t.tok {
            Tok::Num(v) => Ok(Node::Num(v)),
            Tok::Sym('(') => {
                let inner = self.expr()?;
                self.expect(')')?;
                Ok(inner)
            }
            Tok::Ident(name) => {
                let func = match name.as_str() {
                    "min" => Some(Func::Min),
                    "max" => Some(Func::Max),
                    "clamp" => Some(Func::Clamp),
                    _ => None,
                };
                if let Some(func) = func {
                    self.expect('(')?;
                    let mut args = vec![self.expr()?];
                    while self.eat(',') {
                        args.push(self.expr()?);
                    }
                    self.expect(')')?;
                    if args.len() != func.arity() {
                        return Err(FormulaError {
                            at,
                            message: format!("{name} 은(는) 인자 {}개", func.arity()),
                        });
                    }
                    return Ok(Node::Call(func, args));
                }
                match self.vars.iter().position(|v| *v == name) {
                    Some(i) => Ok(Node::Var(i)),
                    None => Err(FormulaError {
                        at,
                        message: format!(
                            "모르는 이름 '{name}' — 쓸 수 있는 이름: {}",
                            self.vars.join(", ")
                        ),
                    }),
                }
            }
            Tok::Sym(c) => Err(FormulaError {
                at,
                message: format!("'{c}' 가 올 자리가 아님"),
            }),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 파생 스탯·직업
// ─────────────────────────────────────────────────────────────────────────────

/// 엔진이 쓰는 파생 스탯. 전투·MP·HUD 가 이 값을 읽는다.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DerivedStat {
    MaxHp,
    MaxMp,
    MpRegen,
    Attack,
    Defense,
}

impl DerivedStat {
    pub const ALL: [Self; 5] = [
        Self::MaxHp,
        Self::MaxMp,
        Self::MpRegen,
        Self::Attack,
        Self::Defense,
    ];

    /// 데이터 파일에 쓰는 이름.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::MaxHp => "max_hp",
            Self::MaxMp => "max_mp",
            Self::MpRegen => "mp_regen",
            Self::Attack => "attack",
            Self::Defense => "defense",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// 파생 스탯 값.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Derived {
    pub max_hp: u32,
    pub max_mp: u32,
    pub mp_regen: u32,
    pub attack: u32,
    pub defense: u32,
}

impl Derived {
    #[must_use]
    pub fn get(&self, stat: DerivedStat) -> u32 {
        match stat {
            DerivedStat::MaxHp => self.max_hp,
            DerivedStat::MaxMp => self.max_mp,
            DerivedStat::MpRegen => self.mp_regen,
            DerivedStat::Attack => self.attack,
            DerivedStat::Defense => self.defense,
        }
    }
}

/// 직업 번호. 서버와 **같은 ID 공간**이다.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ClassId(pub u32);

/// 직업 하나. 벡터는 [`StatRules::stats`] 와 **같은 순서·같은 길이**다 (모자라면 0).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClassDef {
    /// 레벨 1 의 기본 스탯.
    pub base: Vec<i64>,
    /// 레벨이 하나 오를 때마다 더해지는 기본 스탯.
    pub growth: Vec<i64>,
    /// 이 직업만 다른 파생 수식. 없는 것은 공통 수식을 쓴다.
    pub formulas: Vec<(DerivedStat, Formula)>,
}

/// 스탯 체계 — 기본 스탯 이름과 공통 파생 수식.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatRules {
    stats: Vec<String>,
    /// [`DerivedStat::ALL`] 순서.
    formulas: [Formula; 5],
}

/// 장비가 기본 스탯에 더하는 값과 입을 수 있는 직업. 없으면 더하는 것도 제한도 없다.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EquipRules {
    /// `(기본 스탯 번호, 더하는 값)`.
    pub stats: Vec<(usize, i64)>,
    /// 비어 있으면 누구나. 직업이 없는 유닛은 제한 있는 장비를 입지 못한다.
    pub classes: Vec<ClassId>,
}

impl EquipRules {
    /// `class` 가 입을 수 있는가.
    #[must_use]
    pub fn allows(&self, class: Option<ClassId>) -> bool {
        self.classes.is_empty() || class.is_some_and(|c| self.classes.contains(&c))
    }
}

/// 장비에서 모은 값 — 계산 입력.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EquipTotals {
    pub attack: u32,
    pub defense: u32,
    /// 기본 스탯 번호별 합 (모자라면 0).
    pub stats: Vec<i64>,
}

/// 계산 결과 — 기본 스탯 최종값과 파생 스탯.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatSheet {
    /// [`StatRules::stats`] 순서.
    pub base: Vec<i64>,
    pub derived: Derived,
}

impl StatRules {
    /// `formulas` 는 [`DerivedStat::ALL`] 순서로 이미 읽은 수식이다 ([`Self::variables`] 로 읽을 것).
    #[must_use]
    pub fn new(stats: Vec<String>, formulas: [Formula; 5]) -> Self {
        Self { stats, formulas }
    }

    /// 기본 스탯 이름 — 데이터가 정한 순서.
    #[must_use]
    pub fn stats(&self) -> &[String] {
        &self.stats
    }

    /// 수식이 쓸 수 있는 이름 — 기본 스탯 + [`RESERVED`].
    #[must_use]
    pub fn variables(stats: &[String]) -> Vec<String> {
        stats
            .iter()
            .cloned()
            .chain(RESERVED.iter().map(|s| (*s).to_owned()))
            .collect()
    }

    #[must_use]
    pub fn formula(&self, stat: DerivedStat) -> &Formula {
        &self.formulas[stat.index()]
    }

    /// `class` 의 `level` 에서의 스탯. 스탯 디자이너의 미리보기도 이것을 쓴다 — 게임과 같은 계산.
    #[must_use]
    pub fn sheet(&self, class: &ClassDef, level: u32, equip: &EquipTotals) -> StatSheet {
        let n = self.stats.len();
        let grown = i64::from(level.max(1) - 1);
        let at = |v: &[i64], i: usize| v.get(i).copied().unwrap_or(0);
        let base: Vec<i64> = (0..n)
            .map(|i| {
                at(&class.base, i)
                    .saturating_add(at(&class.growth, i).saturating_mul(grown))
                    .saturating_add(at(&equip.stats, i))
            })
            .collect();
        let mut vals = base.clone();
        vals.extend([
            i64::from(level.max(1)),
            i64::from(equip.attack),
            i64::from(equip.defense),
        ]);
        let value = |stat: DerivedStat| {
            let f = class
                .formulas
                .iter()
                .find(|(s, _)| *s == stat)
                .map_or(self.formula(stat), |(_, f)| f);
            u32::try_from(f.eval(&vals).max(0)).unwrap_or(u32::MAX)
        };
        StatSheet {
            base,
            derived: Derived {
                max_hp: value(DerivedStat::MaxHp).max(1),
                max_mp: value(DerivedStat::MaxMp),
                mp_regen: value(DerivedStat::MpRegen),
                attack: value(DerivedStat::Attack),
                defense: value(DerivedStat::Defense),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> Vec<String> {
        StatRules::variables(&["str".into(), "vit".into(), "int".into()])
    }

    fn eval(src: &str, vals: &[i64]) -> i64 {
        Formula::parse(src, &vars()).unwrap().eval(vals)
    }

    #[test]
    fn arithmetic_follows_the_usual_precedence() {
        // str=10, vit=4, int=3, level=5, equip_attack=7, equip_defense=2
        let v = [10, 4, 3, 5, 7, 2];
        assert_eq!(eval("100 + vit * 10 + level * 5", &v), 165);
        assert_eq!(eval("(str + equip_attack) * 3 / 2", &v), 25, "1.5배, 버림");
        assert_eq!(eval("-str + 3", &v), -7);
        assert_eq!(eval("str % 3 - -1", &v), 2);
        assert_eq!(
            eval("min(str, 6) + max(int, 9) + clamp(level, 1, 3)", &v),
            18
        );
    }

    #[test]
    fn division_truncates_toward_zero_and_by_zero_is_zero() {
        assert_eq!(eval("-7 / 2", &[]), -3, "C++ 와 같이 0 쪽으로");
        assert_eq!(eval("7 / (str - str)", &[5]), 0);
        assert_eq!(eval("7 % 0", &[]), 0);
        assert_eq!(
            eval("9223372036854775807 * 2", &[]),
            i64::MAX,
            "넘치면 끝에서 멈춘다"
        );
    }

    #[test]
    fn errors_say_where_and_why() {
        let err = |src: &str| Formula::parse(src, &vars()).unwrap_err();
        let e = err("str * dex");
        assert_eq!(e.at, 6);
        assert!(
            e.message.contains("모르는 이름 'dex'") && e.message.contains("level"),
            "{e}"
        );
        assert!(err("str * 1.5").message.contains("소수점"));
        assert!(err("(str + 1").message.contains("')'"));
        assert!(err("str +").message.contains("끝남"));
        assert!(err("min(str)").message.contains("인자 2개"));
        assert!(err("str str").message.contains("끝나야"));
        assert!(err("").message.contains("비어"));
    }

    fn rules() -> StatRules {
        let stats: Vec<String> = vec!["str".into(), "vit".into(), "int".into()];
        let v = StatRules::variables(&stats);
        let f = |s: &str| Formula::parse(s, &v).unwrap();
        StatRules::new(
            stats,
            [
                f("50 + vit * 10"),
                f("int * 5"),
                f("int / 2"),
                f("str * 2 + equip_attack"),
                f("vit + equip_defense"),
            ],
        )
    }

    #[test]
    fn a_class_grows_its_base_stats_and_the_formulas_follow() {
        let warrior = ClassDef {
            base: vec![8, 6, 1],
            growth: vec![2, 2],
            formulas: Vec::new(),
        };
        let r = rules();
        let lv1 = r.sheet(&warrior, 1, &EquipTotals::default());
        assert_eq!(lv1.base, [8, 6, 1]);
        assert_eq!(
            lv1.derived,
            Derived {
                max_hp: 110,
                max_mp: 5,
                mp_regen: 0,
                attack: 16,
                defense: 6
            }
        );
        let equip = EquipTotals {
            attack: 12,
            defense: 4,
            stats: vec![3],
        };
        let lv5 = r.sheet(&warrior, 5, &equip);
        assert_eq!(
            lv5.base,
            [8 + 8 + 3, 6 + 8, 1],
            "성장 ×4 + 장비 str +3, int 성장 없음"
        );
        assert_eq!(
            (lv5.derived.attack, lv5.derived.defense),
            (19 * 2 + 12, 14 + 4)
        );
    }

    #[test]
    fn a_class_can_override_one_formula_and_results_are_clamped() {
        let v = StatRules::variables(rules().stats());
        let mage = ClassDef {
            base: vec![1, 1, 10],
            growth: Vec::new(),
            formulas: vec![
                (DerivedStat::MaxMp, Formula::parse("int * 20", &v).unwrap()),
                (
                    DerivedStat::Defense,
                    Formula::parse("vit - 100", &v).unwrap(),
                ),
                (DerivedStat::MaxHp, Formula::constant(-5)),
            ],
        };
        let d = rules().sheet(&mage, 1, &EquipTotals::default()).derived;
        assert_eq!(d.max_mp, 200, "직업 수식");
        assert_eq!(d.mp_regen, 5, "공통 수식");
        assert_eq!(d.defense, 0, "음수는 0");
        assert_eq!(d.max_hp, 1, "최대 HP 는 1 이상");
    }

    // ── 월드에서 ─────────────────────────────────────────────────────────────

    mod in_world {
        use std::time::Duration;

        use nexus_core::{Entity, Vec2};

        use super::super::*;
        use super::rules;
        use crate::{
            Authority, BagKind, EquipSlot, Event, Intent, ItemDef, ItemId, ItemKind,
            LocalAuthority, Progress, Rejection, SimWorld, SkillDef, SkillId, Tile, TileMap,
            UnitDef,
        };

        const DT: Duration = Duration::from_millis(50);
        const WARRIOR: ClassId = ClassId(1);
        const MAGE: ClassId = ClassId(2);
        const SWORD: ItemId = ItemId(1101);
        const STAFF: ItemId = ItemId(1201);

        fn world() -> (LocalAuthority, Entity) {
            let mut w = SimWorld::new(TileMap::new(20, 20, 1.0, Vec2::ZERO, Tile::default()));
            w.set_stat_rules(rules());
            w.define_class(
                WARRIOR,
                ClassDef {
                    base: vec![8, 6, 1],
                    growth: vec![2, 2, 0],
                    formulas: Vec::new(),
                },
            );
            w.define_class(MAGE, ClassDef::default());
            for (id, attack) in [(SWORD, 12), (STAFF, 3)] {
                w.define_item(
                    id,
                    ItemDef {
                        kind: ItemKind::Equipment {
                            slot: EquipSlot::Weapon,
                            attack,
                            defense: 0,
                        },
                        max_stack: 1,
                    },
                );
            }
            // 칼은 힘 +3, 지팡이는 마법사만.
            w.define_equip_rules(
                SWORD,
                EquipRules {
                    stats: vec![(0, 3)],
                    classes: Vec::new(),
                },
            );
            w.define_equip_rules(
                STAFF,
                EquipRules {
                    stats: Vec::new(),
                    classes: vec![MAGE],
                },
            );
            w.define_skill(
                SkillId(1),
                SkillDef {
                    range: 3.0,
                    cooldown_ms: 0,
                    damage_mult: 1.0,
                    mp_cost: 0,
                    radius: 0.0,
                    heal: 0,
                },
            );
            let hero = w.spawn_unit(
                Vec2::new(5.5, 5.5),
                0.0,
                UnitDef {
                    class: Some(WARRIOR),
                    // 직업이 있는 유닛은 배운 스킬과 기본 공격만 쓴다 (R3).
                    basic_attack: Some(SkillId(1)),
                    // 직업이 있으면 이 수치는 쓰이지 않는다.
                    max_hp: 9_999,
                    attack: 9_999,
                    ..UnitDef::default()
                },
            );
            (LocalAuthority::new(w), hero)
        }

        #[test]
        fn a_class_unit_spawns_full_with_formula_stats() {
            let (auth, hero) = world();
            let u = auth.world().unit(hero).unwrap();
            assert_eq!(
                (u.max_hp(), u.hp(), u.attack(), u.defense()),
                (110, 110, 16, 6)
            );
            assert_eq!((u.max_mp(), u.mp(), u.mp_regen()), (5, 5, 0));
            assert_eq!(u.stat_sheet().unwrap().base, [8, 6, 1]);
        }

        #[test]
        fn leveling_up_recomputes_and_refills() {
            let (mut auth, hero) = world();
            let victim = auth.world_mut().spawn_unit(
                Vec2::new(6.5, 5.5),
                0.0,
                UnitDef {
                    exp_reward: 100,
                    ..UnitDef::default()
                },
            );
            auth.submit(Intent::Attack {
                unit: hero,
                target: victim,
                skill: SkillId(1),
            });
            let events = auth.tick(DT);
            assert!(
                events
                    .iter()
                    .any(|e| matches!(e, Event::LeveledUp { level: 2, .. }))
            );
            let u = auth.world().unit(hero).unwrap();
            // 레벨 2: str 10, vit 8 → HP 130, 공격 20, 방어 8.
            assert_eq!(
                (u.max_hp(), u.hp(), u.attack(), u.defense()),
                (130, 130, 20, 8)
            );
        }

        #[test]
        fn equipment_adds_to_base_stats_and_some_is_class_only() {
            let (mut auth, hero) = world();
            let w = auth.world_mut();
            assert!(w.give_item(hero, SWORD, 1));
            assert!(w.give_item(hero, STAFF, 1));
            let slot_of = |auth: &LocalAuthority, item| {
                let bag = auth
                    .world()
                    .unit(hero)
                    .unwrap()
                    .inventory()
                    .bag(BagKind::Equipment);
                bag.iter().find(|(_, s)| s.item == item).unwrap().0 as u16
            };
            let staff = slot_of(&auth, STAFF);
            auth.submit(Intent::Equip {
                unit: hero,
                slot: staff,
            });
            let events = auth.tick(DT);
            assert!(events.iter().any(|e| matches!(
                e,
                Event::Rejected {
                    reason: Rejection::ClassRestricted,
                    ..
                }
            )));
            let sword = slot_of(&auth, SWORD);
            auth.submit(Intent::Equip {
                unit: hero,
                slot: sword,
            });
            auth.tick(DT);
            let u = auth.world().unit(hero).unwrap();
            assert_eq!(u.stat_sheet().unwrap().base[0], 11, "힘 8 + 칼 3");
            assert_eq!(u.attack(), 11 * 2 + 12, "수식이 장비 공격력도 읽는다");
        }

        #[test]
        fn a_saved_level_is_restored_through_the_formulas() {
            let (mut auth, hero) = world();
            auth.world_mut().set_progress(hero, Progress::new(5, 0));
            let u = auth.world().unit(hero).unwrap();
            assert_eq!(u.max_hp(), 50 + 14 * 10, "vit 6 + 2×4");
            assert_eq!(
                u.hp(),
                110,
                "되살릴 때 HP 는 채우지 않는다 (저장한 HP 를 따로 심는다)"
            );
        }

        #[test]
        fn units_without_a_class_keep_their_flat_numbers() {
            let (mut auth, _) = world();
            let slime = auth.world_mut().spawn_unit(
                Vec2::ZERO,
                0.0,
                UnitDef {
                    max_hp: 80,
                    attack: 18,
                    mp_regen: 2,
                    ..UnitDef::default()
                },
            );
            let u = auth.world().unit(slime).unwrap();
            assert!(u.stat_sheet().is_none());
            assert_eq!((u.max_hp(), u.attack(), u.mp_regen()), (80, 18, 2));
        }
    }

    #[test]
    fn restricted_equipment_needs_a_listed_class() {
        let r = EquipRules {
            stats: Vec::new(),
            classes: vec![ClassId(2)],
        };
        assert!(r.allows(Some(ClassId(2))));
        assert!(!r.allows(Some(ClassId(1))));
        assert!(!r.allows(None), "직업 없는 유닛은 제한 장비를 못 입는다");
        assert!(EquipRules::default().allows(None));
    }
}
