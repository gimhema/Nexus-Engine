//! 지형 그림 팔레트 창 (P4) — 그림 파일을 눈으로 보고 칸을 골라 붓으로 쓴다.
//!
//! ```text
//! ┌─ 지형 팔레트 ────────────────────────────────────────┐
//! │ [지면] [오브젝트]   칸 16px   확대 ×2                 │
//! │ ┌ 그림 파일 ─────┐ ┌ 그림 (격자) ──────────────────┐ │
//! │ │ …/overworld.png │ │ ▦▦▦▦▦  ← 칸 클릭 = 지면      │ │
//! │ │ …/objects.png   │ │ ▦▦▦▦▦  ← 끌기   = 오브젝트   │ │
//! │ └─────────────────┘ └──────────────────────────────┘ │
//! └──────────────────────────────────────────────────────┘
//! ```
//!
//! 고른 조각은 [`Pick`] 으로 편집기에 넘어가고, 편집기가 `data/terrain.ron` 에 **항목을 자동으로
//! 추가**한 뒤(이미 있으면 그 번호를 쓴다) 그 번호를 붓으로 삼는다 (`terrain::add_to_disk`).
//! 창에서 고르는 것은 **편집이 아니다** — 언두에 남지 않는다(선택과 같은 규칙). 칠하는 것이 편집이다.
//!
//! 새 그림 파일에서 처음 고르면 타일셋도 함께 만들어지는데, 그때 **칸 크기를 1m 로 본다**
//! (`pixels_per_meter` = 칸 크기). 받아온 팩은 대부분 타일 한 칸이 캐릭터 발판 하나라 맞는다.

use std::collections::HashMap;
use std::path::{Component, Path};

use nexus_assets::Image;
use nexus_render_wgpu::egui;

use crate::terrain::{ArtKind, Pick};
use crate::ui::UiActions;

/// 그림을 찾는 폴더 (작업 디렉터리 기준).
const ASSET_DIR: &str = "assets";
/// 칸 크기 기본값 (px) — 받아온 팩의 타일 한 칸.
const DEFAULT_CELL: u32 = 16;
/// 칸 크기로 받는 범위.
const CELL_RANGE: core::ops::RangeInclusive<u32> = 4..=256;
/// 확대 배율 선택지. 정수 배율이라 픽셀이 뭉개지지 않는다.
const ZOOMS: [f32; 5] = [1.0, 2.0, 3.0, 4.0, 6.0];

/// 창 상태. 닫아도 고른 그림·확대율은 남긴다.
pub(crate) struct Palette {
    open: bool,
    /// 지면 칸을 고르나, 오브젝트 사각형을 고르나.
    tab: ArtKind,
    /// `assets/` 아래 PNG 들 — 작업 디렉터리 기준, `/` 구분.
    images: Vec<String>,
    selected: Option<String>,
    cell: u32,
    zoom: f32,
    /// 오브젝트 사각형을 끄는 중이면 시작 칸 `(열, 행)`.
    drag_from: Option<(u32, u32)>,
    /// egui 에 올린 그림. 실패도 기억해 매 프레임 다시 읽지 않는다.
    textures: HashMap<String, Result<(egui::TextureHandle, [u32; 2]), String>>,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            open: false,
            tab: ArtKind::Ground,
            images: Vec::new(),
            selected: None,
            cell: DEFAULT_CELL,
            zoom: 2.0,
            drag_from: None,
            textures: HashMap::new(),
        }
    }
}

impl std::fmt::Debug for Palette {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Palette")
            .field("open", &self.open)
            .field("tab", &self.tab)
            .field("selected", &self.selected)
            .field("cell", &self.cell)
            .finish_non_exhaustive()
    }
}

impl Palette {
    /// 창을 연다. 열 때마다 `assets/` 를 다시 훑는다 — 그림을 넣고 다시 열면 보인다.
    pub(crate) fn open(&mut self, image: Option<String>) {
        self.open = true;
        self.images = scan(Path::new(ASSET_DIR));
        if let Some(image) = image {
            self.selected = Some(image.replace('\\', "/"));
        }
        if self.selected.is_none() {
            // 지형 그림으로 쓰일 법한 것을 먼저 — 없으면 첫 그림.
            self.selected = self
                .images
                .iter()
                .find(|p| p.contains("overworld"))
                .or_else(|| self.images.first())
                .cloned();
        }
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui, actions: &mut UiActions) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new("지형 팔레트")
            .open(&mut open)
            .default_size([760.0, 520.0])
            .resizable(true)
            .show(ui.ctx(), |ui| self.contents(ui, actions));
        self.open = open;
    }

    fn contents(&mut self, ui: &mut egui::Ui, actions: &mut UiActions) {
        ui.horizontal(|ui| {
            for (tab, label) in [(ArtKind::Ground, "지면"), (ArtKind::Prop, "오브젝트")] {
                if ui.selectable_label(self.tab == tab, label).clicked() {
                    self.tab = tab;
                    self.drag_from = None;
                }
            }
            ui.separator();
            ui.label("칸");
            ui.add(
                egui::DragValue::new(&mut self.cell)
                    .range(CELL_RANGE)
                    .suffix(" px"),
            )
            .on_hover_text("그림의 한 칸 크기. 새 그림 파일은 이 크기를 1m 로 본다");
            ui.separator();
            ui.label("확대");
            egui::ComboBox::from_id_salt("palette_zoom")
                .selected_text(format!("×{}", self.zoom))
                .show_ui(ui, |ui| {
                    for z in ZOOMS {
                        ui.selectable_value(&mut self.zoom, z, format!("×{z}"));
                    }
                });
        });
        ui.label(
            egui::RichText::new(match self.tab {
                ArtKind::Ground => "칸을 누르면 그 칸이 지면 붓이 된다.",
                ArtKind::Prop => "끌어서 사각형을 고르면 오브젝트 붓이 된다 (누르기만 하면 한 칸).",
            })
            .small()
            .weak(),
        );
        ui.separator();

        ui.horizontal_top(|ui| {
            // 왼쪽 — 그림 파일 목록
            ui.vertical(|ui| {
                ui.set_width(220.0);
                ui.weak(format!("{ASSET_DIR}/ 의 PNG ({})", self.images.len()));
                egui::ScrollArea::vertical()
                    .id_salt("palette_files")
                    .show(ui, |ui| {
                        for path in &self.images {
                            let shown = path.strip_prefix("assets/").unwrap_or(path);
                            let active = self.selected.as_deref() == Some(path.as_str());
                            if ui.selectable_label(active, shown).clicked() {
                                self.selected = Some(path.clone());
                                self.drag_from = None;
                            }
                        }
                    });
            });
            ui.separator();

            // 오른쪽 — 고른 그림 위에 격자
            let Some(path) = self.selected.clone() else {
                ui.weak("그림 파일이 없습니다 — assets/ 에 PNG 를 넣고 다시 여세요.");
                return;
            };
            match self.texture(ui.ctx(), &path) {
                Ok((texture, dims)) => {
                    egui::ScrollArea::both()
                        .id_salt("palette_image")
                        .show(ui, |ui| self.grid(ui, &path, texture, dims, actions));
                }
                Err(e) => {
                    ui.colored_label(egui::Color32::from_rgb(230, 90, 80), e);
                }
            }
        });
    }

    /// 그림과 격자를 그리고 칸 고르기를 받는다.
    fn grid(
        &mut self,
        ui: &mut egui::Ui,
        path: &str,
        texture: egui::TextureId,
        [w, h]: [u32; 2],
        actions: &mut UiActions,
    ) {
        let cell = self.cell.max(1);
        let step = cell as f32 * self.zoom;
        let (cols, rows) = (w / cell, h / cell);
        let size = egui::vec2(w as f32 * self.zoom, h as f32 * self.zoom);
        let (response, painter) = ui.allocate_painter(size, egui::Sense::click_and_drag());
        let rect = response.rect;

        // 투명한 부분이 보이도록 어두운 바탕.
        painter.rect_filled(rect, 0.0, egui::Color32::from_gray(28));
        painter.image(
            texture,
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );

        let line = egui::Stroke::new(1.0, egui::Color32::from_white_alpha(38));
        let grid_w = cols as f32 * step;
        let grid_h = rows as f32 * step;
        for c in 0..=cols {
            let x = rect.min.x + c as f32 * step;
            painter.line_segment(
                [
                    egui::pos2(x, rect.min.y),
                    egui::pos2(x, rect.min.y + grid_h),
                ],
                line,
            );
        }
        for r in 0..=rows {
            let y = rect.min.y + r as f32 * step;
            painter.line_segment(
                [
                    egui::pos2(rect.min.x, y),
                    egui::pos2(rect.min.x + grid_w, y),
                ],
                line,
            );
        }

        let cell_at = |pos: egui::Pos2| cell_at(pos - rect.min, step, cols, rows);
        let cell_rect = |(c0, r0): (u32, u32), (c1, r1): (u32, u32)| {
            egui::Rect::from_min_max(
                rect.min + egui::vec2(c0 as f32 * step, r0 as f32 * step),
                rect.min + egui::vec2((c1 + 1) as f32 * step, (r1 + 1) as f32 * step),
            )
        };
        let highlight = egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 210, 64));

        if let Some(hovered) = response.hover_pos().and_then(cell_at) {
            painter.rect_stroke(
                cell_rect(hovered, hovered),
                0.0,
                egui::Stroke::new(1.0, egui::Color32::WHITE),
                egui::StrokeKind::Inside,
            );
            let (c, r) = hovered;
            response
                .clone()
                .on_hover_text(format!("({}, {}) px", c * cell, r * cell));
        }

        // 오브젝트: 끌어서 사각형
        if self.tab == ArtKind::Prop {
            if response.drag_started() {
                self.drag_from = response.interact_pointer_pos().and_then(cell_at);
            }
            if let (Some(from), Some(to)) = (
                self.drag_from,
                response.interact_pointer_pos().and_then(cell_at),
            ) {
                let (a, b) = ordered(from, to);
                painter.rect_stroke(cell_rect(a, b), 0.0, highlight, egui::StrokeKind::Inside);
                if response.drag_stopped() {
                    actions.pick_art = Some(self.pick(path, region(from, to, cell)));
                    self.drag_from = None;
                }
            } else if response.drag_stopped() {
                self.drag_from = None;
            }
        }

        // 누르기만 하면 한 칸 — 지면이든 오브젝트든.
        if response.clicked()
            && let Some(at) = response.interact_pointer_pos().and_then(cell_at)
        {
            actions.pick_art = Some(self.pick(path, region(at, at, cell)));
        }
    }

    fn pick(&self, path: &str, px: (u32, u32, u32, u32)) -> Pick {
        Pick {
            image: path.to_string(),
            px,
            kind: self.tab,
            pixels_per_meter: self.cell.max(1) as f32,
        }
    }

    /// 그림을 egui 텍스처로 한 번 올린다. 실패도 기억한다.
    fn texture(
        &mut self,
        ctx: &egui::Context,
        path: &str,
    ) -> Result<(egui::TextureId, [u32; 2]), String> {
        let entry = self.textures.entry(path.to_string()).or_insert_with(|| {
            let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
            let image = Image::decode_png(&bytes).map_err(|e| format!("{path}: {e}"))?;
            let (w, h) = (image.width(), image.height());
            let max = ctx.input(|i| i.max_texture_side);
            if w as usize > max || h as usize > max {
                return Err(format!(
                    "{path}: {w}×{h} — 팔레트에 올리기엔 너무 크다 (최대 {max})"
                ));
            }
            let pixels = egui::ColorImage::from_rgba_unmultiplied(
                [w as usize, h as usize],
                image.desc("palette").rgba,
            );
            let handle = ctx.load_texture(
                format!("palette:{path}"),
                pixels,
                egui::TextureOptions::NEAREST,
            );
            Ok((handle, [w, h]))
        });
        match entry {
            Ok((handle, dims)) => Ok((handle.id(), *dims)),
            Err(e) => Err(e.clone()),
        }
    }
}

/// 화면 위치(그림 왼쪽 위 기준) → 칸. 격자 밖이면 `None` (나누어떨어지지 않는 가장자리 포함).
fn cell_at(offset: egui::Vec2, step: f32, cols: u32, rows: u32) -> Option<(u32, u32)> {
    if offset.x < 0.0 || offset.y < 0.0 || step <= 0.0 {
        return None;
    }
    let (c, r) = ((offset.x / step) as u32, (offset.y / step) as u32);
    (c < cols && r < rows).then_some((c, r))
}

/// 두 칸을 왼쪽 위 / 오른쪽 아래 순으로.
fn ordered((c0, r0): (u32, u32), (c1, r1): (u32, u32)) -> ((u32, u32), (u32, u32)) {
    ((c0.min(c1), r0.min(r1)), (c0.max(c1), r0.max(r1)))
}

/// 두 칸이 이루는 사각형의 **픽셀** 영역 `(x, y, 폭, 높이)` — 어느 방향으로 끌었든 같다.
fn region(a: (u32, u32), b: (u32, u32), cell: u32) -> (u32, u32, u32, u32) {
    let ((c0, r0), (c1, r1)) = ordered(a, b);
    (
        c0 * cell,
        r0 * cell,
        (c1 - c0 + 1) * cell,
        (r1 - r0 + 1) * cell,
    )
}

/// `dir` 아래 PNG 를 모두 찾는다 — 작업 디렉터리 기준 경로, `/` 구분, 이름순.
///
/// 경로를 `/` 로 맞추는 이유: `terrain.ron` 에 그대로 적히고, 그 파일은 두 OS 가 같이 쓴다.
fn scan(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| x.eq_ignore_ascii_case("png"))
            {
                found.push(slash_path(&p));
            }
        }
    }
    found.sort();
    found
}

/// 경로를 `/` 로 잇는다 (`.` 같은 구성 요소는 뺀다).
fn slash_path(p: &Path) -> String {
    p.components()
        .filter_map(|c| match c {
            Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_region_is_the_same_whichever_way_it_was_dragged() {
        // 오른쪽 아래 → 왼쪽 위로 끌어도 같은 사각형이어야 한다.
        assert_eq!(region((1, 2), (3, 4), 16), (16, 32, 48, 48));
        assert_eq!(region((3, 4), (1, 2), 16), (16, 32, 48, 48));
        assert_eq!(region((5, 5), (5, 5), 16), (80, 80, 16, 16), "한 칸");
    }

    #[test]
    fn positions_outside_the_grid_pick_nothing() {
        let step = 32.0; // 16px × 2배
        assert_eq!(cell_at(egui::vec2(40.0, 70.0), step, 10, 10), Some((1, 2)));
        assert_eq!(cell_at(egui::vec2(-1.0, 5.0), step, 10, 10), None);
        // 나누어떨어지지 않는 가장자리(반 칸)는 고를 수 없다 — 그림 밖 UV 가 된다.
        assert_eq!(
            cell_at(egui::vec2(10.0 * step + 1.0, 5.0), step, 10, 10),
            None
        );
    }

    #[test]
    fn paths_use_forward_slashes_on_every_os() {
        let p = Path::new("assets")
            .join("third_party")
            .join("pack")
            .join("a.png");
        assert_eq!(slash_path(&p), "assets/third_party/pack/a.png");
        assert_eq!(slash_path(Path::new("./assets/x.png")), "assets/x.png");
    }

    #[test]
    fn scanning_finds_the_shipped_tilesets() {
        // 저장소에 든 타일셋이 목록에 나와야 한다 (크레이트 폴더에서 돌므로 저장소 루트를 준다).
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let found = scan(&root);
        assert!(
            found.iter().any(|p| p.ends_with("gfx/overworld.png")),
            "{found:?}"
        );
        assert!(found.iter().all(|p| !p.contains('\\')), "역슬래시가 섞였다");
    }
}
