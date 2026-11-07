//! 스프라이트 시트 — 마커·유닛을 애니메이션 빌보드로 그린다.
//!
//! # 시트는 데이터다
//!
//! 시트 하나는 **그림(PNG) + 정의 파일(`*.sheet.ron`)** 한 쌍이다. 정의에는 칸 크기·방향 수·클립과,
//! 받아온 애셋을 이미지 편집 없이 쓰기 위한 두 가지가 더 있다:
//!
//! - [`direction_rows`](SheetFile::direction_rows) — **엔진 방향 → 시트 행** 대응.
//!   엔진은 `0 = 동(+X)` 부터 반시계로 세지만(`nexus_core::units::direction_index`), 받아온 시트는
//!   보통 남·동·북·서 같은 다른 순서다. 그림을 재배치하는 대신 여기서 흡수한다.
//! - [`tinted`](SheetFile::tinted) — **무채색 시트에만** 종류 색을 곱한다. 컬러 아트에 곱하면 색이 뒤집힌다.
//!
//! # 파츠 — 몸 위에 겹치는 시트
//!
//! 머리카락·옷·모자·무기는 **몸 시트와 격자가 똑같은 시트**로 그린다(종이 인형). 파츠는 재생기를 따로
//! 갖지 않고 몸의 재생기가 고른 칸(같은 행·같은 프레임)을 그대로 쓴다 — 그래서 칸 크기·열·행·방향 수·
//! 방향 행이 몸과 같아야 하고([`Sheet::fits`]), 클립은 읽지 않는다(`clips: []` 로 둬도 된다).
//!
//! - 앞뒤는 파츠 시트의 [`layer`](SheetFile::layer) 다. 몸이 `0`, 큰 쪽이 앞. 뒷모습에서 무기·망토가
//!   몸 뒤로 가야 하므로 [`layer_by_direction`](SheetFile::layer_by_direction) 으로 방향마다 바꿀 수 있다.
//! - 같은 발밑 위치에 서므로 깊이가 같다 — 부동소수 오차로 칸마다 이기는 쪽이 바뀌지 않게
//!   층마다 [`PART_DEPTH_STEP`] 씩 앞으로 민다.
//! - 색(피부·머리색)은 층마다 따로 곱한다. **곱하기라 어둡게만 된다** — 색을 바꿀 부위는 밝은 무채색으로 그린다.
//!
//! ## 장비와 외형
//!
//! 유닛이 입는 파츠는 [`compose`] 가 정한다 — **외형을 바꾸는지는 아이템 표시 데이터가 정한다:**
//!
//! | `display.ron` 아이템 | 장착하면 |
//! |---|---|
//! | `parts` 있음 (갑옷·모자·무기) | 그 그림이 겹친다. 템플릿 파츠 중 **같은 자리**(`slot`)인 것은 빠진다 — 기본 옷 → 갑옷 |
//! | `parts` 없음 (목걸이·반지) | 외형이 그대로다 |
//!
//! 템플릿 파츠에 `slot` 이 없으면(머리카락·얼굴) 어떤 장비로도 빠지지 않는다.
//!
//! # 어디서 읽나
//!
//! 기본 시트(플레이스홀더)는 실행 파일에 **내장**되어 작업 디렉터리와 무관하게 항상 뜬다.
//! `data/display.ron` 의 `sprites` 가 종류별 정의 파일을 가리키면 **그 파일을 읽어 덮어쓴다** —
//! 그림을 갈아끼워도 다시 빌드할 필요가 없다. 읽기에 실패하면 그 종류만 내장 시트로 남고
//! 이유를 알린다 (조용히 넘어가지 않는다).

use core::time::Duration;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nexus_assets::{AnimState, Clip, GridAtlas, Image, SpriteAnimator, SpriteSheet};
use nexus_core::{Camera2d, Vec2, Vec3, units};
use nexus_render::{RenderCommand, Renderer, SpriteAnchor, TextureId};
use serde::Deserialize;

use nexus_sim::{EquipSlot, ItemId};

use crate::scene::{ActorId, Item};

/// 내장 플레이스홀더 — 그림과 정의가 짝이다.
const EMBEDDED_PNG: &[u8] = include_bytes!("../../../assets/sprites/markers.png");
const EMBEDDED_DEF: &str = include_str!("../../../assets/sprites/markers.sheet.ron");

/// 화면에서 이보다 작게 그리지 않는다 (픽셀). 줌아웃해도 마커가 사라지지 않도록.
const SPRITE_MIN_PX: f32 = 20.0;

// ─────────────────────────────────────────────────────────────────────────────
// 시트 정의 파일
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SheetFile {
    version: u32,
    /// 그림 경로. 비우면 내장 그림을 쓴다 (내장 시트 정의가 그렇다).
    #[serde(default)]
    image: String,
    /// 한 칸의 픽셀 크기 (가로, 세로). 그림 크기를 정확히 나눠야 한다.
    cell: (u32, u32),
    /// 이 시트가 담은 방향 수 — **엔진이 아니라 시트의 성질이다.**
    directions: u32,
    /// **그림 1미터가 몇 픽셀인가.** 스프라이트의 월드 크기가 여기서 나온다
    /// (`칸 높이 / pixels_per_meter`).
    ///
    /// 받아온 아트마다 축척이 다르다 — 한 팩에서 캐릭터가 16×32px 이고 타일이 16px 이면
    /// 캐릭터는 타일 두 칸 높이(= 2m)다. 그러려면 이 값이 타일셋과 같은 16 이어야 한다.
    /// 내장 플레이스홀더는 32 (`Camera2d::PIXELS_PER_METER`)를 쓴다.
    #[serde(default = "default_pixels_per_meter")]
    pixels_per_meter: f32,
    /// 엔진 방향 `i` 가 쓸 시트 행 오프셋. 비우면 `0, 1, 2, …` 순서.
    ///
    /// 엔진 순서는 `0 = 동`, 반시계(동·북·서·남). 시트가 남·동·북·서 순서라면
    /// `[1, 2, 3, 0]` 이다 — 동은 1행, 북은 2행, 서는 3행, 남은 0행.
    #[serde(default)]
    direction_rows: Vec<u32>,
    /// 무채색 시트인가 — `true` 면 종류별 색을 곱한다. 컬러 아트는 `false`.
    #[serde(default)]
    tinted: bool,
    /// **파츠로 쓸 때의** 앞뒤 — 몸이 `0`, 큰 쪽이 앞. 몸 시트로 쓰면 읽지 않는다.
    #[serde(default = "default_part_layer")]
    layer: i32,
    /// 엔진 방향별 앞뒤. 비우면 모든 방향이 [`layer`](Self::layer).
    /// 뒷모습(북)에서 무기를 몸 뒤로 보내려면 4방향 시트에서 `[1, -1, 1, 1]`.
    #[serde(default)]
    layer_by_direction: Vec<i32>,
    /// 파츠 시트는 클립을 읽지 않으므로 비워도 된다.
    #[serde(default)]
    clips: Vec<ClipFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClipFile {
    state: StateFile,
    /// 첫 방향의 행. 방향 `d` 는 `row + direction_rows[d]` 행을 쓴다.
    row: u32,
    frames: u32,
    frame_ms: u64,
    looping: bool,
}

#[derive(Clone, Copy, Debug, Deserialize)]
enum StateFile {
    Idle,
    Walk,
    Attack,
    Cast,
    Hit,
    Die,
}

impl From<StateFile> for AnimState {
    fn from(s: StateFile) -> Self {
        match s {
            StateFile::Idle => Self::Idle,
            StateFile::Walk => Self::Walk,
            StateFile::Attack => Self::Attack,
            StateFile::Cast => Self::Cast,
            StateFile::Hit => Self::Hit,
            StateFile::Die => Self::Die,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 검증을 마친 시트 (GPU 업로드 전)
// ─────────────────────────────────────────────────────────────────────────────

/// 파츠 시트가 `layer` 를 적지 않았을 때 — 몸 바로 앞.
fn default_part_layer() -> i32 {
    1
}

/// 시트 정의에서 `pixels_per_meter` 를 적지 않았을 때의 값 — 내장 플레이스홀더의 축척.
fn default_pixels_per_meter() -> f32 {
    Camera2d::PIXELS_PER_METER
}

/// 그림과 검증된 메타데이터. GPU 를 모르므로 테스트가 여기까지 확인한다.
#[derive(Debug)]
struct SheetPlan {
    image: Image,
    sheet: SpriteSheet,
    cell: (u32, u32),
    direction_rows: Vec<u32>,
    tinted: bool,
    pixels_per_meter: f32,
    /// 엔진 방향 → 파츠 앞뒤.
    layers: Vec<i32>,
}

/// 시트 정의를 읽고 그림에 맞는지 확인한다.
///
/// `base` 는 정의 파일이 있던 폴더다 — 그림 경로를 작업 디렉터리 기준으로 먼저,
/// 없으면 그 폴더 기준으로 찾는다. 애셋 폴더를 통째로 옮겨도 정의 안의 상대 경로가 유지된다.
///
/// 칸 크기나 클립이 그림을 벗어나면 오류다 — 벗어난 UV 는 옆 칸이나 가장자리를 비추므로
/// 조용히 넘기면 안 된다.
fn plan_sheet(def: &str, base: Option<&Path>) -> Result<SheetPlan, String> {
    let file: SheetFile = ron::from_str(def).map_err(|e| format!("시트 정의: {e}"))?;
    if file.version != 1 {
        return Err(format!(
            "시트 정의 형식 {} 은(는) 읽을 수 없음",
            file.version
        ));
    }

    let (bytes, label) = if file.image.is_empty() {
        (EMBEDDED_PNG.to_vec(), String::from("markers.png (내장)"))
    } else {
        let path = resolve(&file.image, base);
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        (bytes, path.display().to_string())
    };
    let image = Image::decode_png(&bytes).map_err(|e| format!("{label}: {e}"))?;

    let (w, h) = file.cell;
    let atlas = GridAtlas::new(&image, w, h).map_err(|e| format!("{label}: {e}"))?;
    let directions = file.directions.max(1);

    let direction_rows = if file.direction_rows.is_empty() {
        (0..directions).collect()
    } else {
        let rows = file.direction_rows;
        if rows.len() != directions as usize {
            return Err(format!(
                "direction_rows 는 {directions} 개여야 함 (지금 {})",
                rows.len()
            ));
        }
        if let Some(bad) = rows.iter().find(|&&r| r >= directions) {
            return Err(format!(
                "direction_rows 의 {bad} 는 방향 수 {directions} 밖"
            ));
        }
        rows
    };

    let layers = if file.layer_by_direction.is_empty() {
        vec![file.layer; directions as usize]
    } else if file.layer_by_direction.len() == directions as usize {
        file.layer_by_direction
    } else {
        return Err(format!(
            "layer_by_direction 는 {directions} 개여야 함 (지금 {})",
            file.layer_by_direction.len()
        ));
    };

    let mut clips = Vec::new();
    for c in &file.clips {
        if c.frames == 0 || c.frames > atlas.columns() {
            return Err(format!(
                "{:?}: 프레임 {} 개 — 시트는 {} 열",
                c.state,
                c.frames,
                atlas.columns()
            ));
        }
        if c.row + directions > atlas.rows() {
            return Err(format!(
                "{:?}: {} 행부터 {directions} 방향 — 시트는 {} 행",
                c.state,
                c.row,
                atlas.rows()
            ));
        }
        clips.push((
            c.state.into(),
            Clip {
                row: c.row,
                frames: c.frames,
                frame_time: Duration::from_millis(c.frame_ms),
                looping: c.looping,
            },
        ));
    }

    if !(file.pixels_per_meter.is_finite() && file.pixels_per_meter > 0.0) {
        return Err(format!(
            "pixels_per_meter 는 양수여야 함 (지금 {})",
            file.pixels_per_meter
        ));
    }

    Ok(SheetPlan {
        image,
        sheet: SpriteSheet::new(atlas, directions, clips),
        cell: file.cell,
        direction_rows,
        tinted: file.tinted,
        pixels_per_meter: file.pixels_per_meter,
        layers,
    })
}

/// 그림 경로 찾기 — 작업 디렉터리 기준으로 먼저, 없으면 정의 파일 폴더 기준으로.
fn resolve(rel: &str, base: Option<&Path>) -> PathBuf {
    let direct = PathBuf::from(rel);
    if direct.exists() {
        return direct;
    }
    match base {
        Some(dir) => dir.join(rel),
        None => direct,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 올라간 시트
// ─────────────────────────────────────────────────────────────────────────────

/// 시트 정의 한 장의 요약 — 콘텐츠 브라우저의 상세와 시트 뷰어가 쓴다 (P8).
#[derive(Clone, Debug)]
pub(crate) struct SheetInfo {
    /// 그림 경로 (`root` 기준, `/` 구분). 내장 그림이면 저장소의 `assets/sprites/markers.png`.
    pub(crate) image_path: Option<String>,
    /// 사람이 읽는 그림 이름.
    pub(crate) image_label: String,
    pub(crate) cell: (u32, u32),
    pub(crate) directions: u32,
    /// 엔진 방향 → 시트 행 (비어 있던 경우 `0, 1, 2, …` 로 채운다).
    pub(crate) direction_rows: Vec<u32>,
    pub(crate) pixels_per_meter: f32,
    pub(crate) tinted: bool,
    pub(crate) clips: Vec<ClipInfo>,
}

/// 클립 하나 — 시트 뷰어가 재생한다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClipInfo {
    pub(crate) state: String,
    pub(crate) row: u32,
    pub(crate) frames: u32,
    pub(crate) frame_ms: u64,
    pub(crate) looping: bool,
}

/// 시트 정의를 읽어 요약한다. **게임이 읽을 때와 같은 검사**(`plan_sheet`)를 통과해야 한다 —
/// 뷰어에서는 멀쩡해 보이는데 게임에서는 거부되는 일이 없게.
///
/// `key` 는 `root` 기준 경로다 (콘텐츠 브라우저의 항목 키).
pub(crate) fn inspect_sheet(root: &Path, key: &str) -> Result<SheetInfo, String> {
    let path = root.join(key);
    let def = std::fs::read_to_string(&path).map_err(|e| format!("{key}: {e}"))?;
    let base = path.parent();
    let plan = plan_sheet(&def, base).map_err(|e| format!("{key}: {e}"))?;
    let file: SheetFile = ron::from_str(&def).map_err(|e| format!("{key}: {e}"))?;

    // 그림 경로를 `root` 기준으로 — 작업 디렉터리 기준으로 먼저, 없으면 정의 파일 폴더 기준.
    let folder = key.rsplit_once('/').map_or("", |(dir, _)| dir);
    let (image_path, image_label) = if file.image.is_empty() {
        let builtin = "assets/sprites/markers.png";
        (
            root.join(builtin).exists().then(|| builtin.to_owned()),
            String::from("markers.png (내장)"),
        )
    } else if root.join(&file.image).exists() {
        (Some(normalize(&file.image)), file.image.clone())
    } else {
        let joined = normalize(&format!("{folder}/{}", file.image));
        (Some(joined.clone()), joined)
    };

    Ok(SheetInfo {
        image_path,
        image_label,
        cell: plan.cell,
        directions: plan.sheet.directions(),
        direction_rows: plan.direction_rows,
        pixels_per_meter: plan.pixels_per_meter,
        tinted: plan.tinted,
        clips: file
            .clips
            .iter()
            .map(|c| ClipInfo {
                state: format!("{:?}", c.state),
                row: c.row,
                frames: c.frames,
                frame_ms: c.frame_ms,
                looping: c.looping,
            })
            .collect(),
    })
}

/// `a/b/../c` → `a/c`, `\` → `/`. 경로를 브라우저 키와 같은 모양으로.
fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// GPU 에 올라간 시트 하나.
///
/// `Clone` 은 **텍스처를 복사하지 않는다** — `TextureId` 는 핸들이라 여러 액터 타입이
/// 같은 그림을 가리킬 수 있다 (`SpriteLibrary::load` 의 중복 업로드 방지).
#[derive(Clone, Debug)]
pub(crate) struct Sheet {
    texture: TextureId,
    sheet: SpriteSheet,
    /// 한 칸의 픽셀 크기 — 스프라이트의 월드 크기가 여기서 나온다.
    cell: (u32, u32),
    /// 엔진 방향 → 시트 행.
    direction_rows: Vec<u32>,
    tinted: bool,
    /// 그림 1미터의 픽셀 수 — 월드 크기의 기준.
    pixels_per_meter: f32,
    /// 파츠로 쓸 때 엔진 방향별 앞뒤 (몸 = 0).
    layers: Vec<i32>,
}

impl Sheet {
    fn upload(plan: SheetPlan, renderer: &mut impl Renderer) -> Result<Self, String> {
        let texture = renderer
            .load_texture(&plan.image.desc("sprite-sheet"))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            texture,
            sheet: plan.sheet,
            cell: plan.cell,
            direction_rows: plan.direction_rows,
            tinted: plan.tinted,
            pixels_per_meter: plan.pixels_per_meter,
            layers: plan.layers,
        })
    }

    /// 같은 그림을 쓰는 다른 액터 타입에 물려준다 — 텍스처는 그대로 공유한다.
    fn share(&self) -> Self {
        self.clone()
    }

    /// 화면에 그려지는 높이 (m). 머리 위 표시(HP 막대)의 기준.
    ///
    /// 칸 높이를 **그 시트의 축척**(`pixels_per_meter`)으로 나눈 값이다 — 카메라 배율이
    /// 같으면 그림 1픽셀이 화면 1픽셀이 된다. 임의의 키(1.8m 같은)를 넣지 말고,
    /// 캐릭터를 키우려면 시트 칸을 키우거나 축척을 낮춘다.
    pub(crate) fn height(&self, px: f32) -> f32 {
        (self.cell.1 as f32 / self.pixels_per_meter).max(SPRITE_MIN_PX * px)
    }

    /// 애니메이션 진행에 필요한 시트.
    pub(crate) fn anim(&self) -> &SpriteSheet {
        &self.sheet
    }

    /// `pos`(발밑)에 이 시트를 몸으로 세우고 `parts` 를 겹친다. 재생기는 호출자 것 —
    /// 플레이 모드는 유닛마다 따로 둔다.
    ///
    /// 모든 층이 **몸의 재생기가 고른 같은 칸**을 그린다. 몸과 격자가 맞지 않는 파츠는 건너뛴다 —
    /// 읽을 때 [`fits`](Self::fits) 로 걸러 경고하므로 여기까지 오지 않는 것이 정상이다.
    /// 층은 앞뒤(`layer`) 순으로 내고, 같으면 `parts` 에 적은 순서를 따른다.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push(
        &self,
        pos: Vec2,
        look: Look,
        parts: &[Part<'_>],
        animator: &SpriteAnimator,
        px: f32,
        depth_bias: f32,
        out: &mut Vec<RenderCommand>,
    ) {
        let height = self.height(px);
        // 시트 칸 비율을 유지한다. 늘어나면 픽셀아트가 뭉개져 보인다.
        let width = height * self.cell.0 as f32 / self.cell.1 as f32;

        // 카메라 yaw 가 고정이라 방향을 그대로 넘긴다. 회전하는 카메라가 생기면
        // `heading - camera_yaw` 가 된다.
        let index = units::direction_index(look.heading, self.sheet.directions()) as usize;
        let row = self.direction_rows.get(index).copied().unwrap_or(0);
        let uv = animator.uv(&self.sheet, row);

        // (앞뒤, 텍스처, 색). 몸은 0 이고 파츠 앞에 둬서 같은 층이면 파츠가 이긴다.
        let mut layers = vec![(0, self.texture, tint_of(self.tinted, look))];
        for part in parts.iter().filter(|p| p.sheet.fits(self)) {
            let layer = part.sheet.layers.get(index).copied().unwrap_or(1);
            let tint = tint_of(
                part.sheet.tinted,
                Look {
                    tint: part.tint,
                    fallback: NO_TINT,
                    ..look
                },
            );
            layers.push((layer, part.sheet.texture, tint));
        }
        // 안정 정렬 — 같은 앞뒤면 적은 순서 그대로.
        layers.sort_by_key(|(layer, ..)| *layer);

        for (i, (_, texture, tint)) in layers.into_iter().enumerate() {
            out.push(RenderCommand::DrawSprite {
                // 앵커가 BottomCenter 라 발밑에서 위로 선다.
                pos: Vec3::new(pos.x, pos.y, 0.0),
                size: Vec2::new(width, height),
                anchor: SpriteAnchor::BottomCenter,
                depth_bias: depth_bias + i as f32 * PART_DEPTH_STEP,
                uv,
                texture,
                tint,
            });
        }
    }

    /// 이 시트를 `body` 위에 파츠로 겹칠 수 있는가 — 같은 칸을 같은 UV 로 가리키려면
    /// 칸 크기·열·행·방향 수·방향 행이 모두 같아야 한다.
    pub(crate) fn fits(&self, body: &Sheet) -> bool {
        let (a, b) = (self.sheet.atlas(), body.sheet.atlas());
        self.cell == body.cell
            && a.columns() == b.columns()
            && a.rows() == b.rows()
            && self.sheet.directions() == body.sheet.directions()
            && self.direction_rows == body.direction_rows
    }
}

/// 같은 유닛의 층 사이 깊이 간격 (NDC).
///
/// 층들은 같은 발밑에 서서 수학적으로 깊이가 같다. 그대로 두면 정점 계산의 반올림(`1e-7` 안팎)에 따라
/// 칸마다 이기는 층이 바뀌어 지글거린다. 그보다 한 자릿수 크게, `DEPTH_LAYER` 보다는 두 자릿수 작게 —
/// 쿼터뷰 기본 줌에서 앞뒤로 1cm 안쪽에 선 다른 유닛과만 순서가 섞인다.
pub(crate) const PART_DEPTH_STEP: f32 = nexus_render::DEPTH_LAYER / 100.0;

/// 파츠 한 장의 출처 — `data/display.ron` 에서 온다.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PartRef {
    /// 시트 정의 파일 경로.
    pub(crate) sheet: String,
    /// 곱할 색 (sRGB).
    pub(crate) tint: [f32; 4],
    /// 템플릿 파츠만 — 이 자리에 **외형이 있는** 장비를 입으면 빠진다. 장비 파츠는 `None`.
    pub(crate) slot: Option<EquipSlot>,
}

/// [`SpriteLibrary::load`] 가 읽을 것 — `GameData::sprite_sources` 가 만든다.
#[derive(Debug, Default)]
pub(crate) struct SpriteSources {
    /// 액터 타입별 몸 시트.
    pub(crate) sheets: Vec<(ActorId, String)>,
    /// 액터 타입(템플릿)별 기본 파츠.
    pub(crate) actor_parts: Vec<(ActorId, Vec<PartRef>)>,
    /// 외형을 바꾸는 장비의 파츠. 여기에 없는 장비는 외형에 영향이 없다.
    pub(crate) item_parts: Vec<(ItemId, Vec<PartRef>)>,
}

/// 템플릿 파츠와 입은 장비로 유닛이 실제로 겹칠 파츠를 고른다.
///
/// - 외형이 있는 장비(파츠가 하나 이상)가 차지한 자리의 템플릿 파츠는 빠진다.
/// - 외형이 없는 장비(빈 목록 — 목걸이·반지)는 아무것도 바꾸지 않는다.
/// - 순서: 남은 템플릿 파츠(적은 순서) → 장비 파츠(`worn` 순서). 앞뒤는 그리기에서 `layer` 로 다시 정렬된다.
pub(crate) fn compose<'a, T>(
    template: &'a [(T, Option<EquipSlot>)],
    worn: &[(EquipSlot, &'a [T])],
) -> Vec<&'a T> {
    let covered = |slot: EquipSlot| {
        worn.iter()
            .any(|(s, parts)| *s == slot && !parts.is_empty())
    };
    template
        .iter()
        .filter(|(_, slot)| !slot.is_some_and(covered))
        .map(|(part, _)| part)
        .chain(worn.iter().flat_map(|(_, parts)| parts.iter()))
        .collect()
}

/// GPU 에 올라간 파츠 한 장과 데이터가 정한 색.
type LoadedPart = (Sheet, [f32; 4]);

/// 몸 위에 겹치는 파츠 한 장과 그 색.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Part<'a> {
    pub(crate) sheet: &'a Sheet,
    /// 곱할 색 (sRGB) — 머리색·옷 색. [`NO_TINT`] 면 그림 그대로.
    pub(crate) tint: [f32; 4],
}

/// 이 시트에 실제로 곱할 색.
///
/// | | 액터가 색을 명시함 | 명시 안 함 ([`NO_TINT`]) |
/// |---|---|---|
/// | **무채색 시트** (`tinted: true`) | 그 색 | `fallback`(마커 종류 색) — 색이 없으면 형체만 남는다 |
/// | **컬러 시트** | 그 색 — 같은 그림의 색 변종(붉은 슬라임) | 그림 그대로 |
///
/// **종류 색은 무채색 시트에만 간다.** 컬러 아트에 곱하면 플레이어가 초록색으로 물든다
/// (실제로 겪은 버그 — 기본 색과 명시한 색을 한 값으로 합쳐 넘겼더니 구분이 사라졌다).
fn tint_of(tinted: bool, look: Look) -> [f32; 4] {
    match (look.tint != NO_TINT, tinted) {
        (true, _) => look.tint,
        (false, true) => look.fallback,
        (false, false) => NO_TINT,
    }
}

/// "색을 정하지 않음" — 곱해도 그림이 그대로다.
pub(crate) const NO_TINT: [f32; 4] = [1.0; 4];

/// 스프라이트를 어떻게 보이게 할지.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Look {
    /// 바라보는 방향 (라디안) — 시트의 방향 행을 고른다.
    pub(crate) heading: f32,
    /// 액터가 **명시한** 색 (sRGB). 정하지 않았으면 [`NO_TINT`]. 어느 시트에나 곱해진다.
    pub(crate) tint: [f32; 4],
    /// 무채색 시트에만 쓰는 기본 색 — 마커 종류 색.
    pub(crate) fallback: [f32; 4],
}

/// 마커 종류별 시트 모음.
///
/// 에디터 마커는 재생기 **하나를 공유해** 모두 같은 위상으로 움직인다 — 에디터에서는 그게 낫다.
/// 플레이 모드는 유닛마다 따로 재생기를 두고 [`Sheet::push`] 로 그린다.
#[derive(Debug)]
pub(crate) struct SpriteLibrary {
    /// 내장 플레이스홀더 — 타입별 시트가 없거나 읽기에 실패하면 이것을 쓴다.
    fallback: Sheet,
    /// 액터 타입별 시트. 여러 타입이 같은 그림을 써도 한 번만 올린다.
    by_actor: BTreeMap<ActorId, Sheet>,
    /// 액터 타입(템플릿)별 기본 파츠 — 몸 시트와 격자가 맞는 것만 남는다.
    parts_by_actor: BTreeMap<ActorId, Vec<(LoadedPart, Option<EquipSlot>)>>,
    /// 외형을 바꾸는 장비의 파츠. 입은 유닛의 몸과 맞는지는 그릴 때 본다([`Sheet::fits`]).
    parts_by_item: BTreeMap<ItemId, Vec<LoadedPart>>,
    animator: SpriteAnimator,
}

impl SpriteLibrary {
    /// 이 라이브러리가 GPU 에 올린 텍스처 — 그림을 다시 읽을 때 옛 것을 풀려고 (중복 없음).
    pub(crate) fn textures(&self) -> Vec<TextureId> {
        let mut all: Vec<TextureId> = std::iter::once(&self.fallback)
            .chain(self.by_actor.values())
            .chain(self.parts_by_actor.values().flatten().map(|((s, _), _)| s))
            .chain(self.parts_by_item.values().flatten().map(|(s, _)| s))
            .map(|s| s.texture)
            .collect();
        all.sort_by_key(|t| t.index());
        all.dedup();
        all
    }

    /// 내장 시트를 올리고, `sources.sheets` 가 가리키는 **액터 타입별** 시트를 덮어쓴다.
    ///
    /// 같은 경로가 여러 번 나오면(타입·파츠 공유) **그림은 한 번만 GPU 에 올린다.**
    /// 템플릿 파츠는 그 타입의 몸 시트(없으면 내장)와 격자가 맞아야 하고([`Sheet::fits`]),
    /// 장비 파츠는 읽어 둔 몸 시트 중 **하나라도** 맞아야 한다 — 누가 입을지는 플레이 중에 정해진다.
    ///
    /// # Errors
    /// 내장 시트를 올리지 못하면 오류. **타입별 시트 실패는 오류가 아니라** `warnings` 에 남기고
    /// 그 타입만 내장 시트를 쓴다 — 애셋 하나가 잘못됐다고 에디터가 안 열리면 곤란하다.
    /// 읽지 못했거나 몸과 맞지 않는 파츠도 경고를 남기고 그 파츠만 뺀다.
    pub(crate) fn load(
        renderer: &mut impl Renderer,
        sources: &SpriteSources,
        warnings: &mut Vec<String>,
    ) -> Result<Self, String> {
        let fallback = Sheet::upload(plan_sheet(EMBEDDED_DEF, None)?, renderer)?;
        let mut by_actor: BTreeMap<ActorId, Sheet> = BTreeMap::new();
        // 경로 → 이미 올린 시트. 같은 그림을 여러 타입·파츠가 공유할 때 중복 업로드를 막는다.
        let mut uploaded: BTreeMap<&str, Sheet> = BTreeMap::new();

        for (actor, path_str) in &sources.sheets {
            match load_once(renderer, path_str, &mut uploaded) {
                Ok(sheet) => {
                    by_actor.insert(*actor, sheet);
                }
                Err(e) => warnings.push(format!("액터 {} 시트를 쓸 수 없음 — {e}", actor.raw())),
            }
        }

        let mut parts_by_actor = BTreeMap::new();
        for (actor, list) in &sources.actor_parts {
            let body = by_actor.get(actor).unwrap_or(&fallback);
            let mut kept = Vec::new();
            for part in list {
                match load_once(renderer, &part.sheet, &mut uploaded) {
                    Ok(sheet) if sheet.fits(body) => kept.push(((sheet, part.tint), part.slot)),
                    Ok(_) => warnings.push(format!(
                        "액터 {} 파츠 {} — 몸 시트와 칸 크기·행·열·방향이 다르다",
                        actor.raw(),
                        part.sheet
                    )),
                    Err(e) => {
                        warnings.push(format!("액터 {} 파츠를 쓸 수 없음 — {e}", actor.raw()));
                    }
                }
            }
            if !kept.is_empty() {
                parts_by_actor.insert(*actor, kept);
            }
        }

        let mut parts_by_item = BTreeMap::new();
        for (item, list) in &sources.item_parts {
            let mut kept = Vec::new();
            for part in list {
                match load_once(renderer, &part.sheet, &mut uploaded) {
                    Ok(sheet)
                        if std::iter::once(&fallback)
                            .chain(by_actor.values())
                            .any(|b| sheet.fits(b)) =>
                    {
                        kept.push((sheet, part.tint));
                    }
                    Ok(_) => warnings.push(format!(
                        "아이템 {} 파츠 {} — 칸 크기·행·열·방향이 맞는 몸 시트가 없다",
                        item.0, part.sheet
                    )),
                    Err(e) => warnings.push(format!("아이템 {} 파츠를 쓸 수 없음 — {e}", item.0)),
                }
            }
            // 전부 실패해도 빈 목록으로 남기지 않는다 — 빈 목록은 "외형 없음" 이라 템플릿 파츠를 빼지 않는다.
            if !kept.is_empty() {
                parts_by_item.insert(*item, kept);
            }
        }

        Ok(Self {
            fallback,
            by_actor,
            parts_by_actor,
            parts_by_item,
            animator: SpriteAnimator::default(),
        })
    }

    /// 이 액터 타입(템플릿)이 `worn`(자리, 아이템)을 입었을 때 겹칠 파츠 — [`compose`] 규칙.
    /// `tint` 가 있으면 모든 파츠의 색을 그것으로 바꾼다 — 시체·피격처럼 몸과 함께 물들어야 할 때.
    pub(crate) fn parts(
        &self,
        actor: ActorId,
        worn: &[(EquipSlot, ItemId)],
        tint: Option<[f32; 4]>,
    ) -> Vec<Part<'_>> {
        let template = self
            .parts_by_actor
            .get(&actor)
            .map_or(&[][..], Vec::as_slice);
        let worn: Vec<(EquipSlot, &[LoadedPart])> = worn
            .iter()
            .map(|(slot, item)| {
                let parts = self.parts_by_item.get(item).map_or(&[][..], Vec::as_slice);
                (*slot, parts)
            })
            .collect();
        compose(template, &worn)
            .into_iter()
            .map(|(sheet, own)| Part {
                sheet,
                tint: tint.unwrap_or(*own),
            })
            .collect()
    }

    /// 이 액터 타입이 쓸 시트. 없으면 내장 플레이스홀더.
    pub(crate) fn sheet(&self, actor: ActorId) -> &Sheet {
        self.by_actor.get(&actor).unwrap_or(&self.fallback)
    }

    /// 에디터 마커용 공용 재생기를 진행한다. **고정 timestep 에서만** 호출한다 —
    /// 렌더 프레임에서 부르면 프레임레이트에 따라 속도가 달라진다.
    pub(crate) fn advance(&mut self, dt: Duration) {
        // 종류마다 시트가 달라도 재생기는 하나다. 에디터 표시에는 위상이 맞을 필요가 없다.
        let Self {
            fallback, animator, ..
        } = self;
        animator.advance(&fallback.sheet, dt);
    }

    /// 마커 하나를 세운다 (에디터 — 공용 재생기).
    ///
    /// `actor` 는 마커가 실제로 쓸 액터 타입이다 (`GameData::resolve_actor` 가 푼 값).
    /// `tint` 는 그 타입이 **명시한** 색(없으면 [`NO_TINT`]) — 무채색 시트는 대신 마커 종류 색을 쓴다.
    /// `px` 는 화면 1픽셀에 해당하는 월드 길이 — 빌보드는 카메라 축을 쓰므로 가로·세로가 같은 배율이다.
    pub(crate) fn build(
        &self,
        item: &Item,
        actor: ActorId,
        tint: [f32; 4],
        px: f32,
        depth_bias: f32,
        out: &mut Vec<RenderCommand>,
    ) {
        let look = Look {
            heading: item.orientation,
            tint,
            fallback: item.kind.color(),
        };
        let parts = self.parts(actor, &[], None);
        self.sheet(actor)
            .push(item.pos, look, &parts, &self.animator, px, depth_bias, out);
    }
}

/// 시트 하나를 읽어 올린다 — 이미 올린 경로면 텍스처를 공유한다.
fn load_once<'p>(
    renderer: &mut impl Renderer,
    path_str: &'p str,
    uploaded: &mut BTreeMap<&'p str, Sheet>,
) -> Result<Sheet, String> {
    if let Some(sheet) = uploaded.get(path_str) {
        return Ok(sheet.share());
    }
    let path = Path::new(path_str);
    let sheet = std::fs::read_to_string(path)
        .map_err(|e| format!("{}: {e}", path.display()))
        .and_then(|def| plan_sheet(&def, path.parent()))
        .and_then(|plan| Sheet::upload(plan, renderer))?;
    uploaded.insert(path_str, sheet.share());
    Ok(sheet)
}

/// 시험용 — 시트 정의 본문을 GPU·디스크 없이 읽는다. `image` 를 비우면 내장 `markers.png` 를 쓴다.
///
/// 받아온 애셋의 시트를 읽지 않는 이유: 시트마다 있는 클립이 달라서(공격·사망 그림이 없는 팩이 많다)
/// 애셋을 갈아끼우면 재생 규칙 시험이 같이 깨진다. 규칙 시험은 클립을 시험 안에 적는다.
#[cfg(test)]
pub(crate) fn sheet_for_test(def: &str) -> SpriteSheet {
    plan_sheet(def, None).unwrap().sheet
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_render::UvRect;

    fn embedded() -> SheetPlan {
        plan_sheet(EMBEDDED_DEF, None).unwrap()
    }

    /// 정의 안의 한 줄을 바꿔 시험용 정의를 만든다.
    fn tweaked(from: &str, to: &str) -> Result<SheetPlan, String> {
        plan_sheet(&EMBEDDED_DEF.replacen(from, to, 1), None)
    }

    #[test]
    fn shipped_sheet_definition_matches_the_picture() {
        let plan = embedded();
        assert_eq!(plan.cell, (32, 48));
        assert_eq!(plan.sheet.directions(), 4);
        assert!(plan.sheet.clip(AnimState::Walk).is_some());
        assert!(plan.tinted, "플레이스홀더는 무채색이라 색을 입힌다");
        assert_eq!(plan.direction_rows, vec![0, 1, 2, 3], "생략하면 순서대로");
    }

    #[test]
    fn clips_outside_the_picture_are_rejected() {
        assert!(tweaked("frames: 4", "frames: 9").is_err(), "열 수 초과");
        // Walk 는 4 행부터 4 방향 = 8 행. 8 방향으로 늘리면 그림(8 행)을 넘는다.
        assert!(
            tweaked("directions: 4", "directions: 8").is_err(),
            "행 수 초과"
        );
    }

    #[test]
    fn cell_size_must_divide_the_picture() {
        assert!(tweaked("cell: (32, 48)", "cell: (30, 48)").is_err());
    }

    #[test]
    fn direction_rows_map_engine_directions_onto_sheet_rows() {
        // 시트가 남·동·북·서 순서일 때의 매핑 — 받아온 애셋에서 흔한 순서다.
        let plan = tweaked(
            "directions: 4,",
            "directions: 4,\n    direction_rows: [1, 2, 3, 0],",
        )
        .unwrap();
        assert_eq!(plan.direction_rows, vec![1, 2, 3, 0]);

        let row_for = |heading: f32| {
            let i = units::direction_index(heading, 4) as usize;
            plan.direction_rows[i]
        };
        use std::f32::consts::{FRAC_PI_2, PI};
        assert_eq!(row_for(0.0), 1, "동 → 1행");
        assert_eq!(row_for(FRAC_PI_2), 2, "북 → 2행");
        assert_eq!(row_for(PI), 3, "서 → 3행");
        assert_eq!(row_for(-FRAC_PI_2), 0, "남 → 0행");
    }

    #[test]
    fn bad_direction_rows_are_rejected() {
        for rows in ["[1, 2]", "[0, 1, 2, 9]"] {
            let def = format!("directions: 4,\n    direction_rows: {rows},");
            assert!(tweaked("directions: 4,", &def).is_err(), "{rows}");
        }
    }

    #[test]
    fn colour_sheets_keep_their_own_colours_unless_told_otherwise() {
        let plan = tweaked("tinted: true,", "tinted: false,").unwrap();
        assert!(!plan.tinted);

        // 플레이어 마커의 종류 색(초록). 무채색 플레이스홀더에만 가야 한다.
        let kind_green = [0.30, 0.85, 0.55, 1.0];

        // 색을 정하지 않았으면 컬러 아트는 그대로 — **종류 색이 새어 들어가면 안 된다**
        // (플레이어가 초록색으로 물들던 버그).
        let plain = Look {
            heading: 0.0,
            tint: NO_TINT,
            fallback: kind_green,
        };
        assert_eq!(tint_of(plan.tinted, plain), NO_TINT, "컬러 아트가 물들었다");

        // 액터 타입이 색을 명시했으면 컬러 아트에도 곱한다 — 같은 그림의 색 변종.
        let recoloured = Look {
            heading: 0.0,
            tint: [1.0, 0.45, 0.45, 1.0],
            fallback: kind_green,
        };
        assert_eq!(tint_of(plan.tinted, recoloured), recoloured.tint);

        // 무채색 시트는 색이 없으면 형체만 남으므로 종류 색을 쓴다. 명시한 색이 있으면 그것이 이긴다.
        assert_eq!(tint_of(true, plain), kind_green);
        assert_eq!(tint_of(true, recoloured), recoloured.tint);
    }

    /// GPU 없이 시트를 만든다 — 텍스처 번호만 가짜로 붙인다.
    fn sheet_with(def: &str, texture: u32) -> Sheet {
        let plan = plan_sheet(def, None).unwrap();
        Sheet {
            texture: TextureId::from_index(texture),
            sheet: plan.sheet,
            cell: plan.cell,
            direction_rows: plan.direction_rows,
            tinted: plan.tinted,
            pixels_per_meter: plan.pixels_per_meter,
            layers: plan.layers,
        }
    }

    /// 내장 그림을 그대로 쓰는 파츠 정의 — 격자가 몸과 같다.
    fn part_def(layer: &str) -> String {
        format!("(version: 1, cell: (32, 48), directions: 4, {layer} clips: [])")
    }

    /// 그려진 층의 `(텍스처, 깊이 편향, uv, 색)` 을 낸 순서대로.
    fn drawn(heading: f32, parts: &[Part<'_>], body: &Sheet) -> Vec<(u32, f32, UvRect, [f32; 4])> {
        let look = Look {
            heading,
            tint: NO_TINT,
            fallback: [0.5; 4],
        };
        let mut out = Vec::new();
        body.push(
            Vec2::ZERO,
            look,
            parts,
            &SpriteAnimator::default(),
            0.01,
            3.0 * nexus_render::DEPTH_LAYER, // 플레이의 BIAS_SPRITE 와 같은 크기
            &mut out,
        );
        out.iter()
            .map(|c| match c {
                RenderCommand::DrawSprite {
                    texture,
                    depth_bias,
                    uv,
                    tint,
                    ..
                } => (texture.index(), *depth_bias, *uv, *tint),
                other => panic!("빌보드가 아님: {other:?}"),
            })
            .collect()
    }

    #[test]
    fn parts_stack_on_the_body_in_layer_order_with_the_same_cell() {
        let body = sheet_with(EMBEDDED_DEF, 1);
        let hat = sheet_with(&part_def("layer: 2,"), 2);
        let shirt = sheet_with(&part_def(""), 3); // 기본 layer 1
        let cape = sheet_with(&part_def("layer: -1,"), 4);
        let red = [1.0, 0.2, 0.2, 1.0];
        let parts = [
            Part {
                sheet: &hat,
                tint: NO_TINT,
            },
            Part {
                sheet: &shirt,
                tint: red,
            },
            Part {
                sheet: &cape,
                tint: NO_TINT,
            },
        ];

        let layers = drawn(0.0, &parts, &body);
        let order: Vec<u32> = layers.iter().map(|l| l.0).collect();
        assert_eq!(
            order,
            vec![4, 1, 3, 2],
            "망토(-1) → 몸(0) → 옷(1) → 모자(2)"
        );

        // 뒤에 낸 층일수록 앞 — 같은 발밑이라 깊이 편향만이 앞뒤를 정한다.
        for pair in layers.windows(2) {
            assert!(pair[1].1 > pair[0].1, "{pair:?}");
            assert!(pair[1].1 - pair[0].1 >= PART_DEPTH_STEP * 0.99);
        }
        // 다른 유닛과 섞이지 않도록 층이 쌓여도 DEPTH_LAYER 한 칸 안에 머문다.
        assert!(layers[3].1 - layers[0].1 < nexus_render::DEPTH_LAYER);

        assert!(
            layers.iter().all(|l| l.2 == layers[0].2),
            "모든 층이 몸과 같은 칸"
        );
        assert_eq!(layers[2].3, red, "파츠 색은 파츠마다");
        assert_eq!(layers[1].3, [0.5; 4], "무채색 몸은 기본 색 그대로");
    }

    #[test]
    fn a_part_can_go_behind_the_body_in_some_directions() {
        use std::f32::consts::FRAC_PI_2;
        let body = sheet_with(EMBEDDED_DEF, 1);
        // 엔진 방향 1(북 = 뒷모습)에서만 몸 뒤로 — 등에 멘 무기.
        let sword = sheet_with(&part_def("layer_by_direction: [1, -1, 1, 1],"), 2);
        let parts = [Part {
            sheet: &sword,
            tint: NO_TINT,
        }];

        let east: Vec<u32> = drawn(0.0, &parts, &body).iter().map(|l| l.0).collect();
        let north: Vec<u32> = drawn(FRAC_PI_2, &parts, &body)
            .iter()
            .map(|l| l.0)
            .collect();
        assert_eq!(east, vec![1, 2], "앞모습: 몸 위");
        assert_eq!(north, vec![2, 1], "뒷모습: 몸 뒤");
    }

    #[test]
    fn parts_must_share_the_body_grid() {
        let body = sheet_with(EMBEDDED_DEF, 1);
        assert!(sheet_with(&part_def(""), 2).fits(&body));

        // 같은 그림이어도 칸을 다르게 자르면 UV 가 다른 칸을 가리킨다.
        let wide = sheet_with(&part_def("").replace("(32, 48)", "(64, 48)"), 2);
        assert!(!wide.fits(&body));
        // 방향 행 순서가 다르면 앞모습 몸에 뒷모습 모자가 얹힌다.
        let rows = sheet_with(&part_def("direction_rows: [1, 2, 3, 0],"), 2);
        assert!(!rows.fits(&body));

        // 맞지 않는 파츠는 그리지 않는다.
        let parts = [Part {
            sheet: &wide,
            tint: NO_TINT,
        }];
        assert_eq!(drawn(0.0, &parts, &body).len(), 1);
    }

    #[test]
    fn layer_by_direction_needs_one_entry_per_direction() {
        assert!(plan_sheet(&part_def("layer_by_direction: [1, -1],"), None).is_err());
        let plan = plan_sheet(&part_def("layer: 3,"), None).unwrap();
        assert_eq!(plan.layers, vec![3; 4], "비우면 모든 방향이 layer");
    }

    #[test]
    fn equipment_with_a_look_replaces_the_template_part_of_its_slot() {
        // 템플릿: 머리카락(자리 없음) · 기본 옷(몸) · 기본 신발(신발).
        let template = [
            ("머리카락", None),
            ("기본 옷", Some(EquipSlot::Body)),
            ("기본 신발", Some(EquipSlot::Shoes)),
        ];
        let armor = ["갑옷", "갑옷 망토"];
        let hat = ["모자"];
        let none: [&str; 0] = [];

        let bare = compose(&template, &[]);
        assert_eq!(bare, [&"머리카락", &"기본 옷", &"기본 신발"]);

        let worn = [
            (EquipSlot::Head, &hat[..]),
            (EquipSlot::Body, &armor[..]),
            // 외형이 없는 장비 — 목걸이·반지, 그리고 그림 없는 신발.
            (EquipSlot::Accessory(nexus_sim::AccessoryKind(1)), &none[..]),
            (EquipSlot::Accessory(nexus_sim::AccessoryKind(2)), &none[..]),
            (EquipSlot::Shoes, &none[..]),
        ];
        assert_eq!(
            compose(&template, &worn),
            [&"머리카락", &"기본 신발", &"모자", &"갑옷", &"갑옷 망토"],
            "갑옷이 기본 옷을 대신하고, 그림 없는 신발은 기본 신발을 빼지 않는다. 머리카락은 모자와 함께 남는다"
        );
    }

    /// 종이 인형 템플릿(`WorldCanvas/tools/paperdoll.py` 결과) — 모든 파츠가 읽히고 자기 템플릿의 몸과 맞는가.
    ///
    /// 생성 결과는 추적하지 않는 애셋 폴더에 있다. 아직 만들지 않은 작업 폴더에서는 건너뛴다.
    #[test]
    fn paperdoll_parts_fit_their_template_body() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/worldcanvas/paperdoll");
        let Ok(dirs) = std::fs::read_dir(&root) else {
            return;
        };
        let load = |path: &Path| {
            let def = std::fs::read_to_string(path).unwrap();
            let plan = plan_sheet(&def, path.parent())
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            Sheet {
                texture: TextureId::from_index(1),
                sheet: plan.sheet,
                cell: plan.cell,
                direction_rows: plan.direction_rows,
                tinted: plan.tinted,
                pixels_per_meter: plan.pixels_per_meter,
                layers: plan.layers,
            }
        };
        let mut templates = 0;
        for dir in dirs.flatten().filter(|d| d.path().is_dir()) {
            let body = load(&dir.path().join("body.sheet.ron"));
            for part in std::fs::read_dir(dir.path()).unwrap().flatten() {
                let path = part.path();
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                if name.ends_with(".sheet.ron") && name != "body.sheet.ron" {
                    assert!(
                        load(&path).fits(&body),
                        "{} 가 몸과 맞지 않는다",
                        path.display()
                    );
                }
            }
            templates += 1;
        }
        assert!(templates > 0, "템플릿 폴더가 비었다");
    }

    #[test]
    fn a_missing_image_is_reported_with_its_path() {
        let def = tweaked("version: 1,", "version: 1,\n    image: \"없는/그림.png\",");
        let err = def.err().unwrap_or_default();
        assert!(err.contains("없는"), "{err}");
    }

    /// `data/display.ron` 이 가리키는 시트가 실제로 읽히는지.
    ///
    /// 실행 중에는 읽기 실패가 **경고**로 끝나고 그 종류만 내장 시트로 남는다 — 애셋 이름을
    /// 바꾸면 조용히 플레이스홀더로 돌아가므로, 저장소에 들어 있는 조합은 테스트가 지킨다.
    #[test]
    fn sheets_referenced_by_display_data_all_load() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for (kind, rel) in crate::game_data::GameData::embedded().sprite_sheets() {
            let path = root.join(&rel);
            let def = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{kind:?}: {} — {e}", path.display()));
            if let Err(e) = plan_sheet(&def, path.parent()) {
                // 그림 경로는 정의 파일 폴더 기준으로 찾으므로 작업 디렉터리와 무관하다.
                panic!("{kind:?}: {rel} — {e}");
            }
        }
    }

    #[test]
    fn combat_clips_are_one_shot() {
        // 공격·피격·사망은 한 번짜리여야 끝난다 — 루프면 플레이에서 그 동작에 갇힌다.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for (kind, rel) in crate::game_data::GameData::embedded().sprite_sheets() {
            let path = root.join(&rel);
            let def = std::fs::read_to_string(&path).unwrap();
            let sheet = plan_sheet(&def, path.parent()).unwrap().sheet;
            for state in [AnimState::Attack, AnimState::Hit, AnimState::Die] {
                if let Some(clip) = sheet.clip(state) {
                    assert!(!clip.looping, "{kind:?}: {rel} 의 {state:?} 가 루프다");
                }
            }
        }
    }

    #[test]
    fn image_path_falls_back_to_the_definition_folder() {
        // 정의 파일이 애셋 폴더에 있고 그림 경로가 그 폴더 기준일 때.
        let base = Path::new("assets/sprites");
        assert_eq!(
            resolve("markers.png", Some(base)),
            base.join("markers.png"),
            "작업 디렉터리에 없으면 정의 파일 폴더에서 찾는다"
        );
    }
}
