//! 자동 검증용 입력 스크립트 — `NEXUS_SCRIPT`.
//!
//! 스크린샷만으로는 "끄는 중" 상태(박스 선택, 회전 핸들)를 볼 수 없다. 실제 마우스 없이
//! 편집 경로를 그대로 태우기 위해, 스크립트 단계를 **뷰포트 포인터 입력으로 바꿔** 넣는다.
//! UI 가 만든 입력과 같은 `edit::Editing::handle_pointer` 를 지나므로 검증 대상이 같다.
//!
//! 한 프레임에 한 단계씩, `;` 로 구분한다. 좌표는 월드 미터.
//!
//! ```text
//! press 10 10 [shift] [ctrl]   왼쪽 버튼 누름
//! move 30 20 [shift] [ctrl]    포인터 이동 (누른 채면 드래그)
//! release 30 20                왼쪽 버튼 뗌
//! press rotate                 단독 선택 마커의 회전 핸들 위치 (move/release 도 가능)
//! add npc|monster|player X Y   마커 추가
//! tool select|paint|art        뷰포트 포인터가 할 일 바꾸기
//! art 번호 [ground|prop]        지형 그림 붓 (0 = 지우기 — 이때는 층을 적는다)
//! delete | undo | redo | wait
//! play                         플레이 시작/정지 (F5 와 같다). 플레이 중 press 는 클릭 명령이 된다
//! save PATH | open PATH         존 파일 저장 / 열기 (경로에 공백 불가)
//! dialog open|saveas            열기 / 다른 이름으로 저장 창 띄우기
//! palette [그림경로]           지형 팔레트 창 열기
//! pick ground|prop 경로 x y w h [칸]  팔레트에서 그림 조각 고르기 (terrain.ron 자동 추가)
//! brush stroke|rect [반지름]   붓 모양 (반지름 0~3 → 1×1 ~ 7×7)
//! eyedropper                   스포이드 — 다음 클릭한 칸의 그림·타일을 붓으로 집는다
//! scripts [경로]               스크립트 편집기 열기 (경로는 data/ 기준, 예: scripts/goblin.rhai)
//! compile                      스크립트 편집기의 "컴파일" 버튼
//! savegame | deletesave       진행 상황 저장 / 저장 데이터 삭제 (P3)
//! items | panels              HUD 인벤토리 창 / 에디터 패널 숨기기 (P5)
//! uiedit | uiscreen 번호      위젯 편집기 열고 닫기 / 편집할 화면 바꾸기 (P7)
//! uipick 이름 | uidrag dx dy  위젯 고르기 / 고른 위젯 옮기기
//! uisave                      화면 파일(ui/<번호>.ui.ron) 저장
//! uipress x y | uirelease x y 화면 좌표(뷰포트 픽셀)로 누르기 / 떼기 — 버튼 확인용
//! level 번호 | startlevel     레벨 열기 / 시작 레벨 열기 (P7)
//! esc                         일시정지 화면 열고 닫기 (Esc 와 같다)
//! content | contentpick 키    콘텐츠 브라우저 열고 닫기 / 항목 고르기 (P8, 키 = 경로 또는 actor:번호)
//! contentopen                 고른 항목 열기 (두 번 누르기와 같다)
//! contentfolder 경로 | contentview icons|list   브라우저 폴더로 가기 (`/` = 맨 위) / 보기 방식
//! contentmenu                 브라우저 빈 곳 오른쪽 클릭 메뉴 열기 (새로 만들기)
//! contentop delete|duplicate|rename [새이름]  고른 항목에 파일 작업 확인 창 (P9)
//! contentconfirm              확인 창의 "실행"
//! ```
//!
//! 예: `NEXUS_SELECT="상인 NPC" NEXUS_SCRIPT="wait; press rotate; move -8 20 ctrl"`

use std::collections::VecDeque;
use std::path::PathBuf;

use nexus_core::Vec2;

use crate::edit::{BrushShape, Tool};
use crate::scene::{ArtId, ItemKind};
use crate::terrain::{ArtKind, Pick};

pub(crate) const ENV_SCRIPT: &str = "NEXUS_SCRIPT";

/// 포인터 단계의 위치.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Anchor {
    World(Vec2),
    /// 회전 핸들 — 배율(px)에 따라 위치가 달라지므로 실행 시점에 계산한다.
    RotateHandle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Button {
    Press,
    Move,
    Release,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Step {
    Pointer {
        button: Button,
        at: Anchor,
        shift: bool,
        ctrl: bool,
    },
    Add(ItemKind, Vec2),
    SetTool(Tool),
    /// 지형 그림 붓 번호 (`data/terrain.ron`). 0 이면 지우기.
    ///
    /// 층은 번호로 알 수 있으므로 보통 `None` 이다. 지우개는 번호가 없으니 직접 고른다.
    SetArtBrush(ArtId, Option<ArtKind>),
    Delete,
    Undo,
    Redo,
    /// 플레이 시작 / 정지.
    TogglePlay,
    /// 존 파일 저장 / 열기 — 경로는 작업 디렉터리 기준.
    Save(PathBuf),
    Open(PathBuf),
    /// 열기(`false`) / 다른 이름으로 저장(`true`) 창 띄우기.
    Dialog(bool),
    /// 지형 팔레트 창 열기 — 그림 경로를 주면 그 그림을 골라 둔다.
    Palette(Option<String>),
    /// 팔레트에서 그림 조각 고르기 — 창의 마우스 조작 없이 같은 경로(`terrain.ron` 자동 추가)를 탄다.
    Pick(Pick),
    /// 스크립트 편집기 열기 — 경로(`data/` 기준)를 주면 그 파일을 연다.
    Scripts(Option<String>),
    /// 스크립트 편집기의 "컴파일" 버튼.
    Compile,
    /// 진행 상황 저장 (플레이 메뉴의 "게임 저장").
    SaveGame,
    /// 저장 데이터 삭제.
    DeleteSave,
    /// HUD 인벤토리 창 열고 닫기 (I).
    ToggleItems,
    /// 에디터 패널 숨기기 (F9).
    TogglePanels,
    /// 위젯 편집기 열고 닫기 (P7).
    UiEdit,
    /// 편집할 화면 바꾸기.
    UiScreen(String),
    /// 위젯 고르기 — 이름으로.
    UiPick(String),
    /// 고른 위젯을 (dx, dy) UI 픽셀만큼 옮긴다 (끌기와 같은 경로).
    UiDrag(i32, i32),
    /// 화면 좌표(뷰포트 픽셀)로 누르기·떼기 — 버튼을 눌러 보는 데 쓴다.
    ScreenPointer {
        button: Button,
        at: Vec2,
    },
    /// 레벨 열기 (P7) — 언리얼의 Open Level.
    OpenLevel(String),
    /// 시작 레벨 열기 (data/project.ron 의 startup_level).
    StartLevel,
    /// Esc — 일시정지 화면 열고 닫기 (겹친 창이 있으면 그것부터 닫는다).
    Escape,
    /// 콘텐츠 브라우저 열고 닫기 (P8).
    ContentToggle,
    /// 콘텐츠 브라우저에서 항목 고르기 — 경로 또는 `actor:번호`.
    ContentPick(String),
    /// 고른 항목 열기 (두 번 누르기).
    ContentOpen,
    /// 콘텐츠 브라우저의 폴더로 가기 — `/` 는 맨 위.
    ContentFolder(String),
    /// 보기 방식 — `true` = 자세히.
    ContentView(bool),
    /// 브라우저 빈 곳 오른쪽 클릭 메뉴 열기.
    ContentMenu,
    /// 고른 항목에 파일 작업 확인 창 — `(작업, 새 이름)` (P9).
    ContentOp(crate::asset_ops::OpKind, Option<String>),
    /// 확인 창의 "실행".
    ContentConfirm,
    /// 화면 파일 저장 (ui/<번호>.ui.ron).
    UiSave,
    /// 붓 모양과 반지름.
    Brush(BrushShape, u8),
    /// 스포이드 켜기.
    Eyedropper,
    Wait,
}

/// 실행 중인 스크립트.
#[derive(Debug)]
pub(crate) struct Script {
    steps: VecDeque<Step>,
}

impl Script {
    /// `NEXUS_SCRIPT` 를 읽는다. 없으면 `None`, 문법 오류는 출력하고 `None`.
    pub(crate) fn from_env() -> Option<Self> {
        let text = std::env::var(ENV_SCRIPT).ok()?;
        match parse(&text) {
            Ok(steps) => Some(Self {
                steps: steps.into(),
            }),
            Err(e) => {
                eprintln!("{ENV_SCRIPT}: {e}");
                None
            }
        }
    }

    pub(crate) fn next(&mut self) -> Option<Step> {
        self.steps.pop_front()
    }
}

fn parse(text: &str) -> Result<Vec<Step>, String> {
    text.split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(parse_step)
        .collect()
}

fn parse_step(text: &str) -> Result<Step, String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let number = |i: usize| -> Result<f32, String> {
        words
            .get(i)
            .and_then(|w| w.parse().ok())
            .ok_or_else(|| format!("'{text}': {}번째 값이 숫자가 아님", i))
    };

    let button = match words[0] {
        "press" => Button::Press,
        "move" => Button::Move,
        "release" => Button::Release,
        "delete" => return Ok(Step::Delete),
        "undo" => return Ok(Step::Undo),
        "redo" => return Ok(Step::Redo),
        "wait" => return Ok(Step::Wait),
        "play" => return Ok(Step::TogglePlay),
        "dialog" => {
            return match words.get(1).copied() {
                Some("open") => Ok(Step::Dialog(false)),
                Some("saveas") => Ok(Step::Dialog(true)),
                _ => Err(format!("'{text}': 창은 open|saveas")),
            };
        }
        "eyedropper" => return Ok(Step::Eyedropper),
        "brush" => {
            let shape = match words.get(1).copied() {
                Some("stroke") => BrushShape::Stroke,
                Some("rect") => BrushShape::Rect,
                _ => return Err(format!("'{text}': brush 뒤에 stroke|rect")),
            };
            let radius = match words.get(2) {
                None => 0,
                Some(w) => w
                    .parse()
                    .map_err(|_| format!("'{text}': 반지름은 0 이상의 정수"))?,
            };
            return Ok(Step::Brush(shape, radius));
        }
        "scripts" => return Ok(Step::Scripts(words.get(1).map(|w| (*w).to_string()))),
        "compile" => return Ok(Step::Compile),
        "savegame" => return Ok(Step::SaveGame),
        "deletesave" => return Ok(Step::DeleteSave),
        "items" => return Ok(Step::ToggleItems),
        "panels" => return Ok(Step::TogglePanels),
        "uiedit" => return Ok(Step::UiEdit),
        "startlevel" => return Ok(Step::StartLevel),
        "esc" => return Ok(Step::Escape),
        "content" => return Ok(Step::ContentToggle),
        "contentopen" => return Ok(Step::ContentOpen),
        "contentmenu" => return Ok(Step::ContentMenu),
        "contentconfirm" => return Ok(Step::ContentConfirm),
        "contentop" => {
            use crate::asset_ops::OpKind;
            let kind = match words.get(1).copied() {
                Some("delete") => OpKind::Delete,
                Some("duplicate") => OpKind::Duplicate,
                Some("rename") => OpKind::Rename,
                _ => return Err(format!("'{text}': contentop 뒤에 delete|duplicate|rename")),
            };
            return Ok(Step::ContentOp(
                kind,
                words.get(2).map(|w| (*w).to_string()),
            ));
        }
        "contentfolder" => {
            // 가상 폴더 이름에는 공백이 있다 (`게임 데이터/액터`) — 나머지 단어를 이어 붙인다.
            if words.len() < 2 {
                return Err(format!("'{text}': contentfolder 뒤에 폴더 경로"));
            }
            return Ok(Step::ContentFolder(words[1..].join(" ")));
        }
        "contentview" => {
            return match words.get(1).copied() {
                Some("icons") => Ok(Step::ContentView(false)),
                Some("list") => Ok(Step::ContentView(true)),
                _ => Err(format!("'{text}': contentview 뒤에 icons|list")),
            };
        }
        "contentpick" => {
            let key = words
                .get(1)
                .ok_or_else(|| format!("'{text}': contentpick 뒤에 경로 또는 actor:번호"))?;
            return Ok(Step::ContentPick((*key).to_string()));
        }
        "level" => {
            let id = words
                .get(1)
                .ok_or_else(|| format!("'{text}': level 뒤에 레벨 번호"))?;
            return Ok(Step::OpenLevel((*id).to_string()));
        }
        "uipress" | "uirelease" => {
            let num = |i: usize| -> Result<f32, String> {
                words
                    .get(i)
                    .and_then(|w| w.parse::<f32>().ok())
                    .ok_or_else(|| format!("'{text}': 뒤에 화면 좌표 x y (뷰포트 픽셀)"))
            };
            let button = if words[0] == "uipress" {
                Button::Press
            } else {
                Button::Release
            };
            return Ok(Step::ScreenPointer {
                button,
                at: Vec2::new(num(1)?, num(2)?),
            });
        }
        "uisave" => return Ok(Step::UiSave),
        "uiscreen" => {
            let id = words
                .get(1)
                .ok_or_else(|| format!("'{text}': uiscreen 뒤에 화면 번호"))?;
            return Ok(Step::UiScreen((*id).to_string()));
        }
        "uipick" => {
            let id = words
                .get(1)
                .ok_or_else(|| format!("'{text}': uipick 뒤에 위젯 이름"))?;
            return Ok(Step::UiPick((*id).to_string()));
        }
        "uidrag" => {
            let int = |i: usize| -> Result<i32, String> {
                words
                    .get(i)
                    .and_then(|w| w.parse().ok())
                    .ok_or_else(|| format!("'{text}': uidrag 뒤에 dx dy (정수)"))
            };
            return Ok(Step::UiDrag(int(1)?, int(2)?));
        }
        "palette" => return Ok(Step::Palette(words.get(1).map(|w| (*w).to_string()))),
        "pick" => {
            // pick ground|prop 그림경로 x y 폭 높이 [칸크기]
            let kind = match words.get(1).copied() {
                Some("ground") => ArtKind::Ground,
                Some("prop") => ArtKind::Prop,
                _ => return Err(format!("'{text}': pick 뒤에 ground|prop")),
            };
            let image = words
                .get(2)
                .ok_or_else(|| format!("'{text}': 그림 경로가 없음"))?;
            let int = |i: usize| -> Result<u32, String> {
                words
                    .get(i)
                    .and_then(|w| w.parse().ok())
                    .ok_or_else(|| format!("'{text}': {i}번째 값이 정수가 아님"))
            };
            let cell = words.get(7).map_or(Ok(16), |_| int(7))?;
            return Ok(Step::Pick(Pick {
                image: (*image).to_string(),
                px: (int(3)?, int(4)?, int(5)?, int(6)?),
                kind,
                pixels_per_meter: cell as f32,
            }));
        }
        "save" | "open" => {
            let Some(path) = words.get(1) else {
                return Err(format!("'{text}': 경로가 없음"));
            };
            let path = PathBuf::from(path);
            return Ok(if words[0] == "save" {
                Step::Save(path)
            } else {
                Step::Open(path)
            });
        }
        "tool" => {
            return match words.get(1).copied() {
                Some("select") => Ok(Step::SetTool(Tool::Select)),
                Some("paint") => Ok(Step::SetTool(Tool::PaintTile)),
                Some("art") => Ok(Step::SetTool(Tool::PaintArt)),
                _ => Err(format!("'{text}': 도구는 select|paint|art")),
            };
        }
        "art" => {
            let id = words
                .get(1)
                .and_then(|w| w.parse::<u16>().ok())
                .ok_or_else(|| format!("'{text}': art 뒤에 그림 번호 (0 = 지우기)"))?;
            let kind = match words.get(2).copied() {
                None => None,
                Some("ground") => Some(ArtKind::Ground),
                Some("prop") => Some(ArtKind::Prop),
                Some(other) => return Err(format!("'{text}': 층은 ground|prop ('{other}')")),
            };
            return Ok(Step::SetArtBrush(ArtId::new(id), kind));
        }
        "add" => {
            let kind = match words.get(1).copied() {
                Some("npc") => ItemKind::Npc,
                Some("monster") => ItemKind::Monster,
                Some("player") => ItemKind::PlayerSpawn,
                _ => return Err(format!("'{text}': 종류는 npc|monster|player")),
            };
            return Ok(Step::Add(kind, Vec2::new(number(2)?, number(3)?)));
        }
        other => return Err(format!("알 수 없는 단계 '{other}'")),
    };

    let (at, rest) = if words.get(1) == Some(&"rotate") {
        (Anchor::RotateHandle, &words[2..])
    } else {
        (
            Anchor::World(Vec2::new(number(1)?, number(2)?)),
            words.get(3..).unwrap_or_default(),
        )
    };
    Ok(Step::Pointer {
        button,
        at,
        shift: rest.contains(&"shift"),
        ctrl: rest.contains(&"ctrl"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_step_kinds() {
        let steps =
            parse("press 1 2; move rotate ctrl ; release -3.5 4 shift; add npc 5 6; tool paint; delete; undo; play; save zones/a.zone.ron;")
                .unwrap();
        assert_eq!(
            steps,
            [
                Step::Pointer {
                    button: Button::Press,
                    at: Anchor::World(Vec2::new(1.0, 2.0)),
                    shift: false,
                    ctrl: false
                },
                Step::Pointer {
                    button: Button::Move,
                    at: Anchor::RotateHandle,
                    shift: false,
                    ctrl: true
                },
                Step::Pointer {
                    button: Button::Release,
                    at: Anchor::World(Vec2::new(-3.5, 4.0)),
                    shift: true,
                    ctrl: false
                },
                Step::Add(ItemKind::Npc, Vec2::new(5.0, 6.0)),
                Step::SetTool(Tool::PaintTile),
                Step::Delete,
                Step::Undo,
                Step::TogglePlay,
                Step::Save(PathBuf::from("zones/a.zone.ron")),
            ]
        );
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse("jump 1 2").is_err());
        assert!(parse("press 1").is_err());
        assert!(parse("add dragon 0 0").is_err());
        assert!(parse("tool hammer").is_err());
        assert!(parse("brush circle").is_err());
        assert!(parse("brush rect -1").is_err());
    }

    #[test]
    fn parses_brush_steps() {
        assert_eq!(
            parse("brush rect; brush stroke 2; eyedropper").unwrap(),
            [
                Step::Brush(BrushShape::Rect, 0),
                Step::Brush(BrushShape::Stroke, 2),
                Step::Eyedropper,
            ]
        );
    }
}
