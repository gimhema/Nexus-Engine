//! 플레이 화면 HUD — 게임 화면 안에 그리는 정보 (단계 2 P5·P6).
//!
//! ```text
//! ┌─ 뷰포트 ─────────────────────────────┐
//! │                    ┌── ITEMS ──┐     │  ← 인벤토리 창 (I)
//! │                    │ ▢ ▢ ▢ ▢ ▢ │     │
//! │ HP 161             └───────────┘     │
//! │ ████████░░░░                         │  ← HP 막대
//! │ ███░░░░░░░░░  LV 2  EXP 20           │  ← 경험치 막대
//! └──────────────────────────────────────┘
//! ```
//!
//! - **egui 를 쓰지 않는다.** egui 는 렌더러의 `ui` 피처 뒤에 있고 게임 런타임에는 들어가지 않는다
//!   (P5-0 결정). HUD 는 전부 쿼드 — 막대도, 창도, 글자도.
//! - 좌표는 [`Space::Screen`] — **뷰포트 픽셀, 좌상단 원점**이라 카메라를 따라가지 않는다.
//! - **무엇을 어디에 그릴지도 데이터다** (P6): `data/hud.ron` 의 `widgets`. 코드에는 위젯의
//!   *종류*(막대·글자·아이템 창)만 있고 위치·크기·색·문구는 파일에서 온다.
//!   에디터의 HUD 편집기(`hud_editor.rs`)가 그 파일을 고치고 저장한다.
//!
//! # 좌표 규약
//!
//! 위젯의 `pos`·`size` 는 **HUD 픽셀**(확대 배율 `scale` 을 곱하기 전)이다. `anchor` 는 화면의
//! 어느 모서리를 기준으로 삼을지이고, 위젯은 **그 모서리와 같은 쪽 자기 모서리**를 기준점에 맞춘다
//! (`TopRight` 면 위젯의 오른쪽 위). `pos` 는 언제나 오른쪽·아래가 +다 — 그래서 아래쪽·오른쪽
//! 앵커에서는 보통 음수가 된다. 모서리 기준이라 창 크기가 바뀌어도 자리가 유지된다.
//!
//! ⚠ 받아온 폰트에는 **한글도 `/` 도 없다.** 그래서 HUD 문구는 영문 대문자와 숫자만 쓴다
//! (`HP 161`, `LV 2`). 아이템 이름 같은 한글은 에디터 패널 쪽에 남아 있다 — 한글 HUD 는
//! 한글 비트맵 폰트를 만들거나 글립 래스터라이저를 붙여야 한다.

use std::path::Path;

use nexus_assets::{BitmapFont, Image};
use nexus_core::Vec2;
use nexus_render::{DEPTH_LAYER, DrawLayer, RenderCommand, Renderer, Space, TextureId, UvRect};
use serde::{Deserialize, Serialize};

use crate::play::PlaySession;

/// HUD 정의 파일 (작업 디렉터리 기준). 없으면 내장본을 쓴다 — 다른 데이터 파일과 같은 규칙.
pub(crate) const HUD_PATH: &str = "data/hud.ron";
const EMBEDDED: &str = include_str!("../../../data/hud.ron");

/// 실행 파일에 든 정의 — 디스크에 파일이 없을 때 편집기가 바탕으로 삼는다.
pub(crate) fn embedded_text() -> &'static str {
    EMBEDDED
}

const FORMAT_VERSION: u32 = 1;

// 색 (sRGB).
const BAR_BACK: [f32; 4] = [0.05, 0.05, 0.07, 0.85];
const BAR_EDGE: [f32; 4] = [0.85, 0.87, 0.92, 0.9];
const TEXT: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const PANEL_TINT: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const SLOT_BACK: [f32; 4] = [0.12, 0.12, 0.16, 0.9];
/// 편집 중 고른 위젯의 테두리.
const SELECT: [f32; 4] = [1.0, 0.82, 0.25, 1.0];

// 겹침 순서 — 같은 층 안에서 뒤로 갈수록 앞에 그려진다.
const BIAS_PANEL: f32 = 1.0 * DEPTH_LAYER;
const BIAS_BAR: f32 = 2.0 * DEPTH_LAYER;
const BIAS_FILL: f32 = 3.0 * DEPTH_LAYER;
const BIAS_TEXT: f32 = 4.0 * DEPTH_LAYER;
const BIAS_SELECT: f32 = 5.0 * DEPTH_LAYER;

/// 글자 문구에 쓸 수 있는 값 이름 — 여기 없는 이름은 파일을 읽을 때 거부한다.
pub(crate) const PLACEHOLDERS: &[&str] = &["hp", "max_hp", "level", "exp", "exp_to_next"];

// ─────────────────────────────────────────────────────────────────────────────
// 파일 형식 (HUD 편집기가 다시 써낸다 — 그래서 Serialize 도 붙는다)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HudFile {
    pub(crate) version: u32,
    pub(crate) font: FontFile,
    pub(crate) panel: PanelFile,
    /// HUD 확대 배율. **정수여야** 픽셀이 뭉개지지 않는다.
    pub(crate) scale: u32,
    /// 그릴 위젯 — 목록 순서대로 그린다 (뒤가 위).
    pub(crate) widgets: Vec<WidgetFile>,
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

/// 위젯 하나 — 편집기가 고치는 단위.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WidgetFile {
    /// 편집기 목록에 보이는 이름. 겹치면 읽을 때 거부한다.
    pub(crate) id: String,
    pub(crate) anchor: Anchor,
    /// 앵커 모서리에서의 오프셋 (HUD 픽셀, 오른쪽·아래가 +).
    pub(crate) pos: (i32, i32),
    pub(crate) kind: WidgetKind,
}

/// 기준 모서리.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Anchor {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Anchor {
    /// 네 가지 전부 — 편집기 드롭다운용.
    pub(crate) const ALL: [Self; 4] = [
        Self::TopLeft,
        Self::TopRight,
        Self::BottomLeft,
        Self::BottomRight,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::TopLeft => "왼쪽 위",
            Self::TopRight => "오른쪽 위",
            Self::BottomLeft => "왼쪽 아래",
            Self::BottomRight => "오른쪽 아래",
        }
    }

    fn right(self) -> bool {
        matches!(self, Self::TopRight | Self::BottomRight)
    }

    fn bottom(self) -> bool {
        matches!(self, Self::BottomLeft | Self::BottomRight)
    }

    /// 위젯 크기와 화면 크기를 알 때의 왼쪽 위 좌표 (화면 픽셀).
    fn origin(self, pos: (f32, f32), size: (f32, f32), viewport: (f32, f32)) -> Vec2 {
        let x = if self.right() {
            viewport.0 + pos.0 - size.0
        } else {
            pos.0
        };
        let y = if self.bottom() {
            viewport.1 + pos.1 - size.1
        } else {
            pos.1
        };
        Vec2::new(x, y)
    }

    /// 화면에서 끌어 옮긴 양(픽셀)을 `pos` 증분으로. 앵커 방향과 무관하게
    /// `pos` 는 오른쪽·아래가 + 이므로 부호를 바꿀 필요가 없다 — 이 함수는 그 사실을 문서로 남긴다.
    fn drag_to_pos(self, delta: Vec2) -> (f32, f32) {
        let _ = self;
        (delta.x, delta.y)
    }
}

/// 위젯 종류. **코드가 아는 것은 이 종류뿐**이고 나머지는 데이터다.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum WidgetKind {
    /// 비율 막대 (HP·경험치).
    Bar {
        source: BarSource,
        /// HUD 픽셀.
        size: (u32, u32),
        /// 채움 색 (sRGB).
        color: (f32, f32, f32),
    },
    /// 한 줄 글자. `{hp}` 처럼 값 이름을 넣는다 ([`PLACEHOLDERS`]).
    Text { format: String },
    /// 인벤토리 창 — **I 로 열고 닫는다**. 칸 수는 내용에 따라 늘어난다.
    Items { columns: u32 },
}

impl WidgetKind {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Bar { .. } => "막대",
            Self::Text { .. } => "글자",
            Self::Items { .. } => "아이템 창",
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

    /// 막대가 읽는 값 `(지금, 최대)`.
    fn values(self, play: &PlaySession) -> (u32, u32) {
        let Some(u) = play.world().unit(play.player()) else {
            return (0, 0);
        };
        match self {
            Self::Hp => (u.hp(), u.max_hp()),
            Self::Exp => (u.progress().exp(), u.progress().exp_to_next()),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 읽은 결과
// ─────────────────────────────────────────────────────────────────────────────

/// HUD 그림·글꼴과 위젯 목록. 그림을 못 읽으면 비어 있고 HUD 를 그리지 않는다
/// (그림이 없다고 플레이를 막지는 않는다 — 시트와 같은 규칙).
#[derive(Debug)]
pub(crate) struct Hud {
    font: Option<BitmapFont>,
    texture: TextureId,
    panel: Option<PanelUv>,
    /// 파일 그대로 — 편집기가 고치고 다시 써낸다.
    file: Option<HudFile>,
}

impl Default for Hud {
    /// 그림을 못 읽었을 때 — HUD 를 그리지 않는다.
    fn default() -> Self {
        Self {
            font: None,
            texture: TextureId::WHITE,
            panel: None,
            file: None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct PanelUv {
    /// 창 그림의 픽셀 사각형.
    rect: (f32, f32, f32, f32),
    border: f32,
    image: (f32, f32),
}

/// 화면 사각형 — 왼쪽 위 기준 (HUD 좌표 규약).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
}

impl Rect {
    fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    fn center(self) -> Vec2 {
        Vec2::new(self.x + self.w * 0.5, self.y + self.h * 0.5)
    }

    fn size(self) -> Vec2 {
        Vec2::new(self.w, self.h)
    }

    /// 사방으로 `d` 만큼 넓힌 사각형 (테두리용).
    fn grow(self, d: f32) -> Self {
        Self::new(self.x - d, self.y - d, self.w + d * 2.0, self.h + d * 2.0)
    }

    /// 왼쪽에서 `t` 비율만큼 (막대 채움용).
    fn fraction(self, t: f32) -> Self {
        Self::new(self.x, self.y, self.w * t, self.h)
    }

    /// 점이 안에 있는가 — 편집기의 피킹.
    pub(crate) fn contains(self, p: Vec2) -> bool {
        p.x >= self.x && p.x <= self.x + self.w && p.y >= self.y && p.y <= self.y + self.h
    }
}

impl Hud {
    /// 정의를 읽고 그림을 GPU 에 올린다. 실패하면 이유를 돌려주고 HUD 는 비어 있다.
    pub(crate) fn load(renderer: &mut dyn Renderer) -> (Self, Option<String>) {
        let text = match std::fs::read_to_string(HUD_PATH) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => EMBEDDED.to_owned(),
            Err(e) => return (Self::default(), Some(format!("{HUD_PATH}: {e}"))),
        };
        match Self::build(&text, renderer) {
            Ok(hud) => (hud, None),
            Err(e) => (Self::default(), Some(format!("{HUD_PATH}: {e}"))),
        }
    }

    /// 정의를 읽고 형식을 확인한다 — 그림은 아직 읽지 않는다.
    pub(crate) fn read_file(text: &str) -> Result<HudFile, String> {
        let file: HudFile = ron::from_str(text).map_err(|e| format!("읽을 수 없음 — {e}"))?;
        if file.version != FORMAT_VERSION {
            return Err(format!(
                "형식 {} 은(는) 읽을 수 없음 (이 에디터는 {FORMAT_VERSION})",
                file.version
            ));
        }
        if file.scale == 0 {
            return Err(String::from("scale 은 1 이상의 정수여야 함"));
        }
        check_widgets(&file.widgets)?;
        Ok(file)
    }

    /// 그림 크기를 알아야 할 수 있는 검사 — 글자 칸과 창이 그림 안에 들어가는지.
    /// 렌더러가 필요 없어 테스트가 그대로 부른다.
    fn build_font(file: &HudFile, size: (u32, u32)) -> Result<(BitmapFont, PanelUv), String> {
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

    fn build(text: &str, renderer: &mut dyn Renderer) -> Result<Self, String> {
        let file = Self::read_file(text)?;
        let bytes = std::fs::read(Path::new(&file.font.image))
            .map_err(|e| format!("{}: {e}", file.font.image))?;
        let image = Image::decode_png(&bytes).map_err(|e| format!("{}: {e}", file.font.image))?;
        let (font, panel) = Self::build_font(&file, (image.width(), image.height()))?;

        let texture = renderer
            .load_texture(&image.desc("hud-font"))
            .map_err(|e| format!("{}: {e}", file.font.image))?;

        Ok(Self {
            font: Some(font),
            texture,
            panel: Some(panel),
            file: Some(file),
        })
    }

    /// 지금 정의 — 편집기가 읽고 고친다. 그림을 못 읽었으면 `None`.
    pub(crate) fn file(&self) -> Option<&HudFile> {
        self.file.as_ref()
    }

    /// 편집기가 고친 위젯·배율을 받아들인다.
    ///
    /// 폰트 구간·창 그림은 여기서 바꾸지 않는다 — 그림은 시작할 때 한 번 올리므로
    /// 그쪽을 고치려면 파일을 고치고 에디터를 다시 시작해야 한다 (시트와 같은 규칙).
    pub(crate) fn set_widgets(&mut self, widgets: Vec<WidgetFile>, scale: u32) {
        if let Some(file) = self.file.as_mut() {
            file.widgets = widgets;
            file.scale = scale.max(1);
        }
    }

    fn scale(&self) -> f32 {
        self.file.as_ref().map_or(1.0, |f| f.scale.max(1) as f32)
    }

    /// 위젯의 화면 사각형 — 편집기의 피킹·테두리와 그리기가 **같은 계산**을 쓴다.
    pub(crate) fn widget_rect(
        &self,
        widget: &WidgetFile,
        play: &PlaySession,
        viewport: (f32, f32),
    ) -> Rect {
        let s = self.scale();
        let size = match &widget.kind {
            WidgetKind::Bar { size, .. } => (size.0.max(1) as f32 * s, size.1.max(1) as f32 * s),
            WidgetKind::Text { format } => {
                let text = fill(format, play);
                self.font.as_ref().map_or((0.0, 0.0), |f| {
                    (f.width(&text) as f32 * s, f.line_height() as f32 * s)
                })
            }
            WidgetKind::Items { columns } => self.items_size(*columns, play),
        };
        let pos = (widget.pos.0 as f32 * s, widget.pos.1 as f32 * s);
        let at = widget.anchor.origin(pos, size, viewport);
        Rect::new(at.x, at.y, size.0, size.1)
    }

    /// 화면에서 끌어 옮긴 양을 `pos`(HUD 픽셀) 증분으로 바꾼다 — 배율만큼 나눈다.
    /// `snap` 이면 8 HUD 픽셀 격자에 맞춘다.
    pub(crate) fn drag_delta(&self, widget: &WidgetFile, delta: Vec2, snap: bool) -> (i32, i32) {
        let s = self.scale();
        let (dx, dy) = widget.anchor.drag_to_pos(delta / s);
        let grid = if snap { 8.0 } else { 1.0 };
        let round = |v: f32| (v / grid).round() * grid;
        (round(dx) as i32, round(dy) as i32)
    }

    /// 인벤토리 창의 크기 — 칸 수와 내용에 따라 늘어난다.
    fn items_size(&self, columns: u32, play: &PlaySession) -> (f32, f32) {
        let s = self.scale();
        let (Some(font), Some(panel)) = (self.font.as_ref(), self.panel) else {
            return (0.0, 0.0);
        };
        let cols = columns.max(1) as usize;
        let rows = play.hud_slots().len().div_ceil(cols).max(1);
        let cell = 18.0 * s;
        let gap = 3.0 * s;
        let pad = panel.border * s;
        (
            cols as f32 * (cell + gap) + gap + pad * 2.0,
            rows as f32 * (cell + gap) + gap + font.line_height() as f32 * s + pad * 2.0,
        )
    }

    /// 플레이 화면 위에 HUD 를 그린다. `viewport` 는 씬을 그리는 영역의 크기(물리 픽셀)다.
    ///
    /// `selected` 는 HUD 편집기가 고른 위젯 — 테두리를 그려 준다.
    /// 명령을 낸 뒤 좌표 공간을 [`Space::World`] 로 되돌린다 — 다음 프레임이 월드부터 그리므로.
    pub(crate) fn build_commands(
        &self,
        play: &PlaySession,
        viewport: (f32, f32),
        show_items: bool,
        selected: Option<&str>,
        out: &mut Vec<RenderCommand>,
    ) {
        let (Some(font), Some(panel), Some(file)) =
            (self.font.as_ref(), self.panel, self.file.as_ref())
        else {
            return;
        };
        if play.world().unit(play.player()).is_none() {
            return;
        }
        out.push(RenderCommand::SetSpace(Space::Screen));
        out.push(RenderCommand::SetLayer(DrawLayer::Overlay));

        for widget in &file.widgets {
            // 아이템 창은 I 로 열고 닫는다 — 닫혀 있어도 편집 중이면 보여 준다.
            let hidden = matches!(widget.kind, WidgetKind::Items { .. })
                && !show_items
                && selected != Some(widget.id.as_str());
            if hidden {
                continue;
            }
            let r = self.widget_rect(widget, play, viewport);
            match &widget.kind {
                WidgetKind::Bar { source, color, .. } => {
                    let (value, max) = source.values(play);
                    self.bar(r, ratio(value, max), [color.0, color.1, color.2, 1.0], out);
                }
                WidgetKind::Text { format } => {
                    self.text(font, &fill(format, play), r.x, r.y, out);
                }
                WidgetKind::Items { columns } => self.items(font, panel, play, *columns, r, out),
            }
            if selected == Some(widget.id.as_str()) {
                outline(r.grow(self.scale()), SELECT, BIAS_SELECT, self.scale(), out);
            }
        }

        out.push(RenderCommand::SetSpace(Space::World));
    }

    /// 테두리 + 바탕 + 채움 — 막대 하나.
    fn bar(&self, r: Rect, ratio: f32, color: [f32; 4], out: &mut Vec<RenderCommand>) {
        rect(r.grow(self.scale()), BAR_EDGE, BIAS_PANEL, out);
        rect(r, BAR_BACK, BIAS_BAR, out);
        if ratio > 0.0 {
            rect(r.fraction(ratio), color, BIAS_FILL, out);
        }
    }

    /// 한 줄 글자. `(x, y)` 는 왼쪽 위.
    fn text(&self, font: &BitmapFont, s: &str, x: f32, y: f32, out: &mut Vec<RenderCommand>) {
        let scale = self.scale();
        for (dx, glyph) in font.layout(s) {
            let (gw, gh) = (glyph.size.0 as f32 * scale, glyph.size.1 as f32 * scale);
            out.push(RenderCommand::DrawRect {
                center: Vec2::new(x + dx as f32 * scale + gw * 0.5, y + gh * 0.5),
                size: Vec2::new(gw, gh),
                rotation: 0.0,
                z: 0.0,
                depth_bias: BIAS_TEXT,
                color: TEXT,
                uv: glyph.uv,
                texture: self.texture,
            });
        }
    }

    /// 인벤토리 창 — 9-slice 배경 + 아이템 칸 (색 + 개수).
    fn items(
        &self,
        font: &BitmapFont,
        panel: PanelUv,
        play: &PlaySession,
        columns: u32,
        r: Rect,
        out: &mut Vec<RenderCommand>,
    ) {
        let s = self.scale();
        let slots = play.hud_slots();
        let cols = columns.max(1) as usize;
        let cell = 18.0 * s;
        let gap = 3.0 * s;
        let pad = panel.border * s;

        self.nine_slice(panel, r, out);
        self.text(font, "ITEMS", r.x + pad, r.y + pad, out);

        let top = r.y + pad + font.line_height() as f32 * s;
        for (i, (color, count)) in slots.iter().enumerate() {
            let cx = r.x + pad + gap + (i % cols) as f32 * (cell + gap);
            let cy = top + gap + (i / cols) as f32 * (cell + gap);
            let slot = Rect::new(cx, cy, cell, cell);
            rect(slot, SLOT_BACK, BIAS_BAR, out);
            rect(slot.grow(-2.0 * s), *color, BIAS_FILL, out);
            if *count > 1 {
                self.text(font, &format!("x{count}"), cx, cy + cell - 8.0 * s, out);
            }
        }
    }

    /// 창 그림을 아홉 조각으로 늘린다 — 모서리는 그대로, 변은 한 방향으로, 가운데만 양방향으로.
    fn nine_slice(&self, p: PanelUv, r: Rect, out: &mut Vec<RenderCommand>) {
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
                    depth_bias: BIAS_PANEL,
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

/// `{hp}` 같은 이름을 값으로 바꾼다. 없는 이름은 그대로 남긴다 — 읽을 때 이미 거부했다.
fn fill(format: &str, play: &PlaySession) -> String {
    let Some(u) = play.world().unit(play.player()) else {
        return format.to_owned();
    };
    let p = u.progress();
    let mut out = format.to_owned();
    for (name, value) in [
        ("hp", u.hp()),
        ("max_hp", u.max_hp()),
        ("level", p.level()),
        ("exp", p.exp()),
        ("exp_to_next", p.exp_to_next()),
    ] {
        out = out.replace(&format!("{{{name}}}"), &value.to_string());
    }
    out
}

/// 위젯 목록 검사 — 빈 이름, 이름 중복, 없는 값 이름, 0 크기.
fn check_widgets(widgets: &[WidgetFile]) -> Result<(), String> {
    let mut seen: Vec<&str> = Vec::new();
    for w in widgets {
        if w.id.trim().is_empty() {
            return Err(String::from("위젯 이름이 비어 있음"));
        }
        if seen.contains(&w.id.as_str()) {
            return Err(format!("위젯 이름 '{}' 이 두 번 나옴", w.id));
        }
        seen.push(&w.id);
        match &w.kind {
            WidgetKind::Bar { size, .. } => {
                if size.0 == 0 || size.1 == 0 {
                    return Err(format!("'{}': 막대 크기가 0", w.id));
                }
            }
            WidgetKind::Text { format } => {
                for name in placeholder_names(format) {
                    if !PLACEHOLDERS.contains(&name.as_str()) {
                        return Err(format!(
                            "'{}': 알 수 없는 값 이름 {{{name}}} — 쓸 수 있는 것: {}",
                            w.id,
                            PLACEHOLDERS.join(", ")
                        ));
                    }
                }
            }
            WidgetKind::Items { columns } => {
                if *columns == 0 {
                    return Err(format!("'{}': 칸 수가 0", w.id));
                }
            }
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

/// 네 변만 — 고른 위젯 테두리.
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

/// 정의를 RON 문자열로 — HUD 편집기가 저장할 때 쓴다. 줄바꿈은 LF 고정.
pub(crate) fn to_ron(file: &HudFile) -> String {
    let config = ron::ser::PrettyConfig::new()
        .new_line("\n")
        .indentor("    ");
    let body = ron::ser::to_string_pretty(file, config).expect("HUD 정의 직렬화 실패");
    let header = "\
// 플레이 화면 HUD — 폰트·창 그림과 위젯 배치. **클라이언트만 보는 데이터다.**
// 에디터의 HUD 편집기(메뉴 플레이 → HUD 편집기)가 이 파일을 고치고 저장한다.
//
// pos·size 는 HUD 픽셀(scale 을 곱하기 전)이고, anchor 모서리에서 오른쪽·아래가 +다.
// 글자 문구에 쓸 수 있는 값: {hp} {max_hp} {level} {exp} {exp_to_next}
// ⚠ 이 폰트에는 한글도 '/' 도 없다 — HUD 문구는 영문 대문자·숫자만.
";
    format!("{header}{body}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font_image() -> Image {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let file = Hud::read_file(EMBEDDED).unwrap();
        let bytes = std::fs::read(path.join(&file.font.image)).expect("폰트 그림을 열 수 없다");
        Image::decode_png(&bytes).unwrap()
    }

    fn widget(id: &str, kind: WidgetKind) -> WidgetFile {
        WidgetFile {
            id: id.to_owned(),
            anchor: Anchor::TopLeft,
            pos: (0, 0),
            kind,
        }
    }

    #[test]
    fn the_embedded_definition_fits_the_font_image() {
        let image = font_image();
        let file = Hud::read_file(EMBEDDED).unwrap();
        let (font, panel) = Hud::build_font(&file, (image.width(), image.height())).unwrap();
        // HUD 문구에 쓰는 글자가 다 있어야 한다 (없으면 그 자리가 빈칸으로 나간다).
        for c in "HPLVEXPITEMSx0123456789".chars() {
            assert!(font.glyph(c).is_some(), "'{c}' 가 폰트에 없다");
        }
        assert!(
            font.glyph('/').is_none(),
            "이 폰트에는 '/' 가 없다 — 문구를 그렇게 짰다"
        );
        assert!(panel.border > 0.0);
        assert!(!file.widgets.is_empty(), "기본 위젯이 있어야 한다");
    }

    #[test]
    fn a_bad_definition_says_why() {
        let image = font_image();
        let size = (image.width(), image.height());
        let err = Hud::read_file(&EMBEDDED.replace("version: 1", "version: 9")).unwrap_err();
        assert!(err.contains("형식 9"), "{err}");
        let err = Hud::read_file(&EMBEDDED.replace("scale: 2", "scale: 0")).unwrap_err();
        assert!(err.contains("scale"), "{err}");

        // 필드 이름 오타가 기본값으로 조용히 넘어가지 않는다.
        assert!(Hud::read_file(&EMBEDDED.replace("advance:", "advnace:")).is_err());

        let file = Hud::read_file(&EMBEDDED.replace("border: 12", "border: 60")).unwrap();
        let err = Hud::build_font(&file, size).unwrap_err();
        assert!(err.contains("border"), "{err}");

        // 글자 칸이 그림 밖으로 나가면 옆 칸을 비추게 되므로 거부한다.
        let file = Hud::read_file(&EMBEDDED.replace("x: 216, y: 0", "x: 236, y: 0")).unwrap();
        let err = Hud::build_font(&file, size).unwrap_err();
        assert!(err.contains("그림 밖"), "{err}");
    }

    #[test]
    fn unknown_value_names_and_duplicate_ids_are_refused() {
        let err = check_widgets(&[
            widget(
                "a",
                WidgetKind::Text {
                    format: String::from("HP {hp}"),
                },
            ),
            widget(
                "a",
                WidgetKind::Text {
                    format: String::new(),
                },
            ),
        ])
        .unwrap_err();
        assert!(err.contains("두 번"), "{err}");

        let err = check_widgets(&[widget(
            "t",
            WidgetKind::Text {
                format: String::from("HP {hitpoints}"),
            },
        )])
        .unwrap_err();
        assert!(err.contains("hitpoints") && err.contains("max_hp"), "{err}");

        assert!(
            check_widgets(&[widget(
                "t",
                WidgetKind::Text {
                    format: String::from("LV {level}  EXP {exp} {exp_to_next}")
                }
            )])
            .is_ok(),
            "정해진 이름은 통과한다"
        );
    }

    #[test]
    fn anchors_measure_from_their_own_corner() {
        let viewport = (800.0, 600.0);
        let size = (100.0, 20.0);
        // 왼쪽 위: 오프셋 그대로.
        assert_eq!(
            Anchor::TopLeft.origin((10.0, 12.0), size, viewport),
            Vec2::new(10.0, 12.0)
        );
        // 오른쪽 아래: 위젯의 오른쪽 아래 모서리가 기준점에 붙는다.
        assert_eq!(
            Anchor::BottomRight.origin((-10.0, -12.0), size, viewport),
            Vec2::new(800.0 - 10.0 - 100.0, 600.0 - 12.0 - 20.0)
        );
        assert_eq!(
            Anchor::TopRight.origin((-10.0, 12.0), size, viewport),
            Vec2::new(690.0, 12.0)
        );
    }

    #[test]
    fn saving_and_reading_a_definition_round_trips() {
        let file = Hud::read_file(EMBEDDED).unwrap();
        let text = to_ron(&file);
        assert!(!text.contains('\r'), "줄바꿈은 LF 고정");
        assert_eq!(Hud::read_file(&text).unwrap(), file);
        assert_eq!(
            to_ron(&Hud::read_file(&text).unwrap()),
            text,
            "두 번 써도 같다"
        );
    }

    #[test]
    fn ratio_never_divides_by_zero_or_leaves_the_bar() {
        assert_eq!(ratio(0, 0), 0.0);
        assert_eq!(ratio(5, 10), 0.5);
        assert_eq!(ratio(30, 10), 1.0, "넘쳐도 막대를 벗어나지 않는다");
    }

    #[test]
    fn a_rect_hit_test_uses_its_edges() {
        let r = Rect::new(10.0, 20.0, 30.0, 40.0);
        assert!(r.contains(Vec2::new(10.0, 20.0)) && r.contains(Vec2::new(40.0, 60.0)));
        assert!(!r.contains(Vec2::new(9.0, 30.0)) && !r.contains(Vec2::new(20.0, 61.0)));
    }
}
