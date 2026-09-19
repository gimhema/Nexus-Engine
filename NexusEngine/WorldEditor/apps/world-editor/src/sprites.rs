//! 마커 스프라이트 — 스폰 지점을 빌보드로 그린다.
//!
//! 게임 아트가 아직 없으므로 플레이스홀더 시트를 쓴다. 칸 하나가 종류 하나이고,
//! 애니메이션·방향은 **S4 에서** 이 위에 올린다. 그때 시트 메타데이터
//! (프레임 수·방향 수)가 들어오므로, 여기에 그 값을 상수로 박지 말 것.
//!
//! 애셋은 `include_bytes!` 로 실행 파일에 넣는다 — 에디터 기본 표시라 작업 디렉터리에
//! 의존하지 않아야 한다. 존별 애셋의 파일 로딩은 S5/S7 에서 다룬다.

use nexus_assets::{GridAtlas, Image};
use nexus_core::{Vec2, Vec3};
use nexus_render::{RenderCommand, Renderer, SpriteAnchor, TextureId, UvRect};

use crate::scene::{Item, ItemKind};

/// 시트 한 칸의 픽셀 크기. 애셋과 맞아야 한다.
const CELL_W: u32 = 32;
const CELL_H: u32 = 48;

/// 스프라이트의 월드 높이 (m).
///
/// 시트 칸 높이를 기준 배율로 나눈 값이다 — 고정 줌에서 **스프라이트 1픽셀이
/// 화면 1픽셀**이 된다. 픽셀아트가 뭉개지지 않으려면 이 관계를 지켜야 하므로,
/// 임의의 값(1.8 같은)을 넣지 말 것. 더 큰 캐릭터는 칸을 키운다.
const SPRITE_HEIGHT_M: f32 = CELL_H as f32 / nexus_core::Camera2d::PIXELS_PER_METER;

/// 화면에서 이보다 작게 그리지 않는다 (픽셀). 줌아웃해도 마커가 사라지지 않도록.
const SPRITE_MIN_PX: f32 = 20.0;

/// 로드된 마커 시트.
#[derive(Debug)]
pub(crate) struct MarkerSprites {
    texture: TextureId,
    atlas: GridAtlas,
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
        Ok(Self { texture, atlas })
    }

    /// 종류에 대응하는 칸.
    fn uv(&self, kind: ItemKind) -> UvRect {
        let col = match kind {
            ItemKind::PlayerSpawn => 0,
            ItemKind::Npc => 1,
            ItemKind::Monster => 2,
        };
        self.atlas.uv(col, 0)
    }

    /// 마커 하나를 세운다.
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
        let height = SPRITE_HEIGHT_M.max(SPRITE_MIN_PX * px);
        // 시트 칸 비율을 유지한다. 늘어나면 픽셀아트가 뭉개져 보인다.
        let width = height * CELL_W as f32 / CELL_H as f32;

        out.push(RenderCommand::DrawSprite {
            // 발밑이 스폰 지점이다 — 앵커가 BottomCenter 라 여기서 위로 선다.
            pos: Vec3::new(item.pos.x, item.pos.y, 0.0),
            size: Vec2::new(width, height),
            anchor: SpriteAnchor::BottomCenter,
            depth_bias,
            uv: self.uv(item.kind),
            texture: self.texture,
            tint: [1.0; 4],
        });
    }
}
