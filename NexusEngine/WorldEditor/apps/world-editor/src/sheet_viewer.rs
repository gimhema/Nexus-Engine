//! 스프라이트 시트 뷰어 — 시트 정의가 그림을 어떻게 자르는지 본다 (단계 2 P8).
//!
//! ```text
//! ┌ 스프라이트 시트 — player.sheet.ron ────────────┐
//! │ 그림 gfx/character.png · 칸 16×32 · 4방향        │
//! │ ┌──┬──┬──┬──┐   클립 [Walk ▼]  방향 [동 ▼]       │
//! │ │▣ │  │  │  │   ┌────┐                           │
//! │ ├──┼──┼──┼──┤   │ 🧍 │ ← 재생 (frame_ms 마다)     │
//! │ │  │  │  │  │   └────┘                           │
//! └───────────────────────────────────────────────┘
//! ```
//!
//! - **보기 전용이다.** 시트 정의를 고치는 것은 아직 파일에서 한다 — 여기서는 칸이 그림에
//!   맞는지, 클립이 맞는 줄을 가리키는지, 방향 행 매핑이 맞는지를 **눈으로 확인**하는 곳이다.
//! - 읽는 규칙은 게임과 같다(`sprites::inspect_sheet` → `plan_sheet`). 게임이 거부할 시트는
//!   여기서도 오류로 보인다.
//! - 재생은 egui 의 시계로 한다 — 게임의 20Hz 시뮬레이션과 무관한 **미리보기**다.

use nexus_render_wgpu::egui;

use crate::sprites::{ClipInfo, SheetInfo, inspect_sheet};

/// 엔진 방향 이름 (`0 = 동`, 반시계). 방향 수가 4 가 아니면 번호로 보인다.
const DIRECTION_NAMES: [&str; 4] = ["동", "북", "서", "남"];

#[derive(Default)]
pub(crate) struct SheetViewer {
    open: bool,
    path: String,
    info: Option<Result<SheetInfo, String>>,
    texture: Option<(egui::TextureHandle, [u32; 2])>,
    clip: usize,
    direction: u32,
    zoom: u32,
}

impl std::fmt::Debug for SheetViewer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SheetViewer")
            .field("open", &self.open)
            .field("path", &self.path)
            .finish()
    }
}

impl SheetViewer {
    /// 시트 정의 하나를 연다 (작업 디렉터리 기준 경로).
    pub(crate) fn open(&mut self, path: &str) {
        self.open = true;
        if self.path != path {
            self.path = path.to_owned();
            self.info = Some(inspect_sheet(std::path::Path::new("."), path));
            self.texture = None;
            self.clip = 0;
            self.direction = 0;
        }
        if self.zoom == 0 {
            self.zoom = 3;
        }
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        let title = format!(
            "스프라이트 시트 — {}",
            self.path.rsplit('/').next().unwrap_or(&self.path)
        );
        egui::Window::new(title)
            .id(egui::Id::new("sheet_viewer"))
            .open(&mut open)
            .default_size([560.0, 420.0])
            .resizable(true)
            .show(ui.ctx(), |ui| self.body(ui));
        self.open = open;
    }

    fn body(&mut self, ui: &mut egui::Ui) {
        let info = match &self.info {
            Some(Ok(info)) => info.clone(),
            Some(Err(e)) => {
                ui.colored_label(egui::Color32::from_rgb(240, 110, 100), e);
                ui.weak("게임도 이 시트를 거부한다 — 그 액터는 내장 플레이스홀더로 그려진다.");
                return;
            }
            None => return,
        };
        if self.texture.is_none()
            && let Some(path) = &info.image_path
        {
            self.texture = crate::content::load_egui_image(ui.ctx(), path, "sheet");
        }

        ui.horizontal_wrapped(|ui| {
            ui.weak("그림");
            ui.label(&info.image_label);
            ui.weak("· 칸");
            ui.label(format!("{}×{}", info.cell.0, info.cell.1));
            ui.weak("· 방향");
            ui.label(format!("{}", info.directions));
            ui.weak("· 1m =");
            ui.label(format!("{}px", info.pixels_per_meter));
            if info.tinted {
                ui.weak("· 무채색(색 곱하기)");
            }
        });
        ui.horizontal(|ui| {
            egui::ComboBox::from_label("클립")
                .selected_text(
                    info.clips
                        .get(self.clip)
                        .map_or("—", |c| c.state.as_str())
                        .to_owned(),
                )
                .show_ui(ui, |ui| {
                    for (i, c) in info.clips.iter().enumerate() {
                        ui.selectable_value(
                            &mut self.clip,
                            i,
                            format!("{} (행 {}, {}프레임)", c.state, c.row, c.frames),
                        );
                    }
                });
            egui::ComboBox::from_label("방향")
                .selected_text(direction_name(self.direction, info.directions))
                .show_ui(ui, |ui| {
                    for d in 0..info.directions {
                        ui.selectable_value(
                            &mut self.direction,
                            d,
                            direction_name(d, info.directions),
                        );
                    }
                });
            ui.add(
                egui::DragValue::new(&mut self.zoom)
                    .range(1..=8)
                    .prefix("×"),
            );
        });

        let Some((tex, size)) = self.texture.as_ref().map(|(h, s)| (h.id(), *s)) else {
            ui.weak("그림을 읽지 못했습니다.");
            return;
        };
        let clip = info.clips.get(self.clip).cloned();
        // 지금 보여 줄 칸 — 클립 행 + 방향 행 매핑, 시간으로 고른 프레임.
        let (row, frame) = match &clip {
            Some(c) => {
                ui.ctx().request_repaint();
                cell_at(&info, c, self.direction, ui.input(|i| i.time))
            }
            None => (0, 0),
        };

        ui.horizontal_top(|ui| {
            // 왼쪽 — 시트 전체와 칸 격자, 지금 칸은 노란 테두리.
            let zoom = self.zoom.max(1) as f32;
            egui::ScrollArea::both()
                .id_salt("sheet_full")
                .max_width(ui.available_width() * 0.62)
                .show(ui, |ui| {
                    let img_size = egui::vec2(size[0] as f32 * zoom, size[1] as f32 * zoom);
                    let (rect, _) = ui.allocate_exact_size(img_size, egui::Sense::hover());
                    let painter = ui.painter_at(rect);
                    painter.image(
                        tex,
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                    let (cw, ch) = (info.cell.0 as f32 * zoom, info.cell.1 as f32 * zoom);
                    let grid = egui::Stroke::new(
                        1.0,
                        egui::Color32::from_rgba_unmultiplied(120, 170, 255, 90),
                    );
                    let cols = size[0] / info.cell.0.max(1);
                    let rows = size[1] / info.cell.1.max(1);
                    for c in 0..=cols {
                        let x = rect.min.x + c as f32 * cw;
                        painter.line_segment(
                            [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
                            grid,
                        );
                    }
                    for r in 0..=rows {
                        let y = rect.min.y + r as f32 * ch;
                        painter.line_segment(
                            [egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)],
                            grid,
                        );
                    }
                    if clip.is_some() {
                        let at = egui::Rect::from_min_size(
                            egui::pos2(
                                rect.min.x + frame as f32 * cw,
                                rect.min.y + row as f32 * ch,
                            ),
                            egui::vec2(cw, ch),
                        );
                        painter.rect_stroke(
                            at,
                            0.0,
                            egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 210, 60)),
                            egui::StrokeKind::Inside,
                        );
                    }
                });
            // 오른쪽 — 재생 미리보기 (크게).
            ui.vertical(|ui| {
                ui.strong("미리보기");
                if clip.is_some() {
                    let preview = 4.0 * self.zoom.max(1) as f32 / 2.0;
                    let (w, h) = (info.cell.0 as f32, info.cell.1 as f32);
                    let uv = cell_uv(&info, size, row, frame);
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(w * preview, h * preview),
                        egui::Sense::hover(),
                    );
                    ui.painter()
                        .rect_filled(rect, 2.0, egui::Color32::from_gray(40));
                    ui.painter().image(tex, rect, uv, egui::Color32::WHITE);
                    ui.weak(format!("행 {row} · 프레임 {frame}"));
                } else {
                    ui.weak("클립이 없습니다.");
                }
                ui.add_space(6.0);
                ui.weak(format!("방향 행 매핑: {:?}", info.direction_rows));
                ui.weak("엔진 방향 0 = 동, 반시계 (동·북·서·남).");
            });
        });
    }
}

/// 시간 `time`(초)에 보여 줄 칸 `(행, 프레임)` — 클립 행 + 방향 행 매핑, 반복하지 않는 클립은 끝 프레임에 멈춘다.
fn cell_at(info: &SheetInfo, clip: &ClipInfo, direction: u32, time: f64) -> (u32, u32) {
    let dir_row = info
        .direction_rows
        .get(direction as usize)
        .copied()
        .unwrap_or(0);
    let step = (time * 1000.0 / clip.frame_ms.max(1) as f64) as u64;
    let frame = if clip.looping {
        (step % u64::from(clip.frames.max(1))) as u32
    } else {
        step.min(u64::from(clip.frames.saturating_sub(1))) as u32
    };
    (clip.row + dir_row, frame)
}

/// 칸 하나의 UV (egui, 좌상단 원점).
fn cell_uv(info: &SheetInfo, size: [u32; 2], row: u32, frame: u32) -> egui::Rect {
    let (w, h) = (info.cell.0 as f32, info.cell.1 as f32);
    let (sw, sh) = (size[0].max(1) as f32, size[1].max(1) as f32);
    egui::Rect::from_min_max(
        egui::pos2(frame as f32 * w / sw, row as f32 * h / sh),
        egui::pos2((frame + 1) as f32 * w / sw, (row + 1) as f32 * h / sh),
    )
}

/// 시트를 생략한 액터가 쓰는 내장 플레이스홀더의 정의.
const BUILTIN_SHEET: &str = "assets/sprites/markers.sheet.ron";

/// 액터 편집기 안의 작은 재생 미리보기 (P8) — 시트 뷰어와 **같은 규칙**(`inspect_sheet`)으로 읽는다.
/// 시트 경로가 바뀔 때만 다시 읽으므로, 드롭다운에서 시트를 바꾸면 곧바로 새 그림이 보인다.
#[derive(Default)]
pub(crate) struct SheetPreview {
    key: Option<String>,
    info: Option<Result<SheetInfo, String>>,
    texture: Option<(egui::TextureHandle, [u32; 2])>,
    clip: usize,
    direction: u32,
}

impl std::fmt::Debug for SheetPreview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SheetPreview")
            .field("key", &self.key)
            .finish()
    }
}

impl SheetPreview {
    /// 시트 `key`(비면 내장)를 그린다. `tint` 는 액터가 **명시한** 색 — 게임처럼 그림에 곱한다.
    pub(crate) fn show(&mut self, ui: &mut egui::Ui, key: &str, tint: Option<[f32; 3]>) {
        let key = if key.is_empty() { BUILTIN_SHEET } else { key };
        if self.key.as_deref() != Some(key) {
            let info = inspect_sheet(std::path::Path::new("."), key);
            if let Ok(info) = &info {
                // 처음 보이는 모습 — 서 있는 동작, 카메라 쪽(남)을 본다.
                self.clip = info
                    .clips
                    .iter()
                    .position(|c| c.state == "Idle")
                    .unwrap_or(0);
                self.direction = south(info.directions);
            }
            self.key = Some(key.to_owned());
            self.info = Some(info);
            self.texture = None;
        }
        let info = match &self.info {
            Some(Ok(info)) => info.clone(),
            Some(Err(e)) => {
                ui.colored_label(egui::Color32::from_rgb(240, 110, 100), e);
                ui.weak("게임은 이 시트 대신 내장 플레이스홀더로 그립니다.");
                return;
            }
            None => return,
        };
        if self.texture.is_none()
            && let Some(path) = &info.image_path
        {
            self.texture = crate::content::load_egui_image(ui.ctx(), path, "actor_sheet");
        }
        let Some((tex, size)) = self.texture.as_ref().map(|(h, s)| (h.id(), *s)) else {
            ui.weak("그림을 읽지 못했습니다.");
            return;
        };

        ui.horizontal_top(|ui| {
            // 미리보기 — 정수배로 키운다 (픽셀아트가 뭉개지지 않게).
            const BOX: f32 = 112.0;
            let (w, h) = (info.cell.0.max(1) as f32, info.cell.1.max(1) as f32);
            let scale = (BOX / w.max(h)).floor().max(1.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(BOX, BOX), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, 3.0, egui::Color32::from_gray(40));
            if let Some(clip) = info.clips.get(self.clip) {
                ui.ctx().request_repaint();
                let (row, frame) = cell_at(&info, clip, self.direction, ui.input(|i| i.time));
                let at = egui::Rect::from_center_size(rect.center(), egui::vec2(w, h) * scale);
                let color = tint.map_or(egui::Color32::WHITE, |[r, g, b]| {
                    egui::Color32::from_rgb(
                        (r.clamp(0.0, 1.0) * 255.0).round() as u8,
                        (g.clamp(0.0, 1.0) * 255.0).round() as u8,
                        (b.clamp(0.0, 1.0) * 255.0).round() as u8,
                    )
                });
                ui.painter_at(rect)
                    .image(tex, at, cell_uv(&info, size, row, frame), color);
            }
            ui.vertical(|ui| {
                egui::ComboBox::from_id_salt("actor-preview-clip")
                    .selected_text(
                        info.clips
                            .get(self.clip)
                            .map_or("—", |c| c.state.as_str())
                            .to_owned(),
                    )
                    .show_ui(ui, |ui| {
                        for (i, c) in info.clips.iter().enumerate() {
                            ui.selectable_value(&mut self.clip, i, &c.state);
                        }
                    });
                egui::ComboBox::from_id_salt("actor-preview-dir")
                    .selected_text(direction_name(self.direction, info.directions))
                    .show_ui(ui, |ui| {
                        for d in 0..info.directions {
                            ui.selectable_value(
                                &mut self.direction,
                                d,
                                direction_name(d, info.directions),
                            );
                        }
                    });
                ui.weak(format!(
                    "칸 {}×{} · {}방향 · {}m 높이",
                    info.cell.0,
                    info.cell.1,
                    info.directions,
                    info.cell.1 as f32 / info.pixels_per_meter.max(1.0)
                ));
                ui.weak(&info.image_label);
                if tint.is_none() && info.tinted {
                    ui.weak("무채색 시트 — 색을 정하지 않으면 마커 종류 색이 곱해집니다");
                }
            });
        });
    }
}

/// 카메라 쪽(남, -Y)을 보는 방향 번호 — 0 = 동, 반시계로 N 등분.
fn south(directions: u32) -> u32 {
    let n = directions.max(1);
    (n * 3).div_ceil(4) % n
}

fn direction_name(d: u32, directions: u32) -> String {
    if directions == 4 {
        format!("{} ({d})", DIRECTION_NAMES[d as usize % 4])
    } else {
        format!("방향 {d}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_preview_faces_the_camera_by_default() {
        assert_eq!(south(4), 3, "동·북·서·남 중 남");
        assert_eq!(south(8), 6);
        assert_eq!(south(1), 0, "방향 없는 시트");
    }

    #[test]
    fn cells_follow_the_direction_rows_and_stop_on_the_last_frame() {
        let info = SheetInfo {
            image_path: None,
            image_label: String::new(),
            cell: (16, 32),
            directions: 4,
            direction_rows: vec![1, 2, 3, 0],
            pixels_per_meter: 16.0,
            tinted: false,
            clips: Vec::new(),
        };
        let walk = ClipInfo {
            state: String::from("Walk"),
            row: 4,
            frames: 4,
            frame_ms: 100,
            looping: true,
        };
        // 남(3) → 시트 0행, 0.25초 = 2프레임째, 반복하면 0.45초 = 4 → 0.
        assert_eq!(cell_at(&info, &walk, 3, 0.25), (4, 2));
        assert_eq!(cell_at(&info, &walk, 3, 0.45), (4, 0));
        let once = ClipInfo {
            looping: false,
            ..walk
        };
        assert_eq!(cell_at(&info, &once, 0, 5.0), (5, 3), "끝 프레임에 멈춘다");
        let uv = cell_uv(&info, [64, 256], 5, 3);
        assert_eq!(uv.min, egui::pos2(0.75, 0.625));
        assert_eq!(uv.max, egui::pos2(1.0, 0.75));
    }
}
