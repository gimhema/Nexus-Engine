//! 마커 스프라이트 — 스폰 지점을 애니메이션 빌보드로 그린다.
//!
//! 게임 아트가 아직 없으므로 플레이스홀더 시트를 쓴다. 시트는 **무채색**이고
//! 종류별 색은 `tint` 로 입힌다 — 텍스처 하나로 세 종류를 다 그리므로
//! 드로우 콜도 한 번이다.
//!
//! 애셋은 `include_bytes!` 로 실행 파일에 넣는다 — 에디터 기본 표시라 작업 디렉터리에
//! 의존하지 않아야 한다. 존별 애셋의 파일 로딩은 S5/S7 에서 다룬다.

use core::time::Duration;

use nexus_assets::{AnimState, Clip, GridAtlas, Image, SpriteAnimator, SpriteSheet};
use nexus_core::{Camera2d, Vec2, Vec3, units};
use nexus_render::{RenderCommand, Renderer, SpriteAnchor, TextureId};

use crate::scene::Item;

/// 시트 한 칸의 픽셀 크기. 애셋과 맞아야 한다.
const CELL_W: u32 = 32;
const CELL_H: u32 = 48;

/// 시트가 담고 있는 방향 수. **엔진이 아니라 이 시트의 성질이다** —
/// 8방향 시트로 갈아끼우면 이 값만 바뀌고 나머지 코드는 그대로다.
const SHEET_DIRECTIONS: u32 = 4;

/// 스프라이트의 월드 높이 (m).
///
/// 시트 칸 높이를 기준 배율로 나눈 값이다 — 고정 줌에서 **스프라이트 1픽셀이
/// 화면 1픽셀**이 된다. 픽셀아트가 뭉개지지 않으려면 이 관계를 지켜야 하므로,
/// 임의의 값(1.8 같은)을 넣지 말 것. 더 큰 캐릭터는 칸을 키운다.
const SPRITE_HEIGHT_M: f32 = CELL_H as f32 / Camera2d::PIXELS_PER_METER;

/// 화면에서 이보다 작게 그리지 않는다 (픽셀). 줌아웃해도 마커가 사라지지 않도록.
const SPRITE_MIN_PX: f32 = 20.0;

/// 로드된 마커 시트와 재생 상태.
///
/// 에디터 마커는 재생기 **하나를 공유해** 모두 같은 위상으로 움직인다 — 에디터에서는 그게 낫다.
/// 플레이 모드는 유닛마다 따로 재생기를 두고 [`MarkerSprites::push`] 로 그린다.
#[derive(Debug)]
pub(crate) struct MarkerSprites {
    texture: TextureId,
    sheet: SpriteSheet,
    animator: SpriteAnimator,
}

impl MarkerSprites {
    /// 시트를 GPU 에 올린다.
    ///
    /// # Errors
    /// 디코딩이나 업로드에 실패하면 오류 문자열.
    pub(crate) fn load(renderer: &mut impl Renderer) -> Result<Self, String> {
        let bytes = include_bytes!("../../../assets/sprites/markers.png");
        let image = Image::decode_png(bytes).map_err(|e| e.to_string())?;
        let atlas = GridAtlas::new(&image, CELL_W, CELL_H).map_err(|e| e.to_string())?;
        let texture = renderer
            .load_texture(&image.desc("markers"))
            .map_err(|e| e.to_string())?;

        // 시트 배치: 행 = 클립 시작 행 + 방향, 열 = 프레임.
        // 정의 파일에서 읽어 오는 것은 S5/S7 에서 — 지금은 코드가 곧 메타데이터다.
        let sheet = SpriteSheet::new(
            atlas,
            SHEET_DIRECTIONS,
            vec![
                (
                    AnimState::Idle,
                    Clip {
                        row: 0,
                        frames: 4,
                        frame_time: Duration::from_millis(250),
                        looping: true,
                    },
                ),
                (
                    AnimState::Walk,
                    Clip {
                        row: SHEET_DIRECTIONS,
                        frames: 4,
                        frame_time: Duration::from_millis(150),
                        looping: true,
                    },
                ),
            ],
        );

        Ok(Self {
            texture,
            sheet,
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
    pub(crate) fn height(px: f32) -> f32 {
        SPRITE_HEIGHT_M.max(SPRITE_MIN_PX * px)
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
        let height = Self::height(px);
        // 시트 칸 비율을 유지한다. 늘어나면 픽셀아트가 뭉개져 보인다.
        let width = height * CELL_W as f32 / CELL_H as f32;

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
