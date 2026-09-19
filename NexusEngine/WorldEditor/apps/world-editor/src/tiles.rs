//! 타일맵 표시.
//!
//! **높이를 지오메트리로 그리지 않는다.** 지형은 화면에서 완전한 평지이고, 레벨은
//! 색으로만 드러낸다 — 저작 중에는 아트보다 데이터가 보여야 한다.
//! 진짜 타일 아트(절벽 경계 포함)는 텍스처 지면 쿼드가 생길 때 올라온다.
//!
//! 기본값(걸을 수 있는 레벨 0)은 **아무것도 그리지 않는다.** 대부분의 칸이 기본값이라
//! 그리면 화면만 어지럽고 드로우 콜만 늘어난다.

use nexus_core::{Camera2d, Vec2};
use nexus_render::RenderCommand;
use nexus_sim::{TileCoord, TileMap};

/// 지나갈 수 없는 칸.
const BLOCKED: [f32; 4] = [0.55, 0.16, 0.18, 0.75];
/// 경사로 — 레벨을 잇는 칸이라 눈에 띄어야 한다.
const RAMP: [f32; 4] = [0.95, 0.75, 0.25, 0.55];
/// 레벨 1 이상. 레벨이 올라갈수록 밝아진다.
const HIGH: [f32; 4] = [0.35, 0.55, 0.85, 0.22];

/// 한 프레임에 그릴 타일 수 상한.
///
/// 줌아웃하면 보이는 칸이 수만 개가 된다. 그 배율에서는 한 칸이 1픽셀도 안 되므로
/// 그려 봐야 의미가 없고, 인스턴스 버퍼만 부풀린다.
const MAX_TILES_PER_FRAME: usize = 8192;

/// 타일의 표시 색. 기본값이면 `None` — 그리지 않는다.
fn tile_color(tile: nexus_sim::Tile) -> Option<[f32; 4]> {
    if !tile.walkable {
        return Some(BLOCKED);
    }
    if tile.ramp {
        return Some(RAMP);
    }
    if tile.level > 0 {
        let mut c = HIGH;
        // 레벨마다 한 단계씩 진하게. 9레벨에서 포화한다.
        c[3] = (HIGH[3] + 0.07 * f32::from(tile.level)).min(0.85);
        return Some(c);
    }
    None
}

/// 보이는 범위의 타일을 [`RenderCommand`] 로 만들어 `out` 에 넣는다.
///
/// **지면 층에서 호출한다** — 스프라이트를 가리면 안 된다.
pub(crate) fn build(
    map: &TileMap,
    camera: &Camera2d,
    depth_bias: f32,
    out: &mut Vec<RenderCommand>,
) {
    let (view_min, view_max) = camera.visible_bounds();
    let lo = map.world_to_tile(view_min);
    let hi = map.world_to_tile(view_max);

    // 화면 밖은 아예 훑지 않는다 — 맵이 커져도 비용이 화면 크기에 비례한다.
    let x0 = lo.x.max(0);
    let y0 = lo.y.max(0);
    let x1 = hi.x.min(map.width() as i32 - 1);
    let y1 = hi.y.min(map.height() as i32 - 1);
    if x1 < x0 || y1 < y0 {
        return;
    }

    let size = Vec2::splat(map.tile_size());
    let mut drawn = 0usize;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let at = TileCoord::new(x, y);
            let Some(color) = map.get(at).and_then(tile_color) else {
                continue;
            };
            if drawn >= MAX_TILES_PER_FRAME {
                return;
            }
            drawn += 1;
            out.push(RenderCommand::DrawRect {
                center: map.tile_center(at),
                size,
                rotation: 0.0,
                z: 0.0,
                depth_bias,
                color,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_sim::Tile;

    fn camera(view_height: f32, center: Vec2) -> Camera2d {
        Camera2d {
            center,
            view_height,
            viewport: (800, 600),
            pitch: Camera2d::PITCH_QUARTER,
        }
    }

    fn map_with(at: TileCoord, tile: Tile) -> TileMap {
        let mut map = TileMap::new(64, 64, 1.0, Vec2::splat(-32.0), Tile::default());
        map.set(at, tile);
        map
    }

    #[test]
    fn default_tiles_draw_nothing() {
        let map = TileMap::new(64, 64, 1.0, Vec2::splat(-32.0), Tile::default());
        let mut out = Vec::new();
        build(&map, &camera(20.0, Vec2::ZERO), 0.0, &mut out);
        assert!(out.is_empty(), "기본 칸까지 그리면 화면이 어지럽다");
    }

    #[test]
    fn notable_tiles_draw() {
        for tile in [
            Tile {
                walkable: false,
                ..Tile::default()
            },
            Tile {
                ramp: true,
                ..Tile::default()
            },
            Tile {
                level: 2,
                ..Tile::default()
            },
        ] {
            let map = map_with(TileCoord::new(32, 32), tile);
            let mut out = Vec::new();
            build(&map, &camera(20.0, Vec2::ZERO), 0.0, &mut out);
            assert_eq!(out.len(), 1, "{tile:?}");
        }
    }

    #[test]
    fn higher_levels_are_more_opaque() {
        let a = tile_color(Tile {
            level: 1,
            ..Tile::default()
        })
        .unwrap();
        let b = tile_color(Tile {
            level: 5,
            ..Tile::default()
        })
        .unwrap();
        assert!(b[3] > a[3], "레벨 차이가 보이지 않는다");
    }

    #[test]
    fn blocked_beats_ramp_and_level() {
        // 못 걷는 칸은 다른 무엇보다 먼저 드러나야 한다.
        let c = tile_color(Tile {
            walkable: false,
            ramp: true,
            level: 3,
        })
        .unwrap();
        assert_eq!(c, BLOCKED);
    }

    #[test]
    fn offscreen_tiles_are_not_scanned() {
        // 화면에서 멀리 떨어진 칸은 나오지 않아야 한다.
        let map = map_with(
            TileCoord::new(0, 0),
            Tile {
                walkable: false,
                ..Tile::default()
            },
        );
        let mut out = Vec::new();
        build(&map, &camera(10.0, Vec2::new(25.0, 25.0)), 0.0, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn zoomed_out_view_is_capped() {
        // 줌아웃하면 보이는 칸이 수만 개가 된다 — 인스턴스 버퍼가 부풀지 않아야 한다.
        let mut map = TileMap::new(400, 400, 1.0, Vec2::splat(-200.0), Tile::default());
        for y in 0..400 {
            for x in 0..400 {
                map.set(
                    TileCoord::new(x, y),
                    Tile {
                        walkable: false,
                        ..Tile::default()
                    },
                );
            }
        }
        let mut out = Vec::new();
        build(&map, &camera(1000.0, Vec2::ZERO), 0.0, &mut out);
        assert!(out.len() <= MAX_TILES_PER_FRAME, "{} 개", out.len());
    }
}
