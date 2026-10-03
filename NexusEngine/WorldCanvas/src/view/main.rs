//! `worldcanvas-view` — 원본(.canvas)이 만드는 그림을 보는 창. 편집 기능은 없다.
//!
//! 원본 파일을 감시해서 바뀌면 다시 읽는다 — Claude 가 원본을 고치는 동안 열어 두면 결과가 바로 보인다.
//! 읽기에 실패하면 **마지막으로 성공한 그림을 그대로 두고** 오류만 띄운다.
//!
//! ```text
//! worldcanvas-view [폴더 또는 .canvas 파일]     (기본: sprites)
//! ```
//!
//! 자동 확인용 환경 변수 (WorldEditor 의 `NEXUS_SCREENSHOT` 과 같은 방식):
//!
//! | 변수 | 뜻 |
//! |---|---|
//! | `WORLDCANVAS_SELECT=player` | 시작할 때 이 원본을 고른다 (파일 이름, 확장자 없이) |
//! | `WORLDCANVAS_TAB=anim\|sheet\|images` | 시작 탭 |
//! | `WORLDCANVAS_ZOOM=4` | 시작 배율 |
//! | `WORLDCANVAS_SCREENSHOT=out.png` | 몇 프레임 뒤 창을 PNG 로 저장하고 종료 |
//! | `WORLDCANVAS_SCREENSHOT_FRAME=15` | 캡처 프레임 (기본 15). 실행 중 파일 변경을 확인할 때 늘린다 |
//! | `WORLDCANVAS_FONT=/path/font.ttc` | 한글 폰트 직접 지정 |

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use eframe::egui::{
    self, Color32, ColorImage, FontData, FontDefinitions, FontFamily, Pos2, Rect, Sense,
    TextureHandle, TextureOptions, Vec2,
};
use worldcanvas::doc::{Document, ImageKind, Placed};
use worldcanvas::export::{self, Rgba8};
use worldcanvas::parse::parse;
use worldcanvas::resolve::Resolver;

/// 파일 변경 확인 주기.
const POLL: Duration = Duration::from_millis(400);

const FONT_CANDIDATES: &[&str] = &[
    "C:/Windows/Fonts/malgun.ttf",
    "/usr/share/fonts/google-noto-sans-cjk-fonts/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/nanum/NanumGothic.ttf",
    "/usr/share/fonts/nanum/NanumGothic.ttf",
    "/System/Library/Fonts/AppleSDGothicNeo.ttc",
];

fn main() -> eframe::Result {
    let target = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("sprites"), PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 780.0])
            .with_title("WorldCanvas 뷰어"),
        ..Default::default()
    };
    eframe::run_native(
        "worldcanvas-view",
        options,
        Box::new(move |cc| {
            install_font(&cc.egui_ctx);
            Ok(Box::new(Viewer::new(target)))
        }),
    )
}

/// 한글이 보이도록 시스템 폰트를 기본 글꼴 뒤에 붙인다. 못 찾으면 알리고 그대로 간다.
fn install_font(ctx: &egui::Context) {
    let env = std::env::var("WORLDCANVAS_FONT").ok();
    let found = env
        .iter()
        .map(String::as_str)
        .chain(FONT_CANDIDATES.iter().copied())
        .find_map(|p| std::fs::read(p).ok().map(|b| (p.to_owned(), b)));
    let Some((path, bytes)) = found else {
        eprintln!("한글 폰트를 찾지 못함 — WORLDCANVAS_FONT=/path/font.ttc 로 지정할 것");
        return;
    };
    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("hangul".into(), Arc::new(FontData::from_owned(bytes)));
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("hangul".into());
    }
    ctx.set_fonts(fonts);
    eprintln!("한글 폰트 — {path}");
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Anim,
    Sheet,
    Images,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Background {
    Dark,
    Light,
    Checker,
}

/// 읽어 둔 원본 하나 — 마지막으로 성공한 결과와 텍스처.
struct Loaded {
    doc: Document,
    /// 그림 이름 → 텍스처 (프레임·부품 전부).
    images: HashMap<String, TextureHandle>,
    /// 시트 또는 아틀라스 전체.
    composite: Option<TextureHandle>,
    placed: Vec<Placed>,
}

struct Source {
    path: PathBuf,
    modified: Option<SystemTime>,
    loaded: Option<Loaded>,
    error: Option<String>,
}

impl Source {
    fn name(&self) -> String {
        self.path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    }
}

struct Viewer {
    target: PathBuf,
    sources: Vec<Source>,
    selected: usize,
    tab: Tab,
    zoom: u32,
    background: Background,
    playing: bool,
    /// 재생 시간 (초). 멈춰도 그 자리에서 이어진다.
    clock: f64,
    last_time: Option<f64>,
    last_poll: Instant,
    frames: u64,
    screenshot: Option<PathBuf>,
    screenshot_frame: u64,
    /// 원본을 고른 뒤 처음 읽혔을 때 탭을 문서에 맞춘다 (시트 → 애니메이션, 아틀라스 → 시트).
    auto_tab: bool,
}

impl Viewer {
    fn new(target: PathBuf) -> Self {
        let env = |k: &str| std::env::var(k).ok();
        let tab = match env("WORLDCANVAS_TAB").as_deref() {
            Some("sheet") => Some(Tab::Sheet),
            Some("images") => Some(Tab::Images),
            Some("anim") => Some(Tab::Anim),
            _ => None,
        };
        let mut v = Self {
            target,
            sources: Vec::new(),
            selected: 0,
            tab: tab.unwrap_or(Tab::Anim),
            zoom: env("WORLDCANVAS_ZOOM")
                .and_then(|z| z.parse().ok())
                .unwrap_or(4),
            background: Background::Dark,
            playing: true,
            clock: 0.0,
            last_time: None,
            last_poll: Instant::now(),
            frames: 0,
            screenshot: env("WORLDCANVAS_SCREENSHOT").map(PathBuf::from),
            screenshot_frame: env("WORLDCANVAS_SCREENSHOT_FRAME")
                .and_then(|f| f.parse().ok())
                .unwrap_or(15),
            auto_tab: tab.is_none(),
        };
        v.rescan();
        if let Some(want) = env("WORLDCANVAS_SELECT")
            && let Some(i) = v.sources.iter().position(|s| s.name() == want)
        {
            v.selected = i;
        }
        v
    }

    /// 대상 폴더의 `.canvas` 목록을 갱신한다. 이미 읽은 것은 그대로 둔다.
    fn rescan(&mut self) {
        let paths: Vec<PathBuf> = if self.target.is_dir() {
            let mut p: Vec<PathBuf> = std::fs::read_dir(&self.target)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "canvas"))
                .collect();
            p.sort();
            p
        } else {
            vec![self.target.clone()]
        };
        let current = self.sources.get(self.selected).map(|s| s.path.clone());
        let mut old: Vec<Source> = std::mem::take(&mut self.sources);
        self.sources = paths
            .into_iter()
            .map(|path| match old.iter().position(|s| s.path == path) {
                Some(i) => old.swap_remove(i),
                None => Source {
                    path,
                    modified: None,
                    loaded: None,
                    error: None,
                },
            })
            .collect();
        self.selected = current
            .and_then(|c| self.sources.iter().position(|s| s.path == c))
            .unwrap_or(0);
    }

    /// 바뀐 원본을 다시 읽는다.
    fn refresh(&mut self, ctx: &egui::Context) {
        for src in &mut self.sources {
            let modified = std::fs::metadata(&src.path).and_then(|m| m.modified()).ok();
            if modified.is_some() && modified == src.modified {
                continue;
            }
            src.modified = modified;
            match load(ctx, &src.path) {
                Ok(l) => {
                    src.loaded = Some(l);
                    src.error = None;
                }
                Err(e) => src.error = Some(e),
            }
        }
    }

    fn tick_clock(&mut self, now: f64) {
        if let Some(prev) = self.last_time
            && self.playing
        {
            self.clock += now - prev;
        }
        self.last_time = Some(now);
    }

    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else {
            return;
        };
        self.frames += 1;
        if self.frames == self.screenshot_frame {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = shot {
            let [w, h] = image.size;
            let mut out = Rgba8::new(w as u32, h as u32);
            for (i, c) in image.pixels.iter().enumerate() {
                out.put((i % w) as u32, (i / w) as u32, c.to_array());
            }
            match export::encode_png(&out)
                .map_err(|e| e.to_string())
                .and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string()))
            {
                Ok(()) => eprintln!("스크린샷 저장 — {} ({w}x{h})", path.display()),
                Err(e) => eprintln!("스크린샷 실패 — {e}"),
            }
            self.screenshot = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

/// 원본을 읽어 텍스처까지 만든다.
fn load(ctx: &egui::Context, path: &Path) -> Result<Loaded, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let doc = parse(&text).map_err(|e| e.to_string())?;
    let mut r = Resolver::new(&doc);
    let key = path.display().to_string();
    let mut images = HashMap::new();
    for img in &doc.images {
        let bm = r.get(&img.name).map_err(|e| e.to_string())?;
        let rgba = export::bitmap_rgba(&bm, &doc.palette);
        images.insert(
            img.name.clone(),
            texture(ctx, &format!("{key}#{}", img.name), &rgba),
        );
    }
    let (composite, placed) = match (&doc.sheet, &doc.atlas) {
        (_, Some(atlas)) => {
            let (img, placed) = export::build_atlas(&doc, atlas).map_err(|e| e.to_string())?;
            (Some(texture(ctx, &key, &img)), placed)
        }
        (Some(_), None) => {
            let img = export::build_sheet(&doc).map_err(|e| e.to_string())?;
            (Some(texture(ctx, &key, &img)), Vec::new())
        }
        (None, None) => (None, Vec::new()),
    };
    Ok(Loaded {
        doc,
        images,
        composite,
        placed,
    })
}

fn texture(ctx: &egui::Context, name: &str, img: &Rgba8) -> TextureHandle {
    let color =
        ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.data);
    // 픽셀아트 — 확대해도 뭉개지지 않게 Nearest.
    ctx.load_texture(name, color, TextureOptions::NEAREST)
}

impl eframe::App for Viewer {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.last_poll.elapsed() >= POLL || self.frames == 0 {
            self.last_poll = Instant::now();
            self.rescan();
            self.refresh(&ctx);
        }
        self.tick_clock(ui.input(|i| i.time));
        if self.auto_tab
            && let Some(doc) = self
                .sources
                .get(self.selected)
                .and_then(|s| s.loaded.as_ref())
                .map(|l| &l.doc)
        {
            self.tab = if doc.sheet.is_some() {
                Tab::Anim
            } else {
                Tab::Sheet
            };
            self.auto_tab = false;
        }

        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::left("sources")
            .default_size(190.0)
            .show(ui, |ui| self.source_list(ui));
        egui::CentralPanel::default_margins().show(ui, |ui| self.content(ui));

        self.handle_screenshot(&ctx);
        // 재생 중이 아니어도 파일 감시는 계속한다.
        if self.playing || self.screenshot.is_some() {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(POLL);
        }
    }
}

impl Viewer {
    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Anim, "애니메이션");
            ui.selectable_value(&mut self.tab, Tab::Sheet, "시트 / 아틀라스");
            ui.selectable_value(&mut self.tab, Tab::Images, "그림");
            ui.separator();
            ui.label("배율");
            ui.add(egui::Slider::new(&mut self.zoom, 1..=16).suffix("x"));
            ui.separator();
            ui.label("배경");
            ui.selectable_value(&mut self.background, Background::Dark, "어두움");
            ui.selectable_value(&mut self.background, Background::Light, "밝음");
            ui.selectable_value(&mut self.background, Background::Checker, "체커");
            ui.separator();
            let label = if self.playing {
                "⏸ 멈춤"
            } else {
                "▶ 재생"
            };
            if ui.button(label).clicked() {
                self.playing = !self.playing;
            }
        });
    }

    fn source_list(&mut self, ui: &mut egui::Ui) {
        ui.heading("원본");
        ui.label(
            egui::RichText::new(self.target.display().to_string())
                .small()
                .weak(),
        );
        ui.separator();
        let mut clicked = None;
        for (i, src) in self.sources.iter().enumerate() {
            let mut text = egui::RichText::new(src.name());
            if src.error.is_some() {
                text = text.color(Color32::from_rgb(0xe0, 0x60, 0x50));
            }
            if ui.selectable_label(i == self.selected, text).clicked() {
                clicked = Some(i);
            }
        }
        if let Some(i) = clicked {
            self.selected = i;
            self.auto_tab = true;
        }
        if self.sources.is_empty() {
            ui.label("`.canvas` 파일이 없음");
        }
    }

    fn content(&mut self, ui: &mut egui::Ui) {
        let Some(src) = self.sources.get(self.selected) else {
            return;
        };
        ui.horizontal(|ui| {
            ui.heading(src.name());
            ui.label(
                egui::RichText::new(src.path.display().to_string())
                    .small()
                    .weak(),
            );
        });
        if let Some(err) = &src.error {
            ui.colored_label(Color32::from_rgb(0xe0, 0x60, 0x50), format!("오류: {err}"));
            if src.loaded.is_some() {
                ui.label(egui::RichText::new("마지막으로 읽은 그림을 보여 준다").weak());
            }
        }
        let Some(loaded) = &src.loaded else {
            return;
        };
        let doc = &loaded.doc;
        ui.label(summary(doc));
        ui.separator();

        let tab = self.tab;
        let (zoom, bg, clock) = (self.zoom as f32, self.background, self.clock);
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| match tab {
                Tab::Anim => anim_tab(ui, loaded, zoom, bg, clock),
                Tab::Sheet => sheet_tab(ui, loaded, zoom, bg),
                Tab::Images => images_tab(ui, loaded, zoom, bg),
            });
    }
}

fn summary(doc: &Document) -> String {
    let frames = doc
        .images
        .iter()
        .filter(|i| i.kind == ImageKind::Frame)
        .count();
    let mut s = format!(
        "색 {} · 프레임 {frames} · 부품 {}",
        doc.palette.len(),
        doc.images.len() - frames
    );
    if doc.cell.0 > 0 {
        s += &format!(" · 칸 {}x{}", doc.cell.0, doc.cell.1);
    }
    if let Some(sheet) = &doc.sheet {
        s += &format!(" · 방향 {}", sheet.directions);
        if let Some(ppm) = sheet.pixels_per_meter {
            s += &format!(" · {ppm}px/m");
        }
    }
    s
}

/// 방향 이름 — 엔진 순서(0 = 동, 반시계).
fn direction_label(d: usize, n: u32) -> String {
    match n {
        4 => ["동", "북", "서", "남"][d].to_owned(),
        8 => ["동", "북동", "북", "북서", "서", "남서", "남", "남동"][d].to_owned(),
        _ => format!("방향 {d}"),
    }
}

fn anim_tab(ui: &mut egui::Ui, loaded: &Loaded, zoom: f32, bg: Background, clock: f64) {
    let doc = &loaded.doc;
    let Some(sheet) = &doc.sheet else {
        ui.label("sheet 블록이 없어 애니메이션이 없음 — 시트 / 아틀라스 탭을 볼 것");
        return;
    };
    for clip in &sheet.clips {
        let frames = clip.frames().max(1);
        let step = (clock * 1000.0 / clip.frame_ms as f64) as u64;
        let index = if clip.looping {
            step % u64::from(frames)
        } else {
            step.min(u64::from(frames) - 1)
        } as usize;
        let mode = if clip.looping { "반복" } else { "한 번" };
        ui.label(
            egui::RichText::new(format!(
                "{} — {frames}프레임 × {}ms, {mode}  (지금 {})",
                clip.state,
                clip.frame_ms,
                index + 1
            ))
            .strong(),
        );
        ui.horizontal(|ui| {
            for (d, row) in clip.rows.iter().enumerate() {
                ui.vertical(|ui| {
                    if let Some(tex) = row.get(index).and_then(|n| loaded.images.get(n)) {
                        show(ui, tex, zoom, bg);
                    }
                    ui.label(direction_label(d, sheet.directions));
                });
            }
        });
        ui.add_space(10.0);
    }
}

fn sheet_tab(ui: &mut egui::Ui, loaded: &Loaded, zoom: f32, bg: Background) {
    let Some(tex) = &loaded.composite else {
        ui.label("sheet 도 atlas 도 없음 — 그림 탭을 볼 것");
        return;
    };
    let rect = show(ui, tex, zoom, bg);
    // 아틀라스는 그림 경계와 이름을 겹쳐 보여 준다.
    let painter = ui.painter_at(rect);
    for p in &loaded.placed {
        let r = Rect::from_min_size(
            rect.min + Vec2::new(p.x as f32, p.y as f32) * zoom,
            Vec2::new(p.w as f32, p.h as f32) * zoom,
        );
        painter.rect_stroke(
            r,
            0.0,
            egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 220, 90, 160)),
            egui::StrokeKind::Inside,
        );
    }
    if !loaded.placed.is_empty() {
        ui.add_space(8.0);
        ui.label(egui::RichText::new("픽셀 사각형 (x, y, 폭, 높이) — terrain.ron 의 px").strong());
        for p in &loaded.placed {
            ui.monospace(format!("{}: ({}, {}, {}, {})", p.name, p.x, p.y, p.w, p.h));
        }
    }
}

fn images_tab(ui: &mut egui::Ui, loaded: &Loaded, zoom: f32, bg: Background) {
    ui.horizontal_wrapped(|ui| {
        for img in &loaded.doc.images {
            let Some(tex) = loaded.images.get(&img.name) else {
                continue;
            };
            ui.vertical(|ui| {
                show(ui, tex, zoom, bg);
                let kind = match img.kind {
                    ImageKind::Frame => "",
                    ImageKind::Part => " (부품)",
                };
                ui.label(format!("{}{kind}", img.name));
            });
        }
    });
}

/// 텍스처를 배율대로 배경 위에 그린다. 그린 자리를 돌려준다.
fn show(ui: &mut egui::Ui, tex: &TextureHandle, zoom: f32, bg: Background) -> Rect {
    let size = tex.size_vec2() * zoom;
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    match bg {
        Background::Dark => {
            painter.rect_filled(rect, 0.0, Color32::from_rgb(0x4a, 0x50, 0x60));
        }
        Background::Light => {
            painter.rect_filled(rect, 0.0, Color32::from_rgb(0xd8, 0xdc, 0xe2));
        }
        Background::Checker => {
            let cell = 8.0;
            painter.rect_filled(rect, 0.0, Color32::from_rgb(0x9a, 0x9a, 0x9a));
            let (cols, rows) = ((size.x / cell).ceil() as i32, (size.y / cell).ceil() as i32);
            for y in 0..rows {
                for x in 0..cols {
                    if (x + y) % 2 == 0 {
                        let r = Rect::from_min_size(
                            rect.min + Vec2::new(x as f32, y as f32) * cell,
                            Vec2::splat(cell),
                        )
                        .intersect(rect);
                        painter.rect_filled(r, 0.0, Color32::from_rgb(0xc8, 0xc8, 0xc8));
                    }
                }
            }
        }
    }
    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    painter.image(tex.id(), rect, uv, Color32::WHITE);
    rect
}
