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
//! # 어디서 읽나
//!
//! 기본 시트(플레이스홀더)는 실행 파일에 **내장**되어 작업 디렉터리와 무관하게 항상 뜬다.
//! `data/display.ron` 의 `sprites` 가 종류별 정의 파일을 가리키면 **그 파일을 읽어 덮어쓴다** —
//! 그림을 갈아끼워도 다시 빌드할 필요가 없다. 읽기에 실패하면 그 종류만 내장 시트로 남고
//! 이유를 알린다 (조용히 넘어가지 않는다).

use core::time::Duration;
use std::path::{Path, PathBuf};

use nexus_assets::{AnimState, Clip, GridAtlas, Image, SpriteAnimator, SpriteSheet};
use nexus_core::{Camera2d, Vec2, Vec3, units};
use nexus_render::{RenderCommand, Renderer, SpriteAnchor, TextureId};
use serde::Deserialize;

use crate::scene::{Item, ItemKind};

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

/// GPU 에 올라간 시트 하나.
#[derive(Debug)]
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
        })
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

    /// `pos`(발밑)에 스프라이트 하나를 세운다. 재생기는 호출자 것 — 플레이 모드는 유닛마다 따로 둔다.
    pub(crate) fn push(
        &self,
        pos: Vec2,
        look: Look,
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

        out.push(RenderCommand::DrawSprite {
            // 앵커가 BottomCenter 라 발밑에서 위로 선다.
            pos: Vec3::new(pos.x, pos.y, 0.0),
            size: Vec2::new(width, height),
            anchor: SpriteAnchor::BottomCenter,
            depth_bias,
            uv: animator.uv(&self.sheet, row),
            texture: self.texture,
            tint: tint_of(self.tinted, look),
        });
    }
}

/// 무채색 시트면 종류 색을, 컬러 시트면 흰색(그림 그대로)을 쓴다.
fn tint_of(tinted: bool, look: Look) -> [f32; 4] {
    if tinted { look.tint } else { [1.0; 4] }
}

/// 스프라이트를 어떻게 보이게 할지.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Look {
    /// 바라보는 방향 (라디안) — 시트의 방향 행을 고른다.
    pub(crate) heading: f32,
    /// 무채색 시트에 곱할 색 (sRGB). 컬러 시트에서는 무시된다.
    pub(crate) tint: [f32; 4],
}

/// 마커 종류별 시트 모음.
///
/// 에디터 마커는 재생기 **하나를 공유해** 모두 같은 위상으로 움직인다 — 에디터에서는 그게 낫다.
/// 플레이 모드는 유닛마다 따로 재생기를 두고 [`Sheet::push`] 로 그린다.
#[derive(Debug)]
pub(crate) struct SpriteLibrary {
    /// 내장 플레이스홀더 — 종류별 시트가 없거나 읽기에 실패하면 이것을 쓴다.
    fallback: Sheet,
    /// 종류별 시트 (플레이어·NPC·몬스터).
    by_kind: [Option<Sheet>; 3],
    animator: SpriteAnimator,
}

impl SpriteLibrary {
    /// 내장 시트를 올리고, `sheets` 가 가리키는 종류별 시트를 덮어쓴다.
    ///
    /// `sheets` 는 `(종류, 정의 파일 경로)` 목록이다 — `data/display.ron` 에서 온다.
    ///
    /// # Errors
    /// 내장 시트를 올리지 못하면 오류. **종류별 시트 실패는 오류가 아니라** `warnings` 에 남기고
    /// 그 종류만 내장 시트를 쓴다 — 애셋 하나가 잘못됐다고 에디터가 안 열리면 곤란하다.
    pub(crate) fn load(
        renderer: &mut impl Renderer,
        sheets: &[(ItemKind, String)],
        warnings: &mut Vec<String>,
    ) -> Result<Self, String> {
        let fallback = Sheet::upload(plan_sheet(EMBEDDED_DEF, None)?, renderer)?;
        let mut by_kind: [Option<Sheet>; 3] = [None, None, None];

        for (kind, path) in sheets {
            let path = Path::new(path);
            let loaded = std::fs::read_to_string(path)
                .map_err(|e| format!("{}: {e}", path.display()))
                .and_then(|def| plan_sheet(&def, path.parent()))
                .and_then(|plan| Sheet::upload(plan, renderer));
            match loaded {
                Ok(sheet) => by_kind[slot(*kind)] = Some(sheet),
                Err(e) => warnings.push(format!("{} 시트를 쓸 수 없음 — {e}", kind.label())),
            }
        }
        Ok(Self {
            fallback,
            by_kind,
            animator: SpriteAnimator::default(),
        })
    }

    /// 이 종류가 쓸 시트.
    pub(crate) fn sheet(&self, kind: ItemKind) -> &Sheet {
        self.by_kind[slot(kind)].as_ref().unwrap_or(&self.fallback)
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
    /// `px` 는 화면 1픽셀에 해당하는 월드 길이 — 빌보드는 카메라 축을 쓰므로 가로·세로가 같은 배율이다.
    pub(crate) fn build(
        &self,
        item: &Item,
        px: f32,
        depth_bias: f32,
        out: &mut Vec<RenderCommand>,
    ) {
        let look = Look {
            heading: item.orientation,
            tint: item.kind.color(),
        };
        self.sheet(item.kind)
            .push(item.pos, look, &self.animator, px, depth_bias, out);
    }
}

fn slot(kind: ItemKind) -> usize {
    match kind {
        ItemKind::PlayerSpawn => 0,
        ItemKind::Npc => 1,
        ItemKind::Monster => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn colour_sheets_are_not_tinted() {
        let plan = tweaked("tinted: true,", "tinted: false,").unwrap();
        let look = Look {
            heading: 0.0,
            tint: [1.0, 0.0, 0.0, 1.0],
        };
        assert!(!plan.tinted);
        assert_eq!(
            tint_of(plan.tinted, look),
            [1.0; 4],
            "컬러 아트에 색을 곱하면 뒤집힌다"
        );
        assert_eq!(tint_of(true, look), look.tint, "무채색 시트는 색을 입힌다");
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
