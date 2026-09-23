//! 플레이 화면 HUD — 게임 화면 안에 그리는 정보 (단계 2 P5).
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
//! - 글자는 [`BitmapFont`] 로 그린다. 폰트·창 그림 정의는 `data/hud.ron` 이다.
//!
//! ⚠ 받아온 폰트에는 **한글도 `/` 도 없다.** 그래서 HUD 문구는 영문 대문자와 숫자만 쓴다
//! (`HP 161`, `LV 2`). 아이템 이름 같은 한글은 에디터 패널 쪽에 남아 있다 — 한글 HUD 는
//! 한글 비트맵 폰트를 만들거나 글립 래스터라이저를 붙여야 한다.

use std::path::Path;

use nexus_assets::{BitmapFont, Image};
use nexus_core::Vec2;
use nexus_render::{DEPTH_LAYER, DrawLayer, RenderCommand, Renderer, Space, TextureId, UvRect};
use serde::Deserialize;

use crate::play::PlaySession;

/// HUD 정의 파일 (작업 디렉터리 기준). 없으면 내장본을 쓴다 — 다른 데이터 파일과 같은 규칙.
pub(crate) const HUD_PATH: &str = "data/hud.ron";
const EMBEDDED: &str = include_str!("../../../data/hud.ron");

const FORMAT_VERSION: u32 = 1;

// 색 (sRGB).
const BAR_BACK: [f32; 4] = [0.05, 0.05, 0.07, 0.85];
const BAR_EDGE: [f32; 4] = [0.85, 0.87, 0.92, 0.9];
const BAR_HP: [f32; 4] = [0.85, 0.25, 0.25, 1.0];
const BAR_EXP: [f32; 4] = [0.35, 0.75, 0.95, 1.0];
const TEXT: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const PANEL_TINT: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const SLOT_BACK: [f32; 4] = [0.12, 0.12, 0.16, 0.9];

// 겹침 순서 — 같은 층 안에서 뒤로 갈수록 앞에 그려진다.
const BIAS_PANEL: f32 = 1.0 * DEPTH_LAYER;
const BIAS_BAR: f32 = 2.0 * DEPTH_LAYER;
const BIAS_FILL: f32 = 3.0 * DEPTH_LAYER;
const BIAS_TEXT: f32 = 4.0 * DEPTH_LAYER;

// ─────────────────────────────────────────────────────────────────────────────
// 파일 형식
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HudFile {
    version: u32,
    font: FontFile,
    panel: PanelFile,
    scale: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FontFile {
    image: String,
    advance: u32,
    line_height: u32,
    runs: Vec<RunFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunFile {
    chars: String,
    x: u32,
    y: u32,
    size: (u32, u32),
}

/// 화면 사각형 — 왼쪽 위 기준 (HUD 좌표 규약).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
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
}

/// 9-slice 로 늘려 쓰는 창 그림.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PanelFile {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    /// 늘어나지 않는 가장자리 두께 (픽셀).
    border: u32,
}

// ─────────────────────────────────────────────────────────────────────────────
// 읽은 결과
// ─────────────────────────────────────────────────────────────────────────────

/// HUD 그림과 글꼴. 그림을 못 읽으면 비어 있고, 그때는 HUD 를 그리지 않는다
/// (그림이 없다고 플레이를 막지는 않는다 — 시트와 같은 규칙).
#[derive(Debug)]
pub(crate) struct Hud {
    font: Option<BitmapFont>,
    texture: TextureId,
    panel: Option<PanelUv>,
    scale: f32,
}

impl Default for Hud {
    /// 그림을 못 읽었을 때 — HUD 를 그리지 않는다.
    fn default() -> Self {
        Self {
            font: None,
            texture: TextureId::WHITE,
            panel: None,
            scale: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct PanelUv {
    /// 9-slice 의 아홉 조각 UV 와 가장자리 두께(픽셀).
    rect: (f32, f32, f32, f32),
    border: f32,
    image: (f32, f32),
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
    fn read_file(text: &str) -> Result<HudFile, String> {
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
            scale: file.scale as f32,
        })
    }

    /// 플레이 화면 위에 HUD 를 그린다. `viewport` 는 씬을 그리는 영역의 크기(물리 픽셀)다.
    ///
    /// 명령을 낸 뒤 좌표 공간을 [`Space::World`] 로 되돌린다 — 다음 프레임이 월드부터 그리므로.
    pub(crate) fn build_commands(
        &self,
        play: &PlaySession,
        viewport: (f32, f32),
        show_items: bool,
        out: &mut Vec<RenderCommand>,
    ) {
        let (Some(font), Some(panel)) = (self.font.as_ref(), self.panel) else {
            return;
        };
        let Some(me) = play.world().unit(play.player()) else {
            return;
        };
        let s = self.scale;
        out.push(RenderCommand::SetSpace(Space::Screen));
        out.push(RenderCommand::SetLayer(DrawLayer::Overlay));

        // ── 왼쪽 아래: HP · 경험치 막대 ──────────────────────────────────────
        let margin = 12.0 * s;
        let bar_w = 110.0 * s;
        let hp_h = 9.0 * s;
        let exp_h = 5.0 * s;
        let bottom = viewport.1 - margin;

        let progress = me.progress();
        let hp_ratio = ratio(me.hp(), me.max_hp());
        let exp_ratio = ratio(progress.exp(), progress.exp_to_next());

        let exp_top = bottom - exp_h;
        self.bar(
            Rect::new(margin, exp_top, bar_w, exp_h),
            exp_ratio,
            BAR_EXP,
            out,
        );
        let hp_top = exp_top - 3.0 * s - hp_h;
        self.bar(
            Rect::new(margin, hp_top, bar_w, hp_h),
            hp_ratio,
            BAR_HP,
            out,
        );

        let text_h = font.line_height() as f32 * s;
        self.text(
            font,
            &format!("HP {}", me.hp()),
            margin,
            hp_top - text_h,
            out,
        );
        self.text(
            font,
            &format!("LV {}  EXP {}", progress.level(), progress.exp()),
            margin + bar_w + 8.0 * s,
            bottom - text_h,
            out,
        );

        // ── 인벤토리 창 ──────────────────────────────────────────────────────
        if show_items {
            self.items(font, panel, play, viewport, out);
        }

        out.push(RenderCommand::SetSpace(Space::World));
    }

    /// 테두리 + 바탕 + 채움 — 막대 하나.
    fn bar(&self, r: Rect, ratio: f32, color: [f32; 4], out: &mut Vec<RenderCommand>) {
        rect(r.grow(self.scale), BAR_EDGE, BIAS_PANEL, out);
        rect(r, BAR_BACK, BIAS_BAR, out);
        if ratio > 0.0 {
            rect(r.fraction(ratio), color, BIAS_FILL, out);
        }
    }

    /// 한 줄 글자. `(x, y)` 는 왼쪽 위.
    fn text(&self, font: &BitmapFont, s: &str, x: f32, y: f32, out: &mut Vec<RenderCommand>) {
        let scale = self.scale;
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
        viewport: (f32, f32),
        out: &mut Vec<RenderCommand>,
    ) {
        let s = self.scale;
        let slots = play.hud_slots();
        let cols = 6;
        let cell = 18.0 * s;
        let gap = 3.0 * s;
        let rows = slots.len().div_ceil(cols).max(1);
        let inner_w = cols as f32 * (cell + gap) + gap;
        let inner_h = rows as f32 * (cell + gap) + gap + font.line_height() as f32 * s;
        let pad = panel.border * s;
        let (w, h) = (inner_w + pad * 2.0, inner_h + pad * 2.0);
        let x = viewport.0 - w - 12.0 * s;
        let y = 12.0 * s;

        self.nine_slice(panel, Rect::new(x, y, w, h), out);
        self.text(font, "ITEMS", x + pad, y + pad, out);

        let top = y + pad + font.line_height() as f32 * s;
        for (i, (color, count)) in slots.iter().enumerate() {
            let cx = x + pad + gap + (i % cols) as f32 * (cell + gap);
            let cy = top + gap + (i / cols) as f32 * (cell + gap);
            rect(Rect::new(cx, cy, cell, cell), SLOT_BACK, BIAS_BAR, out);
            rect(
                Rect::new(cx, cy, cell, cell).grow(-2.0 * s),
                *color,
                BIAS_FILL,
                out,
            );
            if *count > 1 {
                self.text(font, &format!("x{count}"), cx, cy + cell - 8.0 * s, out);
            }
        }
    }

    /// 창 그림을 아홉 조각으로 늘린다 — 모서리는 그대로, 변은 한 방향으로, 가운데만 양방향으로.
    fn nine_slice(&self, p: PanelUv, r: Rect, out: &mut Vec<RenderCommand>) {
        let (x, y, w, h) = (r.x, r.y, r.w, r.h);
        let b = p.border * self.scale;
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

    /// 저장소에 든 폰트 그림 크기 — 정의가 이 그림에 맞는지 본다.
    fn font_image() -> Image {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let file = Hud::read_file(EMBEDDED).unwrap();
        let bytes = std::fs::read(path.join(&file.font.image)).expect("폰트 그림을 열 수 없다");
        Image::decode_png(&bytes).unwrap()
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
    fn ratio_never_divides_by_zero_or_leaves_the_bar() {
        assert_eq!(ratio(0, 0), 0.0);
        assert_eq!(ratio(5, 10), 0.5);
        assert_eq!(ratio(30, 10), 1.0, "넘쳐도 막대를 벗어나지 않는다");
    }
}
