//! 타일맵 표시.
//!
//! **높이를 지오메트리로 그리지 않는다.** 지형은 화면에서 완전한 평지이고, 레벨은
//! 색으로만 드러낸다 — 저작 중에는 아트보다 데이터가 보여야 한다.
//! 지형 **그림**(타일 아트·건물)은 별개의 층이다 — `crate::terrain`. 이 모듈은 규칙만 그린다.
//!
//! 기본값(걸을 수 있는 레벨 0)은 **아무것도 그리지 않는다.** 대부분의 칸이 기본값이라
//! 그리면 화면만 어지럽고 드로우 콜만 늘어난다.

use nexus_core::{Camera2d, Vec2};
use nexus_render::{RenderCommand, TextureId, UvRect};
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
///
/// `covered` 를 주면 **그림이 칠해진 칸은 건너뛴다.** 플레이 모드에서 쓴다 — 완성된 지형
/// 위에 규칙 색이 덮이면 게임 화면이 아니라 저작 화면이 된다. 에디터에서는 `None` 을 넘겨
/// 그림 위에도 규칙이 보이게 한다 (걷기 막힘을 눈으로 확인해야 하므로).
pub(crate) fn build(
    map: &TileMap,
    camera: &Camera2d,
    depth_bias: f32,
    covered: Option<&crate::terrain::ArtLayer>,
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
    // 기본 타일은 그리지 않으므로 칠한 칸만 훑는다 — 칠하지 않은 청크는 건너뛴다 (R6-3).
    // 맵의 바탕(fill)이 그릴 타일이면 예전처럼 보이는 칸을 전부 본다.
    let cells: Box<dyn Iterator<Item = (TileCoord, nexus_sim::Tile)>> =
        if tile_color(map.fill()).is_none() {
            Box::new(map.painted_in(TileCoord::new(x0, y0), TileCoord::new(x1, y1)))
        } else {
            Box::new((y0..=y1).flat_map(move |y| {
                (x0..=x1).filter_map(move |x| {
                    let at = TileCoord::new(x, y);
                    Some((at, map.get(at)?))
                })
            }))
        };
    for (at, tile) in cells {
        let Some(color) = tile_color(tile) else {
            continue;
        };
        if covered.is_some_and(|art| art.contains_key(&at)) {
            continue;
        }
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
            uv: UvRect::FULL,
            texture: TextureId::WHITE,
        });
    }
}

/// 횡스크롤 — 흙 블록 (두 톤을 번갈아 결이 보이게).
const SIDE_DIRT: [[f32; 4]; 2] = [[0.42, 0.29, 0.20, 1.0], [0.38, 0.26, 0.18, 1.0]];
/// 횡스크롤 — 위가 비어 있는 블록의 윗면 (풀).
const SIDE_GRASS: [f32; 4] = [0.36, 0.62, 0.30, 1.0];
const SIDE_GRASS_EDGE: [f32; 4] = [0.24, 0.46, 0.22, 1.0];
/// 횡스크롤 — 발판 (나무판).
const SIDE_PLANK: [f32; 4] = [0.72, 0.52, 0.30, 1.0];
const SIDE_PLANK_EDGE: [f32; 4] = [0.48, 0.32, 0.18, 1.0];
/// 횡스크롤 — 사다리.
const SIDE_LADDER: [f32; 4] = [0.80, 0.62, 0.36, 1.0];
/// 횡스크롤 하늘 — 화면 위·아래 끝 색. 그 사이를 [`SKY_BANDS`] 띠로 나눠 잇는다.
const SIDE_SKY: [[f32; 4]; 2] = [[0.45, 0.70, 0.92, 1.0], [0.80, 0.90, 0.97, 1.0]];
const SKY_BANDS: usize = 12;

/// 횡스크롤 하늘 — 보이는 범위 전체를 덮는 바탕. 지면 층의 맨 뒤(`depth_bias`)에 깐다.
/// 화면에 붙어 있다(카메라를 따라 움직인다) — 먼 하늘은 움직이지 않아 보여야 한다.
pub(crate) fn build_side_sky(camera: &Camera2d, depth_bias: f32, out: &mut Vec<RenderCommand>) {
    let (min, max) = camera.visible_bounds();
    let band = (max.y - min.y) / SKY_BANDS as f32;
    for i in 0..SKY_BANDS {
        let t = i as f32 / (SKY_BANDS - 1) as f32;
        let mut color = [0.0; 4];
        for (c, (top, bottom)) in color.iter_mut().zip(SIDE_SKY[0].iter().zip(SIDE_SKY[1])) {
            *c = top + (bottom - top) * t;
        }
        out.push(RenderCommand::DrawRect {
            center: Vec2::new((min.x + max.x) * 0.5, max.y - band * (i as f32 + 0.5)),
            // 띠 사이가 픽셀 경계에서 벌어지지 않게 조금씩 겹친다.
            size: Vec2::new(max.x - min.x + 2.0, band * 1.05),
            rotation: 0.0,
            z: 0.0,
            depth_bias,
            color,
            uv: UvRect::FULL,
            texture: TextureId::WHITE,
        });
    }
}

/// 횡스크롤 존의 타일 — 지형 그림 없이도 단면이 읽히게 블록·발판·사다리를 도형으로 그린다
/// (규칙은 `nexus_sim` 의 `platformer` 모듈 doc). 걸을 수 있는 기본 칸(빈 공간)은 그리지 않는다.
pub(crate) fn build_side(
    map: &TileMap,
    camera: &Camera2d,
    depth_bias: f32,
    out: &mut Vec<RenderCommand>,
) {
    let (view_min, view_max) = camera.visible_bounds();
    let lo = map.world_to_tile(view_min);
    let hi = map.world_to_tile(view_max);
    let x0 = lo.x.max(0);
    let y0 = lo.y.max(0);
    let x1 = hi.x.min(map.width() as i32 - 1);
    let y1 = hi.y.min(map.height() as i32 - 1);
    if x1 < x0 || y1 < y0 {
        return;
    }
    let s = map.tile_size();
    let above_solid = |at: TileCoord| {
        map.get(TileCoord::new(at.x, at.y + 1))
            .is_none_or(|t| !t.walkable)
    };
    let is_ladder = |at: TileCoord| map.get(at).is_some_and(|t| t.walkable && t.ramp);
    // 겹치는 장식은 바탕보다 살짝 앞 — 같은 층 안의 순서만 준다.
    let front = depth_bias + nexus_render::DEPTH_LAYER * 0.1;
    let mut rect = |center: Vec2, size: Vec2, color: [f32; 4], bias: f32| {
        out.push(RenderCommand::DrawRect {
            center,
            size,
            rotation: 0.0,
            z: 0.0,
            depth_bias: bias,
            color,
            uv: UvRect::FULL,
            texture: TextureId::WHITE,
        });
    };
    let cells: Box<dyn Iterator<Item = (TileCoord, nexus_sim::Tile)>> = if map.fill().walkable {
        Box::new(map.painted_in(TileCoord::new(x0, y0), TileCoord::new(x1, y1)))
    } else {
        Box::new((y0..=y1).flat_map(move |y| {
            (x0..=x1).filter_map(move |x| {
                let at = TileCoord::new(x, y);
                Some((at, map.get(at)?))
            })
        }))
    };
    let mut drawn = 0usize;
    for (at, tile) in cells {
        if drawn >= MAX_TILES_PER_FRAME {
            return;
        }
        let c = map.tile_center(at);
        if !tile.walkable {
            drawn += 1;
            let dirt = SIDE_DIRT[((at.x + at.y) & 1) as usize];
            rect(c, Vec2::splat(s), dirt, depth_bias);
            if !above_solid(at) {
                rect(
                    Vec2::new(c.x, c.y + s * 0.4),
                    Vec2::new(s, s * 0.2),
                    SIDE_GRASS,
                    front,
                );
                rect(
                    Vec2::new(c.x, c.y + s * 0.28),
                    Vec2::new(s, s * 0.05),
                    SIDE_GRASS_EDGE,
                    front,
                );
            }
        } else if tile.ramp {
            drawn += 1;
            for side in [-0.32, 0.32] {
                rect(
                    Vec2::new(c.x + s * side, c.y),
                    Vec2::new(s * 0.08, s),
                    SIDE_LADDER,
                    depth_bias,
                );
            }
            for k in 0..3 {
                let y = c.y - s * 0.5 + s * (k as f32 + 0.5) / 3.0;
                rect(
                    Vec2::new(c.x, y),
                    Vec2::new(s * 0.64, s * 0.06),
                    SIDE_LADDER,
                    depth_bias,
                );
            }
            // 맨 윗칸은 발판처럼 밟힌다 — 판을 그려 알린다.
            if !is_ladder(TileCoord::new(at.x, at.y + 1)) {
                rect(
                    Vec2::new(c.x, c.y + s * 0.42),
                    Vec2::new(s, s * 0.16),
                    SIDE_PLANK,
                    front,
                );
            }
        } else if tile.level >= 1 {
            drawn += 1;
            rect(
                Vec2::new(c.x, c.y + s * 0.38),
                Vec2::new(s, s * 0.24),
                SIDE_PLANK,
                depth_bias,
            );
            rect(
                Vec2::new(c.x, c.y + s * 0.25),
                Vec2::new(s, s * 0.04),
                SIDE_PLANK_EDGE,
                front,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_sim::Tile;

    #[test]
    fn side_view_draws_blocks_platforms_and_ladders_but_not_air() {
        let mut map = TileMap::new(16, 16, 1.0, Vec2::ZERO, Tile::default());
        let cam = Camera2d {
            center: Vec2::splat(8.0),
            view_height: 20.0,
            viewport: (800, 800),
            pitch: Camera2d::PITCH_TOPDOWN,
            yaw: 0.0,
        };
        let mut out = Vec::new();
        build_side(&map, &cam, 0.0, &mut out);
        assert!(out.is_empty(), "빈 공간은 그리지 않는다");

        let block = Tile {
            walkable: false,
            ..Tile::default()
        };
        map.set(TileCoord::new(2, 2), block);
        map.set(TileCoord::new(2, 3), block);
        build_side(&map, &cam, 0.0, &mut out);
        // 아래 블록은 흙만, 위 블록은 흙 + 풀 두 줄.
        assert_eq!(out.len(), 4);

        out.clear();
        map.set(
            TileCoord::new(5, 5),
            Tile {
                level: 1,
                ..Tile::default()
            },
        );
        map.set(
            TileCoord::new(7, 5),
            Tile {
                ramp: true,
                ..Tile::default()
            },
        );
        build_side(&map, &cam, 0.0, &mut out);
        // 블록 4 + 발판 2 + 사다리(기둥 2 + 가로대 3 + 맨 윗칸 판 1).
        assert_eq!(out.len(), 4 + 2 + 6);
    }

    fn camera(view_height: f32, center: Vec2) -> Camera2d {
        Camera2d {
            center,
            view_height,
            viewport: (800, 600),
            pitch: Camera2d::PITCH_QUARTER,
            yaw: 0.0,
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
        build(&map, &camera(20.0, Vec2::ZERO), 0.0, None, &mut out);
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
            build(&map, &camera(20.0, Vec2::ZERO), 0.0, None, &mut out);
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
        build(
            &map,
            &camera(10.0, Vec2::new(25.0, 25.0)),
            0.0,
            None,
            &mut out,
        );
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
        build(&map, &camera(1000.0, Vec2::ZERO), 0.0, None, &mut out);
        assert!(out.len() <= MAX_TILES_PER_FRAME, "{} 개", out.len());
    }
}
