//! Nexus WorldEditor — 에디터 애플리케이션.
//!
//! M4 단계: 2D 정사영 월드 뷰포트.
//!   - 팬(좌클릭 드래그) / 줌(휠, 커서 기준)
//!   - 줌 연동 그리드 + 월드 축
//!   - 존 경계(AABB), 플레이어 스폰, NPC 스폰 마커
//!
//! 화면에 보이는 배치는 NexusEngine `Server.cpp` 의 기본 존 설정을 옮겨온 것이다.
//! M5 에서 이를 마우스로 편집하고, M6 에서 `ZoneConfig` 로 내보낸다.

mod grid;

use std::time::Duration;

use nexus_core::{Camera2d, Vec2, World};
use nexus_platform::{App, Input, KeyCode, MouseButton, WindowConfig, WindowTarget};
use nexus_render::{FrameStatus, RenderCommand, RenderError, Renderer};
use nexus_render_wgpu::WgpuRenderer;

const CLEAR_COLOR: [f32; 4] = [0.05, 0.06, 0.08, 1.0];

const ZONE_BOUNDS_COLOR: [f32; 4] = [0.35, 0.60, 0.95, 0.9];
const PLAYER_SPAWN_COLOR: [f32; 4] = [0.30, 0.85, 0.55, 1.0];
const NPC_COLOR: [f32; 4] = [0.90, 0.75, 0.30, 1.0];
const MONSTER_COLOR: [f32; 4] = [0.90, 0.35, 0.30, 1.0];

/// 서버 `ZoneConfig` 의 스폰 정의에 대응하는 에디터 측 표현.
///
/// M6 에서 이 구조가 씬 컴포넌트로 옮겨가고 `ZoneConfig` 로 직렬화된다.
#[derive(Clone, Copy, Debug)]
struct Marker {
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
    println!("조작: 좌클릭 드래그 = 팬 | 휠 = 줌 | Home = 시점 초기화");

    nexus_platform::run(config, Editor::default())?;

    println!("WorldEditor 종료");
    Ok(())
}

#[derive(Debug, Default)]
struct Editor {
    renderer: Option<WgpuRenderer>,
    world: World,
    camera: Camera2d,
    markers: Vec<Marker>,
    /// 존 경계 AABB (XY 평면 투영).
    zone_min: Vec2,
    zone_max: Vec2,
    /// 프레임마다 재사용하는 명령 버퍼 — 매 프레임 할당을 피한다.
    commands: Vec<RenderCommand>,
    ticks: u64,
}

impl Editor {
    /// 서버 기본 존 배치를 불러온다.
    ///
    /// `Server.cpp` 는 `boundsMin`/`boundsMax` 를 설정하지 않으므로
    /// `ZoneConfig` 구조체 기본값(±1000cm)이 실제로 쓰인다.
    fn load_default_zone(&mut self) {
        self.zone_min = Vec2::new(-1000.0, -1000.0);
        self.zone_max = Vec2::new(1000.0, 1000.0);

        // playerSpawnPoints
        for pos in [Vec2::new(0.0, 0.0), Vec2::new(5.0, 5.0)] {
            self.markers.push(Marker {
                pos,
                color: PLAYER_SPAWN_COLOR,
                size: 40.0,
            });
            self.world.spawn();
        }

        // npcSpawns — 마을 경비병 / 상인 NPC / 슬라임
        for (pos, color) in [
            (Vec2::new(10.0, 10.0), NPC_COLOR),
            (Vec2::new(-8.0, 12.0), NPC_COLOR),
            (Vec2::new(20.0, -5.0), MONSTER_COLOR),
        ] {
            self.markers.push(Marker {
                pos,
                color,
                size: 30.0,
            });
            self.world.spawn();
        }
    }

    fn reset_view(&mut self) {
        self.camera.center = Vec2::ZERO;
        self.camera.view_height = 2600.0;
    }

    /// 이번 프레임에 그릴 명령을 쌓는다.
    fn build_commands(&mut self) {
        self.commands.clear();
        self.commands
            .push(RenderCommand::Clear { color: CLEAR_COLOR });
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

        self.camera.viewport = (width, height);
        self.reset_view();
        self.load_default_zone();
        self.renderer = Some(renderer);

        println!(
            "기본 존 로드 — 경계 {:?}~{:?}cm, 마커 {}개",
            self.zone_min.to_array(),
            self.zone_max.to_array(),
            self.markers.len()
        );
        Ok(())
    }

    fn fixed_update(&mut self, _dt: Duration, input: &Input) {
        self.ticks += 1;

        // M7 에서 이 자리가 Intent 생성으로 바뀐다.
        // 지금은 카메라 조작만 — 카메라는 시뮬레이션 상태가 아니라 뷰이므로
        // 나중에 Intent 계층을 거치지 않는다.
        if input.button_held(MouseButton::Left) {
            let (dx, dy) = input.cursor_delta();
            if dx != 0.0 || dy != 0.0 {
                self.camera.pan_by_pixels(Vec2::new(dx, dy));
            }
        }

        let wheel = input.wheel();
        if wheel.abs() > f32::EPSILON {
            let (cx, cy) = input.cursor();
            self.camera.zoom_at(wheel, Vec2::new(cx, cy));
        }

        if input.key_pressed(KeyCode::Home) {
            self.reset_view();
        }
    }

    fn render(&mut self, _alpha: f32) {
        self.build_commands();

        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };

        if let Err(e) = draw(renderer, &self.commands) {
            eprintln!("프레임 스킵: {e}");
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.camera.viewport = (width, height);
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.resize(width, height);
        }
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
        // wgpu 는 Drop 에서 GPU 유휴 대기 후 자원을 해제한다.
        self.renderer = None;
    }
}

/// 한 프레임을 그린다. 프레임 생명주기를 한 곳에 모아 중간 반환 시에도
/// `end_frame` 누락이 눈에 띄게 한다. (M5 에서 typestate 로 대체 검토)
fn draw(renderer: &mut WgpuRenderer, commands: &[RenderCommand]) -> Result<(), RenderError> {
    // 최소화·가려짐 상태는 오류가 아니라 정상적인 건너뛰기다.
    if renderer.begin_frame()? == FrameStatus::Skipped {
        return Ok(());
    }
    for &cmd in commands {
        renderer.submit(cmd)?;
    }
    renderer.end_frame()
}
