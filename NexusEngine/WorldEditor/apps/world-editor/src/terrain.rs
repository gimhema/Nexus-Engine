//! 지형 그림 — 타일 아트와 정적 오브젝트(건물·소품).
//!
//! # 규칙과 그림은 다른 층이다
//!
//! 걷기 가능 여부·높이 레벨은 **규칙**(`nexus_sim::TileMap`)이고 서버와 공유할 데이터다.
//! 여기서 다루는 것은 **그림**(`Scene::art`)이며 클라이언트만 본다. 두 층은 좌표만 공유한다 —
//! 라그나로크의 지형 파일과 걷기 맵(GAT)이 나뉘어 있는 것과 같은 이유다:
//!
//! - 같은 풀 그림 위에서 한 칸은 걷을 수 있고 옆 칸은 막혀 있을 수 있다
//! - 건물은 그림 한 장이지만 막히는 칸은 저작자가 따로 칠한다
//! - 그림을 바꿔도 이동·전투 판정이 달라지지 않는다 (서버와 어긋나지 않는다)
//!
//! # 두 가지 그림
//!
//! | 종류 | 어떻게 그리나 | 예 |
//! |---|---|---|
//! | [`ArtKind::Ground`] | **지면에 눕는** 텍스처 쿼드 (`DrawRect`), 칸 크기에 맞춘다 | 풀·물·바닥 |
//! | [`ArtKind::Prop`] | **카메라를 향해 서는** 빌보드 (`DrawSprite`), 칸 중심에 발을 딛는다 | 집·노점·나무 |
//!
//! 정적이라 애니메이션 재생기가 없다 — 그래서 시트 정의(`*.sheet.ron`)가 아니라 이 파일을 쓴다.

use std::collections::BTreeMap;
use std::path::Path;

use nexus_assets::Image;
use nexus_core::{Camera2d, Vec2, Vec3};
use nexus_render::{RenderCommand, Renderer, SpriteAnchor, TextureId, UvRect};
use nexus_sim::{TileCoord, TileMap};
use serde::Deserialize;

use crate::scene::ArtId;

/// 내장 지형 데이터 — 디스크에 `data/terrain.ron` 이 없어도 돈다.
const EMBEDDED: &str = include_str!("../../../data/terrain.ron");

/// 디스크에서 먼저 찾는 경로.
const DISK_PATH: &str = "data/terrain.ron";

/// 한 프레임에 그릴 지형 그림 수 상한 — 줌아웃에서 인스턴스 버퍼가 부풀지 않게.
/// `tiles.rs` 의 상한과 같은 이유다.
const MAX_PER_FRAME: usize = 8192;

// ─────────────────────────────────────────────────────────────────────────────
// 파일 형식
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TerrainFile {
    version: u32,
    /// 그림 파일 모음. 여러 그림이 한 타일셋을 나눠 쓴다 → 텍스처 하나로 배치가 이어진다.
    tilesets: BTreeMap<String, TilesetFile>,
    /// 지면에 눕는 그림.
    #[serde(default)]
    ground: BTreeMap<u16, ArtFile>,
    /// 서서 그려지는 정적 오브젝트.
    #[serde(default)]
    props: BTreeMap<u16, ArtFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TilesetFile {
    /// 그림 경로 (작업 디렉터리 기준).
    image: String,
    /// **그림 1미터가 몇 픽셀인가.** 오브젝트의 월드 크기가 여기서 나온다.
    /// 시트 정의(`*.sheet.ron`)의 같은 이름 필드와 값을 맞춰야 캐릭터와 건물의 축척이 맞는다.
    pixels_per_meter: f32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtFile {
    /// 도구 패널에 나오는 이름.
    name: String,
    /// 어느 타일셋의 어디를 쓰는가.
    tileset: String,
    /// 그림 안의 픽셀 사각형 `(x, y, 폭, 높이)`, 좌상단 원점.
    ///
    /// 칸 번호가 아니라 픽셀이다 — 받아온 타일셋의 오브젝트는 칸 경계에 딱 맞지 않는다.
    px: (u32, u32, u32, u32),
}

// ─────────────────────────────────────────────────────────────────────────────
// 검증을 마친 데이터 (GPU 업로드 전)
// ─────────────────────────────────────────────────────────────────────────────

/// 지면에 눕는 그림인가, 서는 오브젝트인가.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ArtKind {
    /// 기본값 — 붓을 고르기 전에는 지면을 칠한다.
    #[default]
    Ground,
    Prop,
}

/// 그림 하나 — 어느 타일셋의 어느 영역이고 월드에서 얼마나 큰가.
#[derive(Clone, Debug)]
struct Plan {
    name: String,
    kind: ArtKind,
    tileset: String,
    px: (u32, u32, u32, u32),
    /// 오브젝트의 월드 크기 (m). 지면 그림은 칸 크기에 맞추므로 쓰지 않는다.
    size: Vec2,
}

/// 파일을 읽고 그림에 맞는지 확인한 결과. GPU 를 모르므로 테스트가 여기까지 본다.
#[derive(Debug)]
struct TerrainPlan {
    /// 타일셋 이름 → (그림, 축척).
    images: BTreeMap<String, (Image, f32)>,
    entries: BTreeMap<ArtId, Plan>,
}

/// 지형 데이터를 읽는다. 디스크 파일이 있으면 그것이, 없으면 내장본이 쓰인다.
///
/// # Errors
/// 디스크 파일이 **있는데 잘못된 경우**는 오류다 — 고친 것이 적용 안 된 채 모르고 지나가지
/// 않도록 (`game_data` 와 같은 규칙). 그림 파일을 못 읽는 것도 오류다.
fn plan() -> Result<TerrainPlan, String> {
    let path = Path::new(DISK_PATH);
    let (text, label) = match std::fs::read_to_string(path) {
        Ok(text) => (text, DISK_PATH.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            (EMBEDDED.to_string(), "내장 지형 데이터".to_string())
        }
        Err(e) => return Err(format!("{DISK_PATH}: {e}")),
    };
    // 그림 경로는 작업 디렉터리 기준이다 (`data/…` 와 같은 규칙).
    parse(&text, Path::new("")).map_err(|e| format!("{label}: {e}"))
}

/// `base` 는 그림 경로의 기준 폴더다. 실행 중에는 작업 디렉터리(`""`)이고, 테스트는
/// 저장소 루트를 넘긴다 — **작업 디렉터리를 바꾸지 않는다** (테스트가 한 프로세스에서
/// 병렬로 돌기 때문에 전역 상태를 건드리면 서로 깨진다).
fn parse(text: &str, base: &Path) -> Result<TerrainPlan, String> {
    let file: TerrainFile = ron::from_str(text).map_err(|e| e.to_string())?;
    if file.version != 1 {
        return Err(format!("형식 {} 은(는) 읽을 수 없음", file.version));
    }

    let mut problems: Vec<String> = Vec::new();
    let mut images: BTreeMap<String, (Image, f32)> = BTreeMap::new();
    for (name, set) in &file.tilesets {
        if !(set.pixels_per_meter.is_finite() && set.pixels_per_meter > 0.0) {
            problems.push(format!(
                "타일셋 {name}: pixels_per_meter 는 양수여야 함 (지금 {})",
                set.pixels_per_meter
            ));
            continue;
        }
        match std::fs::read(base.join(&set.image)).map_err(|e| e.to_string()) {
            Ok(bytes) => match Image::decode_png(&bytes) {
                Ok(image) => {
                    images.insert(name.clone(), (image, set.pixels_per_meter));
                }
                Err(e) => problems.push(format!("타일셋 {name} ({}): {e}", set.image)),
            },
            Err(e) => problems.push(format!("타일셋 {name} ({}): {e}", set.image)),
        }
    }

    let mut entries: BTreeMap<ArtId, Plan> = BTreeMap::new();
    let tables = [
        (ArtKind::Ground, &file.ground),
        (ArtKind::Prop, &file.props),
    ];
    for (kind, table) in tables {
        for (&id, art) in table {
            if id == ArtId::NONE.raw() {
                problems.push(format!("{}: 번호 0 은 '그림 없음' 예약", art.name));
                continue;
            }
            if art.name.trim().is_empty() {
                problems.push(format!("번호 {id}: 이름이 비어 있음"));
            }
            if entries.contains_key(&ArtId::new(id)) {
                problems.push(format!(
                    "번호 {id} 가 두 번 나온다 (지면/오브젝트 공용 번호)"
                ));
                continue;
            }
            let Some((image, ppm)) = images.get(&art.tileset) else {
                // 타일셋 자체가 잘못됐으면 이미 보고했다 — 같은 오류를 두 번 내지 않는다.
                if file.tilesets.contains_key(&art.tileset) {
                    continue;
                }
                problems.push(format!("{}: 없는 타일셋 {}", art.name, art.tileset));
                continue;
            };
            let (x, y, w, h) = art.px;
            if w == 0 || h == 0 {
                problems.push(format!("{}: 크기가 0 인 영역", art.name));
                continue;
            }
            // 그림 밖을 가리키면 UV 가 옆 칸이나 가장자리를 비춘다 — 조용히 넘기지 않는다.
            if x + w > image.width() || y + h > image.height() {
                problems.push(format!(
                    "{}: 영역 ({x}, {y}, {w}, {h}) 가 그림 {}×{} 밖",
                    art.name,
                    image.width(),
                    image.height()
                ));
                continue;
            }
            entries.insert(
                ArtId::new(id),
                Plan {
                    name: art.name.clone(),
                    kind,
                    tileset: art.tileset.clone(),
                    px: art.px,
                    size: Vec2::new(w as f32 / ppm, h as f32 / ppm),
                },
            );
        }
    }

    if problems.is_empty() {
        Ok(TerrainPlan { images, entries })
    } else {
        Err(problems.join(" / "))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 올라간 데이터
// ─────────────────────────────────────────────────────────────────────────────

/// 그릴 준비가 된 그림 하나.
#[derive(Clone, Debug)]
pub(crate) struct Art {
    pub(crate) name: String,
    pub(crate) kind: ArtKind,
    texture: TextureId,
    uv: UvRect,
    /// 오브젝트의 월드 크기 (m).
    size: Vec2,
}

/// 지형 그림 모음 — 타일셋 텍스처와 그림 목록.
#[derive(Debug, Default)]
pub(crate) struct Terrain {
    entries: BTreeMap<ArtId, Art>,
}

impl Terrain {
    /// 지형 데이터를 읽어 타일셋을 GPU 에 올린다.
    ///
    /// # Errors
    /// 파일이나 그림이 잘못되면 오류 — 호출자가 알림으로 띄우고 **그림 없이** 계속한다
    /// (지형 그림이 없어도 편집·플레이는 돌아간다).
    pub(crate) fn load(renderer: &mut impl Renderer) -> Result<Self, String> {
        let plan = plan()?;
        let mut textures: BTreeMap<String, TextureId> = BTreeMap::new();
        for (name, (image, _)) in &plan.images {
            let id = renderer
                .load_texture(&image.desc("tileset"))
                .map_err(|e| format!("타일셋 {name}: {e}"))?;
            textures.insert(name.clone(), id);
        }

        let mut entries = BTreeMap::new();
        for (id, p) in plan.entries {
            let Some(&texture) = textures.get(&p.tileset) else {
                continue;
            };
            let (image, _) = &plan.images[&p.tileset];
            entries.insert(
                id,
                Art {
                    name: p.name,
                    kind: p.kind,
                    texture,
                    uv: uv_of(p.px, image.width(), image.height()),
                    size: p.size,
                },
            );
        }
        Ok(Self { entries })
    }

    pub(crate) fn get(&self, id: ArtId) -> Option<&Art> {
        self.entries.get(&id)
    }

    /// 도구 패널용 목록 — `(번호, 이름, 종류)`, 번호 오름차순.
    pub(crate) fn list(&self) -> Vec<(ArtId, &str, ArtKind)> {
        self.entries
            .iter()
            .map(|(&id, art)| (id, art.name.as_str(), art.kind))
            .collect()
    }
}

/// 픽셀 사각형 → 정규화 UV. 좌상단 원점은 양쪽이 같다.
fn uv_of((x, y, w, h): (u32, u32, u32, u32), width: u32, height: u32) -> UvRect {
    let (iw, ih) = (width.max(1) as f32, height.max(1) as f32);
    UvRect {
        min: Vec2::new(x as f32 / iw, y as f32 / ih),
        max: Vec2::new((x + w) as f32 / iw, (y + h) as f32 / ih),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 그리기
// ─────────────────────────────────────────────────────────────────────────────

/// 칸마다 칠해진 그림 — 씬과 플레이가 같은 형태로 들고 있다.
pub(crate) type ArtLayer = BTreeMap<TileCoord, ArtId>;

/// 지면 그림을 **지면 층**에 그린다. 보이는 범위만 훑는다.
pub(crate) fn build_ground(
    art: &ArtLayer,
    map: &TileMap,
    terrain: &Terrain,
    camera: &Camera2d,
    depth_bias: f32,
    out: &mut Vec<RenderCommand>,
) {
    let size = Vec2::splat(map.tile_size());
    for (at, entry) in visible(art, terrain, map, camera, ArtKind::Ground) {
        out.push(RenderCommand::DrawRect {
            center: map.tile_center(at),
            size,
            rotation: 0.0,
            z: 0.0,
            depth_bias,
            // 그림 그대로 — 색을 곱하지 않는다.
            color: [1.0; 4],
            uv: entry.uv,
            texture: entry.texture,
        });
    }
}

/// 정적 오브젝트를 **오브젝트 층**에 빌보드로 그린다.
pub(crate) fn build_props(
    art: &ArtLayer,
    map: &TileMap,
    terrain: &Terrain,
    camera: &Camera2d,
    depth_bias: f32,
    out: &mut Vec<RenderCommand>,
) {
    for (at, entry) in visible(art, terrain, map, camera, ArtKind::Prop) {
        let center = map.tile_center(at);
        out.push(RenderCommand::DrawSprite {
            // 칸 중심에 발을 딛고 위로 선다.
            pos: Vec3::new(center.x, center.y, 0.0),
            size: entry.size,
            anchor: SpriteAnchor::BottomCenter,
            depth_bias,
            uv: entry.uv,
            texture: entry.texture,
            tint: [1.0; 4],
        });
    }
}

/// 화면에 보이는 칸 중 `kind` 인 그림만.
///
/// 칠해진 칸만 담은 맵을 훑는다 — 대부분의 칸은 그림이 없으므로 화면 범위로 다시 걸러
/// 먼 곳까지 칠해도 비용이 화면에 비례한다.
fn visible<'a>(
    art: &'a ArtLayer,
    terrain: &'a Terrain,
    map: &'a TileMap,
    camera: &Camera2d,
    kind: ArtKind,
) -> Vec<(TileCoord, &'a Art)> {
    let (view_min, view_max) = camera.visible_bounds();
    let lo = map.world_to_tile(view_min);
    let hi = map.world_to_tile(view_max);
    // 오브젝트는 칸보다 클 수 있다 — 화면 밖 칸에 선 건물의 윗부분이 보일 수 있으므로
    // 범위를 넉넉히 잡는다.
    let pad = match kind {
        ArtKind::Ground => 0,
        ArtKind::Prop => PROP_VIEW_PAD,
    };

    art.iter()
        .filter(|(at, _)| {
            at.x >= lo.x - pad && at.x <= hi.x + pad && at.y >= lo.y - pad && at.y <= hi.y + pad
        })
        .filter_map(|(&at, &id)| {
            let entry = terrain.get(id)?;
            (entry.kind == kind).then_some((at, entry))
        })
        .take(MAX_PER_FRAME)
        .collect()
}

/// 오브젝트를 찾을 때 화면 밖으로 더 훑는 칸 수. 건물 높이(몇 m)를 덮을 만큼.
const PROP_VIEW_PAD: i32 = 8;

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_sim::Tile;

    fn camera() -> Camera2d {
        Camera2d {
            center: Vec2::ZERO,
            view_height: 40.0,
            viewport: (800, 600),
            pitch: Camera2d::PITCH_QUARTER,
        }
    }

    fn map() -> TileMap {
        TileMap::new(64, 64, 1.0, Vec2::splat(-32.0), Tile::default())
    }

    /// 저장소 루트 — 그림 경로의 기준. 테스트는 크레이트 폴더에서 돌아간다.
    fn root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// 파일 형식 한 줄을 바꿔 시험용 데이터를 만든다.
    fn tweaked(from: &str, to: &str) -> Result<TerrainPlan, String> {
        parse(&EMBEDDED.replacen(from, to, 1), &root())
    }

    #[test]
    fn shipped_terrain_data_is_valid() {
        // 저장소에 들어 있는 조합(데이터 + 그림)이 실제로 읽히는지.
        let plan = parse(EMBEDDED, &root()).expect("내장 지형 데이터가 유효해야 한다");
        assert!(!plan.entries.is_empty(), "그림이 하나도 없다");
        assert!(
            plan.entries.values().any(|p| p.kind == ArtKind::Ground),
            "지면 그림이 없다"
        );
        assert!(
            plan.entries.values().any(|p| p.kind == ArtKind::Prop),
            "정적 오브젝트가 없다"
        );
    }

    #[test]
    fn prop_size_comes_from_the_tileset_scale() {
        // 80×64px 그림을 16px/m 타일셋에서 쓰면 5×4m 다. 손으로 크기를 적지 않는다.
        let ppm = 16.0;
        let plan = Plan {
            name: "집".into(),
            kind: ArtKind::Prop,
            tileset: "t".into(),
            px: (0, 0, 80, 64),
            size: Vec2::new(80.0 / ppm, 64.0 / ppm),
        };
        assert_eq!(plan.size, Vec2::new(5.0, 4.0));
    }

    #[test]
    fn a_region_outside_the_picture_is_rejected() {
        let err = tweaked("px: (96, 0, 80, 64)", "px: (96, 0, 8000, 64)")
            .err()
            .unwrap_or_default();
        assert!(err.contains("밖"), "{err}");
    }

    #[test]
    fn an_unknown_tileset_is_rejected() {
        let err = tweaked("tileset: \"overworld\"", "tileset: \"없는것\"")
            .err()
            .unwrap_or_default();
        assert!(err.contains("없는 타일셋"), "{err}");
    }

    #[test]
    fn a_field_typo_is_rejected() {
        // 오타가 조용히 기본값으로 넘어가면 고친 것이 적용되지 않은 채 지나간다.
        let err = tweaked("pixels_per_meter:", "pixels_per_metre:")
            .err()
            .unwrap_or_default();
        assert!(!err.is_empty());
    }

    #[test]
    fn uv_maps_pixels_to_the_unit_square() {
        let uv = uv_of((16, 32, 16, 16), 64, 64);
        assert_eq!(uv.min, Vec2::new(0.25, 0.5));
        assert_eq!(uv.max, Vec2::new(0.5, 0.75));
    }

    #[test]
    fn unpainted_tiles_draw_nothing() {
        let mut out = Vec::new();
        build_ground(
            &ArtLayer::new(),
            &map(),
            &Terrain::default(),
            &camera(),
            0.0,
            &mut out,
        );
        assert!(out.is_empty());
    }

    #[test]
    fn ground_and_props_go_to_different_commands() {
        // 같은 층에 섞이면 건물이 지면에 눕는다.
        let terrain = Terrain {
            entries: BTreeMap::from([
                (
                    ArtId::new(1),
                    Art {
                        name: "풀".into(),
                        kind: ArtKind::Ground,
                        texture: TextureId::WHITE,
                        uv: UvRect::FULL,
                        size: Vec2::ONE,
                    },
                ),
                (
                    ArtId::new(2),
                    Art {
                        name: "집".into(),
                        kind: ArtKind::Prop,
                        texture: TextureId::WHITE,
                        uv: UvRect::FULL,
                        size: Vec2::new(5.0, 4.0),
                    },
                ),
            ]),
        };
        let layer = ArtLayer::from([
            (TileCoord::new(32, 32), ArtId::new(1)),
            (TileCoord::new(33, 32), ArtId::new(2)),
        ]);

        let mut ground = Vec::new();
        build_ground(&layer, &map(), &terrain, &camera(), 0.0, &mut ground);
        let mut props = Vec::new();
        build_props(&layer, &map(), &terrain, &camera(), 0.0, &mut props);

        assert!(matches!(
            ground.as_slice(),
            [RenderCommand::DrawRect { .. }]
        ));
        assert!(matches!(
            props.as_slice(),
            [RenderCommand::DrawSprite { size, .. }] if *size == Vec2::new(5.0, 4.0)
        ));
    }

    #[test]
    fn offscreen_art_is_skipped() {
        let terrain = Terrain {
            entries: BTreeMap::from([(
                ArtId::new(1),
                Art {
                    name: "풀".into(),
                    kind: ArtKind::Ground,
                    texture: TextureId::WHITE,
                    uv: UvRect::FULL,
                    size: Vec2::ONE,
                },
            )]),
        };
        let layer = ArtLayer::from([(TileCoord::new(0, 0), ArtId::new(1))]);
        let mut out = Vec::new();
        build_ground(&layer, &map(), &terrain, &camera(), 0.0, &mut out);
        assert!(
            out.is_empty(),
            "화면 밖 칸까지 그리면 비용이 맵 크기에 비례한다"
        );
    }
}
