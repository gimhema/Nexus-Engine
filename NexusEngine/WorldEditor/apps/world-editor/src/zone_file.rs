//! 존 파일 — 편집 중인 씬의 저장/로드 (S7-2).
//!
//! 형식은 **RON** (`*.zone.ron`). 에디터가 쓰는 저작 파일이라 사람이 읽고 고칠 수 있고 주석을 달 수 있다.
//! 서버 `ZoneConfig` 로 내보내는 것은 이 파일과 별개다 (단계 2).
//!
//! # 파일 구조는 내부 구조와 따로 둔다
//!
//! 여기의 `*File` 타입만 직렬화한다. `Scene`·`Item`·`TileMap` 에 serde 를 붙이지 않는 이유:
//!
//! - 내부 구조에는 **저장하면 안 되는 것**이 있다 — `Entity` 핸들은 실행마다 다르다.
//! - 내부 구조를 고칠 때마다 파일 형식이 따라 바뀌면 옛 파일을 못 읽는다. 둘을 떼어 두면
//!   형식은 `version` 과 이 모듈에서만 관리된다.
//! - `nexus-core` / `nexus-sim` 이 직렬화 의존성을 갖지 않는다 (I/O 없는 크레이트 유지).
//!
//! # 타일은 기본값이 아닌 칸만
//!
//! 대부분의 칸이 기본값(걸을 수 있는 레벨 0)이라 **바뀐 칸만** 적는다. 나중에 실행 중 저장소를
//! 청크로 나누더라도 이 형식은 그대로 쓸 수 있다.
//!
//! # 크로스플랫폼
//!
//! 줄바꿈은 **항상 `\n`** 이다. `ron` 의 기본값은 Windows 에서 `\r\n` 이라 그대로 두면 같은 존을
//! OS 마다 다른 파일로 저장해 git 에서 매번 바뀐 것으로 보인다. 경로는 `Path` 로만 다룬다.

use std::path::{Path, PathBuf};

use nexus_core::{Vec2, World, units};
use nexus_sim::{Tile, TileCoord, TileMap};
use serde::{Deserialize, Serialize};

use crate::scene::{ItemKind, MIN_ZONE_SIZE, Scene, ZoneBounds};

/// 지금 쓰는 형식 번호. 형식이 바뀌면 올리고, 옛 번호를 읽는 길을 남긴다.
pub(crate) const FORMAT_VERSION: u32 = 1;
/// 존 파일을 두는 기본 폴더 (작업 디렉터리 기준). 파일 이름은 전부 소문자로.
pub(crate) const ZONE_DIR: &str = "zones";
/// 확장자.
pub(crate) const EXTENSION: &str = ".zone.ron";
/// 타일맵 한 변의 상한. 잘못된 파일이 수십 GB 를 할당하지 않도록.
const MAX_TILEMAP_SIDE: u32 = 4096;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct ZoneFile {
    version: u32,
    /// 존 경계 AABB (m, XY). 서버 `ZoneConfig::boundsMin/Max` 의 XY.
    bounds: BoundsFile,
    tiles: TilesFile,
    /// 칸마다 칠한 **지형 그림** 번호 (`data/terrain.ron`). 타일(규칙)과는 별개의 층이다.
    ///
    /// 그림을 쓰지 않는 존도 있으므로 생략 가능하게 둔다 — 이 필드가 없는 예전 파일도 읽힌다.
    #[serde(default)]
    art: Vec<ArtRunFile>,
    markers: Vec<MarkerFile>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct BoundsFile {
    min: [f32; 2],
    max: [f32; 2],
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct TilesFile {
    /// 타일 (0, 0) 의 왼쪽 아래 모서리 (m).
    origin: [f32; 2],
    tile_size: f32,
    width: u32,
    height: u32,
    /// 기본값(걸을 수 있음·레벨 0·경사로 아님)이 **아닌** 칸만, 가로로 이어진 같은 칸은 한 줄로.
    runs: Vec<RunFile>,
}

/// 한 행에서 `x` 부터 `len` 칸이 같은 타일이다.
///
/// 칸마다 적으면 12×12 고지대 하나가 144 항목이 된다. 저작한 지형은 대부분 직사각형이라
/// 행 단위로 묶으면 파일이 사람이 읽을 만한 크기로 줄고, diff 도 바뀐 행만 보인다.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct RunFile {
    y: u32,
    x: u32,
    len: u32,
    walkable: bool,
    level: u8,
    ramp: bool,
}

/// 한 행에서 `x` 부터 `len` 칸이 같은 그림이다. 타일 행 묶음과 같은 이유로 묶는다.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct ArtRunFile {
    y: u32,
    x: u32,
    len: u32,
    /// `data/terrain.ron` 의 그림 번호. 0(그림 없음)은 저장하지 않는다.
    id: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
enum MarkerKindFile {
    Player,
    Npc,
    Monster,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct MarkerFile {
    name: String,
    kind: MarkerKindFile,
    /// 위치 (m, XY).
    pos: [f32; 2],
    /// 점유 크기, 한 변 (m).
    size: f32,
    /// 방향 (라디안, 0 = +X, 반시계 +).
    orientation: f32,
}

// ─────────────────────────────────────────────────────────────────────────────
// 씬 ↔ 파일
// ─────────────────────────────────────────────────────────────────────────────

/// 씬을 RON 문자열로.
pub(crate) fn to_ron(scene: &Scene) -> String {
    let file = ZoneFile::from_scene(scene);
    // 좌표(`[f32; 2]`)는 RON 튜플로 한 줄에 나온다. 목록은 항목마다 줄을 나눠 diff 가 읽히게 둔다.
    let config = ron::ser::PrettyConfig::new()
        .new_line("\n")
        .indentor("    ");
    // 직렬화 대상이 전부 단순 값이라 실패할 수 없다. 실패하면 이 모듈의 버그다.
    let body = ron::ser::to_string_pretty(&file, config).expect("존 직렬화 실패");
    format!(
        "// Nexus WorldEditor 존 파일 — 형식 {FORMAT_VERSION}. 단위: 미터, 방향: 라디안(0 = +X, 반시계 +).\n{body}\n"
    )
}

/// RON 문자열에서 씬을 만든다. 형식·값이 잘못됐으면 무엇이 틀렸는지 한국어로 알려 준다.
pub(crate) fn from_ron(text: &str) -> Result<Scene, String> {
    let file: ZoneFile =
        ron::from_str(text).map_err(|e| format!("존 파일을 읽을 수 없음 — {e}"))?;
    file.into_scene()
}

/// 파일로 저장한다. 폴더가 없으면 만든다.
///
/// 임시 파일에 쓴 뒤 이름을 바꾼다 — 쓰는 도중 멈춰도 원래 파일이 반쯤 덮이지 않는다.
/// (`rename` 은 Windows 에서도 기존 파일을 대체한다.)
pub(crate) fn save(scene: &Scene, path: &Path) -> Result<(), String> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("폴더를 만들 수 없음 ({}): {e}", dir.display()))?;
    }
    let tmp = path.with_extension("ron.tmp");
    std::fs::write(&tmp, to_ron(scene))
        .map_err(|e| format!("쓸 수 없음 ({}): {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("저장 실패 ({}): {e}", path.display()))
}

/// 파일에서 읽는다.
pub(crate) fn load(path: &Path) -> Result<Scene, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("열 수 없음 ({}): {e}", path.display()))?;
    from_ron(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// 폴더 안의 존 파일 (이름순). 폴더가 없으면 빈 목록.
pub(crate) fn list(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(EXTENSION))
        })
        .collect();
    files.sort();
    files
}

/// 저장 경로 기본값. `name` 은 소문자로 바꾸고 확장자를 붙인다 — ext4 는 대소문자를 가린다.
pub(crate) fn default_path(name: &str) -> PathBuf {
    Path::new(ZONE_DIR).join(file_name(name))
}

/// 저장 창에 입력한 문자열을 저장 경로로. 폴더가 없으면 [`ZONE_DIR`], 파일 이름은 소문자 +
/// 확장자. Windows 에서 `Town.zone.ron` 과 `town.zone.ron` 은 같은 파일이지만 리눅스에서는
/// 다른 파일이라, 이름을 소문자로 고정해 두 OS 에서 같은 존을 가리키게 한다.
pub(crate) fn save_path_from_input(input: &str) -> PathBuf {
    let path = Path::new(input.trim());
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("untitled");
    match path.parent().filter(|d| !d.as_os_str().is_empty()) {
        Some(dir) => dir.join(file_name(name)),
        None => default_path(name),
    }
}

fn file_name(name: &str) -> String {
    let stem = name.trim().to_lowercase();
    let stem = stem.strip_suffix(EXTENSION).unwrap_or(&stem);
    format!("{stem}{EXTENSION}")
}

impl ZoneFile {
    fn from_scene(scene: &Scene) -> Self {
        let tiles = &scene.tiles;
        let mut runs: Vec<RunFile> = Vec::new();
        for y in 0..tiles.height() {
            let mut x = 0;
            while x < tiles.width() {
                let tile = tiles
                    .get(TileCoord::new(x as i32, y as i32))
                    .unwrap_or_default();
                let mut len = 1;
                while x + len < tiles.width()
                    && tiles.get(TileCoord::new((x + len) as i32, y as i32)) == Some(tile)
                {
                    len += 1;
                }
                if tile != Tile::default() {
                    runs.push(RunFile {
                        y,
                        x,
                        len,
                        walkable: tile.walkable,
                        level: tile.level,
                        ramp: tile.ramp,
                    });
                }
                x += len;
            }
        }

        // 그림도 행 단위로 묶는다. 칠한 칸만 들고 있으므로 행·열 순서로 정렬해서 훑는다
        // (`TileCoord` 의 정렬은 x 가 먼저라 그대로 쓰면 행이 이어지지 않는다).
        let mut painted: Vec<(u32, u32, u16)> = scene
            .art
            .iter()
            .filter(|(_, id)| !id.is_none())
            .filter_map(|(at, id)| {
                let (x, y) = (u32::try_from(at.x).ok()?, u32::try_from(at.y).ok()?);
                (x < tiles.width() && y < tiles.height()).then_some((y, x, id.raw()))
            })
            .collect();
        painted.sort_unstable();

        let mut art: Vec<ArtRunFile> = Vec::new();
        for &(y, x, id) in &painted {
            match art.last_mut() {
                // 같은 행에서 바로 옆 칸이고 같은 그림이면 이어 붙인다.
                Some(run) if run.y == y && run.id == id && run.x + run.len == x => run.len += 1,
                _ => art.push(ArtRunFile { y, x, len: 1, id }),
            }
        }

        Self {
            version: FORMAT_VERSION,
            bounds: BoundsFile {
                min: scene.zone.min.to_array(),
                max: scene.zone.max.to_array(),
            },
            tiles: TilesFile {
                origin: tiles.origin().to_array(),
                tile_size: tiles.tile_size(),
                width: tiles.width(),
                height: tiles.height(),
                runs,
            },
            art,
            markers: scene
                .items
                .iter()
                .map(|item| MarkerFile {
                    name: item.name.clone(),
                    kind: match item.kind {
                        ItemKind::PlayerSpawn => MarkerKindFile::Player,
                        ItemKind::Npc => MarkerKindFile::Npc,
                        ItemKind::Monster => MarkerKindFile::Monster,
                    },
                    pos: item.pos.to_array(),
                    size: item.size,
                    orientation: item.orientation,
                })
                .collect(),
        }
    }

    fn into_scene(self) -> Result<Scene, String> {
        if self.version != FORMAT_VERSION {
            return Err(format!(
                "형식 {} 은(는) 읽을 수 없음 (이 에디터는 {FORMAT_VERSION})",
                self.version
            ));
        }

        let min = finite2(self.bounds.min, "bounds.min")?;
        let max = finite2(self.bounds.max, "bounds.max")?;
        let size = max - min;
        if size.x < MIN_ZONE_SIZE || size.y < MIN_ZONE_SIZE {
            return Err(format!(
                "존 경계가 뒤집혔거나 너무 작음 (한 변 최소 {MIN_ZONE_SIZE} m)"
            ));
        }

        let t = &self.tiles;
        if !(1..=MAX_TILEMAP_SIDE).contains(&t.width) || !(1..=MAX_TILEMAP_SIDE).contains(&t.height)
        {
            return Err(format!(
                "타일맵 크기 {}×{} 는 허용 범위(1~{MAX_TILEMAP_SIDE}) 밖",
                t.width, t.height
            ));
        }
        if !(t.tile_size.is_finite() && t.tile_size > 0.0) {
            return Err(String::from("tile_size 는 0 보다 큰 수여야 함"));
        }
        let origin = finite2(t.origin, "tiles.origin")?;
        let mut tiles = TileMap::new(t.width, t.height, t.tile_size, origin, Tile::default());
        for r in &t.runs {
            let end = r.x.checked_add(r.len);
            if r.len == 0 || r.y >= t.height || end.is_none_or(|e| e > t.width) {
                return Err(format!(
                    "타일 행 (y {}, x {}, {}칸) 이(가) 타일맵 밖이거나 비었음",
                    r.y, r.x, r.len
                ));
            }
            let tile = Tile {
                walkable: r.walkable,
                level: r.level,
                ramp: r.ramp,
            };
            for x in r.x..r.x + r.len {
                tiles.set(TileCoord::new(x as i32, r.y as i32), tile);
            }
        }

        // 지형 그림. 번호가 `data/terrain.ron` 에 없어도 **파일을 거부하지 않는다** —
        // 그림 데이터는 클라이언트 쪽이라 존 파일보다 자주 바뀌고, 없는 번호는 그리지 않을 뿐이다.
        let mut art = crate::terrain::ArtLayer::new();
        for r in &self.art {
            if r.len == 0
                || r.y >= self.tiles.height
                || r.x >= self.tiles.width
                || r.x + r.len > self.tiles.width
            {
                return Err(format!(
                    "그림 행 (y {}, x {}, {}칸) 이(가) 타일맵 밖이거나 비었음",
                    r.y, r.x, r.len
                ));
            }
            if r.id == 0 {
                return Err(format!("그림 행 (y {}, x {}) 의 번호가 0", r.y, r.x));
            }
            for x in r.x..r.x + r.len {
                art.insert(
                    TileCoord::new(x as i32, r.y as i32),
                    crate::scene::ArtId::new(r.id),
                );
            }
        }

        let mut scene = Scene {
            world: World::default(),
            items: Vec::new(),
            zone: ZoneBounds { min, max },
            tiles,
            art,
        };
        for (i, m) in self.markers.into_iter().enumerate() {
            let pos = finite2(m.pos, &format!("markers[{i}].pos"))?;
            if !(m.size.is_finite() && m.size > 0.0) {
                return Err(format!("markers[{i}].size 는 0 보다 큰 수여야 함"));
            }
            if !m.orientation.is_finite() {
                return Err(format!("markers[{i}].orientation 이 수가 아님"));
            }
            let kind = match m.kind {
                MarkerKindFile::Player => ItemKind::PlayerSpawn,
                MarkerKindFile::Npc => ItemKind::Npc,
                MarkerKindFile::Monster => ItemKind::Monster,
            };
            let entity = scene.add(&m.name, kind, pos, m.size);
            if let Some(item) = scene.item_mut(entity) {
                item.orientation = units::normalize_heading(m.orientation);
            }
        }
        Ok(scene)
    }
}

fn finite2(v: [f32; 2], what: &str) -> Result<Vec2, String> {
    if v.iter().all(|x| x.is_finite()) {
        Ok(Vec2::from_array(v))
    } else {
        Err(format!("{what} 에 수가 아닌 값이 있음"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 마커 하나의 저장되는 값 (이름, 종류, 위치, 크기, 방향).
    type MarkerValues = (String, ItemKind, Vec2, f32, f32);

    /// 비교용 — 핸들은 실행마다 다르므로 빼고 본다.
    fn summary(scene: &Scene) -> (ZoneBounds, Vec<MarkerValues>) {
        (
            scene.zone,
            scene
                .items
                .iter()
                .map(|i| (i.name.clone(), i.kind, i.pos, i.size, i.orientation))
                .collect(),
        )
    }

    fn same_tiles(a: &TileMap, b: &TileMap) -> bool {
        a.width() == b.width()
            && a.height() == b.height()
            && a.origin() == b.origin()
            && a.tile_size() == b.tile_size()
            && (0..a.height() as i32).all(|y| {
                (0..a.width() as i32)
                    .all(|x| a.get(TileCoord::new(x, y)) == b.get(TileCoord::new(x, y)))
            })
    }

    #[test]
    fn round_trip_keeps_everything_but_handles() {
        let original = Scene::server_default();
        let loaded = from_ron(&to_ron(&original)).unwrap();
        assert_eq!(summary(&loaded), summary(&original));
        assert!(same_tiles(&loaded.tiles, &original.tiles));
    }

    #[test]
    fn only_non_default_tiles_are_written() {
        let scene = Scene::server_default();
        let file = ZoneFile::from_scene(&scene);
        let changed = (0..scene.tiles.height() as i32)
            .flat_map(|y| (0..scene.tiles.width() as i32).map(move |x| TileCoord::new(x, y)))
            .filter(|&c| scene.tiles.get(c) != Some(Tile::default()))
            .count();
        let written: u32 = file.tiles.runs.iter().map(|r| r.len).sum();
        assert_eq!(
            written as usize, changed,
            "행 묶음이 칸을 빠뜨리거나 겹쳤다"
        );
        assert!(
            file.tiles.runs.len() < 40,
            "직사각형 지형이 행 단위로 묶이지 않았다"
        );
        assert!(changed < (scene.tiles.width() * scene.tiles.height()) as usize / 10);
    }

    #[test]
    fn output_uses_unix_newlines_on_every_os() {
        let text = to_ron(&Scene::server_default());
        assert!(
            !text.contains('\r'),
            "CRLF 가 섞였다 — OS 마다 다른 파일이 된다"
        );
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn saving_twice_gives_identical_bytes() {
        let scene = Scene::server_default();
        assert_eq!(to_ron(&scene), to_ron(&from_ron(&to_ron(&scene)).unwrap()));
    }

    #[test]
    fn committed_sample_matches_the_default_scene() {
        // 저장소의 샘플 파일이 기본 씬과 어긋나지 않게 한다. 기본 씬을 바꿨다면
        // 에디터에서 샘플을 다시 저장할 것 (파일 → 다른 이름으로 저장 → sample).
        let sample = include_str!("../../../zones/sample.zone.ron");
        assert_eq!(sample, to_ron(&Scene::server_default()));
    }

    /// 샘플 파일 다시 만들기 — 기본 씬을 바꾼 뒤 한 번 돌린다.
    /// `cargo test -p world-editor -- --ignored regenerate_sample_zone`
    #[test]
    #[ignore = "샘플 파일을 덮어쓴다 — 필요할 때만 직접 실행"]
    fn regenerate_sample_zone() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../zones/sample.zone.ron");
        save(&Scene::server_default(), &path).unwrap();
    }

    #[test]
    fn bad_files_are_rejected_with_a_reason() {
        let good = to_ron(&Scene::server_default());
        let cases = [
            (good.replacen("version: 1", "version: 99", 1), "형식 99"),
            (
                good.replacen("min: (-1000.0, -1000.0)", "min: (2000.0, -1000.0)", 1),
                "뒤집혔거나",
            ),
            (good.replacen("width: 64", "width: 0", 1), "허용 범위"),
            (good.replacen("len: 1,", "len: 99,", 1), "타일맵 밖"),
            (String::from("(version: 1"), "읽을 수 없음"),
        ];
        for (text, expected) in cases {
            let err = from_ron(&text).err().unwrap_or_default();
            assert!(err.contains(expected), "'{expected}' 기대, 실제: {err}");
        }
    }

    #[test]
    fn default_path_is_lowercase_in_the_zone_folder() {
        assert_eq!(
            default_path("Elwynn Forest"),
            Path::new(ZONE_DIR).join("elwynn forest.zone.ron")
        );
        assert_eq!(
            default_path("town.zone.ron"),
            Path::new(ZONE_DIR).join("town.zone.ron")
        );
    }

    #[test]
    fn typed_save_paths_are_normalised() {
        let zones = Path::new(ZONE_DIR);
        assert_eq!(save_path_from_input(" Town "), zones.join("town.zone.ron"));
        assert_eq!(
            save_path_from_input("town.zone.ron"),
            zones.join("town.zone.ron")
        );
        assert_eq!(
            save_path_from_input("maps/Dungeon.zone.ron"),
            Path::new("maps").join("dungeon.zone.ron"),
            "폴더는 그대로, 파일 이름만 소문자"
        );
        assert_eq!(save_path_from_input(""), zones.join("untitled.zone.ron"));
    }

    #[test]
    fn save_and_load_through_the_file_system() {
        let dir = std::env::temp_dir().join(format!("nexus-zone-test-{}", std::process::id()));
        let path = dir.join("nested").join("t.zone.ron");
        let scene = Scene::server_default();
        save(&scene, &path).unwrap();
        assert!(
            !path.with_extension("ron.tmp").exists(),
            "임시 파일이 남았다"
        );
        let loaded = load(&path).unwrap();
        assert_eq!(summary(&loaded), summary(&scene));
        assert_eq!(list(&dir.join("nested")), vec![path.clone()]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
