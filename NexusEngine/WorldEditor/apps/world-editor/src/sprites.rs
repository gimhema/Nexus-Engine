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
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nexus_assets::{AnimState, Clip, GridAtlas, Image, SpriteAnimator, SpriteSheet};
use nexus_core::{Camera2d, Vec2, Vec3, units};
use nexus_render::{RenderCommand, Renderer, SpriteAnchor, TextureId};
use serde::Deserialize;

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
    animator: SpriteAnimator,
}

impl SpriteLibrary {
    /// 내장 시트를 올리고, `sheets` 가 가리키는 **액터 타입별** 시트를 덮어쓴다.
    ///
    /// `sheets` 는 `(액터 번호, 정의 파일 경로)` 목록이다 — `data/display.ron` 에서 온다.
    /// 같은 경로가 여러 타입에 나오면 **그림은 한 번만 GPU 에 올린다** (텍스처 해제 API 가 없다).
    ///
    /// # Errors
    /// 내장 시트를 올리지 못하면 오류. **타입별 시트 실패는 오류가 아니라** `warnings` 에 남기고
    /// 그 타입만 내장 시트를 쓴다 — 애셋 하나가 잘못됐다고 에디터가 안 열리면 곤란하다.
    pub(crate) fn load(
        renderer: &mut impl Renderer,
        sheets: &[(ActorId, String)],
        warnings: &mut Vec<String>,
    ) -> Result<Self, String> {
        let fallback = Sheet::upload(plan_sheet(EMBEDDED_DEF, None)?, renderer)?;
        let mut by_actor: BTreeMap<ActorId, Sheet> = BTreeMap::new();
        // 경로 → 이미 올린 타입. 같은 그림을 여러 타입이 공유할 때 중복 업로드를 막는다.
        let mut uploaded: BTreeMap<&str, ActorId> = BTreeMap::new();

        for (actor, path_str) in sheets {
            if let Some(first) = uploaded.get(path_str.as_str()) {
                if let Some(sheet) = by_actor.get(first).map(Sheet::share) {
                    by_actor.insert(*actor, sheet);
                }
                continue;
            }
            let path = Path::new(path_str);
            let loaded = std::fs::read_to_string(path)
                .map_err(|e| format!("{}: {e}", path.display()))
                .and_then(|def| plan_sheet(&def, path.parent()))
                .and_then(|plan| Sheet::upload(plan, renderer));
            match loaded {
                Ok(sheet) => {
                    by_actor.insert(*actor, sheet);
                    uploaded.insert(path_str.as_str(), *actor);
                }
                Err(e) => warnings.push(format!("액터 {} 시트를 쓸 수 없음 — {e}", actor.raw())),
            }
        }
        Ok(Self {
            fallback,
            by_actor,
            animator: SpriteAnimator::default(),
        })
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
        self.sheet(actor)
            .push(item.pos, look, &self.animator, px, depth_bias, out);
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
