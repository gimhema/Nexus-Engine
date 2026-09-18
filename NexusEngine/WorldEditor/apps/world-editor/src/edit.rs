//! 편집 상태 — 선택, 드래그, 스냅, 언두/리두.
//!
//! UI 는 [`PointerInput`] / [`InspectorEdit`] 같은 "무슨 일이 있었는가"만 넘기고,
//! 씬을 실제로 바꾸는 것은 전부 이 모듈이다. 그래서 egui 없이 테스트할 수 있다.
//!
//! # 언두 단위
//!
//! - 뷰포트 드래그 한 번 = 1 스텝 (누를 때 원래 값을 잡아 두고, 뗄 때 기록)
//! - 인스펙터 값 편집 한 번 = 1 스텝 (드래그가 끝나거나 입력칸 포커스를 잃을 때 기록)
//! - 움직이지 않은 클릭은 기록하지 않는다

use nexus_core::{Entity, Vec2};

use crate::scene::{Handle, Pick, Scene, Target, ZoneBounds};

/// 언두 기록 상한. 오래된 것부터 버린다.
const HISTORY_LIMIT: usize = 256;

/// 되돌릴 수 있는 편집 하나.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Command {
    /// 대상별 (원래 위치, 새 위치).
    MoveItems(Vec<(Entity, Vec2, Vec2)>),
    SetZone {
        from: ZoneBounds,
        to: ZoneBounds,
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
            Self::SetZone { to, .. } => scene.zone = *to,
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
            Self::SetZone { from, .. } => scene.zone = *from,
        }
    }

    fn is_noop(&self) -> bool {
        match self {
            Self::MoveItems(moves) => moves.iter().all(|&(_, from, to)| from == to),
            Self::SetZone { from, to } => from == to,
        }
    }

    /// 사람이 읽을 요약 (메뉴·상태 바용).
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::MoveItems(moves) if moves.len() == 1 => String::from("이동"),
            Self::MoveItems(moves) => format!("{}개 이동", moves.len()),
            Self::SetZone { .. } => String::from("존 경계 변경"),
        }
    }
}

/// 언두/리두 스택.
#[derive(Debug, Default)]
pub(crate) struct History {
    undo: Vec<Command>,
    redo: Vec<Command>,
}

impl History {
    /// 이미 씬에 적용된 편집을 기록한다. 아무것도 바뀌지 않았으면 무시한다.
    fn record(&mut self, cmd: Command) {
        if cmd.is_noop() {
            return;
        }
        self.redo.clear();
        self.undo.push(cmd);
        if self.undo.len() > HISTORY_LIMIT {
            self.undo.remove(0);
        }
    }

    pub(crate) fn undo_label(&self) -> Option<String> {
        self.undo.last().map(Command::describe)
    }

    pub(crate) fn redo_label(&self) -> Option<String> {
        self.redo.last().map(Command::describe)
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
    /// Ctrl — 그리드 스냅.
    pub(crate) snap: bool,
    /// 피킹 허용 오차 (월드 단위).
    pub(crate) tolerance: f32,
    /// 스냅 간격 (월드 단위, 보통 현재 그리드 간격).
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
    Zone {
        handle: Handle,
        original: ZoneBounds,
    },
}

/// 진행 중인 인스펙터 편집의 원래 값.
#[derive(Clone, Copy, Debug)]
enum LiveEdit {
    Item(Entity, Vec2),
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

    pub(crate) fn is_dragging(&self) -> bool {
        self.drag.is_some()
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

    // ── 언두 / 리두 ──────────────────────────────────────────────────────

    pub(crate) fn undo(&mut self, scene: &mut Scene) -> bool {
        self.cancel_pending(scene);
        let Some(cmd) = self.history.undo.pop() else {
            return false;
        };
        cmd.revert(scene);
        self.history.redo.push(cmd);
        true
    }

    pub(crate) fn redo(&mut self, scene: &mut Scene) -> bool {
        self.cancel_pending(scene);
        let Some(cmd) = self.history.redo.pop() else {
            return false;
        };
        cmd.apply(scene);
        self.history.undo.push(cmd);
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
            Some(Drag::Zone { original, .. }) => scene.zone = original,
            None => {}
        }
        match self.live.take() {
            Some(LiveEdit::Item(e, pos)) => {
                if let Some(item) = scene.item_mut(e) {
                    item.pos = pos;
                }
            }
            Some(LiveEdit::Zone(z)) => scene.zone = z,
            None => {}
        }
    }

    // ── 뷰포트 포인터 ────────────────────────────────────────────────────

    /// 뷰포트 포인터 입력을 처리한다. 반환값은 포인터 아래의 대상(호버 표시용).
    pub(crate) fn handle_pointer(
        &mut self,
        scene: &mut Scene,
        input: &PointerInput,
    ) -> Option<Pick> {
        // 패널 위에서는 호버하지 않는다 — 패널 뒤에 가려진 마커가 강조되지 않도록.
        let hover = input
            .world
            .filter(|_| input.over_viewport)
            .and_then(|w| scene.pick(w, input.tolerance));

        if input.pressed
            && let Some(w) = input.world
        {
            self.begin_press(scene, hover, w, input.additive);
        }

        if let (Some(w), Some(drag)) = (input.world, &self.drag) {
            let drag = drag.clone();
            update_drag(scene, &drag, w, input.snap.then_some(input.grid));
        }

        if input.released {
            self.finish_drag(scene);
        }

        hover
    }

    fn begin_press(&mut self, scene: &Scene, hover: Option<Pick>, cursor: Vec2, additive: bool) {
        match hover {
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
                if !additive {
                    self.clear_selection();
                }
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
            Drag::Zone { original, .. } => Command::SetZone {
                from: original,
                to: scene.zone,
            },
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
                    Some(LiveEdit::Item(e, orig)) if e == entity => orig,
                    _ => item.pos,
                };
                item.pos = pos;
                if finished {
                    self.live = None;
                    self.history
                        .record(Command::MoveItems(vec![(entity, original, pos)]));
                } else {
                    self.live = Some(LiveEdit::Item(entity, original));
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

/// 드래그 중인 대상을 커서 위치에 맞춰 옮긴다. `snap` 이 있으면 그 간격으로 맞춘다.
fn update_drag(scene: &mut Scene, drag: &Drag, cursor: Vec2, snap: Option<f32>) {
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
            if let Some(step) = snap {
                anchor_to = snap_to(anchor_to, step);
            }
            let delta = anchor_to - anchor_from;
            for &(e, from) in originals {
                if let Some(item) = scene.item_mut(e) {
                    item.pos = from + delta;
                }
            }
        }
        Drag::Zone { handle, original } => {
            let to = snap.map_or(cursor, |step| snap_to(cursor, step));
            scene.zone = original.with_handle_moved(*handle, to);
        }
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

    const TOL: f32 = 2.0;

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
            tolerance: TOL,
            grid: 100.0,
            ..Default::default()
        }
    }

    fn move_to(at: Vec2) -> PointerInput {
        PointerInput {
            world: Some(at),
            over_viewport: true,
            tolerance: TOL,
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
}
