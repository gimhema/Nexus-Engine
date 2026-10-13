//! 버튼이 부르는 UI 스크립트 (P17) — 읽기·컴파일 캐시.
//!
//! 스크립트는 **누를 때마다 디스크에서 다시 읽는다** (디스크 우선, 없으면 내장본 — 데이터 파일과 같은 규칙).
//! 본문이 그대로면 컴파일한 것과 상태(`this`)를 이어 쓰고, 바뀌었으면 새로 컴파일한다 — 스크립트 편집기에서
//! 고치고 저장하면 다시 시작하지 않아도 다음 클릭부터 적용된다. 실행 규칙은 `nexus_script::UiScript` 에 있다.

use std::collections::HashMap;

use nexus_script::{UiCommand, UiScript, UiView};

use crate::game_data::read_script;

/// 경로 → (읽은 본문, 컴파일한 스크립트).
#[derive(Debug, Default)]
pub(crate) struct UiScripts {
    cache: HashMap<String, (String, UiScript)>,
}

impl UiScripts {
    /// `path`(`data/` 기준) 의 `call()` 을 부르고 낸 명령을 돌려준다. 오류는 한국어 한 줄로.
    pub(crate) fn run(
        &mut self,
        path: &str,
        call: &str,
        view: UiView,
    ) -> Result<Vec<UiCommand>, String> {
        let source = read_script(path)?.ok_or_else(|| format!("스크립트 data/{path} 가 없음"))?;
        let fresh = self.cache.get(path).is_none_or(|(old, _)| *old != source);
        if fresh {
            let script = UiScript::compile(path, &source)
                .map_err(|d| format!("{path}: 컴파일 오류 — {d}"))?;
            self.cache.insert(path.to_owned(), (source, script));
        }
        let (_, script) = self.cache.get_mut(path).expect("방금 넣었거나 이미 있다");
        script.call(call, view)
    }

    /// 플레이를 멈추거나 레벨이 바뀔 때 — 상태(`this`)를 비운다.
    pub(crate) fn reset(&mut self) {
        self.cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(hp: u32, potions: u32) -> UiView {
        UiView {
            playing: true,
            hp,
            max_hp: 200,
            items: vec![(501, potions)],
            ..UiView::default()
        }
    }

    #[test]
    fn the_shipped_pause_button_drinks_a_potion_and_resumes() {
        // 시험은 작업 디렉터리에 data/ 가 없어 내장본을 읽는다 — 디스크가 없을 때의 규칙 그대로.
        let mut ui = UiScripts::default();
        let path = "scripts/ui/pause.rhai";
        assert_eq!(
            ui.run(path, "drink", view(50, 3)).unwrap(),
            [UiCommand::UseItem(501), UiCommand::Resume]
        );
        assert_eq!(
            ui.run(path, "drink", view(200, 3)).unwrap(),
            [UiCommand::Notify(String::from("체력이 가득합니다"))]
        );
        assert_eq!(
            ui.run(path, "drink", view(50, 0)).unwrap(),
            [UiCommand::Notify(String::from("빨간 포션이 없습니다"))]
        );
        assert!(
            ui.run(path, "nope", view(50, 3))
                .unwrap_err()
                .contains("nope()")
        );
        assert!(
            ui.run("scripts/none.rhai", "drink", view(50, 3))
                .unwrap_err()
                .contains("없음")
        );
    }

    #[test]
    fn the_pause_screen_says_when_there_is_no_potion_as_it_opens() {
        // ui/pause.ui.ron 의 on_open → scripts/ui/pause.rhai 의 opened().
        let screen =
            crate::screen::Screens::read_screen(include_str!("../../../ui/pause.ui.ron")).unwrap();
        let hook = screen.on_open.expect("일시정지 화면에 on_open");
        let mut ui = UiScripts::default();
        assert!(
            ui.run(&hook.path, &hook.call, view(50, 3))
                .unwrap()
                .is_empty()
        );
        let said = ui.run(&hook.path, &hook.call, view(50, 0)).unwrap();
        assert!(
            matches!(said.as_slice(), [UiCommand::Notify(t)] if t.contains("포션이 없습니다")),
            "{said:?}"
        );
    }
}
