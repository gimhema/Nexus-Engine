//! 스토리보드 — 퀘스트 선행 관계와 플래그를 **켜는 곳·읽는 곳**을 한 그래프로 (R8).
//!
//! ```text
//!   NPC 상인 ──담당──▶ 퀘스트 1 ──선행──▶ 퀘스트 2 ◀──담당── NPC 대장장이
//!   트리거 4 ──시작──▶ 퀘스트 3
//!   트리거 1 ──켬──▶ 플래그 C1 ──읽음──▶ 트리거 2
//!   레벨 dungeon(입장 조건) ◀──읽음── 퀘스트 …
//! ```
//!
//! **데이터를 따로 갖지 않는다** — `rules.ron`·`levels/` 를 읽어 그릴 뿐이다. 고치는 곳은 각 디자이너다.
//! 판정 어휘가 한 벌(R1 의 `Condition`/`Effect`)이라 이 모듈은 조건·효과를 훑기만 하면 된다:
//!
//! - 조건이 **읽는 것**: `Flag` → 그 플래그, `Quest` → 그 퀘스트. 읽는 쪽 = 트리거의 `when` · 퀘스트의 받을 조건·조건 목표 ·
//!   스킬의 습득 조건 · 제작 레시피 조건 · 레벨의 입장 조건·클리어 조건 목표.
//! - 효과가 **켜는 것**: `SetFlag`/`AddFlag` → 그 플래그, `StartQuest` → 그 퀘스트. 켜는 쪽 = 트리거의 `then` ·
//!   퀘스트의 단계 효과·보상 · 레벨의 클리어 보상.
//! - NPC 기능 `Quest(N)` → 그 NPC 가 퀘스트를 **담당**한다.
//!
//! 덤으로 **끊긴 곳**을 알려 준다 — 아무도 켜지 않는데 읽히는 플래그(영영 거짓), 켜기만 하고 아무도 읽지 않는 플래그.

use std::collections::{BTreeMap, BTreeSet};

use nexus_render_wgpu::egui;
use nexus_sim::{Condition, Effect, FlagScope, Objective, Service};

use crate::game_data::GameData;
use crate::level::Levels;

/// 그래프의 점.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Node {
    Npc(u32),
    Trigger(u32),
    Quest(u32),
    /// `(범위, 번호)` — 범위는 0 캐릭터 · 1 월드 · 2 인스턴스.
    Flag(u8, u32),
    Skill(u32),
    /// `(제작 표, 레시피)`.
    Recipe(u32, u32),
    Level(String),
}

impl Node {
    /// 열 순서의 기준 — 주는 쪽(NPC·트리거)이 왼쪽, 받는 쪽(스킬·레벨)이 오른쪽.
    fn rank(&self) -> u32 {
        match self {
            Self::Npc(_) => 0,
            Self::Trigger(_) => 1,
            Self::Quest(_) => 2,
            Self::Flag(..) => 3,
            Self::Skill(_) | Self::Recipe(..) | Self::Level(_) => 4,
        }
    }

    fn color(&self) -> egui::Color32 {
        match self {
            Self::Npc(_) => egui::Color32::from_rgb(90, 150, 90),
            Self::Trigger(_) => egui::Color32::from_rgb(170, 120, 60),
            Self::Quest(_) => egui::Color32::from_rgb(70, 110, 180),
            Self::Flag(..) => egui::Color32::from_rgb(140, 90, 160),
            Self::Skill(_) | Self::Recipe(..) | Self::Level(_) => {
                egui::Color32::from_rgb(90, 90, 100)
            }
        }
    }
}

/// 선의 뜻.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Link {
    /// NPC 가 퀘스트를 준다·받는다.
    Gives,
    /// 퀘스트·트리거를 시작한다 (`StartQuest`).
    Starts,
    /// 플래그를 켠다·바꾼다 (`SetFlag`/`AddFlag`).
    Writes,
    /// 조건이 읽는다 (`Flag`·`Quest` — 선행 퀘스트 포함). 선은 **읽히는 것 → 읽는 쪽**.
    Reads,
}

impl Link {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Gives => "담당",
            Self::Starts => "시작",
            Self::Writes => "켬",
            Self::Reads => "읽음",
        }
    }

    fn color(self) -> egui::Color32 {
        match self {
            Self::Gives => egui::Color32::from_rgb(120, 200, 120),
            Self::Starts => egui::Color32::from_rgb(240, 190, 90),
            Self::Writes => egui::Color32::from_rgb(220, 120, 220),
            Self::Reads => egui::Color32::from_rgb(140, 170, 230),
        }
    }
}

/// 모은 그래프.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Storyboard {
    /// 점과 그 이름.
    pub(crate) nodes: BTreeMap<Node, String>,
    /// `(보내는 쪽, 받는 쪽, 뜻)` — 중복 없음.
    pub(crate) edges: BTreeSet<(Node, Node, Link)>,
}

fn scope_code(scope: FlagScope) -> u8 {
    match scope {
        FlagScope::Character => 0,
        FlagScope::World => 1,
        FlagScope::Instance => 2,
    }
}

fn flag_name(scope: u8, flag: u32) -> String {
    let s = match scope {
        0 => "캐릭터",
        1 => "월드",
        _ => "인스턴스",
    };
    format!("{s} 플래그 {flag}")
}

impl Storyboard {
    /// 게임 데이터와 레벨에서 모은다.
    pub(crate) fn collect(data: &GameData, levels: &Levels) -> Self {
        let mut b = Self::default();
        for (id, t) in data.triggers() {
            let me = Node::Trigger(id.0);
            b.node(me.clone(), format!("트리거 #{}", id.0));
            b.reads(&t.when, &me, data);
            b.writes(&t.then, &me, data);
        }
        for (id, q) in data.quests() {
            let me = Node::Quest(id.0);
            b.node(
                me.clone(),
                format!("퀘스트 #{} {}", id.0, data.quest_title(*id)),
            );
            b.reads(&q.requires, &me, data);
            for stage in &q.stages {
                for o in &stage.objectives {
                    if let Objective::Check(c) = o {
                        b.reads(c, &me, data);
                    }
                }
                b.writes(&stage.then, &me, data);
            }
            b.writes(&q.rewards, &me, data);
        }
        for (actor, list) in data.services() {
            for s in list {
                if let Service::Quest(q) = s {
                    let npc = Node::Npc(actor.raw());
                    b.node(
                        npc.clone(),
                        data.actor_type_name(nexus_sim::ActorType(actor.raw())),
                    );
                    b.quest(q.0, data);
                    b.edge(npc, Node::Quest(q.0), Link::Gives);
                }
            }
        }
        for (id, rules) in data.skill_rules() {
            if rules.requires != Condition::Always {
                let me = Node::Skill(id.0);
                b.node(me.clone(), format!("스킬 {}", data.skill_name(*id)));
                b.reads(&rules.requires, &me, data);
            }
        }
        for (id, craft) in data.crafts() {
            for (i, r) in craft.recipes.iter().enumerate() {
                if r.requires != Condition::Always {
                    let me = Node::Recipe(id.0, i as u32);
                    b.node(
                        me.clone(),
                        format!("제작 #{} {}", id.0, data.item_name(r.output)),
                    );
                    b.reads(&r.requires, &me, data);
                }
            }
        }
        let entry = levels.entry_conditions();
        for (id, name) in levels.list() {
            let me = Node::Level(id.clone());
            let label = format!("레벨 {name}");
            if let Some(c) = entry.get(&id) {
                b.node(me.clone(), label.clone());
                b.reads(c, &me, data);
            }
            if let Some(clear) = levels.clear_of(&id) {
                b.node(me.clone(), label);
                for o in &clear.objectives {
                    if let Objective::Check(c) = o {
                        b.reads(c, &me, data);
                    }
                }
                b.writes(&clear.rewards, &me, data);
            }
        }
        b
    }

    fn node(&mut self, n: Node, label: String) {
        self.nodes.entry(n).or_insert(label);
    }

    fn quest(&mut self, q: u32, data: &GameData) {
        let id = nexus_sim::QuestId(q);
        self.node(
            Node::Quest(q),
            format!("퀘스트 #{q} {}", data.quest_title(id)),
        );
    }

    fn edge(&mut self, from: Node, to: Node, link: Link) {
        if from != to {
            self.edges.insert((from, to, link));
        }
    }

    /// 조건이 읽는 플래그·퀘스트 → `reader`.
    fn reads(&mut self, c: &Condition, reader: &Node, data: &GameData) {
        match c {
            Condition::Flag { scope, flag, .. } => {
                let f = Node::Flag(scope_code(*scope), flag.0);
                self.node(f.clone(), flag_name(scope_code(*scope), flag.0));
                self.edge(f, reader.clone(), Link::Reads);
            }
            Condition::Quest { quest, .. } => {
                self.quest(quest.0, data);
                self.edge(Node::Quest(quest.0), reader.clone(), Link::Reads);
            }
            Condition::All(list) | Condition::Any(list) => {
                for c in list {
                    self.reads(c, reader, data);
                }
            }
            Condition::Not(c) => self.reads(c, reader, data),
            _ => {}
        }
    }

    /// 효과가 켜는 플래그·시작하는 퀘스트 ← `writer`.
    fn writes(&mut self, effects: &[Effect], writer: &Node, data: &GameData) {
        for e in effects {
            match *e {
                Effect::SetFlag { scope, flag, .. } | Effect::AddFlag { scope, flag, .. } => {
                    let f = Node::Flag(scope_code(scope), flag.0);
                    self.node(f.clone(), flag_name(scope_code(scope), flag.0));
                    self.edge(writer.clone(), f, Link::Writes);
                }
                Effect::StartQuest(q) => {
                    self.quest(q.0, data);
                    self.edge(writer.clone(), Node::Quest(q.0), Link::Starts);
                }
                _ => {}
            }
        }
    }

    /// 끊긴 곳 — 읽히는데 아무도 켜지 않는 플래그(영영 0), 켜기만 하고 아무도 읽지 않는 플래그.
    /// 퀘스트는 NPC·트리거가 시작하지 않으면 받을 길이 없다 (선행 퀘스트로 읽히기만 하는 것도).
    pub(crate) fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        let has = |n: &Node, link: Link, incoming: bool| {
            self.edges
                .iter()
                .any(|(a, b, l)| *l == link && if incoming { b == n } else { a == n })
        };
        for (n, label) in &self.nodes {
            match n {
                Node::Flag(..) => {
                    let written = has(n, Link::Writes, true);
                    let read = has(n, Link::Reads, false);
                    if read && !written {
                        out.push(format!("{label} — 읽히는데 켜는 곳이 없다 (늘 0)"));
                    }
                    if written && !read {
                        out.push(format!("{label} — 켜기만 하고 읽는 곳이 없다"));
                    }
                }
                Node::Quest(_) if !has(n, Link::Gives, true) && !has(n, Link::Starts, true) => {
                    out.push(format!(
                        "{label} — 담당 NPC 도 시작하는 트리거도 없다 (받을 길이 없다)"
                    ));
                }
                _ => {}
            }
        }
        out
    }

    /// 고리를 닫는 선 — 깊이 우선으로 훑다가 지금 훑는 길 위의 점으로 돌아가는 선.
    /// "한 번만" 패턴(트리거가 플래그를 읽고 같은 플래그를 켬)이 늘 고리를 만든다.
    fn back_edges(&self) -> BTreeSet<(Node, Node, Link)> {
        let mut out = BTreeSet::new();
        // 0 = 아직, 1 = 훑는 중(길 위), 2 = 끝.
        let mut state: BTreeMap<&Node, u8> = BTreeMap::new();
        for root in self.nodes.keys() {
            if state.contains_key(root) {
                continue;
            }
            // (점, 다음에 볼 선 번호) — 재귀 대신 스택.
            let mut stack: Vec<(&Node, usize)> = vec![(root, 0)];
            state.insert(root, 1);
            while let Some((n, i)) = stack.pop() {
                let next = self.edges.iter().filter(|(a, _, _)| a == n).nth(i);
                let Some(edge @ (_, b, _)) = next else {
                    state.insert(n, 2);
                    continue;
                };
                stack.push((n, i + 1));
                match state.get(b).copied().unwrap_or(0) {
                    0 => {
                        state.insert(b, 1);
                        stack.push((b, 0));
                    }
                    1 => {
                        out.insert(edge.clone());
                    }
                    _ => {}
                }
            }
        }
        out
    }

    /// 열(깊이) 배치 — 선을 따라 오른쪽으로. 고리를 닫는 선은 배치에서 뺀다(안 빼면 고리를 돌며 끝없이 밀린다).
    fn columns(&self) -> BTreeMap<Node, u32> {
        let back = self.back_edges();
        let mut depth: BTreeMap<Node, u32> = self.nodes.keys().map(|n| (n.clone(), 0)).collect();
        for _ in 0..self.nodes.len() {
            let mut changed = false;
            for edge @ (a, b, _) in &self.edges {
                if back.contains(edge) {
                    continue;
                }
                let want = depth[a] + 1;
                if let Some(d) = depth.get_mut(b)
                    && *d < want
                {
                    *d = want;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        depth
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 창
// ─────────────────────────────────────────────────────────────────────────────

const NODE_W: f32 = 170.0;
const NODE_H: f32 = 30.0;
const COL_GAP: f32 = 70.0;
const ROW_GAP: f32 = 14.0;

/// 스토리보드 창.
#[derive(Debug, Default)]
pub(crate) struct StoryboardWindow {
    open: bool,
    board: Option<Storyboard>,
    error: Option<String>,
}

impl StoryboardWindow {
    pub(crate) fn open(&mut self) {
        self.open = true;
        self.reload();
    }

    fn reload(&mut self) {
        match GameData::load() {
            Ok(data) => {
                let (levels, _) = Levels::load();
                self.board = Some(Storyboard::collect(&data, &levels));
                self.error = None;
            }
            Err(e) => self.error = Some(format!("게임 데이터를 읽을 수 없음 — {e}")),
        }
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui) {
        let mut open = self.open;
        let mut reload = false;
        egui::Window::new("스토리보드")
            .open(&mut open)
            .default_size([980.0, 540.0])
            .resizable(true)
            .show(ui.ctx(), |ui| {
                ui.horizontal(|ui| {
                    if ui.button("다시 읽기").clicked() {
                        reload = true;
                    }
                    for link in [Link::Gives, Link::Starts, Link::Writes, Link::Reads] {
                        ui.colored_label(link.color(), format!("━ {}", link.label()));
                    }
                    ui.weak("· 점에 마우스를 올리면 그 선만 밝게");
                });
                if let Some(e) = &self.error {
                    ui.colored_label(egui::Color32::from_rgb(240, 110, 100), e);
                }
                let Some(board) = &self.board else { return };
                let warnings = board.warnings();
                if !warnings.is_empty() {
                    egui::CollapsingHeader::new(format!("끊긴 곳 {}개", warnings.len()))
                        .default_open(true)
                        .show(ui, |ui| {
                            for w in &warnings {
                                ui.colored_label(egui::Color32::from_rgb(240, 190, 90), w);
                            }
                        });
                }
                ui.separator();
                egui::ScrollArea::both().show(ui, |ui| graph(ui, board));
            });
        self.open = open;
        if reload {
            self.reload();
        }
    }
}

/// 그래프를 그린다 — 열은 깊이, 열 안은 종류·번호 순.
fn graph(ui: &mut egui::Ui, board: &Storyboard) {
    let depth = board.columns();
    let mut cols: BTreeMap<u32, Vec<&Node>> = BTreeMap::new();
    // 선이 하나도 없는 점(레벨만 보는 습득 조건 등)은 그리지 않는다 — 이야기 흐름에 끼지 않는다.
    let linked: BTreeSet<&Node> = board.edges.iter().flat_map(|(a, b, _)| [a, b]).collect();
    let hidden = board.nodes.len() - linked.len();
    if hidden > 0 {
        ui.weak(format!(
            "선이 없는 항목 {hidden}개는 숨겼다 (플래그·퀘스트를 읽거나 켜지 않는다)"
        ));
    }
    for n in linked {
        cols.entry(depth[n]).or_default().push(n);
    }
    for list in cols.values_mut() {
        list.sort_by_key(|n| (n.rank(), (*n).clone()));
    }
    let rows = cols.values().map(Vec::len).max().unwrap_or(0) as f32;
    let size = egui::vec2(
        cols.len() as f32 * (NODE_W + COL_GAP) + 20.0,
        rows * (NODE_H + ROW_GAP) + 20.0,
    );
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let mut at: BTreeMap<&Node, egui::Rect> = BTreeMap::new();
    for (ci, list) in cols.values().enumerate() {
        for (ri, n) in list.iter().enumerate() {
            let min = rect.min
                + egui::vec2(
                    10.0 + ci as f32 * (NODE_W + COL_GAP),
                    10.0 + ri as f32 * (NODE_H + ROW_GAP),
                );
            at.insert(
                n,
                egui::Rect::from_min_size(min, egui::vec2(NODE_W, NODE_H)),
            );
        }
    }
    let hovered = response
        .hover_pos()
        .and_then(|p| at.iter().find(|(_, r)| r.contains(p)).map(|(n, _)| *n));
    // 선 — 오른쪽 가장자리에서 왼쪽 가장자리로, 화살촉.
    for (a, b, link) in &board.edges {
        let (Some(ra), Some(rb)) = (at.get(a), at.get(b)) else {
            continue;
        };
        let lit = hovered.is_none_or(|h| h == a || h == b);
        let color = if lit {
            link.color()
        } else {
            link.color().gamma_multiply(0.15)
        };
        let (from, to) = if ra.center().x <= rb.center().x {
            (ra.right_center(), rb.left_center())
        } else {
            (ra.left_center(), rb.right_center())
        };
        let stroke = egui::Stroke::new(if lit { 2.0 } else { 1.0 }, color);
        painter.line_segment([from, to], stroke);
        let dir = (to - from).normalized();
        let side = egui::vec2(-dir.y, dir.x) * 4.0;
        let back = to - dir * 9.0;
        painter.add(egui::Shape::convex_polygon(
            vec![to, back + side, back - side],
            color,
            egui::Stroke::NONE,
        ));
    }
    for (n, r) in &at {
        let lit = hovered.is_none_or(|h| {
            h == *n
                || board
                    .edges
                    .iter()
                    .any(|(a, b, _)| (a == h && b == *n) || (b == h && a == *n))
        });
        let fill = if lit {
            n.color()
        } else {
            n.color().gamma_multiply(0.35)
        };
        painter.rect_filled(*r, 4.0, fill);
        let label = board.nodes.get(*n).map_or("", String::as_str);
        painter.text(
            r.left_center() + egui::vec2(6.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(12.0),
            egui::Color32::WHITE,
        );
    }
    if let Some(h) = hovered {
        let lines: Vec<String> = board
            .edges
            .iter()
            .filter(|(a, b, _)| a == h || b == h)
            .map(|(a, b, l)| {
                let name = |n: &Node| board.nodes.get(n).cloned().unwrap_or_default();
                format!("{} ─{}→ {}", name(a), l.label(), name(b))
            })
            .collect();
        if !lines.is_empty() {
            response.on_hover_text(lines.join("\n"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_data_draws_quest_chains_and_flags() {
        let (levels, _) = Levels::load();
        let b = Storyboard::collect(&GameData::embedded(), &levels);
        let has = |a: Node, z: Node, l: Link| b.edges.contains(&(a, z, l));
        // 퀘스트 2 는 퀘스트 1 완료가 선행 (R5 데이터 예).
        assert!(
            has(Node::Quest(1), Node::Quest(2), Link::Reads),
            "{:#?}",
            b.edges
        );
        // 상인(11)이 퀘스트 1, 대장장이(12)가 퀘스트 2 를 준다.
        assert!(has(Node::Npc(11), Node::Quest(1), Link::Gives));
        assert!(has(Node::Npc(12), Node::Quest(2), Link::Gives));
        // 트리거 4 가 퀘스트 3 을 시작한다.
        assert!(has(Node::Trigger(4), Node::Quest(3), Link::Starts));
        // 트리거 1 이 캐릭터 플래그 1 을 켜고 트리거 2 가 읽는다 (R1 데이터 예).
        assert!(has(Node::Trigger(1), Node::Flag(0, 1), Link::Writes));
        assert!(has(Node::Flag(0, 1), Node::Trigger(2), Link::Reads));
        // 모든 선의 양 끝은 점으로 있다.
        assert!(
            b.edges
                .iter()
                .all(|(a, z, _)| b.nodes.contains_key(a) && b.nodes.contains_key(z))
        );
        // 배치가 끝난다(고리가 있어도) — 선은 대체로 오른쪽으로.
        let depth = b.columns();
        assert!(depth[&Node::Quest(2)] > depth[&Node::Quest(1)]);
        // "한 번만" 고리(트리거 2 가 플래그 2 를 읽고 켬)가 열을 끝없이 밀지 않는다.
        assert!(depth.values().all(|d| *d < 6), "{depth:?}");
        assert_eq!(depth[&Node::Trigger(2)], depth[&Node::Flag(0, 1)] + 1);
    }

    #[test]
    fn dangling_flags_and_unreachable_quests_are_reported() {
        let mut b = Storyboard::default();
        let t = Node::Trigger(1);
        b.node(t.clone(), String::from("트리거 #1"));
        // 읽기만 하는 플래그 7, 켜기만 하는 플래그 8.
        b.node(Node::Flag(1, 7), flag_name(1, 7));
        b.edge(Node::Flag(1, 7), t.clone(), Link::Reads);
        b.node(Node::Flag(0, 8), flag_name(0, 8));
        b.edge(t, Node::Flag(0, 8), Link::Writes);
        b.node(Node::Quest(5), String::from("퀘스트 #5"));
        let w = b.warnings();
        assert!(
            w.iter()
                .any(|m| m.starts_with("월드 플래그 7") && m.contains("켜는 곳이 없다")),
            "{w:?}"
        );
        assert!(
            w.iter()
                .any(|m| m.starts_with("캐릭터 플래그 8") && m.contains("읽는 곳이 없다"))
        );
        assert!(
            w.iter()
                .any(|m| m.starts_with("퀘스트 #5") && m.contains("받을 길이 없다"))
        );
    }
}
