//! 에디터 씬 — 편집 대상 데이터와 피킹.
//!
//! 서버 `ZoneConfig` 의 편집 가능한 부분(존 경계 AABB, 스폰 위치)을 담는다.
//! M6 에서 ECS 컴포넌트로 옮기고 `ZoneConfig` 로 직렬화한다.
//!
//! 이 모듈은 GPU·UI 를 모른다. 좌표는 모두 월드 공간(미터, XY 평면)이다 — `nexus_core::units`.

use nexus_core::{Entity, Vec2, World, units};
use nexus_sim::{Tile, TileCoord, TileMap};

/// 존 경계의 최소 한 변 길이 (m). 핸들을 끌어 뒤집히거나 0 이 되는 것을 막는다.
pub(crate) const MIN_ZONE_SIZE: f32 = 1.0;

/// 기본 타일맵 크기 (칸). 존 전체(2km)를 1m 타일로 덮으면 400만 칸이라,
/// 지금은 원점 주변만 저작 영역으로 둔다. 존 전체 덮기는 청크 분할과 함께 온다.
pub(crate) const TILEMAP_SIZE: u32 = 64;
/// 타일 한 변 (m).
pub(crate) const TILE_SIZE: f32 = 1.0;

/// 스폰 마커 종류. 서버의 `playerSpawnPoints` / `npcSpawns(NPC|MONSTER)` 에 대응한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ItemKind {
    PlayerSpawn,
    Npc,
    Monster,
}

impl ItemKind {
    /// 표시 색 (sRGB).
    pub(crate) fn color(self) -> [f32; 4] {
        match self {
            Self::PlayerSpawn => [0.30, 0.85, 0.55, 1.0],
            Self::Npc => [0.90, 0.75, 0.30, 1.0],
            Self::Monster => [0.90, 0.35, 0.30, 1.0],
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::PlayerSpawn => "플레이어 스폰",
            Self::Npc => "NPC",
            Self::Monster => "몬스터",
        }
    }

    /// 세 종류 전부 — 종류별 표를 만들 때 쓴다.
    pub(crate) const ALL: [Self; 3] = [Self::PlayerSpawn, Self::Npc, Self::Monster];

    /// 배열 색인용 번호. [`ALL`](Self::ALL) 순서와 같다.
    pub(crate) fn index(self) -> usize {
        match self {
            Self::PlayerSpawn => 0,
            Self::Npc => 1,
            Self::Monster => 2,
        }
    }

    /// 새로 놓을 때의 점유 크기 (m). 서버에 크기 필드가 없어 캐릭터 크기 정도로 둔다.
    pub(crate) fn default_size(self) -> f32 {
        match self {
            Self::PlayerSpawn => 1.0,
            Self::Npc | Self::Monster => 0.8,
        }
    }
}

/// 뷰포트에 놓인 편집 대상 하나.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Item {
    pub(crate) entity: Entity,
    pub(crate) name: String,
    pub(crate) kind: ItemKind,
    /// 월드 위치 (m).
    pub(crate) pos: Vec2,
    /// 실제 점유 크기, 한 변 (m). 화면에서는 [`MARKER_MIN_PX`] 보다 작게 그려지지 않는다.
    pub(crate) size: f32,
    /// 바라보는 방향 (라디안, `0 = +X`, 반시계가 +). 서버 `SpawnPoint::orientation` 과 같은 값.
    pub(crate) orientation: f32,
    /// 이 마커가 스폰할 **액터 타입**. [`ActorId::DEFAULT`] 면 종류별 기본값을 쓴다.
    ///
    /// 수치·스프라이트는 전부 여기서 나온다 — 마커는 "어디에 무엇을" 만 정한다.
    pub(crate) actor: ActorId,
}

/// 존 경계 핸들. 모서리는 두 축을, 변은 한 축을 움직인다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Handle {
    BottomLeft,
    BottomRight,
    TopRight,
    TopLeft,
    Left,
    Right,
    Bottom,
    Top,
}

impl Handle {
    pub(crate) const CORNERS: [Self; 4] = [
        Self::BottomLeft,
        Self::BottomRight,
        Self::TopRight,
        Self::TopLeft,
    ];
    pub(crate) const EDGES: [Self; 4] = [Self::Left, Self::Right, Self::Bottom, Self::Top];
}

/// 존 경계 AABB (XY 평면).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ZoneBounds {
    pub(crate) min: Vec2,
    pub(crate) max: Vec2,
}

impl ZoneBounds {
    pub(crate) fn size(self) -> Vec2 {
        self.max - self.min
    }

    pub(crate) fn center(self) -> Vec2 {
        (self.min + self.max) * 0.5
    }

    /// 핸들의 월드 위치.
    pub(crate) fn handle_pos(self, handle: Handle) -> Vec2 {
        let c = self.center();
        match handle {
            Handle::BottomLeft => self.min,
            Handle::BottomRight => Vec2::new(self.max.x, self.min.y),
            Handle::TopRight => self.max,
            Handle::TopLeft => Vec2::new(self.min.x, self.max.y),
            Handle::Left => Vec2::new(self.min.x, c.y),
            Handle::Right => Vec2::new(self.max.x, c.y),
            Handle::Bottom => Vec2::new(c.x, self.min.y),
            Handle::Top => Vec2::new(c.x, self.max.y),
        }
    }

    /// 핸들을 `to` 로 옮긴 결과. 반대편을 넘거나 [`MIN_ZONE_SIZE`] 보다 작아지지 않는다.
    pub(crate) fn with_handle_moved(self, handle: Handle, to: Vec2) -> Self {
        let mut b = self;
        let moves_left = matches!(handle, Handle::Left | Handle::BottomLeft | Handle::TopLeft);
        let moves_right = matches!(
            handle,
            Handle::Right | Handle::BottomRight | Handle::TopRight
        );
        let moves_bottom = matches!(
            handle,
            Handle::Bottom | Handle::BottomLeft | Handle::BottomRight
        );
        let moves_top = matches!(handle, Handle::Top | Handle::TopLeft | Handle::TopRight);

        if moves_left {
            b.min.x = to.x.min(self.max.x - MIN_ZONE_SIZE);
        }
        if moves_right {
            b.max.x = to.x.max(self.min.x + MIN_ZONE_SIZE);
        }
        if moves_bottom {
            b.min.y = to.y.min(self.max.y - MIN_ZONE_SIZE);
        }
        if moves_top {
            b.max.y = to.y.max(self.min.y + MIN_ZONE_SIZE);
        }
        b
    }
}

/// 선택·조작 대상.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Target {
    Zone,
    Item(Entity),
}

/// 포인터 아래에 무엇이 있는가.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pick {
    Item(Entity),
    /// 단독 선택된 마커의 방향 화살표 끝 회전 핸들.
    RotateHandle(Entity),
    ZoneHandle(Handle),
}

impl Pick {
    pub(crate) fn target(self) -> Target {
        match self {
            Self::Item(e) | Self::RotateHandle(e) => Target::Item(e),
            Self::ZoneHandle(_) => Target::Zone,
        }
    }
}

/// 액터 타입 번호 — `data/rules.ron` 의 `actors` 를 가리킨다. 0 은 "마커 종류의 기본값".
///
/// **서버 테이블과 같은 번호 공간**이다 (`ItemId`·`SkillId` 와 같은 규칙).
/// 수치는 `rules.ron`, 이름·스프라이트는 `display.ron` — 표를 반으로 쪼개는 규칙 그대로다.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub(crate) struct ActorId(u32);

impl ActorId {
    /// 마커가 타입을 정하지 않았다 — 종류별 기본 액터를 쓴다 (예전 존 파일도 이 값이다).
    pub(crate) const DEFAULT: Self = Self(0);

    pub(crate) fn new(raw: u32) -> Self {
        Self(raw)
    }

    pub(crate) fn raw(self) -> u32 {
        self.0
    }

    pub(crate) fn is_default(self) -> bool {
        self.0 == 0
    }
}

/// 지형 그림 번호 — `data/terrain.ron` 의 항목을 가리킨다. 0 은 "그림 없음".
///
/// 그림은 **규칙이 아니다.** 같은 번호가 지면 그림일 수도 정적 오브젝트일 수도 있고,
/// 어느 쪽인지는 `terrain.ron` 이 정한다 (`crate::terrain`).
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub(crate) struct ArtId(u16);

impl ArtId {
    /// 그림 없음 — 칠하지 않은 칸.
    pub(crate) const NONE: Self = Self(0);

    pub(crate) fn new(raw: u16) -> Self {
        Self(raw)
    }

    pub(crate) fn raw(self) -> u16 {
        self.0
    }

    pub(crate) fn is_none(self) -> bool {
        self.0 == 0
    }
}

/// 편집 중인 존 하나.
#[derive(Debug)]
pub(crate) struct Scene {
    pub(crate) world: World,
    /// 그리기 순서 = 목록 순서. 뒤쪽이 위에 그려지고 피킹도 뒤쪽이 우선이다.
    pub(crate) items: Vec<Item>,
    pub(crate) zone: ZoneBounds,
    /// 걷기 가능 여부·높이 레벨. 서버 이동 검증과 클라 길찾기가 같은 데이터를 본다.
    pub(crate) tiles: TileMap,
    /// 칸마다 칠한 **지면 그림** — 규칙(`tiles`)과 좌표만 공유하는 별개의 층이다.
    ///
    /// 칠한 칸만 담는다 (대부분의 칸은 비어 있다). 순서가 고정된 맵을 쓰는 이유는
    /// 저장 파일이 실행마다 같은 바이트여야 하기 때문이다.
    pub(crate) art: crate::terrain::ArtLayer,
    /// 칸마다 놓인 **정적 오브젝트**(건물·소품). 지면과 **다른 층**이다 —
    /// 건물을 놓아도 그 칸의 지면 그림이 지워지지 않아야 한다.
    pub(crate) props: crate::terrain::ArtLayer,
}

impl Scene {
    /// NexusEngine `Server.cpp` 의 기본 존 배치.
    ///
    /// `Server.cpp` 는 `boundsMin`/`boundsMax` 를 설정하지 않으므로
    /// `ZoneConfig` 구조체 기본값 ±1000 이 실제로 쓰인다 — 미터로 읽으면 2km × 2km 존.
    pub(crate) fn server_default() -> Self {
        let mut scene = Self {
            world: World::default(),
            items: Vec::new(),
            zone: ZoneBounds {
                min: Vec2::new(-1000.0, -1000.0),
                max: Vec2::new(1000.0, 1000.0),
            },
            // 원점을 한가운데 두어 기본 시점에서 바로 보이게 한다.
            tiles: TileMap::new(
                TILEMAP_SIZE,
                TILEMAP_SIZE,
                TILE_SIZE,
                Vec2::splat(-(TILEMAP_SIZE as f32) * TILE_SIZE * 0.5),
                Tile::default(),
            ),
            art: crate::terrain::ArtLayer::new(),
            props: crate::terrain::ArtLayer::new(),
        };

        // Server.cpp 의 숫자를 그대로 옮기고 미터로 읽는다.
        //
        // ⚠ 서버 샘플 스폰은 Y-up 으로 작성돼 있다 — 모든 스폰의 y 가 0 이고 x·z 만 바뀐다
        //   ({10,0,10}, {20,0,-5}, {-8,0,12} …). 문서의 Z-up 대로 읽으면 슬라임이 땅속 5m,
        //   상인이 공중 12m 에 놓인다. 그래서 여기서는 서버 (x, z) 를 지면 (X, Y) 로 옮긴다.
        //   엔진 규약은 Z-up 그대로이며, 서버 데이터 쪽을 고쳐야 한다 (CLAUDE.md 참고).
        //
        // 점유 크기는 서버에 없으므로 캐릭터 크기 정도로 둔다.
        // (이름, 종류, 서버 x, 서버 z, 방향 rad)
        let spawns = [
            ("플레이어 스폰 #1", ItemKind::PlayerSpawn, 0.0, 0.0, 0.0),
            ("플레이어 스폰 #2", ItemKind::PlayerSpawn, 5.0, 5.0, 0.0),
            ("마을 경비병", ItemKind::Npc, 10.0, 10.0, 0.0),
            ("상인 NPC", ItemKind::Npc, -8.0, 12.0, 1.5),
            ("슬라임", ItemKind::Monster, 20.0, -5.0, 0.0),
        ];
        scene.seed_sample_terrain();

        for (name, kind, server_x, server_z, orientation) in spawns {
            let e = scene.add(
                name,
                kind,
                Vec2::new(server_x, server_z),
                kind.default_size(),
            );
            if let Some(item) = scene.item_mut(e) {
                item.orientation = orientation;
            }
        }
        scene
    }

    /// 샘플 지형 — 고지대 한 덩이 + 경사로 하나 + 벽 한 줄.
    ///
    /// 스폰 마커와 같은 성격의 예시 데이터다. 존 파일을 읽어 오게 되면(S7) 사라진다.
    /// 배치는 마커들이 보이는 범위와 겹치도록 잡았다.
    fn seed_sample_terrain(&mut self) {
        let put = |map: &mut TileMap, x: i32, y: i32, tile: Tile| {
            map.set(TileCoord::new(x, y), tile);
        };
        let high = Tile {
            walkable: true,
            level: 1,
            ramp: false,
        };
        let ramp = Tile { ramp: true, ..high };
        let wall = Tile {
            walkable: false,
            ..Tile::default()
        };

        // 동쪽 고지대. 서쪽 변 가운데 한 칸만 경사로라 거기로만 오르내린다.
        for y in 28..40 {
            for x in 40..52 {
                put(&mut self.tiles, x, y, high);
            }
        }
        for y in 33..36 {
            put(&mut self.tiles, 40, y, ramp);
        }
        // 서쪽 벽 — 길찾기가 돌아가야 하는 장애물.
        for y in 26..38 {
            put(&mut self.tiles, 26, y, wall);
        }
    }

    pub(crate) fn add(&mut self, name: &str, kind: ItemKind, pos: Vec2, size: f32) -> Entity {
        let entity = self.world.spawn();
        self.items.push(Item {
            entity,
            name: name.to_owned(),
            kind,
            pos,
            size,
            orientation: 0.0,
            // 종류의 기본 액터를 쓴다. 인스펙터에서 바꾼다.
            actor: ActorId::DEFAULT,
        });
        entity
    }

    /// 새 마커를 만든다. 씬에는 아직 넣지 않는다 — 넣는 것은 언두 가능한 편집이 한다.
    ///
    /// 이름은 `"NPC #3"` 처럼 종류별로 비어 있는 가장 작은 번호를 쓴다.
    pub(crate) fn new_item(&mut self, kind: ItemKind, pos: Vec2) -> Item {
        let name = (1..)
            .map(|n| format!("{} #{n}", kind.label()))
            .find(|name| self.items.iter().all(|i| &i.name != name))
            .unwrap_or_default();
        Item {
            entity: self.world.spawn(),
            name,
            kind,
            pos,
            size: kind.default_size(),
            orientation: 0.0,
            actor: ActorId::DEFAULT,
        }
    }

    /// 목록의 `index` 자리에 넣는다 (범위를 넘으면 끝에).
    ///
    /// 삭제를 되돌릴 때 원래 자리로 돌아가야 그리기·피킹 순서가 유지된다.
    pub(crate) fn insert(&mut self, index: usize, item: Item) {
        let index = index.min(self.items.len());
        self.items.insert(index, item);
    }

    /// 목록에서 빼고 (원래 자리, 항목) 을 돌려준다.
    ///
    /// 엔티티는 `World` 에서 해제하지 않는다 — 언두로 되살릴 때 **같은 핸들**이어야
    /// 선택·언두 기록이 그대로 들어맞는다. 기록 상한(256)만큼만 남으므로 누수는 유한하다.
    pub(crate) fn remove(&mut self, entity: Entity) -> Option<(usize, Item)> {
        let index = self.items.iter().position(|i| i.entity == entity)?;
        Some((index, self.items.remove(index)))
    }

    /// 사각형(월드)과 화면에 그려진 마커가 겹치는 항목. 박스 선택용.
    pub(crate) fn items_in_rect(&self, min: Vec2, max: Vec2, px: f32) -> Vec<Entity> {
        self.items
            .iter()
            .filter(|item| {
                let half = Vec2::splat(marker_half_extent(item, px));
                let (a, b) = (item.pos - half, item.pos + half);
                a.x <= max.x && b.x >= min.x && a.y <= max.y && b.y >= min.y
            })
            .map(|item| item.entity)
            .collect()
    }

    pub(crate) fn item(&self, entity: Entity) -> Option<&Item> {
        self.items.iter().find(|i| i.entity == entity)
    }

    pub(crate) fn item_mut(&mut self, entity: Entity) -> Option<&mut Item> {
        self.items.iter_mut().find(|i| i.entity == entity)
    }

    /// 이름으로 대상을 찾는다. "존 경계" 는 존을 가리킨다. (자동 검증용)
    pub(crate) fn find_by_label(&self, label: &str) -> Option<Target> {
        if label == ZONE_LABEL {
            return Some(Target::Zone);
        }
        self.items
            .iter()
            .find(|i| i.name == label)
            .map(|i| Target::Item(i.entity))
    }

    /// 표시 이름.
    pub(crate) fn label(&self, target: Target) -> Option<&str> {
        match target {
            Target::Zone => Some(ZONE_LABEL),
            Target::Item(e) => self.item(e).map(|i| i.name.as_str()),
        }
    }

    /// `p` 아래의 대상을 찾는다. `px` 는 화면 1픽셀의 월드 길이(m) — 줌에 따라 달라진다.
    ///
    /// `rotatable` 은 회전 핸들을 보이고 있는 마커(단독 선택)다.
    ///
    /// 우선순위: 회전 핸들 → 마커(위에 그려진 것부터) → 존 모서리 → 존 변.
    /// 존 내부의 빈 곳은 존을 고르지 않는다 — 존은 화면 대부분을 덮기 때문이다.
    pub(crate) fn pick(&self, p: Vec2, px: f32, rotatable: Option<Entity>) -> Option<Pick> {
        let tolerance = PICK_TOLERANCE_PX * px;

        if let Some(item) = rotatable.and_then(|e| self.item(e))
            && (p - rotate_handle_pos(item, px)).length() <= HANDLE_SIZE_PX * 0.5 * px + tolerance
        {
            return Some(Pick::RotateHandle(item.entity));
        }

        for item in self.items.iter().rev() {
            // 그려지는 크기와 같은 규칙을 쓴다 — 보이는 만큼 잡혀야 한다.
            let half = marker_half_extent(item, px) + tolerance;
            let d = (p - item.pos).abs();
            if d.x <= half && d.y <= half {
                return Some(Pick::Item(item.entity));
            }
        }

        // 모서리는 변보다 넉넉하게 잡는다 — 모서리를 잡으려다 변을 잡는 일이 없도록.
        let corner_tol = tolerance * 1.5;
        for handle in Handle::CORNERS {
            if (p - self.zone.handle_pos(handle)).length() <= corner_tol {
                return Some(Pick::ZoneHandle(handle));
            }
        }

        let z = self.zone;
        let within_x = p.x >= z.min.x - tolerance && p.x <= z.max.x + tolerance;
        let within_y = p.y >= z.min.y - tolerance && p.y <= z.max.y + tolerance;
        if within_y && (p.x - z.min.x).abs() <= tolerance {
            return Some(Pick::ZoneHandle(Handle::Left));
        }
        if within_y && (p.x - z.max.x).abs() <= tolerance {
            return Some(Pick::ZoneHandle(Handle::Right));
        }
        if within_x && (p.y - z.min.y).abs() <= tolerance {
            return Some(Pick::ZoneHandle(Handle::Bottom));
        }
        if within_x && (p.y - z.max.y).abs() <= tolerance {
            return Some(Pick::ZoneHandle(Handle::Top));
        }
        None
    }
}

/// 씬 목록·인스펙터에 쓰는 존 표시 이름.
pub(crate) const ZONE_LABEL: &str = "존 경계";

/// 피킹 허용 오차 (화면 픽셀).
pub(crate) const PICK_TOLERANCE_PX: f32 = 6.0;

/// 마커가 화면에서 최소한 차지하는 한 변 크기 (픽셀).
///
/// 존은 수 km, 캐릭터는 1m 안팎이다. 존 전체를 보는 배율에서 실제 크기로 그리면
/// 마커가 1픽셀도 안 되므로, 아이콘처럼 최소 화면 크기를 보장한다.
pub(crate) const MARKER_MIN_PX: f32 = 12.0;

/// 마커가 그려지고 잡히는 반폭 (m). 실제 크기와 최소 화면 크기 중 큰 쪽.
pub(crate) fn marker_half_extent(item: &Item, px: f32) -> f32 {
    (item.size * 0.5).max(MARKER_MIN_PX * 0.5 * px)
}

/// 핸들(존 경계·회전)의 화면 크기, 한 변 (픽셀).
pub(crate) const HANDLE_SIZE_PX: f32 = 11.0;

/// 방향 화살표가 마커 가장자리 밖으로 뻗는 길이 (픽셀).
pub(crate) const ARROW_PX: f32 = 14.0;

/// 회전 핸들이 마커 가장자리에서 떨어진 거리 (픽셀). 화살표 끝보다 조금 더 바깥.
pub(crate) const ROTATE_HANDLE_GAP_PX: f32 = 26.0;

/// 방향 화살표 끝 (m).
pub(crate) fn arrow_tip(item: &Item, px: f32) -> Vec2 {
    item.pos
        + units::heading_to_dir(item.orientation) * (marker_half_extent(item, px) + ARROW_PX * px)
}

/// 회전 핸들 위치 (m). 화살표 연장선 위에 있다.
pub(crate) fn rotate_handle_pos(item: &Item, px: f32) -> Vec2 {
    item.pos
        + units::heading_to_dir(item.orientation)
            * (marker_half_extent(item, px) + ROTATE_HANDLE_GAP_PX * px)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone() -> ZoneBounds {
        ZoneBounds {
            min: Vec2::new(-100.0, -100.0),
            max: Vec2::new(100.0, 100.0),
        }
    }

    #[test]
    fn corner_moves_two_axes_edge_moves_one() {
        let z = zone().with_handle_moved(Handle::TopRight, Vec2::new(300.0, 250.0));
        assert_eq!(z.max, Vec2::new(300.0, 250.0));
        assert_eq!(z.min, Vec2::new(-100.0, -100.0));

        let z = zone().with_handle_moved(Handle::Left, Vec2::new(-400.0, 999.0));
        assert_eq!(
            z.min,
            Vec2::new(-400.0, -100.0),
            "변 핸들은 한 축만 움직인다"
        );
        assert_eq!(z.max, Vec2::new(100.0, 100.0));
    }

    #[test]
    fn handle_cannot_cross_opposite_side() {
        // 왼쪽 변을 오른쪽 변 너머까지 끌어도 최소 폭이 유지된다
        let z = zone().with_handle_moved(Handle::Left, Vec2::new(500.0, 0.0));
        assert!((z.size().x - MIN_ZONE_SIZE).abs() < 1e-3, "{:?}", z);
        assert!(z.min.x < z.max.x);
    }

    #[test]
    fn topmost_item_wins_when_overlapping() {
        let mut s = Scene::server_default();
        let bottom = s.add("아래", ItemKind::Npc, Vec2::new(500.0, 500.0), 40.0);
        let top = s.add("위", ItemKind::Npc, Vec2::new(505.0, 500.0), 40.0);

        assert_eq!(
            s.pick(Vec2::new(503.0, 500.0), 1.0, None),
            Some(Pick::Item(top))
        );
        assert_ne!(
            s.pick(Vec2::new(503.0, 500.0), 1.0, None),
            Some(Pick::Item(bottom))
        );
    }

    #[test]
    fn zone_is_picked_by_edges_and_corners_only() {
        let s = Scene::server_default();
        let px = 1.0; // 허용 오차 6m
        assert_eq!(
            s.pick(Vec2::new(1000.0, 1000.0), px, None),
            Some(Pick::ZoneHandle(Handle::TopRight))
        );
        assert_eq!(
            s.pick(Vec2::new(-1005.0, 300.0), px, None),
            Some(Pick::ZoneHandle(Handle::Left))
        );
        assert_eq!(
            s.pick(Vec2::new(400.0, 400.0), px, None),
            None,
            "존 내부 빈 곳은 아무것도 고르지 않는다"
        );
    }

    #[test]
    fn zoomed_out_markers_are_picked_by_screen_size() {
        // 2km 존 전체를 보는 배율 (1px ≈ 3m). 0.8m 슬라임은 실제 크기로는 픽셀 이하지만
        // 최소 화면 크기(12px)로 그려지므로 그만큼 잡혀야 한다.
        let s = Scene::server_default();
        let slime = s.find_by_label("슬라임").unwrap();
        let px = 3.0;
        // 반폭 = 12px/2 × 3m = 18m, 허용 오차 6px × 3m = 18m → 36m 까지
        assert_eq!(
            s.pick(Vec2::new(20.0 + 35.0, -5.0), px, None)
                .map(Pick::target),
            Some(slime)
        );
    }

    #[test]
    fn zoomed_in_markers_are_picked_by_real_size() {
        // 1px = 1cm. 이때는 실제 크기(0.8m, 반폭 0.4m)가 최소 화면 크기(반폭 6cm)보다 크다.
        let s = Scene::server_default();
        let slime = s.find_by_label("슬라임").unwrap();
        let px = 0.01;
        assert_eq!(
            s.pick(Vec2::new(20.45, -5.0), px, None).map(Pick::target),
            Some(slime),
            "반폭 0.4m + 오차 0.06m 안쪽"
        );
        assert_eq!(s.pick(Vec2::new(20.5, -5.0), px, None), None);
    }

    #[test]
    fn server_sample_spawns_are_spread_in_meters() {
        // 서버 (x, z) 를 지면으로 읽고 미터로 해석 → 수 m ~ 20m 간격으로 흩어진다
        let s = Scene::server_default();
        let guard = s.find_by_label("마을 경비병").unwrap();
        let merchant = s.find_by_label("상인 NPC").unwrap();
        let pos = |t| match t {
            Target::Item(e) => s.item(e).unwrap().pos,
            Target::Zone => unreachable!(),
        };
        assert!((pos(guard) - pos(merchant)).length() > 10.0);

        let Target::Item(m) = merchant else {
            unreachable!()
        };
        assert!(
            (s.item(m).unwrap().orientation - 1.5).abs() < 1e-6,
            "상인 방향 1.5 rad"
        );
    }

    #[test]
    fn sample_terrain_is_walkable_by_simulation() {
        use nexus_sim::{Authority, Event, Intent, LocalAuthority, SimWorld, UnitDef};

        // 에디터가 저작한 타일을 시뮬레이션이 그대로 쓴다 — 벽 서쪽에서 출발해
        // 벽을 돌아 경사로로 동쪽 고지대에 오른다.
        let s = Scene::server_default();
        let tiles = s.tiles.clone();
        let mut auth = LocalAuthority::new(SimWorld::new(s.tiles));
        let unit = auth.world_mut().spawn_unit(
            Vec2::new(-10.0, 0.0),
            0.0,
            UnitDef {
                move_speed: 2.5,
                ..UnitDef::default()
            },
        );
        let target = Vec2::new(15.0, 2.0);
        auth.submit(Intent::MoveTo { unit, target });

        let mut visited = vec![tiles.world_to_tile(Vec2::new(-10.0, 0.0))];
        let mut arrived = false;
        for _ in 0..2000 {
            let events = auth.tick(std::time::Duration::from_millis(50));
            let here = tiles.world_to_tile(auth.world().unit(unit).unwrap().pos());
            let last = *visited.last().unwrap();
            if here != last {
                assert!(tiles.can_step(last, here), "{last:?} → {here:?} 불법 걸음");
                visited.push(here);
            }
            if events.contains(&Event::Arrived { unit }) {
                arrived = true;
                break;
            }
            assert!(events.is_empty(), "예상 밖 이벤트: {events:?}");
        }
        assert!(arrived, "고지대에 도착하지 못했다");
        assert!(
            visited
                .iter()
                .any(|&c| tiles.get(c).is_some_and(|t| t.ramp)),
            "경사로를 지나지 않고 고지대에 올랐다"
        );
        assert_eq!(tiles.get(*visited.last().unwrap()).unwrap().level, 1);
    }

    #[test]
    fn labels_round_trip() {
        let s = Scene::server_default();
        for label in [ZONE_LABEL, "상인 NPC"] {
            let t = s.find_by_label(label).unwrap();
            assert_eq!(s.label(t), Some(label));
        }
        assert!(s.find_by_label("없는 이름").is_none());
    }
}
