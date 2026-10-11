//! UI 스크립트 — 화면 버튼이 부르는 Rhai 함수 (단계 2 P17).
//!
//! 액터 스크립트([`RhaiHost`](crate::RhaiHost))와 **따로** 돈다: 시뮬레이션 tick 이 아니라 버튼을 누를 때
//! 한 번 불리고, 플레이 중이 아니어도(메인 메뉴) 동작한다. 할 수 있는 일은 **명령 목록**([`UiCommand`])으로
//! 돌려줄 뿐이다 — 화면을 열고 닫고 레벨을 여는 것은 부른 쪽(앱)이 기존 버튼 동작과 같은 경로로 한다.
//! 게임에 손대는 것(`use_item`)도 앱이 플레이어 Intent 로 바꿔 낸다 — 월드를 직접 고치지 않는다.
//!
//! ```text
//! // data/scripts/ui/pause.rhai — 버튼 action: Script(path: "scripts/ui/pause.rhai", call: "drink")
//! fn drink() {
//!     if item_count(501) == 0 { notify("빨간 포션이 없습니다"); return; }
//!     use_item(501);
//!     resume();
//! }
//! ```
//!
//! | 명령 | |
//! |---|---|
//! | `open_screen(이름)` · `close()` · `open_level(이름)` · `open_start_level()` · `resume()` · `quit()` | 버튼 고정 동작과 같다 |
//! | `notify(글)` · `print(글)` | 상태 바 알림 |
//! | `use_item(아이템번호)` | 플레이어가 그 소모품을 하나 쓴다 (플레이 중일 때만) |
//!
//! | 읽기 | |
//! |---|---|
//! | `playing()` | 플레이 중인가 |
//! | `hp()` `max_hp()` `mp()` `max_mp()` `level()` | 플레이어 값 (플레이 중이 아니면 0) |
//! | `item_count(아이템번호)` | 가방에 든 개수 |
//! | `level_id()` | 지금 레벨 이름 (`main`·`village` …) |
//!
//! 상태는 `this` 에 둔다 — 스크립트 파일마다 하나이고, 스크립트를 다시 읽으면 비워진다.
//! 불리는 함수는 **인자가 없어야** 한다.

use std::cell::RefCell;
use std::rc::Rc;

use rhai::{AST, CallFnOptions, Dynamic, Engine, INT, Map, Scope};

use crate::{Diagnostic, base_engine, compile};

/// 버튼 스크립트가 낸 일 — 부른 쪽이 차례대로 실행한다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiCommand {
    OpenScreen(String),
    Close,
    OpenLevel(String),
    OpenStartLevel,
    Resume,
    Quit,
    /// 상태 바에 알린다 (`notify` · `print`).
    Notify(String),
    /// 플레이어가 그 소모품을 하나 쓴다.
    UseItem(u32),
}

/// 스크립트가 읽는 값 — 부를 때마다 부른 쪽이 채운다.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UiView {
    pub playing: bool,
    pub hp: u32,
    pub max_hp: u32,
    pub mp: u32,
    pub max_mp: u32,
    pub level: u32,
    /// 가방 내용 — `(아이템 번호, 개수)`. 같은 아이템이 여러 칸이면 여러 줄이어도 된다.
    pub items: Vec<(u32, u32)>,
    /// 지금 레벨 이름.
    pub level_id: String,
}

#[derive(Default)]
struct UiCtx {
    view: UiView,
    out: Vec<UiCommand>,
}

/// 컴파일한 UI 스크립트 하나. 상태(`this`)를 부를 때마다 이어 간다.
pub struct UiScript {
    name: String,
    engine: Engine,
    ast: AST,
    state: Dynamic,
    ctx: Rc<RefCell<UiCtx>>,
}

impl std::fmt::Debug for UiScript {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiScript")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl UiScript {
    /// 컴파일한다 — 액터 스크립트와 같은 규칙(제한·strict 변수)으로.
    pub fn compile(name: &str, source: &str) -> Result<Self, Diagnostic> {
        let ctx = Rc::new(RefCell::new(UiCtx::default()));
        let engine = ui_engine(&ctx);
        let (ast, _) = compile(&engine, source)?;
        Ok(Self {
            name: name.to_owned(),
            engine,
            ast,
            state: Dynamic::from_map(Map::new()),
            ctx,
        })
    }

    /// 인자 없는 함수 `func` 가 있는가 — 버튼이 부를 수 있는 함수.
    #[must_use]
    pub fn has(&self, func: &str) -> bool {
        self.ast
            .iter_functions()
            .any(|f| f.name == func && f.params.is_empty())
    }

    /// 인자 없는 함수 이름들 — 위젯 편집기의 고르기 목록.
    #[must_use]
    pub fn functions(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .ast
            .iter_functions()
            .filter(|f| f.params.is_empty())
            .map(|f| f.name.to_owned())
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// `func()` 를 부르고 낸 명령을 돌려준다. 오류가 나면 **그때까지 낸 명령은 버린다** —
    /// 반쯤 실행된 버튼(레벨은 열고 알림은 안 하는 등)을 만들지 않게.
    pub fn call(&mut self, func: &str, view: UiView) -> Result<Vec<UiCommand>, String> {
        if !self.has(func) {
            return Err(format!("{}: 인자 없는 함수 {func}() 가 없음", self.name));
        }
        {
            let mut c = self.ctx.borrow_mut();
            c.view = view;
            c.out.clear();
        }
        let options = CallFnOptions::new()
            .eval_ast(false)
            .bind_this_ptr(&mut self.state);
        let result = self.engine.call_fn_with_options::<Dynamic>(
            options,
            &mut Scope::new(),
            &self.ast,
            func,
            (),
        );
        let out = std::mem::take(&mut self.ctx.borrow_mut().out);
        match result {
            Ok(_) => Ok(out),
            Err(e) => Err(format!("{}: {func}() 오류 — {e}", self.name)),
        }
    }
}

/// UI 스크립트 엔진 — 화면 명령과 읽기 함수만 등록한다 (월드·파일 접근 없음).
fn ui_engine(ctx: &Rc<RefCell<UiCtx>>) -> Engine {
    let mut engine = base_engine();
    {
        let ctx = ctx.clone();
        engine.on_print(move |s| {
            ctx.borrow_mut().out.push(UiCommand::Notify(s.to_owned()));
        });
    }

    // ── 명령 ─────────────────────────────────────────────────────────────
    macro_rules! command {
        ($name:literal, || $cmd:expr) => {{
            let ctx = ctx.clone();
            engine.register_fn($name, move || ctx.borrow_mut().out.push($cmd));
        }};
        ($name:literal, |$arg:ident: &str| $cmd:expr) => {{
            let ctx = ctx.clone();
            engine.register_fn($name, move |$arg: &str| ctx.borrow_mut().out.push($cmd));
        }};
    }
    command!("open_screen", |id: &str| UiCommand::OpenScreen(
        id.to_owned()
    ));
    command!("open_level", |id: &str| UiCommand::OpenLevel(id.to_owned()));
    command!("notify", |text: &str| UiCommand::Notify(text.to_owned()));
    command!("close", || UiCommand::Close);
    command!("open_start_level", || UiCommand::OpenStartLevel);
    command!("resume", || UiCommand::Resume);
    command!("quit", || UiCommand::Quit);
    {
        let ctx = ctx.clone();
        engine.register_fn(
            "use_item",
            move |item: INT| -> Result<(), Box<rhai::EvalAltResult>> {
                let item = u32::try_from(item)
                    .map_err(|_| format!("use_item — 아이템 번호 {item} 가 범위 밖"))?;
                ctx.borrow_mut().out.push(UiCommand::UseItem(item));
                Ok(())
            },
        );
    }

    // ── 읽기 ─────────────────────────────────────────────────────────────
    macro_rules! read {
        ($name:literal, |$v:ident| $body:expr) => {{
            let ctx = ctx.clone();
            engine.register_fn($name, move || {
                let $v = &ctx.borrow().view;
                $body
            });
        }};
    }
    read!("playing", |v| v.playing);
    read!("hp", |v| INT::from(v.hp));
    read!("max_hp", |v| INT::from(v.max_hp));
    read!("mp", |v| INT::from(v.mp));
    read!("max_mp", |v| INT::from(v.max_mp));
    read!("level", |v| INT::from(v.level));
    read!("level_id", |v| v.level_id.clone());
    {
        let ctx = ctx.clone();
        engine.register_fn("item_count", move |item: INT| -> INT {
            let c = ctx.borrow();
            c.view
                .items
                .iter()
                .filter(|(id, _)| INT::from(*id) == item)
                .map(|(_, n)| INT::from(*n))
                .sum()
        });
    }
    engine
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> UiView {
        UiView {
            playing: true,
            hp: 50,
            max_hp: 200,
            items: vec![(501, 2), (501, 1), (909, 4)],
            level_id: String::from("village"),
            ..UiView::default()
        }
    }

    #[test]
    fn a_button_function_returns_commands_in_order() {
        let src = r#"
            fn drink() {
                if item_count(501) == 0 { notify("없음"); return; }
                use_item(501);
                print(`HP ${hp()}/${max_hp()} · ${level_id()}`);
                resume();
            }
        "#;
        let mut s = UiScript::compile("pause.rhai", src).unwrap();
        assert_eq!(
            s.call("drink", view()).unwrap(),
            [
                UiCommand::UseItem(501),
                UiCommand::Notify(String::from("HP 50/200 · village")),
                UiCommand::Resume,
            ]
        );
        let empty = UiView {
            items: Vec::new(),
            ..view()
        };
        assert_eq!(
            s.call("drink", empty).unwrap(),
            [UiCommand::Notify(String::from("없음"))]
        );
    }

    #[test]
    fn state_lives_across_clicks() {
        let src = r#"
            fn count() {
                if this.n == () { this.n = 0; }
                this.n += 1;
                notify(`${this.n}`);
            }
        "#;
        let mut s = UiScript::compile("c.rhai", src).unwrap();
        s.call("count", UiView::default()).unwrap();
        assert_eq!(
            s.call("count", UiView::default()).unwrap(),
            [UiCommand::Notify(String::from("2"))]
        );
    }

    #[test]
    fn missing_functions_and_errors_drop_half_done_commands() {
        let src = r#"
            fn takes(x) { }
            fn broken() { open_level("village"); let y = 1 / 0; }
            fn menu() { open_screen("settings"); close(); open_start_level(); quit(); }
        "#;
        let mut s = UiScript::compile("b.rhai", src).unwrap();
        assert!(
            s.call("nope", UiView::default())
                .unwrap_err()
                .contains("nope()")
        );
        assert!(!s.has("takes"), "인자가 있는 함수는 버튼이 부를 수 없다");
        assert_eq!(s.functions(), ["broken", "menu"]);
        let err = s.call("broken", UiView::default()).unwrap_err();
        assert!(err.starts_with("b.rhai: broken() 오류"), "{err}");
        assert_eq!(
            s.call("menu", UiView::default()).unwrap(),
            [
                UiCommand::OpenScreen(String::from("settings")),
                UiCommand::Close,
                UiCommand::OpenStartLevel,
                UiCommand::Quit,
            ]
        );
    }

    #[test]
    fn ui_scripts_cannot_reach_the_world() {
        // 액터 스크립트의 함수(move_to 등)는 등록되지 않았다 — 실행할 때 "함수 없음".
        let mut s = UiScript::compile("x.rhai", "fn walk() { move_to(1, 2, 3); }").unwrap();
        assert!(s.call("walk", UiView::default()).is_err());
        // 컴파일 규칙은 액터 스크립트와 같다 — 선언 안 한 변수는 컴파일 오류.
        assert!(UiScript::compile("x.rhai", "fn walk() { tpyo += 1; }").is_err());
    }
}
