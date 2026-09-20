//! 편집 상태 — 선택, 드래그, 스냅, 언두/리두.
//!
//! UI 는 [`PointerInput`] / [`InspectorEdit`] 같은 "무슨 일이 있었는가"만 넘기고,
//! 씬을 실제로 바꾸는 것은 전부 이 모듈이다. 그래서 egui 없이 테스트할 수 있다.
//!
//! # 언두 단위
//!
//! - 뷰포트 드래그 한 번 = 1 스텝 (누를 때 원래 값을 잡아 두고, 뗄 때 기록)
//! - 인스펙터 값 편집 한 번 = 1 스텝 (드래그가 끝나거나 입력칸 포커스를 잃을 때 기록)
//! - 추가·삭제 한 번 = 1 스텝 (여러 개를 지워도 하나)
//! - 움직이지 않은 클릭, 박스 선택은 기록하지 않는다 — 선택은 편집이 아니다

use core::f32::consts::PI;

use std::collections::BTreeMap;

use nexus_core::{Entity, Vec2, units};
use nexus_sim::{Tile, TileCoord};

use crate::scene::{ArtId, Handle, Item, ItemKind, Pick, Scene, Target, ZoneBounds};
use crate::terrain::ArtKind;

/// 뷰포트에서 포인터가 무슨 일을 하는가.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Tool {
    /// 마커·존을 고르고 옮긴다.
    #[default]
    Select,
    /// 타일 **규칙**(걷기·레벨·경사로)을 칠한다.
    PaintTile,
    /// 타일 **그림**(지형 아트·건물)을 칠한다. 규칙은 건드리지 않는다.
    PaintArt,
}

/// 언두 기록 상한. 오래된 것부터 버린다.
const HISTORY_LIMIT: usize = 256;

/// 한 프레임에 이어 칠할 최대 칸 수. 포인터가 화면 밖에서 튀어 들어와도 폭주하지 않게.
const PAINT_MAX_STEPS: f32 = 512.0;

/// Ctrl 회전 스냅 간격 — 15°.
pub(crate) const ROTATE_SNAP: f32 = PI / 12.0;

/// 되돌릴 수 있는 편집 하나.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Command {
    /// 대상별 (원래 위치, 새 위치).
    MoveItems(Vec<(Entity, Vec2, Vec2)>),
    /// 대상별 (원래 방향, 새 방향). 라디안.
    RotateItems(Vec<(Entity, f32, f32)>),
    /// (목록 위치, 항목). 위치 오름차순.
    AddItems(Vec<(usize, Item)>),
    /// (원래 목록 위치, 항목). 위치 오름차순.
    RemoveItems(Vec<(usize, Item)>),
    SetZone {
        from: ZoneBounds,
        to: ZoneBounds,
    },
    /// 칠하기 한 번(누름→뗌)이 통째로 한 스텝이다. `(좌표, 원래, 새것)`.
    PaintTiles(Vec<(TileCoord, Tile, Tile)>),
    /// 그림 칠하기 한 번(누름→뗌). `(좌표, 원래, 새것)`. `kind` 가 어느 층인지 정한다.
    PaintArt {
        kind: ArtKind,
        edits: Vec<(TileCoord, ArtId, ArtId)>,
    },
}

impl Command {
    fn apply(&self, scene: &mut Scene) {
        match self {
            Self::MoveItems(moves) => {
                for &(e, _, to) in moves {
                    if let Some(item) = scene.item_mut(e) {
                        item.pos = to;
                    }
                }
            }
            Self::RotateItems(turns) => {
                set_headings(scene, turns.iter().map(|&(e, _, to)| (e, to)))
            }
            Self::AddItems(items) => insert_all(scene, items),
            Self::RemoveItems(items) => remove_all(scene, items),
            Self::SetZone { to, .. } => scene.zone = *to,
            Self::PaintTiles(edits) => {
                for &(at, _, to) in edits {
                    scene.tiles.set(at, to);
                }
            }
            Self::PaintArt { kind, edits } => {
                for &(at, _, to) in edits {
                    set_art(scene, *kind, at, to);
                }
            }
        }
    }

    fn revert(&self, scene: &mut Scene) {
        match self {
            Self::MoveItems(moves) => {
                for &(e, from, _) in moves {
                    if let Some(item) = scene.item_mut(e) {
                        item.pos = from;
                    }
                }
            }
            Self::RotateItems(turns) => {
                set_headings(scene, turns.iter().map(|&(e, from, _)| (e, from)));
            }
            Self::AddItems(items) => remove_all(scene, items),
            Self::RemoveItems(items) => insert_all(scene, items),
            Self::SetZone { from, .. } => scene.zone = *from,
            Self::PaintTiles(edits) => {
                for &(at, from, _) in edits {
                    scene.tiles.set(at, from);
                }
            }
            Self::PaintArt { kind, edits } => {
                for &(at, from, _) in edits {
                    set_art(scene, *kind, at, from);
                }
            }
        }
    }

    fn is_noop(&self) -> bool {
        match self {
            Self::MoveItems(moves) => moves.iter().all(|&(_, from, to)| from == to),
            Self::RotateItems(turns) => turns.iter().all(|&(_, from, to)| from == to),
            Self::AddItems(items) | Self::RemoveItems(items) => items.is_empty(),
            Self::SetZone { from, to } => from == to,
            Self::PaintTiles(edits) => edits.is_empty(),
            Self::PaintArt { edits, .. } => edits.is_empty(),
        }
    }

    /// 사람이 읽을 요약 (메뉴·상태 바용).
    pub(crate) fn describe(&self) -> String {
        let count = |n: usize, verb: &str| {
            if n == 1 {
                String::from(verb)
            } else {
                format!("{n}개 {verb}")
            }
        };
        match self {
            Self::MoveItems(moves) => count(moves.len(), "이동"),
            Self::RotateItems(turns) => count(turns.len(), "회전"),
            Self::AddItems(items) => match items.as_slice() {
                [(_, item)] => format!("{} 추가", item.name),
                _ => count(items.len(), "추가"),
            },
            Self::RemoveItems(items) => match items.as_slice() {
                [(_, item)] => format!("{} 삭제", item.name),
                _ => count(items.len(), "삭제"),
            },
            Self::SetZone { .. } => String::from("존 경계 변경"),
            Self::PaintTiles(edits) => count(edits.len(), "타일 칠하기"),
            Self::PaintArt { kind, edits } => count(
                edits.len(),
                match kind {
                    ArtKind::Ground => "지면 칠하기",
                    ArtKind::Prop => "오브젝트 놓기",
                },
            ),
        }
    }
}

/// 층을 고른다. 지면과 오브젝트는 좌표만 공유하는 별개의 맵이다.
fn layer_mut(scene: &mut Scene, kind: ArtKind) -> &mut crate::terrain::ArtLayer {
    match kind {
        ArtKind::Ground => &mut scene.art,
        ArtKind::Prop => &mut scene.props,
    }
}

fn layer(scene: &Scene, kind: ArtKind) -> &crate::terrain::ArtLayer {
    match kind {
        ArtKind::Ground => &scene.art,
        ArtKind::Prop => &scene.props,
    }
}

/// 칸의 그림을 바꾼다. "없음"은 **항목을 지운다** — 칠하지 않은 칸을 들고 있으면
/// 저장 파일이 부풀고 화면 훑기도 느려진다.
fn set_art(scene: &mut Scene, kind: ArtKind, at: TileCoord, id: ArtId) {
    if id.is_none() {
        layer_mut(scene, kind).remove(&at);
    } else {
        layer_mut(scene, kind).insert(at, id);
    }
}

fn set_headings(scene: &mut Scene, headings: impl Iterator<Item = (Entity, f32)>) {
    for (e, heading) in headings {
        if let Some(item) = scene.item_mut(e) {
            item.orientation = heading;
        }
    }
}

/// 위치 오름차순으로 넣는다 — 앞쪽이 먼저 자리를 잡아야 뒤쪽 위치가 맞는다.
fn insert_all(scene: &mut Scene, items: &[(usize, Item)]) {
    for (index, item) in items {
        scene.insert(*index, item.clone());
    }
}

/// 위치 내림차순으로 뺀다 — 뒤쪽부터 빼야 앞쪽 위치가 흔들리지 않는다.
fn remove_all(scene: &mut Scene, items: &[(usize, Item)]) {
    for (_, item) in items.iter().rev() {
        scene.remove(item.entity);
    }
}

/// 언두/리두 스택.
///
/// 기록마다 **고유 번호**를 붙인다. 씬의 "지금 상태" 는 언두 스택 맨 위의 번호다
/// ([`state_id`](Self::state_id)) — 저장할 때 이 번호를 적어 두면, 언두로 저장 시점에
/// 되돌아왔을 때도 "저장 안 됨" 이 풀린다. 편집 횟수를 세는 방식으로는 이것이 안 된다.
#[derive(Debug, Default)]
pub(crate) struct History {
    undo: Vec<(u64, Command)>,
    redo: Vec<(u64, Command)>,
    /// 마지막으로 발급한 번호.
    last_id: u64,
    /// 언두 스택이 비었을 때의 상태 번호. 상한 때문에 버린 기록이 있으면 그 번호가 된다.
    base_id: u64,
}

impl History {
    /// 이미 씬에 적용된 편집을 기록한다. 아무것도 바뀌지 않았으면 무시한다.
    fn record(&mut self, cmd: Command) {
        if cmd.is_noop() {
            return;
        }
        self.last_id += 1;
        self.redo.clear();
        self.undo.push((self.last_id, cmd));
        if self.undo.len() > HISTORY_LIMIT {
            let (dropped, _) = self.undo.remove(0);
            self.base_id = dropped;
        }
    }

    /// 지금 씬 상태의 번호. 같은 번호면 같은 편집 상태다.
    pub(crate) fn state_id(&self) -> u64 {
        self.undo.last().map_or(self.base_id, |(id, _)| *id)
    }

    pub(crate) fn undo_label(&self) -> Option<String> {
        self.undo.last().map(|(_, c)| c.describe())
    }

    pub(crate) fn redo_label(&self) -> Option<String> {
        self.redo.last().map(|(_, c)| c.describe())
    }
}

/// 한 프레임의 뷰포트 포인터 상태. 좌표는 모두 월드 단위.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PointerInput {
    /// 포인터 위치. 드래그 중에는 뷰포트 밖이어도 들어온다.
    pub(crate) world: Option<Vec2>,
    /// 뷰포트 위에서 왼쪽 버튼이 이번 프레임에 눌렸다.
    pub(crate) pressed: bool,
    /// 포인터가 뷰포트 위에 있다 (패널에 가려지지 않음). 아니면 호버를 계산하지 않는다.
    pub(crate) over_viewport: bool,
    /// 왼쪽 버튼이 이번 프레임에 떼어졌다 (위치 무관).
    pub(crate) released: bool,
    /// Shift — 선택에 더하기/빼기.
    pub(crate) additive: bool,
    /// Ctrl — 그리드 스냅 / 회전 15° 스냅.
    pub(crate) snap: bool,
    /// 화면 1픽셀의 월드 길이 (m). 피킹 허용 오차·마커 최소 크기 계산에 쓴다.
    pub(crate) px: f32,
    /// 스냅 간격 (m, 보통 현재 그리드 간격).
    pub(crate) grid: f32,
}

/// 인스펙터에서 값을 바꿨다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum InspectorEdit {
    ItemPos {
        entity: Entity,
        pos: Vec2,
        /// 이번 편집이 끝났다 (드래그 종료·포커스 상실) — 언두에 기록할 시점.
        finished: bool,
    },
    /// 방향 (라디안). 저장 전에 `[0, 2π)` 로 정규화한다.
    ItemHeading {
        entity: Entity,
        heading: f32,
        finished: bool,
    },
    Zone {
        bounds: ZoneBounds,
        finished: bool,
    },
}

/// 진행 중인 뷰포트 드래그.
#[derive(Clone, Debug)]
enum Drag {
    Items {
        anchor: Entity,
        start_cursor: Vec2,
        originals: Vec<(Entity, Vec2)>,
    },
    Rotate {
        entity: Entity,
        original: f32,
    },
    Zone {
        handle: Handle,
        original: ZoneBounds,
    },
    /// 빈 곳에서 시작한 박스 선택. `base` 는 시작 시점의 선택 (Shift 면 유지, 아니면 비움).
    Box {
        start: Vec2,
        current: Vec2,
        base: Vec<Target>,
    },
}

/// 진행 중인 인스펙터 편집의 원래 값.
#[derive(Clone, Copy, Debug)]
enum LiveEdit {
    ItemPos(Entity, Vec2),
    ItemHeading(Entity, f32),
    Zone(ZoneBounds),
}

/// 편집 상태 전체.
#[derive(Debug, Default)]
pub(crate) struct Editing {
    /// 선택 순서를 유지한다 (첫 항목이 기준).
    selection: Vec<Target>,
    history: History,
    drag: Option<Drag>,
    live: Option<LiveEdit>,
    tool: Tool,
    /// 칠하기용 붓.
    brush: Tile,
    /// 칠하는 중이면 `Some`. 칸마다 **맨 처음 값**을 기억해 둔다 —
    /// 같은 칸을 여러 번 지나가도 언두가 원래대로 돌아가도록.
    ///
    /// 순서가 고정된 맵을 쓴다. `HashMap` 이면 언두 기록 순서가 실행마다 달라져
    /// 테스트가 불안정해진다.
    stroke: Option<BTreeMap<TileCoord, Tile>>,
    /// 그림 칠하기용 붓 — `data/terrain.ron` 의 번호. 기본은 "없음"(지우개).
    art_brush: ArtId,
    /// 붓이 어느 층을 칠하는가. 지면과 오브젝트는 다른 층이라 **지우개도 층을 골라야** 한다
    /// (건물만 지우고 지면은 남기는 것이 보통이다).
    art_layer: ArtKind,
    /// 그림 칠하기 진행 중이면 `Some`. [`Editing::stroke`] 와 같은 이유로 맨 처음 값을 기억한다.
    art_stroke: Option<BTreeMap<TileCoord, ArtId>>,
    /// 직전 칠하기 지점. 두 지점 사이를 이어 칠하는 데 쓴다.
    paint_last: Option<Vec2>,
}

impl Editing {
    pub(crate) fn selection(&self) -> &[Target] {
        &self.selection
    }

    pub(crate) fn is_selected(&self, target: Target) -> bool {
        self.selection.contains(&target)
    }

    pub(crate) fn history(&self) -> &History {
        &self.history
    }

    pub(crate) fn tool(&self) -> Tool {
        self.tool
    }

    pub(crate) fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
    }

    pub(crate) fn brush(&self) -> Tile {
        self.brush
    }

    pub(crate) fn set_brush(&mut self, brush: Tile) {
        self.brush = brush;
    }

    pub(crate) fn art_brush(&self) -> ArtId {
        self.art_brush
    }

    pub(crate) fn art_layer(&self) -> ArtKind {
        self.art_layer
    }

    /// 붓을 바꾼다. `kind` 는 그 번호가 속한 층 (`data/terrain.ron` 이 정한다).
    pub(crate) fn set_art_brush(&mut self, id: ArtId, kind: ArtKind) {
        self.art_brush = id;
        self.art_layer = kind;
    }

    /// 칠하는 중인가. 진행 중에는 언두·삭제 같은 다른 편집을 막는다.
    pub(crate) fn is_painting(&self) -> bool {
        self.stroke.is_some() || self.art_stroke.is_some()
    }

    /// 직전 지점부터 `world` 까지 지나가는 칸들. 칠하기 두 종류가 같이 쓴다.
    ///
    /// 한 프레임에 포인터가 여러 칸을 건너뛰면(빠른 드래그, 스크립트 입력) 사이가 비어
    /// 점선이 되므로 **사이를 이어** 칠한다.
    fn stroke_cells(&mut self, scene: &Scene, world: Vec2) -> Vec<TileCoord> {
        let from = self.paint_last.unwrap_or(world);
        let delta = world - from;
        let step = scene.tiles.tile_size().max(f32::EPSILON) * 0.5;
        let steps = (delta.length() / step).ceil().clamp(1.0, PAINT_MAX_STEPS) as u32;
        self.paint_last = Some(world);

        (0..=steps)
            .map(|i| {
                scene
                    .tiles
                    .world_to_tile(from + delta * (i as f32 / steps as f32))
            })
            .collect()
    }

    /// 지형 그림 칠하기 — [`Tool::PaintArt`] 일 때 포인터 입력을 받는다.
    ///
    /// 타일 **규칙은 건드리지 않는다.** 건물을 놓아도 막히는 칸은 저작자가 따로 칠한다
    /// (`crate::terrain` 머리 주석 참고).
    pub(crate) fn paint_art_pointer(&mut self, scene: &mut Scene, p: &PointerInput) {
        if p.pressed && p.over_viewport {
            self.art_stroke = Some(BTreeMap::new());
            self.paint_last = None;
        }
        if self.art_stroke.is_some()
            && let Some(world) = p.world
            && p.over_viewport
        {
            let (brush, kind) = (self.art_brush, self.art_layer);
            for at in self.stroke_cells(scene, world) {
                // 타일맵 밖은 칠하지 않는다 — 저장 파일이 타일맵 범위를 기준으로 묶인다.
                if scene.tiles.get(at).is_none() {
                    continue;
                }
                let before = layer(scene, kind).get(&at).copied().unwrap_or(ArtId::NONE);
                if let Some(stroke) = self.art_stroke.as_mut() {
                    stroke.entry(at).or_insert(before);
                }
                set_art(scene, kind, at, brush);
            }
        }
        if p.released
            && let Some(stroke) = self.art_stroke.take()
        {
            self.paint_last = None;
            let kind = self.art_layer;
            let edits: Vec<_> = stroke
                .into_iter()
                .filter_map(|(at, before)| {
                    let after = layer(scene, kind).get(&at).copied().unwrap_or(ArtId::NONE);
                    (before != after).then_some((at, before, after))
                })
                .collect();
            self.history.record(Command::PaintArt { kind, edits });
        }
    }

    /// 타일 칠하기 — [`Tool::PaintTile`] 일 때 포인터 입력을 받는다.
    ///
    /// 누름→끌기→뗌 한 번이 **언두 한 스텝**이다. 칸 하나를 여러 번 지나가도
    /// 기록은 하나로 남는다.
    pub(crate) fn paint_pointer(&mut self, scene: &mut Scene, p: &PointerInput) {
        if p.pressed && p.over_viewport {
            self.stroke = Some(BTreeMap::new());
            self.paint_last = None;
        }
        if self.stroke.is_some()
            && let Some(world) = p.world
            && p.over_viewport
        {
            let brush = self.brush;
            for at in self.stroke_cells(scene, world) {
                if let Some(before) = scene.tiles.get(at) {
                    // 이 칸의 **맨 처음** 값만 남긴다 — 같은 칸을 여러 번 지나가도
                    // 언두가 원래대로 돌아가도록.
                    if let Some(stroke) = self.stroke.as_mut() {
                        stroke.entry(at).or_insert(before);
                    }
                    scene.tiles.set(at, brush);
                }
            }
        }
        if p.released
            && let Some(stroke) = self.stroke.take()
        {
            self.paint_last = None;
            let edits: Vec<_> = stroke
                .into_iter()
                .filter_map(|(at, before)| {
                    let after = scene.tiles.get(at)?;
                    (before != after).then_some((at, before, after))
                })
                .collect();
            // 값이 그대로면 기록하지 않는다 — 같은 붓으로 덧칠한 경우.
            self.history.record(Command::PaintTiles(edits));
        }
    }

    pub(crate) fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// 회전 핸들을 보일 마커 — 마커 하나만 선택됐을 때.
    pub(crate) fn rotatable(&self) -> Option<Entity> {
        match self.selection.as_slice() {
            [Target::Item(e)] => Some(*e),
            _ => None,
        }
    }

    /// 진행 중인 박스 선택 사각형 (min, max). 그리기용.
    pub(crate) fn box_rect(&self) -> Option<(Vec2, Vec2)> {
        match &self.drag {
            Some(Drag::Box { start, current, .. }) => {
                Some((start.min(*current), start.max(*current)))
            }
            _ => None,
        }
    }

    /// 선택을 바꾼다. `additive` 면 토글, 아니면 그것만 선택.
    pub(crate) fn select(&mut self, target: Target, additive: bool) {
        if additive {
            if let Some(i) = self.selection.iter().position(|&t| t == target) {
                self.selection.remove(i);
            } else {
                self.selection.push(target);
            }
        } else {
            self.selection.clear();
            self.selection.push(target);
        }
    }

    pub(crate) fn clear_selection(&mut self) {
        self.selection.clear();
    }

    /// 씬에 더 이상 없는 대상을 선택에서 뺀다 (추가를 언두한 뒤 등).
    fn prune_selection(&mut self, scene: &Scene) {
        self.selection.retain(|t| match t {
            Target::Zone => true,
            Target::Item(e) => scene.item(*e).is_some(),
        });
    }

    // ── 언두 / 리두 ──────────────────────────────────────────────────────

    pub(crate) fn undo(&mut self, scene: &mut Scene) -> bool {
        self.cancel_pending(scene);
        let Some((id, cmd)) = self.history.undo.pop() else {
            return false;
        };
        cmd.revert(scene);
        self.history.redo.push((id, cmd));
        self.prune_selection(scene);
        true
    }

    pub(crate) fn redo(&mut self, scene: &mut Scene) -> bool {
        self.cancel_pending(scene);
        let Some((id, cmd)) = self.history.redo.pop() else {
            return false;
        };
        cmd.apply(scene);
        self.history.undo.push((id, cmd));
        self.prune_selection(scene);
        true
    }

    /// 진행 중인 드래그·인스펙터 편집을 원래 값으로 되돌리고 버린다.
    /// (드래그 도중 Ctrl+Z 가 눌렸을 때 기록과 씬이 어긋나지 않도록)
    fn cancel_pending(&mut self, scene: &mut Scene) {
        match self.drag.take() {
            Some(Drag::Items { originals, .. }) => {
                for (e, pos) in originals {
                    if let Some(item) = scene.item_mut(e) {
                        item.pos = pos;
                    }
                }
            }
            Some(Drag::Rotate { entity, original }) => {
                set_headings(scene, std::iter::once((entity, original)));
            }
            Some(Drag::Zone { original, .. }) => scene.zone = original,
            Some(Drag::Box { base, .. }) => self.selection = base,
            None => {}
        }
        match self.live.take() {
            Some(LiveEdit::ItemPos(e, pos)) => {
                if let Some(item) = scene.item_mut(e) {
                    item.pos = pos;
                }
            }
            Some(LiveEdit::ItemHeading(e, heading)) => {
                set_headings(scene, std::iter::once((e, heading)));
            }
            Some(LiveEdit::Zone(z)) => scene.zone = z,
            None => {}
        }
    }

    // ── 추가 / 삭제 ──────────────────────────────────────────────────────

    /// 마커를 `pos` 에 새로 놓고 그것만 선택한다.
    pub(crate) fn add_item(&mut self, scene: &mut Scene, kind: ItemKind, pos: Vec2) -> Entity {
        self.cancel_pending(scene);
        let item = scene.new_item(kind, pos);
        let entity = item.entity;
        let cmd = Command::AddItems(vec![(scene.items.len(), item)]);
        cmd.apply(scene);
        self.history.record(cmd);
        self.select(Target::Item(entity), false);
        entity
    }

    /// 선택된 마커를 지운다. 존은 지울 수 없으므로 선택에 남는다. 지운 게 있으면 `true`.
    pub(crate) fn delete_selected(&mut self, scene: &mut Scene) -> bool {
        self.cancel_pending(scene);
        let mut removed: Vec<(usize, Item)> = scene
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| self.is_selected(Target::Item(item.entity)))
            .map(|(i, item)| (i, item.clone()))
            .collect();
        if removed.is_empty() {
            return false;
        }
        removed.sort_by_key(|(i, _)| *i);
        let cmd = Command::RemoveItems(removed);
        cmd.apply(scene);
        self.history.record(cmd);
        self.prune_selection(scene);
        true
    }

    // ── 뷰포트 포인터 ────────────────────────────────────────────────────

    /// 뷰포트 포인터 입력을 처리한다. 반환값은 포인터 아래의 대상(호버 표시용).
    pub(crate) fn handle_pointer(
        &mut self,
        scene: &mut Scene,
        input: &PointerInput,
    ) -> Option<Pick> {
        // 패널 위에서는 호버하지 않는다 — 패널 뒤에 가려진 마커가 강조되지 않도록.
        let rotatable = self.rotatable();
        let hover = input
            .world
            .filter(|_| input.over_viewport)
            .and_then(|w| scene.pick(w, input.px, rotatable));

        if input.pressed
            && let Some(w) = input.world
        {
            self.begin_press(scene, hover, w, input.additive);
        }

        if let Some(w) = input.world
            && let Some(drag) = self.drag.as_mut()
        {
            match drag {
                Drag::Box {
                    start,
                    current,
                    base,
                } => {
                    *current = w;
                    let (min, max) = (start.min(w), start.max(w));
                    self.selection.clone_from(base);
                    for e in scene.items_in_rect(min, max, input.px) {
                        let target = Target::Item(e);
                        if !self.selection.contains(&target) {
                            self.selection.push(target);
                        }
                    }
                }
                other => update_drag(scene, other, w, input.snap, input.grid),
            }
        }

        if input.released {
            self.finish_drag(scene);
        }

        hover
    }

    fn begin_press(&mut self, scene: &Scene, hover: Option<Pick>, cursor: Vec2, additive: bool) {
        match hover {
            Some(Pick::RotateHandle(entity)) => {
                if let Some(item) = scene.item(entity) {
                    self.drag = Some(Drag::Rotate {
                        entity,
                        original: item.orientation,
                    });
                }
            }
            Some(Pick::Item(entity)) => {
                let target = Target::Item(entity);
                if additive {
                    self.select(target, true);
                    // Shift 로 선택을 해제했다면 끌 대상이 아니다
                    if !self.is_selected(target) {
                        return;
                    }
                } else if !self.is_selected(target) {
                    self.select(target, false);
                }

                let originals = self
                    .selection
                    .iter()
                    .filter_map(|t| match t {
                        Target::Item(e) => scene.item(*e).map(|i| (*e, i.pos)),
                        Target::Zone => None,
                    })
                    .collect();
                self.drag = Some(Drag::Items {
                    anchor: entity,
                    start_cursor: cursor,
                    originals,
                });
            }
            Some(Pick::ZoneHandle(handle)) => {
                // 핸들을 잡는 것은 곧 끌기 시작이므로 Shift 여도 선택을 해제하지 않는다.
                if !additive {
                    self.select(Target::Zone, false);
                } else if !self.is_selected(Target::Zone) {
                    self.selection.push(Target::Zone);
                }
                self.drag = Some(Drag::Zone {
                    handle,
                    original: scene.zone,
                });
            }
            None => {
                // 빈 곳 클릭 = 선택 해제, 그대로 끌면 박스 선택.
                if !additive {
                    self.clear_selection();
                }
                self.drag = Some(Drag::Box {
                    start: cursor,
                    current: cursor,
                    base: self.selection.clone(),
                });
            }
        }
    }

    fn finish_drag(&mut self, scene: &Scene) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        let cmd = match drag {
            Drag::Items { originals, .. } => Command::MoveItems(
                originals
                    .into_iter()
                    .filter_map(|(e, from)| scene.item(e).map(|i| (e, from, i.pos)))
                    .collect(),
            ),
            Drag::Rotate { entity, original } => Command::RotateItems(
                scene
                    .item(entity)
                    .map(|i| (entity, original, i.orientation))
                    .into_iter()
                    .collect(),
            ),
            Drag::Zone { original, .. } => Command::SetZone {
                from: original,
                to: scene.zone,
            },
            // 선택은 이미 드래그 중에 반영됐다. 기록할 편집이 아니다.
            Drag::Box { .. } => return,
        };
        self.history.record(cmd);
    }

    // ── 인스펙터 ─────────────────────────────────────────────────────────

    pub(crate) fn apply_inspector(&mut self, scene: &mut Scene, edit: InspectorEdit) {
        match edit {
            InspectorEdit::ItemPos {
                entity,
                pos,
                finished,
            } => {
                let Some(item) = scene.item_mut(entity) else {
                    return;
                };
                let original = match self.live {
                    Some(LiveEdit::ItemPos(e, orig)) if e == entity => orig,
                    _ => item.pos,
                };
                item.pos = pos;
                if finished {
                    self.live = None;
                    self.history
                        .record(Command::MoveItems(vec![(entity, original, pos)]));
                } else {
                    self.live = Some(LiveEdit::ItemPos(entity, original));
                }
            }
            InspectorEdit::ItemHeading {
                entity,
                heading,
                finished,
            } => {
                let Some(item) = scene.item_mut(entity) else {
                    return;
                };
                let original = match self.live {
                    Some(LiveEdit::ItemHeading(e, orig)) if e == entity => orig,
                    _ => item.orientation,
                };
                let heading = units::normalize_heading(heading);
                item.orientation = heading;
                if finished {
                    self.live = None;
                    self.history
                        .record(Command::RotateItems(vec![(entity, original, heading)]));
                } else {
                    self.live = Some(LiveEdit::ItemHeading(entity, original));
                }
            }
            InspectorEdit::Zone { bounds, finished } => {
                let original = match self.live {
                    Some(LiveEdit::Zone(orig)) => orig,
                    _ => scene.zone,
                };
                scene.zone = bounds;
                if finished {
                    self.live = None;
                    self.history.record(Command::SetZone {
                        from: original,
                        to: bounds,
                    });
                } else {
                    self.live = Some(LiveEdit::Zone(original));
                }
            }
        }
    }
}

/// 드래그 중인 대상을 커서 위치에 맞춰 옮긴다. `snap` 이면 위치는 `grid` 간격, 회전은 15° 로 맞춘다.
fn update_drag(scene: &mut Scene, drag: &Drag, cursor: Vec2, snap: bool, grid: f32) {
    match drag {
        Drag::Items {
            anchor,
            start_cursor,
            originals,
        } => {
            let Some(&(_, anchor_from)) = originals.iter().find(|(e, _)| e == anchor) else {
                return;
            };
            // 기준 대상(누른 것)을 스냅하고, 나머지는 같은 이동량만큼 따라온다.
            let mut anchor_to = anchor_from + (cursor - *start_cursor);
            if snap {
                anchor_to = snap_to(anchor_to, grid);
            }
            let delta = anchor_to - anchor_from;
            for &(e, from) in originals {
                if let Some(item) = scene.item_mut(e) {
                    item.pos = from + delta;
                }
            }
        }
        Drag::Rotate { entity, .. } => {
            let Some(item) = scene.item_mut(*entity) else {
                return;
            };
            let dir = cursor - item.pos;
            // 커서가 마커 중심에 겹치면 방향이 정해지지 않는다 — 직전 값을 유지.
            if dir.length_squared() <= f32::EPSILON {
                return;
            }
            let mut heading = units::dir_to_heading(dir);
            if snap {
                heading = (heading / ROTATE_SNAP).round() * ROTATE_SNAP;
            }
            item.orientation = units::normalize_heading(heading);
        }
        Drag::Zone { handle, original } => {
            let to = if snap { snap_to(cursor, grid) } else { cursor };
            scene.zone = original.with_handle_moved(*handle, to);
        }
        // 박스 선택은 선택만 바꾼다 — `handle_pointer` 가 처리한다.
        Drag::Box { .. } => {}
    }
}

fn snap_to(p: Vec2, step: f32) -> Vec2 {
    if step <= 0.0 {
        return p;
    }
    (p / step).round() * step
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::ItemKind;

    /// 1px = 0.3m (테스트 기준 배율)
    const PX: f32 = 0.3;

    /// 서로 떨어진 마커 두 개 + ±1000 존.
    fn scene() -> (Scene, Entity, Entity) {
        let mut s = Scene::server_default();
        s.items.clear();
        let a = s.add("A", ItemKind::Npc, Vec2::new(100.0, 100.0), 20.0);
        let b = s.add("B", ItemKind::Monster, Vec2::new(-300.0, 50.0), 20.0);
        (s, a, b)
    }

    fn press(at: Vec2) -> PointerInput {
        PointerInput {
            world: Some(at),
            pressed: true,
            over_viewport: true,
            px: PX,
            grid: 100.0,
            ..Default::default()
        }
    }

    fn move_to(at: Vec2) -> PointerInput {
        PointerInput {
            world: Some(at),
            over_viewport: true,
            px: PX,
            grid: 100.0,
            ..Default::default()
        }
    }

    fn release_at(at: Vec2) -> PointerInput {
        PointerInput {
            released: true,
            ..move_to(at)
        }
    }

    /// 누르고 → 옮기고 → 떼는 한 번의 드래그.
    fn drag(ed: &mut Editing, s: &mut Scene, from: Vec2, to: Vec2) {
        ed.handle_pointer(s, &press(from));
        ed.handle_pointer(s, &move_to(to));
        ed.handle_pointer(s, &release_at(to));
    }

    // ── 타일 칠하기 ──────────────────────────────────────────────────────────

    fn blocked() -> Tile {
        Tile {
            walkable: false,
            ..Tile::default()
        }
    }

    /// 칠하기용 씬 — 원점 주변 타일맵만 쓴다.
    fn paint_setup() -> (Scene, Editing) {
        let s = Scene::server_default();
        let mut ed = Editing::default();
        ed.set_tool(Tool::PaintTile);
        ed.set_brush(blocked());
        (s, ed)
    }

    /// 한 번의 칠하기 (누름 → 지점들 → 뗌).
    fn paint(ed: &mut Editing, s: &mut Scene, points: &[Vec2]) {
        let (first, rest) = points.split_first().expect("지점이 필요하다");
        ed.paint_pointer(s, &press(*first));
        for p in rest {
            ed.paint_pointer(s, &move_to(*p));
        }
        ed.paint_pointer(s, &release_at(*points.last().unwrap()));
    }

    fn tile_at(s: &Scene, world: Vec2) -> Tile {
        let at = s.tiles.world_to_tile(world);
        s.tiles.get(at).expect("타일맵 밖")
    }

    #[test]
    fn a_stroke_is_a_single_undo_step() {
        let (mut s, mut ed) = paint_setup();
        paint(&mut ed, &mut s, &[Vec2::new(0.5, 0.5), Vec2::new(3.5, 0.5)]);

        assert!(!tile_at(&s, Vec2::new(0.5, 0.5)).walkable);
        assert!(!tile_at(&s, Vec2::new(3.5, 0.5)).walkable);

        // 여러 칸을 칠해도 언두 한 번에 전부 돌아와야 한다.
        assert!(ed.undo(&mut s));
        assert!(tile_at(&s, Vec2::new(0.5, 0.5)).walkable);
        assert!(tile_at(&s, Vec2::new(3.5, 0.5)).walkable);
        assert!(!ed.undo(&mut s), "스텝이 하나여야 한다");
    }

    #[test]
    fn fast_drag_does_not_leave_gaps() {
        // 포인터가 한 프레임에 여러 칸을 건너뛰어도 사이가 비면 안 된다.
        let (mut s, mut ed) = paint_setup();
        paint(&mut ed, &mut s, &[Vec2::new(0.5, 0.5), Vec2::new(8.5, 0.5)]);

        for x in 0..=8 {
            let at = Vec2::new(x as f32 + 0.5, 0.5);
            assert!(!tile_at(&s, at).walkable, "x={x} 가 비었다");
        }
    }

    #[test]
    fn repainting_a_tile_keeps_the_original_for_undo() {
        // 같은 칸을 여러 번 지나가도 언두는 맨 처음 값으로 돌아가야 한다.
        let (mut s, mut ed) = paint_setup();
        let at = Vec2::new(0.5, 0.5);
        paint(&mut ed, &mut s, &[at, Vec2::new(2.5, 0.5), at]);

        assert!(!tile_at(&s, at).walkable);
        ed.undo(&mut s);
        assert!(tile_at(&s, at).walkable, "원래 값으로 안 돌아갔다");
    }

    #[test]
    fn painting_the_same_value_records_nothing() {
        // 같은 붓으로 덧칠하면 기록이 남지 않아야 한다 — 언두가 빈 스텝으로 더럽혀진다.
        let (mut s, mut ed) = paint_setup();
        ed.set_brush(Tile::default());
        paint(&mut ed, &mut s, &[Vec2::new(0.5, 0.5), Vec2::new(2.5, 0.5)]);
        assert!(ed.history().undo_label().is_none());
    }

    #[test]
    fn painting_outside_the_map_is_ignored() {
        let (mut s, mut ed) = paint_setup();
        paint(&mut ed, &mut s, &[Vec2::new(900.0, 900.0)]);
        assert!(ed.history().undo_label().is_none());
        assert!(!ed.is_painting(), "칠하기가 끝나지 않았다");
    }

    #[test]
    fn redo_reapplies_the_stroke() {
        let (mut s, mut ed) = paint_setup();
        let at = Vec2::new(0.5, 0.5);
        paint(&mut ed, &mut s, &[at]);
        ed.undo(&mut s);
        assert!(tile_at(&s, at).walkable);
        assert!(ed.redo(&mut s));
        assert!(!tile_at(&s, at).walkable);
    }

    // ── 지형 그림 칠하기 ────────────────────────────────────────────────────

    /// 그림 칠하기용 씬 — 붓은 `kind` 층의 `id` 번 그림.
    fn art_setup(id: u16, kind: ArtKind) -> (Scene, Editing) {
        let s = Scene::server_default();
        let mut ed = Editing::default();
        ed.set_tool(Tool::PaintArt);
        ed.set_art_brush(ArtId::new(id), kind);
        (s, ed)
    }

    /// 한 번의 그림 칠하기 (누름 → 지점들 → 뗌).
    fn paint_art(ed: &mut Editing, s: &mut Scene, points: &[Vec2]) {
        let (first, rest) = points.split_first().expect("지점이 필요하다");
        ed.paint_art_pointer(s, &press(*first));
        for p in rest {
            ed.paint_art_pointer(s, &move_to(*p));
        }
        ed.paint_art_pointer(s, &release_at(*points.last().unwrap()));
    }

    fn art_at(s: &Scene, world: Vec2) -> ArtId {
        let at = s.tiles.world_to_tile(world);
        s.art.get(&at).copied().unwrap_or(ArtId::NONE)
    }

    fn prop_at(s: &Scene, world: Vec2) -> ArtId {
        let at = s.tiles.world_to_tile(world);
        s.props.get(&at).copied().unwrap_or(ArtId::NONE)
    }

    #[test]
    fn painting_art_does_not_touch_the_rules() {
        // 그림과 걷기 규칙은 다른 층이다 — 건물을 놓아도 길이 막히지 않아야 한다.
        let (mut s, mut ed) = art_setup(100, ArtKind::Prop);
        let at = Vec2::new(0.5, 0.5);
        paint_art(&mut ed, &mut s, &[at, Vec2::new(4.5, 0.5)]);

        assert_eq!(prop_at(&s, at), ArtId::new(100));
        assert!(tile_at(&s, at).walkable, "그림이 규칙을 바꿨다");
        assert_eq!(tile_at(&s, at), Tile::default());
    }

    #[test]
    fn a_prop_does_not_erase_the_ground_under_it() {
        // 두 층이 따로 있어야 건물이 풀 위에 선다 — 한 층이면 지면이 지워진다.
        let (mut s, mut ed) = art_setup(1, ArtKind::Ground);
        let at = Vec2::new(0.5, 0.5);
        paint_art(&mut ed, &mut s, &[at]);

        ed.set_art_brush(ArtId::new(100), ArtKind::Prop);
        paint_art(&mut ed, &mut s, &[at]);

        assert_eq!(art_at(&s, at), ArtId::new(1), "지면 그림이 지워졌다");
        assert_eq!(prop_at(&s, at), ArtId::new(100));
    }

    #[test]
    fn an_art_stroke_is_a_single_undo_step() {
        let (mut s, mut ed) = art_setup(1, ArtKind::Ground);
        paint_art(&mut ed, &mut s, &[Vec2::new(0.5, 0.5), Vec2::new(5.5, 0.5)]);
        for x in 0..=5 {
            assert_eq!(art_at(&s, Vec2::new(x as f32 + 0.5, 0.5)), ArtId::new(1));
        }

        assert!(ed.undo(&mut s));
        for x in 0..=5 {
            assert!(art_at(&s, Vec2::new(x as f32 + 0.5, 0.5)).is_none());
        }
        assert!(!ed.undo(&mut s), "스텝이 하나여야 한다");
    }

    #[test]
    fn the_eraser_only_clears_its_own_layer() {
        // "없음"으로 칠하면 항목이 남지 않아야 한다 — 남으면 저장 파일이 부푼다.
        // 그리고 건물만 지우고 지면은 남길 수 있어야 한다.
        let (mut s, mut ed) = art_setup(2, ArtKind::Ground);
        let at = Vec2::new(0.5, 0.5);
        paint_art(&mut ed, &mut s, &[at]);
        ed.set_art_brush(ArtId::new(100), ArtKind::Prop);
        paint_art(&mut ed, &mut s, &[at]);

        ed.set_art_brush(ArtId::NONE, ArtKind::Prop);
        paint_art(&mut ed, &mut s, &[at]);
        assert!(s.props.is_empty(), "지운 칸이 남아 있다");
        assert_eq!(art_at(&s, at), ArtId::new(2), "지면까지 지워졌다");

        // 언두하면 되살아난다.
        ed.undo(&mut s);
        assert_eq!(prop_at(&s, at), ArtId::new(100));
    }

    #[test]
    fn repainting_the_same_art_records_nothing() {
        let (mut s, mut ed) = art_setup(3, ArtKind::Ground);
        let at = Vec2::new(0.5, 0.5);
        paint_art(&mut ed, &mut s, &[at]);
        let before = ed.history().undo_label();
        paint_art(&mut ed, &mut s, &[at]);
        assert_eq!(ed.history().undo_label(), before, "빈 스텝이 기록됐다");
    }

    #[test]
    fn painting_art_outside_the_map_is_ignored() {
        let (mut s, mut ed) = art_setup(1, ArtKind::Ground);
        paint_art(&mut ed, &mut s, &[Vec2::new(900.0, 900.0)]);
        assert!(s.art.is_empty());
        assert!(ed.history().undo_label().is_none());
        assert!(!ed.is_painting(), "칠하기가 끝나지 않았다");
    }

    #[test]
    fn click_selects_and_empty_click_deselects() {
        let (mut s, a, _) = scene();
        let mut ed = Editing::default();

        ed.handle_pointer(&mut s, &press(Vec2::new(100.0, 100.0)));
        ed.handle_pointer(&mut s, &release_at(Vec2::new(100.0, 100.0)));
        assert_eq!(ed.selection(), &[Target::Item(a)]);

        ed.handle_pointer(&mut s, &press(Vec2::new(500.0, 500.0)));
        ed.handle_pointer(&mut s, &release_at(Vec2::new(500.0, 500.0)));
        assert!(ed.selection().is_empty());
    }

    #[test]
    fn no_hover_when_pointer_is_over_a_panel() {
        // 포인터 월드 좌표는 마커 위지만, 화면상으로는 패널이 가리고 있다
        let (mut s, _, _) = scene();
        let mut ed = Editing::default();
        let mut p = move_to(Vec2::new(100.0, 100.0));
        p.over_viewport = false;
        assert_eq!(ed.handle_pointer(&mut s, &p), None);
    }

    #[test]
    fn click_without_moving_does_not_create_history() {
        let (mut s, _, _) = scene();
        let mut ed = Editing::default();
        drag(
            &mut ed,
            &mut s,
            Vec2::new(100.0, 100.0),
            Vec2::new(100.0, 100.0),
        );
        assert!(ed.history().undo_label().is_none());
    }

    #[test]
    fn drag_moves_and_undo_redo_round_trips() {
        let (mut s, a, _) = scene();
        let mut ed = Editing::default();

        drag(
            &mut ed,
            &mut s,
            Vec2::new(100.0, 100.0),
            Vec2::new(250.0, 40.0),
        );
        assert_eq!(s.item(a).unwrap().pos, Vec2::new(250.0, 40.0));

        assert!(ed.undo(&mut s));
        assert_eq!(s.item(a).unwrap().pos, Vec2::new(100.0, 100.0));

        assert!(ed.redo(&mut s));
        assert_eq!(s.item(a).unwrap().pos, Vec2::new(250.0, 40.0));
    }

    #[test]
    fn shift_selection_moves_together_as_one_step() {
        let (mut s, a, b) = scene();
        let mut ed = Editing::default();

        ed.handle_pointer(&mut s, &press(Vec2::new(100.0, 100.0)));
        ed.handle_pointer(&mut s, &release_at(Vec2::new(100.0, 100.0)));
        let mut add = press(Vec2::new(-300.0, 50.0));
        add.additive = true;
        ed.handle_pointer(&mut s, &add);
        ed.handle_pointer(&mut s, &release_at(Vec2::new(-300.0, 50.0)));
        assert_eq!(ed.selection().len(), 2);

        // A 를 잡고 끌면 B 도 같은 만큼 따라온다
        drag(
            &mut ed,
            &mut s,
            Vec2::new(100.0, 100.0),
            Vec2::new(110.0, 130.0),
        );
        assert_eq!(s.item(a).unwrap().pos, Vec2::new(110.0, 130.0));
        assert_eq!(s.item(b).unwrap().pos, Vec2::new(-290.0, 80.0));

        // 한 번의 드래그 = 한 번의 언두
        ed.undo(&mut s);
        assert_eq!(s.item(a).unwrap().pos, Vec2::new(100.0, 100.0));
        assert_eq!(s.item(b).unwrap().pos, Vec2::new(-300.0, 50.0));
    }

    #[test]
    fn shift_click_on_selected_deselects_without_dragging() {
        let (mut s, a, _) = scene();
        let mut ed = Editing::default();
        ed.select(Target::Item(a), false);

        let mut p = press(Vec2::new(100.0, 100.0));
        p.additive = true;
        ed.handle_pointer(&mut s, &p);
        assert!(ed.selection().is_empty());
        assert!(!ed.is_dragging());
    }

    #[test]
    fn snap_aligns_anchor_to_grid() {
        let (mut s, a, _) = scene();
        let mut ed = Editing::default();

        ed.handle_pointer(&mut s, &press(Vec2::new(100.0, 100.0)));
        let mut m = move_to(Vec2::new(237.0, 162.0));
        m.snap = true;
        ed.handle_pointer(&mut s, &m);
        assert_eq!(s.item(a).unwrap().pos, Vec2::new(200.0, 200.0));
    }

    #[test]
    fn zone_handle_drag_resizes_and_undoes() {
        let (mut s, _, _) = scene();
        let mut ed = Editing::default();

        drag(
            &mut ed,
            &mut s,
            Vec2::new(1000.0, 1000.0),
            Vec2::new(1500.0, 1200.0),
        );
        assert_eq!(ed.selection(), &[Target::Zone]);
        assert_eq!(s.zone.max, Vec2::new(1500.0, 1200.0));
        assert_eq!(ed.history().undo_label().as_deref(), Some("존 경계 변경"));

        ed.undo(&mut s);
        assert_eq!(s.zone.max, Vec2::new(1000.0, 1000.0));
    }

    #[test]
    fn new_edit_clears_redo() {
        let (mut s, _, _) = scene();
        let mut ed = Editing::default();
        drag(
            &mut ed,
            &mut s,
            Vec2::new(100.0, 100.0),
            Vec2::new(150.0, 100.0),
        );
        ed.undo(&mut s);
        assert!(ed.history().redo_label().is_some());

        drag(
            &mut ed,
            &mut s,
            Vec2::new(-300.0, 50.0),
            Vec2::new(-200.0, 50.0),
        );
        assert!(
            ed.history().redo_label().is_none(),
            "새 편집 후 리두는 사라져야 한다"
        );
    }

    #[test]
    fn undo_during_drag_restores_and_cancels() {
        let (mut s, a, _) = scene();
        let mut ed = Editing::default();
        ed.handle_pointer(&mut s, &press(Vec2::new(100.0, 100.0)));
        ed.handle_pointer(&mut s, &move_to(Vec2::new(400.0, 400.0)));

        ed.undo(&mut s);
        assert_eq!(s.item(a).unwrap().pos, Vec2::new(100.0, 100.0));
        assert!(!ed.is_dragging());

        // 이어서 떼도 아무것도 기록되지 않는다
        ed.handle_pointer(&mut s, &release_at(Vec2::new(400.0, 400.0)));
        assert!(ed.history().undo_label().is_none());
    }

    #[test]
    fn inspector_edit_records_once_when_finished() {
        let (mut s, a, _) = scene();
        let mut ed = Editing::default();

        // 드래그 값 편집: 중간 값 여러 번 → 마지막에 finished
        for x in [110.0, 130.0, 160.0] {
            ed.apply_inspector(
                &mut s,
                InspectorEdit::ItemPos {
                    entity: a,
                    pos: Vec2::new(x, 100.0),
                    finished: false,
                },
            );
        }
        ed.apply_inspector(
            &mut s,
            InspectorEdit::ItemPos {
                entity: a,
                pos: Vec2::new(160.0, 100.0),
                finished: true,
            },
        );

        ed.undo(&mut s);
        assert_eq!(
            s.item(a).unwrap().pos,
            Vec2::new(100.0, 100.0),
            "언두 한 번에 편집 시작 전 값으로 돌아가야 한다"
        );
        assert!(ed.history().undo_label().is_none());
    }

    #[test]
    fn history_is_capped() {
        let (mut s, a, _) = scene();
        let mut ed = Editing::default();
        for i in 0..(HISTORY_LIMIT + 10) {
            ed.apply_inspector(
                &mut s,
                InspectorEdit::ItemPos {
                    entity: a,
                    pos: Vec2::new(i as f32, 0.0),
                    finished: true,
                },
            );
        }
        assert_eq!(ed.history.undo.len(), HISTORY_LIMIT);
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn rotate_handle_drag_turns_single_selection() {
        let (mut s, a, _) = scene();
        let mut ed = Editing::default();
        ed.select(Target::Item(a), false);

        // 방향 0 → 핸들은 마커 오른쪽(+X)에 있다
        let handle = crate::scene::rotate_handle_pos(s.item(a).unwrap(), PX);
        assert_eq!(
            ed.handle_pointer(&mut s, &move_to(handle)),
            Some(Pick::RotateHandle(a))
        );

        // 핸들을 마커 위쪽(+Y)으로 끌면 방향 90°
        drag(&mut ed, &mut s, handle, Vec2::new(100.0, 300.0));
        assert!(close(s.item(a).unwrap().orientation, PI / 2.0));
        assert_eq!(
            s.item(a).unwrap().pos,
            Vec2::new(100.0, 100.0),
            "위치는 그대로"
        );
        assert_eq!(ed.history().undo_label().as_deref(), Some("회전"));

        ed.undo(&mut s);
        assert!(close(s.item(a).unwrap().orientation, 0.0));
    }

    #[test]
    fn rotate_handle_only_for_single_selection() {
        let (mut s, a, b) = scene();
        let mut ed = Editing::default();
        ed.select(Target::Item(a), false);
        ed.select(Target::Item(b), true);
        assert_eq!(ed.rotatable(), None);

        let handle = crate::scene::rotate_handle_pos(s.item(a).unwrap(), PX);
        assert_ne!(
            ed.handle_pointer(&mut s, &move_to(handle)),
            Some(Pick::RotateHandle(a))
        );
    }

    #[test]
    fn rotate_snap_is_15_degrees() {
        let (mut s, a, _) = scene();
        let mut ed = Editing::default();
        ed.select(Target::Item(a), false);
        let handle = crate::scene::rotate_handle_pos(s.item(a).unwrap(), PX);

        ed.handle_pointer(&mut s, &press(handle));
        // 중심에서 약 37° 방향
        let mut m =
            move_to(Vec2::new(100.0, 100.0) + units::heading_to_dir(37f32.to_radians()) * 50.0);
        m.snap = true;
        ed.handle_pointer(&mut s, &m);
        assert!(close(s.item(a).unwrap().orientation, 30f32.to_radians()));
    }

    #[test]
    fn heading_edit_normalizes_and_records_once() {
        let (mut s, a, _) = scene();
        let mut ed = Editing::default();
        for deg in [-10.0f32, -45.0, -90.0] {
            ed.apply_inspector(
                &mut s,
                InspectorEdit::ItemHeading {
                    entity: a,
                    heading: deg.to_radians(),
                    finished: deg == -90.0,
                },
            );
        }
        // -90° → 270°
        assert!(close(s.item(a).unwrap().orientation, 1.5 * PI));
        ed.undo(&mut s);
        assert!(close(s.item(a).unwrap().orientation, 0.0));
        assert!(ed.history().undo_label().is_none());
    }

    #[test]
    fn state_id_tracks_undo_back_to_the_saved_point() {
        let (mut s, _, _) = scene();
        let mut ed = Editing::default();
        let saved = ed.history().state_id();

        ed.add_item(&mut s, ItemKind::Npc, Vec2::ZERO);
        let one = ed.history().state_id();
        assert_ne!(one, saved, "편집했는데 상태가 같다");

        ed.undo(&mut s);
        assert_eq!(ed.history().state_id(), saved, "저장 시점으로 되돌아왔다");
        ed.redo(&mut s);
        assert_eq!(ed.history().state_id(), one);

        // 되돌린 뒤 다른 편집을 하면, 개수가 같아도 다른 상태다.
        ed.undo(&mut s);
        ed.add_item(&mut s, ItemKind::Monster, Vec2::ZERO);
        assert_ne!(ed.history().state_id(), one);
        assert_ne!(ed.history().state_id(), saved);
    }

    #[test]
    fn state_id_survives_history_trimming() {
        let (mut s, _, _) = scene();
        let mut ed = Editing::default();
        for _ in 0..HISTORY_LIMIT + 5 {
            ed.add_item(&mut s, ItemKind::Npc, Vec2::ZERO);
        }
        let top = ed.history().state_id();
        while ed.undo(&mut s) {}
        // 버린 기록이 있으니 "처음 상태(0)" 로 돌아간 것이 아니다.
        assert_ne!(ed.history().state_id(), 0);
        assert_ne!(ed.history().state_id(), top);
    }

    #[test]
    fn add_then_undo_redo_keeps_same_entity() {
        let (mut s, _, _) = scene();
        let mut ed = Editing::default();
        let e = ed.add_item(&mut s, ItemKind::Npc, Vec2::new(5.0, 5.0));
        assert_eq!(s.item(e).unwrap().name, "NPC #1");
        assert_eq!(ed.selection(), &[Target::Item(e)]);

        ed.undo(&mut s);
        assert!(s.item(e).is_none());
        assert!(ed.selection().is_empty(), "사라진 항목은 선택에서도 빠진다");

        ed.redo(&mut s);
        assert_eq!(
            s.item(e).unwrap().pos,
            Vec2::new(5.0, 5.0),
            "같은 핸들로 되살아난다"
        );
    }

    #[test]
    fn new_names_fill_the_lowest_free_number() {
        let (mut s, _, _) = scene();
        let mut ed = Editing::default();
        let first = ed.add_item(&mut s, ItemKind::Monster, Vec2::ZERO);
        ed.add_item(&mut s, ItemKind::Monster, Vec2::ZERO);
        ed.select(Target::Item(first), false);
        ed.delete_selected(&mut s);
        let again = ed.add_item(&mut s, ItemKind::Monster, Vec2::ZERO);
        assert_eq!(s.item(again).unwrap().name, "몬스터 #1");
    }

    #[test]
    fn delete_restores_original_order_on_undo() {
        let (mut s, a, b) = scene();
        let c = s.add("C", ItemKind::Npc, Vec2::new(0.0, -300.0), 20.0);
        let mut ed = Editing::default();
        // A 와 C 를 지운다 (가운데 B 는 남김). 선택 순서를 목록 순서와 다르게.
        ed.select(Target::Item(c), false);
        ed.select(Target::Item(a), true);
        ed.select(Target::Zone, true);

        assert!(ed.delete_selected(&mut s));
        assert_eq!(s.items.iter().map(|i| i.entity).collect::<Vec<_>>(), [b]);
        assert_eq!(
            ed.selection(),
            &[Target::Zone],
            "존은 지워지지 않고 선택에 남는다"
        );
        assert_eq!(ed.history().undo_label().as_deref(), Some("2개 삭제"));

        ed.undo(&mut s);
        assert_eq!(
            s.items.iter().map(|i| i.entity).collect::<Vec<_>>(),
            [a, b, c],
            "원래 자리로 돌아와야 그리기·피킹 순서가 유지된다"
        );
    }

    #[test]
    fn delete_with_nothing_selected_records_nothing() {
        let (mut s, _, _) = scene();
        let mut ed = Editing::default();
        ed.select(Target::Zone, false);
        assert!(!ed.delete_selected(&mut s));
        assert!(ed.history().undo_label().is_none());
    }

    #[test]
    fn box_drag_on_empty_space_selects_overlapping_markers() {
        let (mut s, a, b) = scene();
        let mut ed = Editing::default();

        // A(100,100) 만 감싸는 박스
        ed.handle_pointer(&mut s, &press(Vec2::new(50.0, 50.0)));
        ed.handle_pointer(&mut s, &move_to(Vec2::new(150.0, 150.0)));
        assert_eq!(ed.selection(), &[Target::Item(a)], "끄는 중에도 반영된다");
        assert!(ed.box_rect().is_some());
        ed.handle_pointer(&mut s, &release_at(Vec2::new(150.0, 150.0)));
        assert!(ed.box_rect().is_none());
        assert!(ed.history().undo_label().is_none(), "선택은 편집이 아니다");

        // Shift 박스는 기존 선택에 더한다
        let mut p = press(Vec2::new(-350.0, 0.0));
        p.additive = true;
        ed.handle_pointer(&mut s, &p);
        ed.handle_pointer(&mut s, &release_at(Vec2::new(-250.0, 100.0)));
        assert_eq!(ed.selection(), &[Target::Item(a), Target::Item(b)]);
    }

    #[test]
    fn shrinking_box_drops_markers_again() {
        let (mut s, a, _) = scene();
        let mut ed = Editing::default();
        ed.handle_pointer(&mut s, &press(Vec2::new(50.0, 50.0)));
        ed.handle_pointer(&mut s, &move_to(Vec2::new(150.0, 150.0)));
        assert!(ed.is_selected(Target::Item(a)));
        ed.handle_pointer(&mut s, &move_to(Vec2::new(60.0, 60.0)));
        assert!(ed.selection().is_empty());
    }
}
