//! Nexus WorldEditor — 에디터 애플리케이션.
//!
//! 현재 단계: 2D 정사영 월드 뷰포트 + egui 에디터 골격.
//!   - 메뉴 바 / 씬 목록 / 인스펙터(빈 칸) / 상태 바
//!   - 뷰포트: 가운데·오른쪽 드래그 = 팬, 휠 = 커서 기준 줌, Home = 시점 초기화
//!   - F12 또는 `NEXUS_SCREENSHOT` = 스크린샷
//!
//! 화면에 보이는 배치는 NexusEngine `Server.cpp` 의 기본 존 설정을 옮겨온 것이다.
//! M5 에서 이를 마우스로 편집하고, M6 에서 `ZoneConfig` 로 내보낸다.

mod grid;
mod screenshot;
mod ui;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use nexus_core::{Camera2d, Vec2, World};
use nexus_platform::{App, Input, WindowConfig, WindowEvent, WindowTarget};
use nexus_render::{FrameStatus, RenderCommand, RenderError, Renderer};
use nexus_render_wgpu::{TextureCarry, UiFrame, WgpuRenderer};

use ui::{EditorUi, FrameStats, OutlineItem};

// 색은 모두 sRGB. 렌더러가 선형으로 변환한다.
const CLEAR_COLOR: [f32; 4] = [0.05, 0.06, 0.08, 1.0];
const ZONE_BOUNDS_COLOR: [f32; 4] = [0.35, 0.60, 0.95, 0.9];
const PLAYER_SPAWN_COLOR: [f32; 4] = [0.30, 0.85, 0.55, 1.0];
const NPC_COLOR: [f32; 4] = [0.90, 0.75, 0.30, 1.0];
const MONSTER_COLOR: [f32; 4] = [0.90, 0.35, 0.30, 1.0];

/// 서버 `ZoneConfig` 의 스폰 정의에 대응하는 에디터 측 표현.
///
/// M6 에서 이 구조가 씬 컴포넌트로 옮겨가고 `ZoneConfig` 로 직렬화된다.
#[derive(Clone, Debug)]
struct Marker {
    name: String,
    pos: Vec2,
    color: [f32; 4],
    /// 월드 크기 (cm).
    size: f32,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = WindowConfig::default();

    println!(
        "WorldEditor 시작 — {}x{}, 시뮬레이션 {}Hz",
        config.width, config.height, config.sim_hz
    );
    println!("조작: 가운데/오른쪽 드래그 = 팬 | 휠 = 줌 | Home = 시점 초기화 | F12 = 스크린샷");

    nexus_platform::run(config, Editor::default())?;

    println!("WorldEditor 종료");
    Ok(())
}

#[derive(Debug, Default)]
struct Editor {
    renderer: Option<WgpuRenderer>,
    ui: Option<EditorUi>,
    target: Option<WindowTarget>,
    /// 건너뛴 프레임에서 적용하지 못한 UI 텍스처 변경분.
    texture_carry: TextureCarry,

    world: World,
    camera: Camera2d,
    markers: Vec<Marker>,
    /// 존 경계 AABB (XY 평면 투영).
    zone_min: Vec2,
    zone_max: Vec2,
    /// 씬 목록 표시용 — 데이터가 바뀔 때만 다시 만든다.
    outline: Vec<OutlineItem>,
    /// 프레임마다 재사용하는 명령 버퍼 — 매 프레임 할당을 피한다.
    commands: Vec<RenderCommand>,

    // ── 통계 ─────────────────────────────────────────────────────────────
    ticks: u64,
    last_frame: Option<Instant>,
    fps: f32,

    // ── 스크린샷 ─────────────────────────────────────────────────────────
    frame_index: u64,
    auto_shot: Option<screenshot::AutoPlan>,
    manual_shot_requested: bool,
    /// 캡처를 요청해 두고 결과를 기다리는 중인 저장 경로. `true` 면 저장 후 종료.
    pending_shot: Option<(PathBuf, bool)>,
    exit_requested: bool,
}

impl Editor {
    /// 서버 기본 존 배치를 불러온다.
    ///
    /// `Server.cpp` 는 `boundsMin`/`boundsMax` 를 설정하지 않으므로
    /// `ZoneConfig` 구조체 기본값(±1000cm)이 실제로 쓰인다.
    fn load_default_zone(&mut self) {
        self.zone_min = Vec2::new(-1000.0, -1000.0);
        self.zone_max = Vec2::new(1000.0, 1000.0);

        let spawns = [
            // playerSpawnPoints
            (
                "플레이어 스폰 #1",
                Vec2::new(0.0, 0.0),
                PLAYER_SPAWN_COLOR,
                40.0,
            ),
            (
                "플레이어 스폰 #2",
                Vec2::new(5.0, 5.0),
                PLAYER_SPAWN_COLOR,
                40.0,
            ),
            // npcSpawns
            ("마을 경비병", Vec2::new(10.0, 10.0), NPC_COLOR, 30.0),
            ("상인 NPC", Vec2::new(-8.0, 12.0), NPC_COLOR, 30.0),
            ("슬라임", Vec2::new(20.0, -5.0), MONSTER_COLOR, 30.0),
        ];
        for (name, pos, color, size) in spawns {
            self.markers.push(Marker {
                name: name.to_owned(),
                pos,
                color,
                size,
            });
            self.world.spawn();
        }

        self.rebuild_outline();
    }

    fn rebuild_outline(&mut self) {
        self.outline.clear();
        self.outline.push(OutlineItem {
            label: String::from("존 경계"),
            color: ZONE_BOUNDS_COLOR,
        });
        self.outline
            .extend(self.markers.iter().map(|m| OutlineItem {
                label: m.name.clone(),
                color: m.color,
            }));
    }

    fn reset_view(&mut self) {
        self.camera.center = Vec2::ZERO;
        self.camera.view_height = 2600.0;
    }

    fn update_fps(&mut self) {
        let now = Instant::now();
        if let Some(prev) = self.last_frame.replace(now) {
            let dt = (now - prev).as_secs_f32();
            if dt > 0.0 {
                // 지수 평균 — 숫자가 매 프레임 튀지 않게
                let instant = 1.0 / dt;
                self.fps = if self.fps == 0.0 {
                    instant
                } else {
                    self.fps * 0.9 + instant * 0.1
                };
            }
        }
    }

    /// UI 를 한 프레임 실행하고, 그 결과(뷰포트 영역 등)를 편집기 상태에 반영한다.
    fn run_ui(&mut self) -> Option<(UiFrame, Option<[u32; 4]>)> {
        let (Some(ui), Some(target)) = (self.ui.as_mut(), self.target.as_ref()) else {
            return None;
        };

        let stats = FrameStats {
            fps: self.fps,
            quads: self
                .renderer
                .as_ref()
                .map_or(0, WgpuRenderer::last_quad_count),
            ticks: self.ticks,
        };

        let (mut frame, actions) = ui.run(target, &mut self.camera, &self.outline, stats);
        self.texture_carry.apply_to(&mut frame);

        if actions.reset_view {
            self.reset_view();
        }
        self.manual_shot_requested |= actions.screenshot;

        Some((frame, actions.viewport_px))
    }

    /// 이번 프레임에 그릴 씬 명령을 쌓는다.
    fn build_commands(&mut self, viewport: Option<[u32; 4]>) {
        self.commands.clear();
        self.commands
            .push(RenderCommand::Clear { color: CLEAR_COLOR });
        if let Some([x, y, width, height]) = viewport {
            self.commands.push(RenderCommand::SetViewport {
                x,
                y,
                width,
                height,
            });
        }
        self.commands.push(RenderCommand::SetCamera {
            view_proj: self.camera.view_proj(),
        });

        grid::build(&self.camera, &mut self.commands);

        // 존 경계 — 화면상 두께가 일정하도록 뷰 높이에 비례
        grid::build_outline(
            self.zone_min,
            self.zone_max,
            self.camera.view_height * 0.003,
            0.5,
            ZONE_BOUNDS_COLOR,
            &mut self.commands,
        );

        for m in &self.markers {
            self.commands.push(RenderCommand::DrawRect {
                center: m.pos,
                size: Vec2::splat(m.size),
                z: 1.0,
                color: m.color,
            });
        }
    }

    /// 이번 프레임에 캡처를 걸어야 하면 렌더러에 요청한다.
    fn schedule_capture(&mut self) {
        if self.pending_shot.is_some() {
            return;
        }

        let request = match &self.auto_shot {
            Some(plan) if self.frame_index >= plan.at_frame => Some((plan.path.clone(), true)),
            _ if self.manual_shot_requested => Some((screenshot::manual_path(), false)),
            _ => None,
        };
        let Some((path, exit_after)) = request else {
            return;
        };
        self.manual_shot_requested = false;
        if exit_after {
            self.auto_shot = None;
        }

        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        match renderer.request_capture() {
            Ok(()) => self.pending_shot = Some((path, exit_after)),
            Err(e) => {
                eprintln!("스크린샷 요청 실패: {e}");
                self.exit_requested |= exit_after;
            }
        }
    }

    /// 캡처 결과가 나왔으면 PNG 로 저장한다.
    fn collect_capture(&mut self) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        let Some(result) = renderer.take_capture() else {
            return;
        };
        let Some((path, exit_after)) = self.pending_shot.take() else {
            return;
        };

        match result.map_err(|e| e.to_string()).and_then(|capture| {
            screenshot::save_png(&path, &capture)
                .map(|()| capture)
                .map_err(|e| e.to_string())
        }) {
            Ok(capture) => println!(
                "스크린샷 저장 — {} ({}x{}, frame {})",
                path.display(),
                capture.width,
                capture.height,
                self.frame_index
            ),
            Err(e) => eprintln!("스크린샷 실패 ({}): {e}", path.display()),
        }

        self.exit_requested |= exit_after;
    }
}

impl App for Editor {
    fn init(&mut self, target: &WindowTarget) -> Result<(), String> {
        let (width, height) = target.size();

        // WindowTarget 은 Clone + Send + Sync + 'static 이므로 wgpu 서피스 타깃으로
        // 그대로 넘길 수 있다. 플랫폼 분기는 wgpu 내부에 있다.
        let renderer =
            WgpuRenderer::new(target.clone(), width, height).map_err(|e| e.to_string())?;

        let info = renderer.device_info();
        println!(
            "렌더러 초기화 — {} | {:?} | {} | {}x{} @ {:.2}x",
            info.name,
            info.backend,
            if info.is_integrated {
                "통합 GPU"
            } else {
                "외장 GPU"
            },
            width,
            height,
            target.scale_factor(),
        );

        let ui = EditorUi::new(target, renderer.max_texture_side());
        println!("{}", ui.font_note());

        self.camera.viewport = (width, height);
        self.reset_view();
        self.load_default_zone();
        self.renderer = Some(renderer);
        self.ui = Some(ui);
        self.target = Some(target.clone());

        println!(
            "기본 존 로드 — 경계 {:?}~{:?}cm, 마커 {}개",
            self.zone_min.to_array(),
            self.zone_max.to_array(),
            self.markers.len()
        );

        self.auto_shot = screenshot::plan_from_env();
        if let Some(plan) = &self.auto_shot {
            println!(
                "자동 스크린샷 — frame {} → {}",
                plan.at_frame,
                plan.path.display()
            );
        }
        Ok(())
    }

    fn window_event(&mut self, target: &WindowTarget, event: &WindowEvent) -> bool {
        self.ui
            .as_mut()
            .is_some_and(|ui| ui.on_window_event(target, event))
    }

    fn fixed_update(&mut self, _dt: Duration, _input: &Input) {
        self.ticks += 1;
        // M7 에서 Intent → Authority → World 경로가 여기에 들어온다.
        // 에디터 조작(카메라·단축키)은 뷰 조작이므로 UI 프레임에서 처리한다.
    }

    fn render(&mut self, _alpha: f32) {
        self.frame_index += 1;
        self.update_fps();

        let (mut ui_frame, viewport) = match self.run_ui() {
            Some((frame, viewport)) => (Some(frame), viewport),
            None => (None, None),
        };

        self.build_commands(viewport);
        self.schedule_capture();

        if let Some(renderer) = self.renderer.as_mut()
            && let Err(e) = draw(renderer, &self.commands, &mut ui_frame)
        {
            eprintln!("프레임 스킵: {e}");
        }

        // 그리지 못한 UI 프레임(최소화 등)의 텍스처 변경분은 다음 프레임으로 넘긴다.
        if let Some(frame) = ui_frame {
            self.texture_carry.stash(frame);
        }

        self.collect_capture();
    }

    fn resize(&mut self, width: u32, height: u32) {
        // 카메라 뷰포트는 UI 가 매 프레임 실제 씬 영역으로 다시 설정한다.
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.resize(width, height);
        }
    }

    fn should_exit(&self) -> bool {
        self.exit_requested
    }

    fn shutdown(&mut self) {
        println!(
            "종료 — 총 {} tick, 엔티티 {}, 마지막 프레임 쿼드 {}",
            self.ticks,
            self.world.entity_count(),
            self.renderer
                .as_ref()
                .map_or(0, WgpuRenderer::last_quad_count),
        );
        // egui 텍스처가 렌더러보다 먼저 사라지도록 UI 부터 정리한다.
        self.ui = None;
        // wgpu 는 Drop 에서 GPU 유휴 대기 후 자원을 해제한다.
        self.renderer = None;
    }
}

/// 한 프레임을 그린다. 프레임 생명주기를 한 곳에 모아 중간 반환 시에도
/// `end_frame` 누락이 눈에 띄게 한다.
///
/// `ui` 는 실제로 렌더러에 넘겼을 때만 비워진다. 프레임을 건너뛰면 그대로 남아
/// 호출자가 텍스처 변경분을 보관할 수 있다.
fn draw(
    renderer: &mut WgpuRenderer,
    commands: &[RenderCommand],
    ui: &mut Option<UiFrame>,
) -> Result<(), RenderError> {
    // 최소화·가려짐 상태는 오류가 아니라 정상적인 건너뛰기다.
    if renderer.begin_frame()? == FrameStatus::Skipped {
        return Ok(());
    }
    for &cmd in commands {
        renderer.submit(cmd)?;
    }
    if let Some(frame) = ui.take() {
        renderer.submit_ui(frame)?;
    }
    renderer.end_frame()
}
