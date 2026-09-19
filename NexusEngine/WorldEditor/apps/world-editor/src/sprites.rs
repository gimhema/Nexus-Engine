//! 마커 스프라이트 — 스폰 지점을 애니메이션 빌보드로 그린다.
//!
//! 게임 아트가 아직 없으므로 플레이스홀더 시트를 쓴다. 시트는 **무채색**이고
//! 종류별 색은 `tint` 로 입힌다 — 텍스처 하나로 세 종류를 다 그리므로
//! 드로우 콜도 한 번이다.
//!
//! 그림(`markers.png`)과 시트 정의(`markers.sheet.ron` — 칸 크기·방향 수·클립)는 짝이라
//! 둘 다 `include_*!` 로 실행 파일에 넣는다. 에디터 기본 표시라 작업 디렉터리에 의존하지 않아야 한다.

use core::time::Duration;

use nexus_assets::{AnimState, Clip, GridAtlas, Image, SpriteAnimator, SpriteSheet};
use nexus_core::{Camera2d, Vec2, Vec3, units};
use nexus_render::{RenderCommand, Renderer, SpriteAnchor, TextureId};
use serde::Deserialize;

use crate::scene::Item;

const SHEET_PNG: &[u8] = include_bytes!("../../../assets/sprites/markers.png");
const SHEET_DEF: &str = include_str!("../../../assets/sprites/markers.sheet.ron");

/// 화면에서 이보다 작게 그리지 않는다 (픽셀). 줌아웃해도 마커가 사라지지 않도록.
const SPRITE_MIN_PX: f32 = 20.0;

/// 시트 정의 파일.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SheetFile {
    version: u32,
    /// 한 칸의 픽셀 크기 (가로, 세로). 그림과 맞아야 한다.
    cell: (u32, u32),
    /// 이 시트가 담은 방향 수 — **엔진이 아니라 시트의 성질이다.**
    directions: u32,
    clips: Vec<ClipFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClipFile {
    state: StateFile,
    /// 첫 방향의 행. 방향 d 는 `row + d` 행.
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

/// 시트 정의를 읽고 그림에 맞는지 확인한다. 칸 크기와 클립이 그림을 벗어나면 오류 —
/// 벗어난 UV 는 옆 칸이나 가장자리를 비추므로 조용히 넘기면 안 된다.
fn parse_sheet(def: &str, image: &Image) -> Result<(SpriteSheet, (u32, u32)), String> {
    let file: SheetFile = ron::from_str(def).map_err(|e| format!("시트 정의: {e}"))?;
    if file.version != 1 {
        return Err(format!(
            "시트 정의 형식 {} 은(는) 읽을 수 없음",
            file.version
        ));
    }
    let (w, h) = file.cell;
    let atlas = GridAtlas::new(image, w, h).map_err(|e| e.to_string())?;
    let directions = file.directions.max(1);
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
    Ok((SpriteSheet::new(atlas, directions, clips), file.cell))
}

/// 로드된 마커 시트와 재생 상태.
///
/// 에디터 마커는 재생기 **하나를 공유해** 모두 같은 위상으로 움직인다 — 에디터에서는 그게 낫다.
/// 플레이 모드는 유닛마다 따로 재생기를 두고 [`MarkerSprites::push`] 로 그린다.
#[derive(Debug)]
pub(crate) struct MarkerSprites {
    texture: TextureId,
    sheet: SpriteSheet,
    /// 한 칸의 픽셀 크기 — 스프라이트의 월드 크기가 여기서 나온다.
    cell: (u32, u32),
    animator: SpriteAnimator,
}

impl MarkerSprites {
    /// 시트를 GPU 에 올린다.
    ///
    /// # Errors
    /// 디코딩·시트 정의·업로드에 실패하면 오류 문자열.
    pub(crate) fn load(renderer: &mut impl Renderer) -> Result<Self, String> {
        let image = Image::decode_png(SHEET_PNG).map_err(|e| e.to_string())?;
        let (sheet, cell) = parse_sheet(SHEET_DEF, &image)?;
        let texture = renderer
            .load_texture(&image.desc("markers"))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            texture,
            sheet,
            cell,
            animator: SpriteAnimator::default(),
        })
    }

    /// 애니메이션을 진행한다. **고정 timestep 에서만** 호출한다 —
    /// 렌더 프레임에서 부르면 프레임레이트에 따라 속도가 달라진다.
    pub(crate) fn advance(&mut self, dt: Duration) {
        self.animator.advance(&self.sheet, dt);
    }

    /// 시트 — 플레이 모드가 유닛마다 따로 재생기를 돌릴 때 쓴다.
    pub(crate) fn sheet(&self) -> &SpriteSheet {
        &self.sheet
    }

    /// 화면에 그려지는 스프라이트 높이 (m). 머리 위 표시(HP 막대)의 기준.
    ///
    /// 칸 높이를 기준 배율로 나눈 값이다 — 고정 줌에서 **스프라이트 1픽셀이 화면 1픽셀**이 된다.
    /// 픽셀아트가 뭉개지지 않으려면 이 관계를 지켜야 하므로 임의의 키(1.8m 같은)를 넣지 않는다.
    /// 더 큰 캐릭터는 시트 칸을 키운다.
    pub(crate) fn height(&self, px: f32) -> f32 {
        (self.cell.1 as f32 / Camera2d::PIXELS_PER_METER).max(SPRITE_MIN_PX * px)
    }

    /// 마커 하나를 세운다 (에디터 — 공용 재생기).
    ///
    /// `px` 는 화면 1픽셀에 해당하는 월드 길이 — 빌보드는 카메라 축을 쓰므로
    /// 가로·세로가 같은 배율이다.
    pub(crate) fn build(
        &self,
        item: &Item,
        px: f32,
        depth_bias: f32,
        out: &mut Vec<RenderCommand>,
    ) {
        // 시트가 무채색이라 여기서 종류별 색이 입혀진다.
        let look = Look {
            heading: item.orientation,
            tint: item.kind.color(),
        };
        self.push(item.pos, look, &self.animator, px, depth_bias, out);
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
        let direction = units::direction_index(look.heading, self.sheet.directions());

        out.push(RenderCommand::DrawSprite {
            // 앵커가 BottomCenter 라 발밑에서 위로 선다.
            pos: Vec3::new(pos.x, pos.y, 0.0),
            size: Vec2::new(width, height),
            anchor: SpriteAnchor::BottomCenter,
            depth_bias,
            uv: animator.uv(&self.sheet, direction),
            texture: self.texture,
            tint: look.tint,
        });
    }
}

/// 스프라이트를 어떻게 보이게 할지.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Look {
    /// 바라보는 방향 (라디안) — 시트의 방향 행을 고른다.
    pub(crate) heading: f32,
    /// 무채색 시트에 곱할 색 (sRGB).
    pub(crate) tint: [f32; 4],
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image() -> Image {
        Image::decode_png(SHEET_PNG).unwrap()
    }

    #[test]
    fn shipped_sheet_definition_matches_the_picture() {
        let (sheet, cell) = parse_sheet(SHEET_DEF, &image()).unwrap();
        assert_eq!(cell, (32, 48));
        assert_eq!(sheet.directions(), 4);
        assert!(sheet.clip(AnimState::Walk).is_some());
    }

    #[test]
    fn clips_outside_the_picture_are_rejected() {
        let too_many_frames = SHEET_DEF.replacen("frames: 4", "frames: 9", 1);
        assert!(parse_sheet(&too_many_frames, &image()).is_err());
        // Walk 는 4 행부터 4 방향 = 8 행. 8 방향으로 늘리면 그림(8 행)을 넘는다.
        let eight_way = SHEET_DEF.replacen("directions: 4", "directions: 8", 1);
        let err = parse_sheet(&eight_way, &image()).unwrap_err();
        assert!(err.contains("Idle") || err.contains("Walk"), "{err}");
    }

    #[test]
    fn cell_size_must_divide_the_picture() {
        let odd = SHEET_DEF.replacen("cell: (32, 48)", "cell: (30, 48)", 1);
        assert!(parse_sheet(&odd, &image()).is_err());
    }
}
