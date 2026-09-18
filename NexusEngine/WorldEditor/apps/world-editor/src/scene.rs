//! 에디터 씬 — 편집 대상 데이터와 피킹.
//!
//! 서버 `ZoneConfig` 의 편집 가능한 부분(존 경계 AABB, 스폰 위치)을 담는다.
//! M6 에서 ECS 컴포넌트로 옮기고 `ZoneConfig` 로 직렬화한다.
//!
//! 이 모듈은 GPU·UI 를 모른다. 좌표는 모두 월드 공간(cm, XY 평면)이다.

use nexus_core::{Entity, Vec2, World};

/// 존 경계의 최소 한 변 길이 (cm). 핸들을 끌어 뒤집히거나 0 이 되는 것을 막는다.
pub(crate) const MIN_ZONE_SIZE: f32 = 50.0;

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
}

/// 뷰포트에 놓인 편집 대상 하나.
#[derive(Clone, Debug)]
pub(crate) struct Item {
    pub(crate) entity: Entity,
    pub(crate) name: String,
    pub(crate) kind: ItemKind,
    /// 월드 위치 (cm).
    pub(crate) pos: Vec2,
    /// 마커 한 변 크기 (cm).
    pub(crate) size: f32,
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
    ZoneHandle(Handle),
}

impl Pick {
    pub(crate) fn target(self) -> Target {
        match self {
            Self::Item(e) => Target::Item(e),
            Self::ZoneHandle(_) => Target::Zone,
        }
    }
}

/// 편집 중인 존 하나.
#[derive(Debug)]
pub(crate) struct Scene {
    pub(crate) world: World,
    /// 그리기 순서 = 목록 순서. 뒤쪽이 위에 그려지고 피킹도 뒤쪽이 우선이다.
    pub(crate) items: Vec<Item>,
    pub(crate) zone: ZoneBounds,
}

impl Scene {
    /// NexusEngine `Server.cpp` 의 기본 존 배치.
    ///
    /// `Server.cpp` 는 `boundsMin`/`boundsMax` 를 설정하지 않으므로
    /// `ZoneConfig` 구조체 기본값(±1000cm)이 실제로 쓰인다.
    pub(crate) fn server_default() -> Self {
        let mut scene = Self {
            world: World::default(),
            items: Vec::new(),
            zone: ZoneBounds {
                min: Vec2::new(-1000.0, -1000.0),
                max: Vec2::new(1000.0, 1000.0),
            },
        };

        let spawns = [
            (
                "플레이어 스폰 #1",
                ItemKind::PlayerSpawn,
                Vec2::new(0.0, 0.0),
                40.0,
            ),
            (
                "플레이어 스폰 #2",
                ItemKind::PlayerSpawn,
                Vec2::new(5.0, 5.0),
                40.0,
            ),
            ("마을 경비병", ItemKind::Npc, Vec2::new(10.0, 10.0), 30.0),
            ("상인 NPC", ItemKind::Npc, Vec2::new(-8.0, 12.0), 30.0),
            ("슬라임", ItemKind::Monster, Vec2::new(20.0, -5.0), 30.0),
        ];
        for (name, kind, pos, size) in spawns {
            scene.add(name, kind, pos, size);
        }
        scene
    }

    pub(crate) fn add(&mut self, name: &str, kind: ItemKind, pos: Vec2, size: f32) -> Entity {
        let entity = self.world.spawn();
        self.items.push(Item {
            entity,
            name: name.to_owned(),
            kind,
            pos,
            size,
        });
        entity
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

    /// `p` 아래의 대상을 찾는다. `tolerance` 는 월드 단위 허용 오차(보통 화면 몇 픽셀).
    ///
    /// 우선순위: 마커(위에 그려진 것부터) → 존 모서리 → 존 변.
    /// 존 내부의 빈 곳은 존을 고르지 않는다 — 존은 화면 대부분을 덮기 때문이다.
    pub(crate) fn pick(&self, p: Vec2, tolerance: f32) -> Option<Pick> {
        for item in self.items.iter().rev() {
            let half = item.size * 0.5 + tolerance;
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

        assert_eq!(s.pick(Vec2::new(503.0, 500.0), 1.0), Some(Pick::Item(top)));
        assert_ne!(
            s.pick(Vec2::new(503.0, 500.0), 1.0),
            Some(Pick::Item(bottom))
        );
    }

    #[test]
    fn zone_is_picked_by_edges_and_corners_only() {
        let s = Scene::server_default();
        let tol = 10.0;
        assert_eq!(
            s.pick(Vec2::new(1000.0, 1000.0), tol),
            Some(Pick::ZoneHandle(Handle::TopRight))
        );
        assert_eq!(
            s.pick(Vec2::new(-1005.0, 300.0), tol),
            Some(Pick::ZoneHandle(Handle::Left))
        );
        assert_eq!(
            s.pick(Vec2::new(400.0, 400.0), tol),
            None,
            "존 내부 빈 곳은 아무것도 고르지 않는다"
        );
    }

    #[test]
    fn small_markers_get_click_tolerance() {
        let s = Scene::server_default();
        // 슬라임(20,-5) 크기 30 → 반폭 15. 허용 오차 5 를 더하면 20 까지 잡힌다.
        let slime = s.find_by_label("슬라임").unwrap();
        assert_eq!(
            s.pick(Vec2::new(39.0, -5.0), 5.0).map(Pick::target),
            Some(slime)
        );
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
