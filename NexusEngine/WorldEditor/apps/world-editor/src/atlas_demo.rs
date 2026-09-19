//! 텍스처 파이프라인 확인용 표시 — `NEXUS_DEBUG_ATLAS`.
//!
//! S1 에는 아직 게임 스프라이트가 없으므로, 파이프라인이 실제로 동작하는지 볼 대상이 필요하다.
//! 이 모듈은 디버그 아틀라스를 띄워 **네 가지를 한 화면에서 확인**한다:
//!
//! 1. 텍스처가 올라가고 샘플링되는가 (아틀라스 전체 표시)
//! 2. UV 영역 계산이 맞는가 (칸을 하나씩 따로 표시 — 순서와 방향이 어긋나면 바로 보인다)
//! 3. 알파 컷아웃이 동작하는가 (칸 배경이 투명해 뒤의 판이 비쳐 보인다)
//! 4. 샘플러가 `Nearest` 인가 (0번 칸의 체커보드가 또렷해야 한다 — 뭉개지면 Linear 다)
//!
//! 게임 스프라이트가 들어오는 S3 에서 이 모듈은 사라진다.

use nexus_assets::{GridAtlas, Image};
use nexus_core::Vec2;
use nexus_render::{RenderCommand, RenderError, Renderer, TextureId, UvRect};

pub(crate) const ENV_DEBUG_ATLAS: &str = "NEXUS_DEBUG_ATLAS";

/// 아틀라스 칸 크기 (픽셀). 애셋과 맞아야 한다.
const CELL_PX: u32 = 32;

/// 칸 하나를 그릴 월드 크기 (m).
const CELL_M: f32 = 2.0;

/// 표시 영역의 Z. 마커보다 뒤, 그리드보다 앞.
const Z_DEMO: f32 = 0.2;
const Z_BACKDROP: f32 = 0.1;

/// 아틀라스 전체를 표시할 왼쪽 판의 중심.
const FULL_CENTER: Vec2 = Vec2::new(-9.0, 4.0);
const FULL_SIZE: f32 = 10.0;

/// 칸별 표시 격자의 좌상단 칸 중심.
const CELLS_ORIGIN: Vec2 = Vec2::new(2.0, 8.0);

/// 컷아웃이 동작하는지 보려고 칸 뒤에 까는 판의 색.
const BACKDROP_COLOR: [f32; 4] = [0.20, 0.22, 0.30, 1.0];

/// 로드된 디버그 아틀라스.
#[derive(Debug)]
pub(crate) struct AtlasDemo {
    texture: TextureId,
    atlas: GridAtlas,
}

impl AtlasDemo {
    /// `NEXUS_DEBUG_ATLAS` 가 설정돼 있으면 아틀라스를 올린다.
    ///
    /// 애셋은 `include_bytes!` 로 실행 파일에 넣는다 — 디버그용이라 작업 디렉터리에
    /// 의존하지 않는 편이 낫다. 실제 게임 애셋의 파일 로딩은 S5/S7 에서 다룬다.
    ///
    /// # Errors
    /// 디코딩이나 업로드에 실패하면 오류 문자열.
    pub(crate) fn load(renderer: &mut impl Renderer) -> Result<Option<Self>, String> {
        if std::env::var_os(ENV_DEBUG_ATLAS).is_none() {
            return Ok(None);
        }

        let bytes = include_bytes!("../../../assets/sprites/debug_atlas.png");
        let image = Image::decode_png(bytes).map_err(|e| e.to_string())?;
        let atlas = GridAtlas::new(&image, CELL_PX, CELL_PX).map_err(|e| e.to_string())?;
        let texture = renderer
            .load_texture(&image.desc("debug-atlas"))
            .map_err(|e: RenderError| e.to_string())?;

        println!(
            "{ENV_DEBUG_ATLAS}: {}x{} 아틀라스 로드 — {}x{} 칸 {}개",
            image.width(),
            image.height(),
            atlas.columns(),
            atlas.rows(),
            atlas.cell_count()
        );
        Ok(Some(Self { texture, atlas }))
    }

    /// 표시 영역을 감싸는 사각형. 카메라를 여기에 맞춘다.
    pub(crate) fn bounds(&self) -> (Vec2, Vec2) {
        let half = Vec2::splat(FULL_SIZE * 0.5);
        let cells_w = self.atlas.columns() as f32 * CELL_M;
        let cells_h = self.atlas.rows() as f32 * CELL_M;

        let min = (FULL_CENTER - half).min(CELLS_ORIGIN - Vec2::splat(CELL_M * 0.5));
        let max = (FULL_CENTER + half)
            .max(CELLS_ORIGIN + Vec2::new(cells_w, 0.0) - Vec2::new(CELL_M * 0.5, cells_h));
        (min.min(max), min.max(max))
    }

    /// 그리기 명령을 쌓는다.
    pub(crate) fn build(&self, out: &mut Vec<RenderCommand>) {
        // ① 아틀라스 전체 — 텍스처가 올라갔는지, 상하가 뒤집히지 않았는지.
        out.push(RenderCommand::DrawSprite {
            center: FULL_CENTER,
            size: Vec2::splat(FULL_SIZE),
            rotation: 0.0,
            z: Z_DEMO,
            uv: UvRect::FULL,
            texture: self.texture,
            tint: [1.0; 4],
        });

        // ② 칸별 표시. 뒤에 불투명한 판을 깔아 두면 컷아웃으로 뚫린 배경이 눈에 보인다.
        let cols = self.atlas.columns();
        let rows = self.atlas.rows();

        out.push(RenderCommand::DrawRect {
            center: CELLS_ORIGIN
                + Vec2::new(
                    (cols as f32 - 1.0) * CELL_M * 0.5,
                    -(rows as f32 - 1.0) * CELL_M * 0.5,
                ),
            size: Vec2::new(cols as f32 * CELL_M, rows as f32 * CELL_M),
            rotation: 0.0,
            z: Z_BACKDROP,
            color: BACKDROP_COLOR,
        });

        for row in 0..rows {
            for col in 0..cols {
                out.push(RenderCommand::DrawSprite {
                    // 화면 위쪽이 월드 +Y 이므로 행이 늘수록 아래로 간다.
                    center: CELLS_ORIGIN + Vec2::new(col as f32 * CELL_M, -(row as f32) * CELL_M),
                    size: Vec2::splat(CELL_M * 0.9),
                    rotation: 0.0,
                    z: Z_DEMO,
                    uv: self.atlas.uv(col, row),
                    texture: self.texture,
                    tint: [1.0; 4],
                });
            }
        }
    }
}
