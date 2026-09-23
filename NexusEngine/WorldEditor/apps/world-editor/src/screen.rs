//! 화면(UI) — 위젯 **트리**를 화면 공간에 그린다 (단계 2 P7).
//!
//! ```text
//! ui/hud.ui.ron                     ui/main_menu.ui.ron
//! ┌─ 뷰포트 ─────────────────┐      ┌─ 뷰포트 ─────────────┐
//! │              ┌ 창 ────┐  │      │      ┌ 창 ──────┐    │
//! │              │ ITEMS  │  │      │      │  NEXUS   │    │
//! │ HP 161       │ ▢ ▢ ▢ │  │      │      │  [START] │    │
//! │ ███████░░░   └────────┘  │      │      │  [EXIT]  │    │
//! └──────────────────────────┘      └──────────────────────┘
//! ```
//!
//! - **egui 를 쓰지 않는다.** egui 는 렌더러의 `ui` 피처 뒤에 있고 게임 런타임에는 들어가지
//!   않는다 (P5-0 결정). 화면은 전부 쿼드 — 막대도, 창도, 글자도.
//! - 좌표는 [`Space::Screen`] — **뷰포트 픽셀, 좌상단 원점**이라 카메라를 따라가지 않는다.
//! - **무엇을 어디에 그릴지는 데이터다.** 코드가 아는 것은 위젯의 *종류*뿐이고
//!   위치·크기·색·문구·계층은 파일에서 온다.
//!
//! # 파일 두 종류 (역할이 다르다)
//!
//! | 파일 | 무엇 | 누가 고치나 |
//! |---|---|---|
//! | `data/ui.ron` | **테마** — 폰트 그림·구간, 9-slice 창 그림, 확대 배율 | 사람이 직접 (편집기는 `scale` 줄만) |
//! | `ui/<id>.ui.ron` | **화면 한 장** — 위젯 트리 | 위젯 편집기가 통째로 다시 쓴다 |
//!
//! 화면 파일은 편집기가 소유한다 — 저장하면 **파일이 다시 쓰이므로 손으로 쓴 주석은 남지
//! 않는다**. 테마 파일은 편집기가 `scale` 줄만 갈아 끼우므로 주석이 남는다.
//!
//! # 좌표 규약
//!
//! 위젯의 `pos`·`size` 는 **UI 픽셀**(확대 배율 `scale` 을 곱하기 전)이다. `anchor` 는
//! **부모 사각형**(최상위 위젯이면 뷰포트)의 어디를 기준으로 삼을지이고, 위젯은 그 기준과
//! **같은 쪽 자기 모서리**를 맞춘다 (`TopRight` 면 위젯의 오른쪽 위, `Center` 면 가운데).
//! `pos` 는 언제나 오른쪽·아래가 +다 — 그래서 아래쪽·오른쪽 앵커에서는 보통 음수가 된다.
//!
//! ⚠ 받아온 비트맵 폰트에는 **한글도 `/` 도 없다.** 그래서 지금 화면 문구는 영문 대문자와
//! 숫자만 쓴다 (`HP 161`, `LV 2`). 한글 UI 는 시스템 폰트 래스터라이저를 붙이는 단계에서 푼다.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use nexus_assets::{BitmapFont, GlyphSheet, Image};
use nexus_core::Vec2;
use nexus_render::{DEPTH_LAYER, DrawLayer, RenderCommand, Renderer, Space, TextureId, UvRect};
use serde::{Deserialize, Serialize};

use crate::play::PlaySession;

/// 테마 파일 (작업 디렉터리 기준). 없으면 내장본을 쓴다 — 다른 데이터 파일과 같은 규칙.
pub(crate) const THEME_PATH: &str = "data/ui.ron";
const EMBEDDED_THEME: &str = include_str!("../../../data/ui.ron");

/// 화면 파일이 있는 폴더. 파일 이름이 곧 화면 번호다 (`ui/hud.ui.ron` → `"hud"`).
pub(crate) const SCREEN_DIR: &str = "ui";
const SCREEN_EXT: &str = ".ui.ron";

/// 실행 파일에 든 화면 — 디스크에 `ui/` 가 없어도 게임이 돈다 (데이터 파일과 같은 규칙).
const EMBEDDED_SCREENS: &[(&str, &str)] = &[
    ("hud", include_str!("../../../ui/hud.ui.ron")),
    ("main_menu", include_str!("../../../ui/main_menu.ui.ron")),
    ("settings", include_str!("../../../ui/settings.ui.ron")),
    ("pause", include_str!("../../../ui/pause.ui.ron")),
];

const FORMAT_VERSION: u32 = 1;

/// 트리 깊이 상한 — 데이터 실수로 끝없이 깊어지는 것을 막는다.
const MAX_DEPTH: usize = 8;

// 색 (sRGB).
const BAR_BACK: [f32; 4] = [0.05, 0.05, 0.07, 0.85];
const BAR_EDGE: [f32; 4] = [0.85, 0.87, 0.92, 0.9];
const TEXT: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const TEXT_DIM: [f32; 4] = [0.62, 0.64, 0.70, 1.0];
const PANEL_TINT: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const SLOT_BACK: [f32; 4] = [0.12, 0.12, 0.16, 0.9];
const BUTTON_BACK: [f32; 4] = [0.10, 0.11, 0.15, 0.92];
const BUTTON_HOVER: [f32; 4] = [0.20, 0.24, 0.34, 0.95];
const BUTTON_PRESS: [f32; 4] = [0.30, 0.36, 0.50, 0.97];
const BUTTON_EDGE: [f32; 4] = [0.80, 0.83, 0.90, 0.9];
/// 겹쳐 뜬 창 아래를 덮는 색 (알파는 선형 블렌딩이라 낮아도 진하게 보인다).
const BACKDROP: [f32; 4] = [0.0, 0.0, 0.02, 0.45];
/// 편집 중 고른 위젯의 테두리.
const SELECT: [f32; 4] = [1.0, 0.82, 0.25, 1.0];
/// 편집 중 컨테이너(그림 없는 패널)의 테두리 — 안 보이면 잡을 수 없다.
const GUIDE: [f32; 4] = [0.45, 0.55, 0.75, 0.55];

/// 위젯 하나가 쓰는 깊이 칸 수 (테두리·바탕·채움·글자·선택 테두리).
const BIAS_SLOTS: f32 = 6.0;
const BIAS_EDGE: f32 = 1.0;
const BIAS_BACK: f32 = 2.0;
const BIAS_FILL: f32 = 3.0;
const BIAS_TEXT: f32 = 4.0;
const BIAS_SELECT: f32 = 5.0;
/// 깊이 구간을 넘지 않도록 위젯 수를 제한한다 (Overlay 구간 0.32 / 1e-4 = 3200 칸).
const MAX_DRAWN: usize = 400;

/// 글자 문구에 쓸 수 있는 값 이름 — 여기 없는 이름은 파일을 읽을 때 거부한다.
pub(crate) const PLACEHOLDERS: &[&str] = &["hp", "max_hp", "level", "exp", "exp_to_next"];

// ─────────────────────────────────────────────────────────────────────────────
// 테마 파일
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ThemeFile {
    pub(crate) version: u32,
    pub(crate) font: FontFile,
    pub(crate) panel: PanelFile,
    /// UI 확대 배율. **정수여야** 픽셀이 뭉개지지 않는다.
    pub(crate) scale: u32,
    /// 시스템 폰트로 글자를 그린다 (P7-E) — **한글을 쓰려면 이것이 필요하다.**
    /// 없으면 비트맵 폰트(`font`)로 그린다 (영문·숫자만).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) text_font: Option<TextFontFile>,
}

/// 시스템 폰트 설정 — 파일을 찾아 글자를 그때그때 구워 쓴다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TextFontFile {
    /// 글자 크기 (UI 픽셀). 실제로는 `scale` 을 곱한 픽셀로 구워 1:1 로 그린다.
    pub(crate) size: u32,
    /// 폰트 파일 후보 — 앞에서부터 찾는다. **비우면 OS 별 기본 후보 목록**을 쓴다.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) candidates: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FontFile {
    pub(crate) image: String,
    pub(crate) advance: u32,
    pub(crate) line_height: u32,
    pub(crate) runs: Vec<RunFile>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunFile {
    pub(crate) chars: String,
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) size: (u32, u32),
}

/// 9-slice 로 늘려 쓰는 창 그림.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PanelFile {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// 늘어나지 않는 가장자리 두께 (픽셀).
    pub(crate) border: u32,
}

// ─────────────────────────────────────────────────────────────────────────────
// 화면 파일
// ─────────────────────────────────────────────────────────────────────────────

/// 화면 한 장 = 위젯 트리 하나.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScreenFile {
    pub(crate) version: u32,
    /// 사람이 읽는 이름 — 편집기 목록에 보인다.
    pub(crate) name: String,
    /// 최상위 위젯 — 목록 순서대로 그린다 (뒤가 위).
    pub(crate) widgets: Vec<Widget>,
}

impl ScreenFile {
    /// 새 화면 — 편집기의 "새 화면".
    pub(crate) fn new(name: &str) -> Self {
        Self {
            version: FORMAT_VERSION,
            name: name.to_owned(),
            widgets: Vec::new(),
        }
    }
}

/// 위젯 하나 — 편집기가 고치는 단위. 자식을 가질 수 있다 (계층).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Widget {
    /// **한 화면 안에서 고유한** 이름. 겹치면 읽을 때 거부한다.
    pub(crate) id: String,
    pub(crate) anchor: Anchor,
    /// 앵커 기준점에서의 오프셋 (UI 픽셀, 오른쪽·아래가 +).
    pub(crate) pos: (i32, i32),
    /// 크기 (UI 픽셀). `None` 이면 종류마다 다르다 — [`WidgetKind`] 참고.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) size: Option<(u32, u32)>,
    /// 언제 보이는가. 숨은 위젯의 **자식도 함께 숨는다.**
    #[serde(default, skip_serializing_if = "Show::is_default")]
    pub(crate) show: Show,
    pub(crate) kind: WidgetKind,
    /// 자식 — 이 위젯의 사각형이 자식들의 부모 사각형이 된다.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) children: Vec<Widget>,
}

impl Widget {
    pub(crate) fn new(id: &str, kind: WidgetKind) -> Self {
        Self {
            id: id.to_owned(),
            anchor: Anchor::TopLeft,
            pos: (0, 0),
            size: kind.default_size(),
            show: Show::Always,
            kind,
            children: Vec::new(),
        }
    }
}

/// 기준점 — 부모 사각형의 어느 지점에 붙을지.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Anchor {
    #[default]
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

/// 한 축에서의 붙는 위치.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Start,
    Mid,
    End,
}

impl Anchor {
    /// 아홉 가지 전부 — 편집기 드롭다운용. 화면에 보이는 자리 순서대로.
    pub(crate) const ALL: [Self; 9] = [
        Self::TopLeft,
        Self::Top,
        Self::TopRight,
        Self::Left,
        Self::Center,
        Self::Right,
        Self::BottomLeft,
        Self::Bottom,
        Self::BottomRight,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::TopLeft => "왼쪽 위",
            Self::Top => "위 가운데",
            Self::TopRight => "오른쪽 위",
            Self::Left => "왼쪽 가운데",
            Self::Center => "가운데",
            Self::Right => "오른쪽 가운데",
            Self::BottomLeft => "왼쪽 아래",
            Self::Bottom => "아래 가운데",
            Self::BottomRight => "오른쪽 아래",
        }
    }

    fn horizontal(self) -> Side {
        match self {
            Self::TopLeft | Self::Left | Self::BottomLeft => Side::Start,
            Self::Top | Self::Center | Self::Bottom => Side::Mid,
            Self::TopRight | Self::Right | Self::BottomRight => Side::End,
        }
    }

    fn vertical(self) -> Side {
        match self {
            Self::TopLeft | Self::Top | Self::TopRight => Side::Start,
            Self::Left | Self::Center | Self::Right => Side::Mid,
            Self::BottomLeft | Self::Bottom | Self::BottomRight => Side::End,
        }
    }

    /// 부모 사각형 안에서의 왼쪽 위 좌표 (화면 픽셀).
    fn origin(self, parent: Rect, pos: (f32, f32), size: (f32, f32)) -> Vec2 {
        let along = |side: Side, start: f32, room: f32, len: f32, offset: f32| match side {
            Side::Start => start + offset,
            Side::Mid => start + (room - len) * 0.5 + offset,
            Side::End => start + room - len + offset,
        };
        Vec2::new(
            along(self.horizontal(), parent.x, parent.w, size.0, pos.0),
            along(self.vertical(), parent.y, parent.h, size.1, pos.1),
        )
    }
}

/// 언제 보이는가.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Show {
    /// 화면이 떠 있으면 늘 보인다.
    #[default]
    Always,
    /// 인벤토리를 열었을 때만 (**I**).
    ItemsOpen,
}

impl Show {
    pub(crate) const ALL: [Self; 2] = [Self::Always, Self::ItemsOpen];

    fn is_default(&self) -> bool {
        matches!(self, Self::Always)
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Always => "항상",
            Self::ItemsOpen => "인벤토리 열었을 때",
        }
    }

    fn visible(self, values: &Values) -> bool {
        match self {
            Self::Always => true,
            Self::ItemsOpen => values.show_items,
        }
    }
}

/// 위젯 종류. **코드가 아는 것은 이 종류뿐**이고 나머지는 데이터다.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum WidgetKind {
    /// 창·그룹. `frame` 이면 9-slice 창 그림을 깔고, 아니면 자리만 잡는 투명 컨테이너다.
    /// `size` 가 없으면 **부모 사각형 전체**를 차지한다.
    Panel { frame: bool },
    /// 한 줄 글자. `{hp}` 처럼 값 이름을 넣는다 ([`PLACEHOLDERS`]).
    /// `size` 가 없으면 글자 크기에 맞춘다.
    Text { format: String },
    /// 비율 막대 (HP·경험치). `size` 가 필요하다.
    Bar {
        source: BarSource,
        /// 채움 색 (sRGB).
        color: (f32, f32, f32),
    },
    /// 인벤토리 칸 격자. `size` 가 없으면 칸 수와 내용에 맞춘다.
    Items { columns: u32 },
    /// 누를 수 있는 버튼 — 눌리면 [`Action`] 을 낸다.
    /// `size` 가 없으면 글자 + 여백에 맞춘다.
    Button { label: String, action: Action },
}

impl WidgetKind {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Panel { .. } => "창",
            Self::Text { .. } => "글자",
            Self::Bar { .. } => "막대",
            Self::Items { .. } => "아이템 칸",
            Self::Button { .. } => "버튼",
        }
    }

    /// 편집기에서 새로 만들 때의 크기 — `None` 이면 내용에 맞춘다.
    fn default_size(&self) -> Option<(u32, u32)> {
        match self {
            Self::Bar { .. } => Some((110, 9)),
            Self::Panel { .. } => Some((160, 90)),
            _ => None,
        }
    }
}

/// 막대가 보여 주는 값.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum BarSource {
    Hp,
    Exp,
}

impl BarSource {
    pub(crate) const ALL: [Self; 2] = [Self::Hp, Self::Exp];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Hp => "HP",
            Self::Exp => "경험치",
        }
    }

    fn values(self, v: &Values) -> (u32, u32) {
        match self {
            Self::Hp => (v.hp, v.max_hp),
            Self::Exp => (v.exp, v.exp_to_next),
        }
    }
}

/// 버튼이 하는 일. **고정 목록 + 스크립트** — 흔한 것은 데이터로, 특별한 것은 Rhai 로.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum Action {
    /// 화면을 하나 더 띄운다 (스택에 쌓기).
    OpenScreen(String),
    /// 맨 위 화면을 닫는다.
    Close,
    /// 레벨을 연다 — 언리얼의 `Open Level`.
    OpenLevel(String),
    /// 시작 레벨로 (메인 화면으로 돌아가기).
    OpenStartLevel,
    /// 게임을 잇는다 (일시정지 해제).
    Resume,
    /// 엔진을 닫는다. 에디터에서는 플레이를 멈춘다.
    Quit,
    /// 아무것도 하지 않는다 — 배치만 해 두는 버튼.
    None,
}

impl Action {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::OpenScreen(_) => "화면 열기",
            Self::Close => "화면 닫기",
            Self::OpenLevel(_) => "레벨 열기",
            Self::OpenStartLevel => "시작 레벨로",
            Self::Resume => "계속하기",
            Self::Quit => "종료",
            Self::None => "없음",
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 그릴 때 쓰는 값 — 플레이 중이 아니어도 화면을 그릴 수 있어야 한다
// ─────────────────────────────────────────────────────────────────────────────

/// 화면이 읽는 값 모음. 플레이 중이면 시뮬레이션에서, 편집 중이면 [`Values::preview`] 에서 온다.
///
/// **화면 그리기가 `PlaySession` 을 직접 보지 않는 이유**: 메인 화면·설정 화면은 플레이 중이
/// 아닐 때 떠 있어야 하고, 위젯 편집기도 플레이 없이 미리보기를 해야 한다.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Values {
    pub(crate) hp: u32,
    pub(crate) max_hp: u32,
    pub(crate) level: u32,
    pub(crate) exp: u32,
    pub(crate) exp_to_next: u32,
    /// 인벤토리 칸 — `(색, 개수)`.
    pub(crate) items: Vec<([f32; 4], u32)>,
    pub(crate) show_items: bool,
}

impl Values {
    /// 플레이 중인 값. 플레이어가 없으면(시체 정리 등) `None`.
    pub(crate) fn from_play(play: &PlaySession, show_items: bool) -> Option<Self> {
        let u = play.world().unit(play.player())?;
        let p = u.progress();
        Some(Self {
            hp: u.hp(),
            max_hp: u.max_hp(),
            level: p.level(),
            exp: p.exp(),
            exp_to_next: p.exp_to_next(),
            items: play.hud_slots(),
            show_items,
        })
    }

    /// 편집기 미리보기 값 — 실제 플레이와 비슷한 자릿수로 (칸 크기를 눈으로 보려고).
    pub(crate) fn preview() -> Self {
        Self {
            hp: 161,
            max_hp: 200,
            level: 2,
            exp: 20,
            exp_to_next: 400,
            items: vec![
                ([0.85, 0.25, 0.25, 1.0], 3),
                ([0.85, 0.80, 0.35, 1.0], 1),
                ([0.45, 0.65, 0.95, 1.0], 12),
            ],
            show_items: true,
        }
    }

    /// 플레이 중이면 그 값, 아니면 미리보기 값 — 편집기·플레이가 같은 경로를 쓰게.
    pub(crate) fn of(play: Option<&PlaySession>, show_items: bool) -> Self {
        play.and_then(|p| Self::from_play(p, show_items))
            .unwrap_or_else(|| Self {
                show_items,
                ..Self::preview()
            })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 읽은 결과
// ─────────────────────────────────────────────────────────────────────────────

/// 테마(폰트·창 그림)와 화면들. 그림을 못 읽으면 비어 있고 아무것도 그리지 않는다
/// (그림이 없다고 플레이를 막지는 않는다 — 시트와 같은 규칙).
#[derive(Debug)]
pub(crate) struct Screens {
    font: Option<BitmapFont>,
    texture: TextureId,
    /// 시스템 폰트로 구운 글자 아틀라스 (P7-E). 없으면 비트맵 폰트로 그린다.
    text: Option<TextFont>,
    panel: Option<PanelUv>,
    theme: Option<ThemeFile>,
    /// 화면 번호 → 화면. `BTreeMap` 이라 목록 순서가 OS·실행마다 같다.
    screens: BTreeMap<String, ScreenFile>,
}

impl Default for Screens {
    fn default() -> Self {
        Self {
            font: None,
            texture: TextureId::WHITE,
            text: None,
            panel: None,
            theme: None,
            screens: BTreeMap::new(),
        }
    }
}

/// 시스템 폰트로 구운 글자 아틀라스와 그 텍스처.
///
/// `bytes` 를 들고 있는 이유: 편집 중 새 글자가 생기면 **다시 구워야** 한다.
#[derive(Debug)]
struct TextFont {
    bytes: Vec<u8>,
    path: String,
    /// 구운 픽셀 크기 (= `size` × `scale`).
    px: u32,
    sheet: GlyphSheet,
    texture: TextureId,
}

#[derive(Clone, Copy, Debug)]
struct PanelUv {
    /// 창 그림의 픽셀 사각형.
    rect: (f32, f32, f32, f32),
    border: f32,
    image: (f32, f32),
}

/// 화면 사각형 — 왼쪽 위 기준 (UI 좌표 규약).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
}

impl Rect {
    pub(crate) fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    fn center(self) -> Vec2 {
        Vec2::new(self.x + self.w * 0.5, self.y + self.h * 0.5)
    }

    fn size(self) -> Vec2 {
        Vec2::new(self.w, self.h)
    }

    /// 사방으로 `d` 만큼 넓힌 사각형 (테두리용).
    pub(crate) fn grow(self, d: f32) -> Self {
        Self::new(self.x - d, self.y - d, self.w + d * 2.0, self.h + d * 2.0)
    }

    /// 왼쪽에서 `t` 비율만큼 (막대 채움용).
    fn fraction(self, t: f32) -> Self {
        Self::new(self.x, self.y, self.w * t, self.h)
    }

    /// 점이 안에 있는가 — 편집기의 피킹, 버튼의 히트 테스트.
    pub(crate) fn contains(self, p: Vec2) -> bool {
        p.x >= self.x && p.x <= self.x + self.w && p.y >= self.y && p.y <= self.y + self.h
    }
}

/// 그릴 때의 상태 — 버튼 호버·누름과 편집 표시.
///
/// 인자를 묶어 두는 구조체다 (그리기 함수의 인자가 늘어나 clippy 가 잡았다).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct DrawState<'a> {
    /// 마우스가 올라간 버튼.
    pub(crate) hover: Option<&'a str>,
    /// 누르고 있는 버튼.
    pub(crate) pressed: Option<&'a str>,
    /// 편집기가 고른 위젯 — 테두리를 그려 준다.
    pub(crate) selected: Option<&'a str>,
    /// 편집 중 — 투명 컨테이너에도 옅은 테두리를 그린다 (안 보이면 잡을 수 없다).
    pub(crate) editing: bool,
}

/// 자리를 계산한 위젯 하나 — 그리기·피킹·편집기 테두리가 **같은 결과**를 쓴다.
#[derive(Clone, Debug)]
pub(crate) struct Laid<'a> {
    pub(crate) widget: &'a Widget,
    pub(crate) rect: Rect,
}

impl Screens {
    /// 테마와 화면들을 읽고 그림을 GPU 에 올린다. 못 읽은 것은 경고로 돌려준다
    /// (화면이 없다고 플레이를 막지 않는다).
    pub(crate) fn load(renderer: &mut dyn Renderer) -> (Self, Vec<String>) {
        let mut warnings = Vec::new();
        let text = match std::fs::read_to_string(THEME_PATH) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => EMBEDDED_THEME.to_owned(),
            Err(e) => {
                warnings.push(format!("{THEME_PATH}: {e}"));
                EMBEDDED_THEME.to_owned()
            }
        };
        let mut me = match Self::build_theme(&text, renderer) {
            Ok(me) => me,
            Err(e) => {
                warnings.push(format!("{THEME_PATH}: {e}"));
                Self::default()
            }
        };
        me.screens = load_screens(&mut warnings);
        // 글자 폰트는 화면을 읽은 뒤에 굽는다 — 문구에 쓰인 글자를 알아야 하기 때문이다.
        if let Some(e) = me.load_text_font(renderer) {
            warnings.push(format!("UI 글자 폰트: {e}"));
        }
        (me, warnings)
    }

    /// 테마를 읽고 형식을 확인한다 — 그림은 아직 읽지 않는다.
    pub(crate) fn read_theme(text: &str) -> Result<ThemeFile, String> {
        let file: ThemeFile = ron::from_str(text).map_err(|e| format!("읽을 수 없음 — {e}"))?;
        if file.version != FORMAT_VERSION {
            return Err(format!(
                "형식 {} 은(는) 읽을 수 없음 (이 에디터는 {FORMAT_VERSION})",
                file.version
            ));
        }
        if file.scale == 0 {
            return Err(String::from("scale 은 1 이상의 정수여야 함"));
        }
        Ok(file)
    }

    /// 화면 하나를 읽고 검사한다.
    pub(crate) fn read_screen(text: &str) -> Result<ScreenFile, String> {
        let file: ScreenFile = ron::from_str(text).map_err(|e| format!("읽을 수 없음 — {e}"))?;
        if file.version != FORMAT_VERSION {
            return Err(format!(
                "형식 {} 은(는) 읽을 수 없음 (이 에디터는 {FORMAT_VERSION})",
                file.version
            ));
        }
        if file.name.trim().is_empty() {
            return Err(String::from("화면 이름이 비어 있음"));
        }
        let mut seen: Vec<&str> = Vec::new();
        check_widgets(&file.widgets, 0, &mut seen)?;
        Ok(file)
    }

    /// 그림 크기를 알아야 할 수 있는 검사 — 글자 칸과 창이 그림 안에 들어가는지.
    /// 렌더러가 필요 없어 테스트가 그대로 부른다.
    fn build_font(file: &ThemeFile, size: (u32, u32)) -> Result<(BitmapFont, PanelUv), String> {
        let mut font = BitmapFont::new(size, file.font.advance, file.font.line_height)
            .map_err(|e| e.to_string())?;
        for run in &file.font.runs {
            font.add_run(&run.chars, run.x, run.y, run.size)
                .map_err(|e| format!("'{}': {e}", run.chars))?;
        }
        let p = file.panel;
        if p.x + p.width > size.0 || p.y + p.height > size.1 {
            return Err(String::from("panel 이 그림 밖으로 나감"));
        }
        if p.border == 0 || p.border * 2 >= p.width.min(p.height) {
            return Err(String::from("panel.border 가 창 절반을 넘음"));
        }
        Ok((
            font,
            PanelUv {
                rect: (p.x as f32, p.y as f32, p.width as f32, p.height as f32),
                border: p.border as f32,
                image: (size.0 as f32, size.1 as f32),
            },
        ))
    }

    fn build_theme(text: &str, renderer: &mut dyn Renderer) -> Result<Self, String> {
        let theme = Self::read_theme(text)?;
        let bytes = std::fs::read(Path::new(&theme.font.image))
            .map_err(|e| format!("{}: {e}", theme.font.image))?;
        let image = Image::decode_png(&bytes).map_err(|e| format!("{}: {e}", theme.font.image))?;
        let (font, panel) = Self::build_font(&theme, (image.width(), image.height()))?;
        let texture = renderer
            .load_texture(&image.desc("ui-font"))
            .map_err(|e| format!("{}: {e}", theme.font.image))?;
        Ok(Self {
            font: Some(font),
            texture,
            text: None,
            panel: Some(panel),
            theme: Some(theme),
            screens: BTreeMap::new(),
        })
    }

    /// 그림을 읽었는가 — 아니면 아무것도 그리지 않는다.
    pub(crate) fn ready(&self) -> bool {
        self.panel.is_some() && (self.font.is_some() || self.text.is_some())
    }

    /// 화면 문구에 쓰이는 글자 전부 — 글자 아틀라스를 구울 목록이다.
    ///
    /// 숫자는 실행 중에 값으로 채워지므로 늘 넣고, `{`·`}` 와 값 이름은 그려지지 않지만
    /// 걸러 내지 않는다 (글리프 몇 개 차이라 걸러 내는 코드가 더 비싸다).
    fn wanted_chars(&self) -> BTreeSet<char> {
        let mut out: BTreeSet<char> = "0123456789 .,:%+-/".chars().collect();
        for screen in self.screens.values() {
            let mut stack: Vec<&Widget> = screen.widgets.iter().collect();
            while let Some(w) = stack.pop() {
                stack.extend(w.children.iter());
                match &w.kind {
                    WidgetKind::Text { format } => out.extend(format.chars()),
                    WidgetKind::Button { label, .. } => out.extend(label.chars()),
                    _ => {}
                }
            }
        }
        out
    }

    /// 테마가 시스템 폰트를 쓰라고 하면 폰트를 찾아 글자를 구워 올린다.
    /// 실패하면 이유를 돌려주고 **비트맵 폰트로 계속 간다** (한글만 안 보인다).
    fn load_text_font(&mut self, renderer: &mut dyn Renderer) -> Option<String> {
        let cfg = self.theme.as_ref()?.text_font.clone()?;
        if cfg.size == 0 {
            return Some(String::from("text_font.size 는 1 이상이어야 합니다"));
        }
        let (path, bytes) = find_font(&cfg.candidates)?;
        let px = cfg.size * self.theme.as_ref().map_or(1, |t| t.scale.max(1));
        let sheet = match GlyphSheet::rasterize(&bytes, px, self.wanted_chars()) {
            Ok(sheet) => sheet,
            Err(e) => return Some(format!("{path}: {e}")),
        };
        match renderer.load_texture(&sheet.image().desc("ui-text")) {
            Ok(texture) => {
                println!("UI 글자 폰트 — {path} ({px}px, 글리프 {}개)", sheet.len());
                self.text = Some(TextFont {
                    bytes,
                    path,
                    px,
                    sheet,
                    texture,
                });
                None
            }
            Err(e) => Some(format!("{path}: {e}")),
        }
    }

    /// 화면 문구에 **새 글자**가 생겼으면 아틀라스를 다시 구워 올린다 (편집 중 타이핑).
    ///
    /// 텍스처 해제 API 가 없어 **이전 아틀라스는 GPU 에 남는다** — 편집 중에만 일어나는
    /// 일이라 그대로 둔다. 실패하면 이유를 돌려주고 옛 아틀라스를 계속 쓴다.
    pub(crate) fn ensure_text_glyphs(&mut self, renderer: &mut dyn Renderer) -> Option<String> {
        let wanted = self.wanted_chars();
        let text = self.text.as_ref()?;
        if text.sheet.covers(wanted.iter().copied()) {
            return None;
        }
        let sheet = match GlyphSheet::rasterize(&text.bytes, text.px, wanted) {
            Ok(sheet) => sheet,
            Err(e) => return Some(format!("{}: {e}", text.path)),
        };
        match renderer.load_texture(&sheet.image().desc("ui-text")) {
            Ok(texture) => {
                let text = self.text.as_mut().expect("위에서 확인했다");
                text.sheet = sheet;
                text.texture = texture;
                None
            }
            Err(e) => Some(format!("{}: {e}", text.path)),
        }
    }

    pub(crate) fn theme(&self) -> Option<&ThemeFile> {
        self.theme.as_ref()
    }

    pub(crate) fn scale(&self) -> f32 {
        self.theme.as_ref().map_or(1.0, |t| t.scale.max(1) as f32)
    }

    /// 편집기가 고친 배율을 받아들인다 (폰트·창 그림은 다시 올릴 수 없어 바꾸지 않는다).
    pub(crate) fn set_scale(&mut self, scale: u32) {
        if let Some(theme) = self.theme.as_mut() {
            theme.scale = scale.max(1);
        }
    }

    pub(crate) fn screen(&self, id: &str) -> Option<&ScreenFile> {
        self.screens.get(id)
    }

    /// 화면 번호 목록 (사전 순).
    pub(crate) fn ids(&self) -> Vec<String> {
        self.screens.keys().cloned().collect()
    }

    /// 편집기가 고친 화면을 받아들인다 (화면에 바로 보이게).
    pub(crate) fn set_screen(&mut self, id: &str, screen: ScreenFile) {
        self.screens.insert(id.to_owned(), screen);
    }

    /// 화면의 위젯 자리를 전부 계산한다 — **그리는 순서**(앞이 아래)로 늘어놓는다.
    /// 숨은 위젯과 그 자식은 빠진다.
    pub(crate) fn layout<'a>(
        &self,
        screen: &'a ScreenFile,
        values: &Values,
        viewport: (f32, f32),
    ) -> Vec<Laid<'a>> {
        let mut out = Vec::new();
        let root = Rect::new(0.0, 0.0, viewport.0, viewport.1);
        self.lay_children(&screen.widgets, root, values, 0, &mut out);
        out
    }

    fn lay_children<'a>(
        &self,
        widgets: &'a [Widget],
        parent: Rect,
        values: &Values,
        depth: usize,
        out: &mut Vec<Laid<'a>>,
    ) {
        if depth > MAX_DEPTH {
            return;
        }
        for w in widgets {
            if !w.show.visible(values) || out.len() >= MAX_DRAWN {
                continue;
            }
            let rect = self.rect_in(w, parent, values);
            out.push(Laid { widget: w, rect });
            self.lay_children(&w.children, rect, values, depth + 1, out);
        }
    }

    /// 위젯 하나의 사각형 — 부모 사각형 안에서.
    fn rect_in(&self, w: &Widget, parent: Rect, values: &Values) -> Rect {
        let s = self.scale();
        let size = match (w.size, &w.kind) {
            (Some(size), _) => (size.0.max(1) as f32 * s, size.1.max(1) as f32 * s),
            (None, WidgetKind::Panel { .. }) => (parent.w, parent.h),
            (None, WidgetKind::Text { format }) => self.text_size(&fill(format, values)),
            (None, WidgetKind::Button { label, .. }) => {
                let (w, h) = self.text_size(label);
                (w + 16.0 * s, h + 8.0 * s)
            }
            (None, WidgetKind::Items { columns }) => self.items_size(*columns, values),
            (None, WidgetKind::Bar { .. }) => (110.0 * s, 9.0 * s),
        };
        let pos = (w.pos.0 as f32 * s, w.pos.1 as f32 * s);
        let at = if matches!((w.size, &w.kind), (None, WidgetKind::Panel { .. })) {
            // 부모를 가득 채우는 패널은 앵커·오프셋을 무시한다 (컨테이너로 쓰는 경우).
            Vec2::new(parent.x, parent.y)
        } else {
            w.anchor.origin(parent, pos, size)
        };
        Rect::new(at.x, at.y, size.0, size.1)
    }

    /// 한 줄 글자의 크기 — **그리는 것과 같은 폰트**로 재야 자리가 맞는다.
    fn text_size(&self, text: &str) -> (f32, f32) {
        if let Some(t) = self.text.as_ref() {
            // 시스템 폰트는 scale 을 곱한 픽셀로 구워 두었으므로 1:1 이다.
            return (t.sheet.width(text) as f32, t.sheet.line_height() as f32);
        }
        let s = self.scale();
        self.font.as_ref().map_or((0.0, 0.0), |f| {
            (f.width(text) as f32 * s, f.line_height() as f32 * s)
        })
    }

    /// 인벤토리 칸 격자의 크기 — 칸 수와 내용에 따라 늘어난다.
    fn items_size(&self, columns: u32, values: &Values) -> (f32, f32) {
        let s = self.scale();
        let cols = columns.max(1) as usize;
        let rows = values.items.len().div_ceil(cols).max(1);
        let (cell, gap) = (18.0 * s, 3.0 * s);
        (
            cols as f32 * (cell + gap) - gap,
            rows as f32 * (cell + gap) - gap,
        )
    }

    /// 그 자리에 있는 버튼 — 위에 그려진 것이 먼저다. 버튼이 아니면 `None`.
    ///
    /// 그리기와 **같은 [`Self::layout`]** 을 쓰므로 눈에 보이는 자리와 누르는 자리가 어긋나지 않는다.
    pub(crate) fn button_at(
        &self,
        id: &str,
        values: &Values,
        viewport: (f32, f32),
        at: Vec2,
    ) -> Option<(String, Action)> {
        let screen = self.screens.get(id)?;
        self.layout(screen, values, viewport)
            .into_iter()
            .rev()
            .find_map(|l| match &l.widget.kind {
                WidgetKind::Button { action, .. } if l.rect.contains(at) => {
                    Some((l.widget.id.clone(), action.clone()))
                }
                _ => None,
            })
    }

    /// 화면에서 끌어 옮긴 양을 `pos`(UI 픽셀) 증분으로 바꾼다 — 배율만큼 나눈다.
    /// `snap` 이면 8 UI 픽셀 격자에 맞춘다. `pos` 는 앵커와 무관하게 오른쪽·아래가 +다.
    pub(crate) fn drag_delta(&self, delta: Vec2, snap: bool) -> (i32, i32) {
        let d = delta / self.scale();
        let grid = if snap { 8.0 } else { 1.0 };
        let round = |v: f32| (v / grid).round() * grid;
        (round(d.x) as i32, round(d.y) as i32)
    }

    /// 화면 픽셀 크기를 UI 픽셀 크기로 (크기 조절 핸들용).
    pub(crate) fn to_ui_size(&self, size: Vec2, snap: bool) -> (u32, u32) {
        let s = self.scale();
        let grid = if snap { 8.0 } else { 1.0 };
        let round = |v: f32| ((v / s / grid).round() * grid).max(1.0) as u32;
        (round(size.x), round(size.y))
    }

    /// 화면 스택을 그린다 (`ids` 의 앞이 바탕, 뒤가 위에 뜬 창).
    ///
    /// 명령을 낸 뒤 좌표 공간을 [`Space::World`] 로 되돌린다 — 다음 프레임이 월드부터 그리므로.
    pub(crate) fn build_commands(
        &self,
        ids: &[String],
        values: &Values,
        state: DrawState<'_>,
        viewport: (f32, f32),
        out: &mut Vec<RenderCommand>,
    ) {
        let DrawState {
            hover,
            pressed,
            selected,
            editing,
        } = state;
        let Some(panel) = self.panel else {
            return;
        };
        out.push(RenderCommand::SetSpace(Space::Screen));
        out.push(RenderCommand::SetLayer(DrawLayer::Overlay));

        let mut base = 0.0_f32;
        for (depth, id) in ids.iter().enumerate() {
            let Some(screen) = self.screens.get(id) else {
                continue;
            };
            // 겹쳐 뜬 창(일시정지·설정)은 아래 화면을 어둡게 덮는다 — 아래 버튼을 누를 수
            // 없다는 것을 눈으로 알 수 있게 (모달 규칙과 같은 뜻이다).
            if depth > 0 {
                rect(
                    Rect::new(0.0, 0.0, viewport.0, viewport.1),
                    BACKDROP,
                    (base + BIAS_BACK) * DEPTH_LAYER,
                    out,
                );
                base += BIAS_SLOTS;
            }
            for laid in self.layout(screen, values, viewport) {
                let bias = |slot: f32| (base + slot) * DEPTH_LAYER;
                let r = laid.rect;
                match &laid.widget.kind {
                    WidgetKind::Panel { frame } => {
                        if *frame {
                            self.nine_slice(panel, r, bias(BIAS_BACK), out);
                        } else if editing {
                            outline(r, GUIDE, bias(BIAS_EDGE), 1.0, out);
                        }
                    }
                    WidgetKind::Text { format } => {
                        self.text(&fill(format, values), r, TEXT, bias(BIAS_TEXT), out);
                    }
                    WidgetKind::Bar { source, color } => {
                        let (value, max) = source.values(values);
                        let fill_color = [color.0, color.1, color.2, 1.0];
                        rect(r.grow(self.scale()), BAR_EDGE, bias(BIAS_EDGE), out);
                        rect(r, BAR_BACK, bias(BIAS_BACK), out);
                        let t = ratio(value, max);
                        if t > 0.0 {
                            rect(r.fraction(t), fill_color, bias(BIAS_FILL), out);
                        }
                    }
                    WidgetKind::Items { columns } => {
                        self.items(values, *columns, r, &bias, out);
                    }
                    WidgetKind::Button { label, .. } => {
                        let id = laid.widget.id.as_str();
                        let back = if pressed == Some(id) {
                            BUTTON_PRESS
                        } else if hover == Some(id) {
                            BUTTON_HOVER
                        } else {
                            BUTTON_BACK
                        };
                        rect(r.grow(self.scale()), BUTTON_EDGE, bias(BIAS_EDGE), out);
                        rect(r, back, bias(BIAS_BACK), out);
                        let color = if hover == Some(id) { TEXT } else { TEXT_DIM };
                        let (tw, th) = self.text_size(label);
                        let at = Rect::new(r.x + (r.w - tw) * 0.5, r.y + (r.h - th) * 0.5, tw, th);
                        self.text(label, at, color, bias(BIAS_TEXT), out);
                    }
                }
                if selected == Some(laid.widget.id.as_str()) {
                    outline(
                        r.grow(self.scale()),
                        SELECT,
                        bias(BIAS_SELECT),
                        self.scale(),
                        out,
                    );
                }
                base += BIAS_SLOTS;
            }
        }

        out.push(RenderCommand::SetSpace(Space::World));
    }

    /// 한 줄 글자. 사각형의 왼쪽 위에서 시작한다.
    ///
    /// 시스템 폰트가 있으면 그것으로 (한글이 나온다), 없으면 비트맵 폰트로 그린다.
    fn text(&self, s: &str, r: Rect, color: [f32; 4], bias: f32, out: &mut Vec<RenderCommand>) {
        let mut quad = |x: f32, y: f32, w: f32, h: f32, uv: UvRect, texture: TextureId| {
            out.push(RenderCommand::DrawRect {
                center: Vec2::new(x + w * 0.5, y + h * 0.5),
                size: Vec2::new(w, h),
                rotation: 0.0,
                z: 0.0,
                depth_bias: bias,
                color,
                uv,
                texture,
            });
        };
        if let Some(t) = self.text.as_ref() {
            // 이미 scale 을 곱한 픽셀로 구워 두었으므로 1:1 로 놓는다.
            // 글리프마다 offset 이 있다 — 베이스라인 계산은 아틀라스가 흡수했다.
            for (dx, g) in t.sheet.layout(s) {
                quad(
                    r.x + (dx + g.offset.0) as f32,
                    r.y + g.offset.1 as f32,
                    g.size.0 as f32,
                    g.size.1 as f32,
                    g.uv,
                    t.texture,
                );
            }
            return;
        }
        let Some(font) = self.font.as_ref() else {
            return;
        };
        let scale = self.scale();
        for (dx, glyph) in font.layout(s) {
            quad(
                r.x + dx as f32 * scale,
                r.y,
                glyph.size.0 as f32 * scale,
                glyph.size.1 as f32 * scale,
                glyph.uv,
                self.texture,
            );
        }
    }

    /// 인벤토리 칸 격자 — 색 네모 + 개수.
    fn items(
        &self,
        values: &Values,
        columns: u32,
        r: Rect,
        bias: &dyn Fn(f32) -> f32,
        out: &mut Vec<RenderCommand>,
    ) {
        let s = self.scale();
        let cols = columns.max(1) as usize;
        let (cell, gap) = (18.0 * s, 3.0 * s);
        for (i, (color, count)) in values.items.iter().enumerate() {
            let cx = r.x + (i % cols) as f32 * (cell + gap);
            let cy = r.y + (i / cols) as f32 * (cell + gap);
            let slot = Rect::new(cx, cy, cell, cell);
            rect(slot, SLOT_BACK, bias(BIAS_BACK), out);
            rect(slot.grow(-2.0 * s), *color, bias(BIAS_FILL), out);
            if *count > 1 {
                let at = Rect::new(cx, cy + cell - 8.0 * s, cell, 8.0 * s);
                self.text(&format!("x{count}"), at, TEXT, bias(BIAS_TEXT), out);
            }
        }
    }

    /// 창 그림을 아홉 조각으로 늘린다 — 모서리는 그대로, 변은 한 방향으로, 가운데만 양방향으로.
    fn nine_slice(&self, p: PanelUv, r: Rect, bias: f32, out: &mut Vec<RenderCommand>) {
        let (x, y, w, h) = (r.x, r.y, r.w, r.h);
        let b = p.border * self.scale();
        let (px, py, pw, ph) = p.rect;
        let bs = p.border;
        // (화면 조각, 그림 조각) 을 가로·세로로 셋씩.
        let cols = [
            (x, b, px, bs),
            (x + b, w - b * 2.0, px + bs, pw - bs * 2.0),
            (x + w - b, b, px + pw - bs, bs),
        ];
        let rows = [
            (y, b, py, bs),
            (y + b, h - b * 2.0, py + bs, ph - bs * 2.0),
            (y + h - b, b, py + ph - bs, bs),
        ];
        for &(sy, sh, ty, th) in &rows {
            for &(sx, sw, tx, tw) in &cols {
                out.push(RenderCommand::DrawRect {
                    center: Vec2::new(sx + sw * 0.5, sy + sh * 0.5),
                    size: Vec2::new(sw, sh),
                    rotation: 0.0,
                    z: 0.0,
                    depth_bias: bias,
                    color: PANEL_TINT,
                    uv: UvRect {
                        min: Vec2::new(tx / p.image.0, ty / p.image.1),
                        max: Vec2::new((tx + tw) / p.image.0, (ty + th) / p.image.1),
                    },
                    texture: self.texture,
                });
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 파일 읽기·쓰기
// ─────────────────────────────────────────────────────────────────────────────

/// `ui/` 의 화면을 전부 읽는다. 디스크가 우선이고, 없는 것은 내장본으로 채운다.
/// 읽다 실패한 파일은 경고로 남기고 그 화면만 빠진다.
fn load_screens(warnings: &mut Vec<String>) -> BTreeMap<String, ScreenFile> {
    let mut out = BTreeMap::new();
    for (id, text) in EMBEDDED_SCREENS {
        match Screens::read_screen(text) {
            Ok(screen) => {
                out.insert((*id).to_owned(), screen);
            }
            // 내장본이 깨진 것은 빌드 실수다 — 테스트가 막지만 조용히 넘기지는 않는다.
            Err(e) => warnings.push(format!("내장 화면 '{id}': {e}")),
        }
    }
    let Ok(dir) = std::fs::read_dir(SCREEN_DIR) else {
        return out; // 폴더가 없으면 내장본만 쓴다
    };
    let mut paths: Vec<PathBuf> = dir
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(SCREEN_EXT))
        })
        .collect();
    paths.sort(); // 실행마다 같은 순서
    for path in paths {
        let Some(id) = screen_id(&path) else { continue };
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| Screens::read_screen(&t))
        {
            Ok(screen) => {
                out.insert(id, screen);
            }
            Err(e) => warnings.push(format!("{}: {e}", path.display())),
        }
    }
    out
}

/// 폰트 파일을 찾는다 — 후보를 앞에서부터, 비어 있으면 OS 별 기본 목록에서.
///
/// 에디터의 egui 폰트 탐지와 **같은 목록**을 쓴다 (`NEXUS_UI_FONT` 도 통한다) —
/// 두 곳이 다른 폰트를 쓰면 에디터와 게임 화면의 글자가 달라 보인다.
fn find_font(candidates: &[String]) -> Option<(String, Vec<u8>)> {
    let env = std::env::var("NEXUS_UI_FONT").ok();
    let paths = env
        .iter()
        .map(String::as_str)
        .chain(candidates.iter().map(String::as_str))
        .chain(crate::ui::KOREAN_FONT_CANDIDATES.iter().copied());
    for path in paths {
        if let Ok(bytes) = std::fs::read(path) {
            return Some((path.to_owned(), bytes));
        }
    }
    eprintln!("UI 글자 폰트를 찾지 못했습니다 — 비트맵 폰트로 그립니다 (한글은 보이지 않습니다)");
    None
}

/// `ui/hud.ui.ron` → `"hud"`.
fn screen_id(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    Some(name.strip_suffix(SCREEN_EXT)?.to_ascii_lowercase())
}

/// 화면 번호 → 파일 경로. 이름은 **소문자**로 고정한다 (ext4 는 대소문자를 구분한다).
pub(crate) fn screen_path(id: &str) -> PathBuf {
    Path::new(SCREEN_DIR).join(format!("{}{SCREEN_EXT}", id.to_ascii_lowercase()))
}

/// 화면 번호로 쓸 수 있는 이름인가 — 파일 이름이 되므로 경로 문자를 막는다.
pub(crate) fn valid_screen_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// 화면을 RON 문자열로 — 편집기가 저장할 때 쓴다. 줄바꿈은 LF 고정.
///
/// 화면 파일은 편집기가 소유하므로 **통째로 다시 쓴다** — 그래서 손으로 쓴 주석은 남지 않는다
/// (머리 주석만 늘 새로 붙는다). 테마 파일은 반대로 `scale` 줄만 갈아 끼운다.
pub(crate) fn to_ron(screen: &ScreenFile) -> String {
    let config = ron::ser::PrettyConfig::new()
        .new_line("\n")
        .indentor("    ")
        .struct_names(false)
        .depth_limit(64);
    let body = ron::ser::to_string_pretty(screen, config).expect("화면 직렬화 실패");
    let header = "\
// 화면(UI) 한 장 — 위젯 트리. **에디터의 위젯 편집기가 이 파일을 다시 써낸다.**
// 메뉴 UI → 위젯 편집기. 저장하면 이 파일이 통째로 다시 쓰이므로 주석은 남지 않는다.
//
// pos·size 는 UI 픽셀(테마의 scale 을 곱하기 전)이고, anchor 기준점에서 오른쪽·아래가 +다.
// 글자 문구에 쓸 수 있는 값: {hp} {max_hp} {level} {exp} {exp_to_next}
// ⚠ 비트맵 폰트에 한글도 '/' 도 없다 — 화면 문구는 영문 대문자·숫자만.
";
    format!("{header}{body}\n")
}

/// 테마 파일의 `scale:` 줄만 갈아 끼운다 — 손으로 쓴 주석(폰트 구간 설명)을 지키려고.
/// 줄을 찾지 못하면 오류다 (그때는 부르는 쪽이 알린다).
pub(crate) fn patch_scale(text: &str, scale: u32) -> Result<String, String> {
    let mut out = Vec::with_capacity(text.lines().count());
    let mut found = false;
    for line in text.lines() {
        if !found && line.trim_start().starts_with("scale:") {
            let pad = " ".repeat(line.len() - line.trim_start().len());
            out.push(format!("{pad}scale: {},", scale.max(1)));
            found = true;
            continue;
        }
        out.push(line.to_owned());
    }
    if !found {
        return Err(String::from("scale 줄을 찾지 못함"));
    }
    let mut text = out.join("\n");
    text.push('\n');
    Ok(text)
}

/// `{hp}` 같은 이름을 값으로 바꾼다. 없는 이름은 그대로 남긴다 — 읽을 때 이미 거부했다.
fn fill(format: &str, v: &Values) -> String {
    let mut out = format.to_owned();
    for (name, value) in [
        ("hp", v.hp),
        ("max_hp", v.max_hp),
        ("level", v.level),
        ("exp", v.exp),
        ("exp_to_next", v.exp_to_next),
    ] {
        out = out.replace(&format!("{{{name}}}"), &value.to_string());
    }
    out
}

/// 위젯 트리 검사 — 빈 이름, 이름 중복(트리 전체), 없는 값 이름, 0 크기, 너무 깊은 트리.
fn check_widgets<'a>(
    widgets: &'a [Widget],
    depth: usize,
    seen: &mut Vec<&'a str>,
) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(format!("위젯 트리가 {MAX_DEPTH} 단계보다 깊음"));
    }
    for w in widgets {
        if w.id.trim().is_empty() {
            return Err(String::from("위젯 이름이 비어 있음"));
        }
        if seen.contains(&w.id.as_str()) {
            return Err(format!("위젯 이름 '{}' 이 두 번 나옴", w.id));
        }
        seen.push(&w.id);
        if let Some((sw, sh)) = w.size
            && (sw == 0 || sh == 0)
        {
            return Err(format!("'{}': 크기가 0", w.id));
        }
        match &w.kind {
            WidgetKind::Bar { .. } => {
                if w.size.is_none() {
                    return Err(format!("'{}': 막대는 size 가 필요함", w.id));
                }
            }
            WidgetKind::Text { format } => check_format(&w.id, format)?,
            WidgetKind::Button { label, action } => {
                if label.trim().is_empty() {
                    return Err(format!("'{}': 버튼 글자가 비어 있음", w.id));
                }
                if let Action::OpenScreen(target) | Action::OpenLevel(target) = action
                    && target.trim().is_empty()
                {
                    return Err(format!("'{}': {} 대상이 비어 있음", w.id, action.label()));
                }
            }
            WidgetKind::Items { columns } => {
                if *columns == 0 {
                    return Err(format!("'{}': 칸 수가 0", w.id));
                }
            }
            WidgetKind::Panel { .. } => {}
        }
        check_widgets(&w.children, depth + 1, seen)?;
    }
    Ok(())
}

fn check_format(id: &str, format: &str) -> Result<(), String> {
    for name in placeholder_names(format) {
        if !PLACEHOLDERS.contains(&name.as_str()) {
            return Err(format!(
                "'{id}': 알 수 없는 값 이름 {{{name}}} — 쓸 수 있는 것: {}",
                PLACEHOLDERS.join(", ")
            ));
        }
    }
    Ok(())
}

/// 문구에서 `{이름}` 을 뽑는다.
pub(crate) fn placeholder_names(format: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = format;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                names.push(after[..close].to_owned());
                rest = &after[close + 1..];
            }
            None => break,
        }
    }
    names
}

/// 단색 사각형 — 화면 좌표, `(x, y)` 는 왼쪽 위.
fn rect(r: Rect, color: [f32; 4], bias: f32, out: &mut Vec<RenderCommand>) {
    out.push(RenderCommand::DrawRect {
        center: r.center(),
        size: r.size(),
        rotation: 0.0,
        z: 0.0,
        depth_bias: bias,
        color,
        uv: UvRect::FULL,
        texture: TextureId::WHITE,
    });
}

/// 네 변만 — 테두리.
fn outline(r: Rect, color: [f32; 4], bias: f32, thick: f32, out: &mut Vec<RenderCommand>) {
    let t = thick.max(1.0);
    for side in [
        Rect::new(r.x, r.y, r.w, t),
        Rect::new(r.x, r.y + r.h - t, r.w, t),
        Rect::new(r.x, r.y, t, r.h),
        Rect::new(r.x + r.w - t, r.y, t, r.h),
    ] {
        rect(side, color, bias, out);
    }
}

/// 0 나눗셈 없는 비율.
fn ratio(value: u32, max: u32) -> f32 {
    if max == 0 {
        0.0
    } else {
        (value as f32 / max as f32).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font_image() -> Image {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let theme = Screens::read_theme(EMBEDDED_THEME).unwrap();
        let bytes = std::fs::read(path.join(&theme.font.image)).expect("폰트 그림을 열 수 없다");
        Image::decode_png(&bytes).unwrap()
    }

    /// 그림 없이 자리 계산만 하는 화면 — 배율 2, 글자는 8×16.
    fn measured() -> Screens {
        let image = font_image();
        let theme = Screens::read_theme(EMBEDDED_THEME).unwrap();
        let (font, panel) = Screens::build_font(&theme, (image.width(), image.height())).unwrap();
        Screens {
            font: Some(font),
            texture: TextureId::WHITE,
            // 테스트는 비트맵 폰트로 잰다 — 시스템 폰트는 머신마다 달라 값이 흔들린다.
            text: None,
            panel: Some(panel),
            theme: Some(theme),
            screens: BTreeMap::new(),
        }
    }

    fn text(id: &str, anchor: Anchor, pos: (i32, i32)) -> Widget {
        Widget {
            anchor,
            pos,
            ..Widget::new(
                id,
                WidgetKind::Text {
                    format: String::from("HP"),
                },
            )
        }
    }

    /// 비트맵 폰트는 **시스템 폰트를 못 찾았을 때의 폴백**이다. 그때도 영문·숫자는
    /// 보여야 하므로, 화면 문구의 ASCII 글자가 폰트에 다 있는지 본다.
    /// (한글은 비트맵 폰트에 없다 — 시스템 폰트가 있어야 나온다.)
    #[test]
    fn the_embedded_theme_fits_the_font_image() {
        let image = font_image();
        let theme = Screens::read_theme(EMBEDDED_THEME).unwrap();
        let (font, _) = Screens::build_font(&theme, (image.width(), image.height())).unwrap();
        for (id, text) in EMBEDDED_SCREENS {
            let screen = Screens::read_screen(text).unwrap_or_else(|e| panic!("{id}: {e}"));
            let mut stack: Vec<&Widget> = screen.widgets.iter().collect();
            while let Some(w) = stack.pop() {
                stack.extend(w.children.iter());
                let s = match &w.kind {
                    WidgetKind::Text { format } => format.clone(),
                    WidgetKind::Button { label, .. } => label.clone(),
                    _ => continue,
                };
                for c in s.chars().filter(|c| *c != ' ' && !"{}".contains(*c)) {
                    // 값 이름의 글자(소문자)와 한글은 건너뛴다 — 값 이름은 그려지지 않고,
                    // 한글은 시스템 폰트가 그린다.
                    if c.is_ascii_lowercase() || c == '_' || !c.is_ascii() {
                        continue;
                    }
                    assert!(
                        font.glyph(c).is_some(),
                        "화면 '{id}' 의 '{c}' 글자가 폰트에 없다"
                    );
                }
            }
        }
    }

    #[test]
    fn every_embedded_screen_loads() {
        for (id, text) in EMBEDDED_SCREENS {
            Screens::read_screen(text).unwrap_or_else(|e| panic!("내장 화면 '{id}': {e}"));
        }
    }

    /// 저장소의 화면 파일은 **편집기가 저장하는 모양 그대로**여야 한다 — 열어서 그냥 저장만
    /// 해도 바이트가 달라지면 diff 가 지저분해진다 (존 샘플 파일과 같은 규칙).
    #[test]
    fn shipped_screens_are_already_in_editor_format() {
        for (id, text) in EMBEDDED_SCREENS {
            let screen = Screens::read_screen(text).unwrap();
            assert_eq!(
                to_ron(&screen),
                *text,
                "ui/{id}.ui.ron 을 편집기로 저장하면 바이트가 바뀐다 — \
                 `cargo test -p world-editor -- --ignored regenerate_shipped_screens` 로 갱신하라"
            );
        }
    }

    #[test]
    #[ignore = "저장소의 화면 파일을 덮어쓴다 — 필요할 때만 직접 실행"]
    fn regenerate_shipped_screens() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for (id, text) in EMBEDDED_SCREENS {
            let screen = Screens::read_screen(text).unwrap();
            let path = root.join(screen_path(id));
            std::fs::write(&path, to_ron(&screen)).unwrap();
            println!("{} 갱신", path.display());
        }
    }

    #[test]
    fn a_bad_screen_says_why() {
        let cases = [
            ("(version: 9, name: \"x\", widgets: [])", "형식"),
            ("(version: 1, name: \" \", widgets: [])", "이름이 비어"),
            ("(version: 1, name: \"x\", widget: [])", "읽을 수 없음"),
            (
                "(version: 1, name: \"x\", widgets: [(id: \"\", anchor: TopLeft, pos: (0,0), kind: Panel(frame: false))])",
                "이름이 비어",
            ),
            (
                "(version: 1, name: \"x\", widgets: [(id: \"a\", anchor: TopLeft, pos: (0,0), kind: Bar(source: Hp, color: (1.0,0.0,0.0)))])",
                "size 가 필요",
            ),
            (
                "(version: 1, name: \"x\", widgets: [(id: \"a\", anchor: TopLeft, pos: (0,0), kind: Items(columns: 0))])",
                "칸 수가 0",
            ),
        ];
        for (text, want) in cases {
            let err = Screens::read_screen(text).expect_err(text);
            assert!(err.contains(want), "'{err}' 에 '{want}' 가 없다");
        }
    }

    #[test]
    fn duplicate_ids_anywhere_in_the_tree_are_refused() {
        let mut screen = ScreenFile::new("x");
        let mut parent = Widget::new("panel", WidgetKind::Panel { frame: false });
        parent.children.push(text("same", Anchor::TopLeft, (0, 0)));
        screen.widgets.push(parent);
        screen.widgets.push(text("same", Anchor::TopLeft, (0, 0)));
        let err = Screens::read_screen(&to_ron(&screen)).expect_err("같은 이름");
        assert!(err.contains("두 번"), "{err}");
    }

    #[test]
    fn unknown_value_names_are_refused() {
        let mut screen = ScreenFile::new("x");
        screen.widgets.push(Widget::new(
            "t",
            WidgetKind::Text {
                format: String::from("HP {hitpoints}"),
            },
        ));
        let err = Screens::read_screen(&to_ron(&screen)).expect_err("없는 값 이름");
        assert!(err.contains("hitpoints"), "{err}");
    }

    #[test]
    fn anchors_place_widgets_inside_their_parent() {
        let ui = measured();
        let values = Values::preview();
        let viewport = (800.0, 600.0);
        let mut screen = ScreenFile::new("x");
        // 부모: 가운데 200×100 패널. 자식: 오른쪽 아래 글자.
        let mut panel = Widget::new("panel", WidgetKind::Panel { frame: true });
        panel.anchor = Anchor::Center;
        panel.pos = (0, 0);
        panel.size = Some((100, 50)); // × 배율 2 = 200×100
        panel
            .children
            .push(text("child", Anchor::BottomRight, (-2, -3)));
        screen.widgets.push(panel);

        let laid = ui.layout(&screen, &values, viewport);
        assert_eq!(laid.len(), 2, "부모와 자식");
        let p = laid[0].rect;
        assert_eq!((p.x, p.y, p.w, p.h), (300.0, 250.0, 200.0, 100.0));

        let c = laid[1].rect;
        // 글자 "HP" = 8px × 2글자 × 배율 2 = 32, 높이 16 × 2 = 32.
        assert_eq!((c.w, c.h), (32.0, 32.0));
        // 부모의 오른쪽 아래 − 크기 + 오프셋(−2,−3) × 배율 2.
        assert_eq!(c.x, 300.0 + 200.0 - 32.0 - 4.0);
        assert_eq!(c.y, 250.0 + 100.0 - 32.0 - 6.0);
    }

    #[test]
    fn a_panel_without_a_size_fills_its_parent() {
        let ui = measured();
        let mut screen = ScreenFile::new("x");
        let mut root = Widget::new("root", WidgetKind::Panel { frame: false });
        root.size = None;
        root.anchor = Anchor::BottomRight; // 무시된다
        root.pos = (100, 100); // 무시된다
        root.children.push(text("child", Anchor::Center, (0, 0)));
        screen.widgets.push(root);

        let laid = ui.layout(&screen, &Values::preview(), (640.0, 480.0));
        assert_eq!(
            (
                laid[0].rect.x,
                laid[0].rect.y,
                laid[0].rect.w,
                laid[0].rect.h
            ),
            (0.0, 0.0, 640.0, 480.0)
        );
        // 자식은 화면 가운데.
        assert_eq!(laid[1].rect.center().x, 320.0);
    }

    #[test]
    fn hidden_widgets_take_their_children_with_them() {
        let ui = measured();
        let mut screen = ScreenFile::new("x");
        let mut window = Widget::new("window", WidgetKind::Panel { frame: true });
        window.show = Show::ItemsOpen;
        window.children.push(text("title", Anchor::TopLeft, (0, 0)));
        screen.widgets.push(window);

        let open = Values {
            show_items: true,
            ..Values::preview()
        };
        let shut = Values {
            show_items: false,
            ..Values::preview()
        };
        assert_eq!(ui.layout(&screen, &open, (800.0, 600.0)).len(), 2);
        assert!(
            ui.layout(&screen, &shut, (800.0, 600.0)).is_empty(),
            "숨으면 자식도 함께 숨는다"
        );
    }

    #[test]
    fn saving_and_reading_a_screen_round_trips() {
        let mut screen = ScreenFile::new("게임 화면");
        let mut window = Widget::new("window", WidgetKind::Panel { frame: true });
        window.show = Show::ItemsOpen;
        window
            .children
            .push(Widget::new("items", WidgetKind::Items { columns: 6 }));
        screen.widgets.push(window);
        screen.widgets.push(Widget {
            size: Some((110, 9)),
            ..Widget::new(
                "hp",
                WidgetKind::Bar {
                    source: BarSource::Hp,
                    color: (0.85, 0.25, 0.25),
                },
            )
        });
        screen.widgets.push(Widget::new(
            "start",
            WidgetKind::Button {
                label: String::from("START"),
                action: Action::OpenLevel(String::from("village")),
            },
        ));

        let text = to_ron(&screen);
        assert!(!text.contains('\r'), "줄바꿈은 LF 고정");
        assert_eq!(Screens::read_screen(&text).unwrap(), screen);
        // 두 번 써도 같은 바이트 — 저장을 반복해도 파일이 흔들리지 않는다.
        assert_eq!(to_ron(&Screens::read_screen(&text).unwrap()), text);
    }

    #[test]
    fn the_theme_scale_line_is_patched_in_place() {
        let text = "\
// 주석은 남아야 한다
(
    version: 1,
    // 배율 설명
    scale: 2,
)
";
        let out = patch_scale(text, 3).unwrap();
        assert!(out.contains("// 주석은 남아야 한다"));
        assert!(out.contains("// 배율 설명"));
        assert!(out.contains("    scale: 3,"));
        assert!(!out.contains("scale: 2,"));
        assert!(
            patch_scale("(version: 1)\n", 2).is_err(),
            "줄이 없으면 오류"
        );
    }

    #[test]
    fn screen_ids_come_from_file_names_and_stay_lowercase() {
        assert_eq!(
            screen_id(Path::new("ui/main_menu.ui.ron")).as_deref(),
            Some("main_menu")
        );
        assert_eq!(screen_id(Path::new("ui/hud.ron")), None);
        assert_eq!(screen_path("HUD"), Path::new("ui/hud.ui.ron"));
        assert!(valid_screen_id("main_menu"));
        assert!(!valid_screen_id("메인"), "파일 이름이 되므로 영문만");
        assert!(!valid_screen_id("a/b"));
        assert!(!valid_screen_id(""));
    }

    /// 버튼 히트 테스트는 **그리기와 같은 자리**를 봐야 한다 — 어긋나면 눌러도 반응이 없다.
    #[test]
    fn a_click_finds_the_button_drawn_at_that_spot() {
        let mut ui = measured();
        let mut screen = ScreenFile::new("메뉴");
        let mut window = Widget::new("window", WidgetKind::Panel { frame: true });
        window.anchor = Anchor::Center;
        window.size = Some((100, 60));
        window.children.push(Widget {
            anchor: Anchor::Center,
            pos: (0, 0),
            ..Widget::new(
                "start",
                WidgetKind::Button {
                    label: String::from("GO"),
                    action: Action::OpenLevel(String::from("village")),
                },
            )
        });
        screen.widgets.push(window);
        ui.set_screen("menu", screen.clone());

        let values = Values::preview();
        let viewport = (800.0, 600.0);
        let rect = ui
            .layout(&screen, &values, viewport)
            .into_iter()
            .find(|l| l.widget.id == "start")
            .expect("버튼")
            .rect;

        let hit = ui.button_at("menu", &values, viewport, rect.center());
        assert_eq!(
            hit,
            Some((
                String::from("start"),
                Action::OpenLevel(String::from("village"))
            ))
        );
        // 버튼 밖(창 안이지만 버튼이 아닌 곳)은 아무것도 아니다 — 창은 버튼이 아니다.
        let outside = Vec2::new(rect.x - 10.0, rect.y - 10.0);
        assert_eq!(ui.button_at("menu", &values, viewport, outside), None);
        assert_eq!(
            ui.button_at("없는화면", &values, viewport, rect.center()),
            None
        );
    }

    #[test]
    fn ratio_never_divides_by_zero_or_leaves_the_bar() {
        assert_eq!(ratio(5, 0), 0.0);
        assert_eq!(ratio(5, 10), 0.5);
        assert_eq!(ratio(50, 10), 1.0);
    }

    #[test]
    fn a_rect_hit_test_uses_its_edges() {
        let r = Rect::new(10.0, 20.0, 100.0, 40.0);
        assert!(r.contains(Vec2::new(10.0, 20.0)));
        assert!(r.contains(Vec2::new(110.0, 60.0)));
        assert!(!r.contains(Vec2::new(9.0, 40.0)));
    }
}
