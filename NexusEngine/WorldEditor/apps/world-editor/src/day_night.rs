//! 낮과 밤 — 월드 시계(`nexus_sim::SimWorld::time_of_day`)를 화면 조명으로 바꾼다.
//!
//! 시각은 규칙(`rules.ron` 의 `day_cycle`, sim 이 센다)이고, 여기는 **그리는 법**만 안다 (`display.ron` 의 `day_night`):
//!
//! - **낮**에는 조명을 켜지 않는다 — 그림 색 그대로 (`SetLighting` 을 내지 않는다).
//! - **해 질 녘·새벽**(경계 앞뒤 `twilight_hours`) 동안 주변광이 낮 → 노을 → 밤 색으로 이어진다.
//! - **밤**에는 주변광이 어둡고 **플레이어 주변에만** 빛이 든다 (`player_light`). 등불 머리처럼 시트가 지닌 광원도
//!   조명이 켜져야 보이므로 밤에만 빛난다.
//!
//! 레벨이 켜야 한다(`levels/*.level.ron` 의 `day_night: true`). 고정 조명(`lighting`)이 있는 레벨(동굴)은 그쪽이 이긴다.

use nexus_core::{Vec2, Vec3};
use nexus_render::{Lighting, PointLight, RenderCommand};
use serde::Deserialize;

/// `rules.ron` 의 `day_cycle` — 하루 길이와 낮·밤 경계. 서버 `GameLogic` 과 같은 기본값(실제 1분 = 게임 1시간).
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct DayCycleFile {
    /// 하루(24시간)가 지나는 데 걸리는 실제 분.
    pub(crate) day_minutes: f32,
    pub(crate) dawn_hour: f32,
    pub(crate) dusk_hour: f32,
    /// 새 게임을 시작하는 시각.
    pub(crate) start_hour: f32,
}

impl Default for DayCycleFile {
    fn default() -> Self {
        Self {
            day_minutes: 24.0,
            dawn_hour: 6.0,
            dusk_hour: 19.0,
            start_hour: 8.0,
        }
    }
}

impl DayCycleFile {
    pub(crate) fn check(&self) -> Result<(), String> {
        let hour = |h: f32| h.is_finite() && (0.0..24.0).contains(&h);
        if !(self.day_minutes.is_finite() && self.day_minutes >= 1.0 / 60.0) {
            return Err(String::from("day_cycle: day_minutes 는 1초(1/60) 이상"));
        }
        if !(hour(self.dawn_hour) && hour(self.dusk_hour) && hour(self.start_hour)) {
            return Err(String::from("day_cycle: 시각은 0 이상 24 미만"));
        }
        Ok(())
    }

    pub(crate) fn to_sim(self) -> nexus_sim::DayCycle {
        nexus_sim::DayCycle {
            day_ms: (f64::from(self.day_minutes) * 60_000.0) as u64,
            dawn_hour: self.dawn_hour,
            dusk_hour: self.dusk_hour,
        }
    }
}

/// `display.ron` 의 `day_night` — 밤의 색과 플레이어 주변 빛. 색은 sRGB (0~1).
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct DayNightLook {
    /// 노을 주변광 — 해 질 녘·새벽의 한가운데.
    pub(crate) dusk_ambient: (f32, f32, f32),
    /// 밤 주변광.
    pub(crate) night_ambient: (f32, f32, f32),
    /// 경계 앞뒤로 색이 이어지는 시간 (게임 시간, 시).
    pub(crate) twilight_hours: f32,
    /// 플레이어를 따라다니는 빛 — 밤의 깊이만큼 세진다.
    pub(crate) player_light: PlayerLightFile,
    /// 밝기 단계 수 (레벨 `lighting` 과 같은 뜻 — 0 이면 매끈하게).
    pub(crate) steps: u32,
    pub(crate) dither: bool,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct PlayerLightFile {
    pub(crate) color: (f32, f32, f32),
    /// 빛이 닿는 거리 (m).
    pub(crate) radius: f32,
    pub(crate) intensity: f32,
    /// 발밑에서 위로 (m) — 몸 한가운데쯤이 가장 밝다.
    pub(crate) height: f32,
}

impl Default for PlayerLightFile {
    fn default() -> Self {
        Self {
            color: (1.0, 0.86, 0.62),
            radius: 6.0,
            intensity: 1.1,
            height: 1.0,
        }
    }
}

impl Default for DayNightLook {
    fn default() -> Self {
        Self {
            dusk_ambient: (0.85, 0.55, 0.45),
            night_ambient: (0.12, 0.14, 0.26),
            twilight_hours: 1.0,
            player_light: PlayerLightFile::default(),
            steps: 5,
            dither: false,
        }
    }
}

impl DayNightLook {
    pub(crate) fn check(&self) -> Result<(), String> {
        let l = self.player_light;
        let ok = |v: f32| v.is_finite() && v >= 0.0;
        if !(ok(self.twilight_hours) && self.twilight_hours <= 6.0) {
            return Err(String::from("day_night: twilight_hours 는 0~6"));
        }
        if !(ok(l.radius) && l.radius > 0.0 && ok(l.intensity) && ok(l.height)) {
            return Err(String::from(
                "day_night: player_light 의 radius 는 양수, intensity·height 는 0 이상",
            ));
        }
        Ok(())
    }

    /// 플레이어 주변 빛의 중심. 횡스크롤은 Y 가 높이라 몸 한가운데(위로 `height`)에 두고 화면 쪽으로 조금 띄운다.
    /// 다른 시점은 Z 가 높이다.
    pub(crate) fn light_center(&self, feet: Vec2, side_view: bool) -> Vec3 {
        let h = self.player_light.height;
        if side_view {
            Vec3::new(feet.x, feet.y + h, h)
        } else {
            Vec3::new(feet.x, feet.y, h)
        }
    }

    /// 밤의 깊이 — 0 = 한낮, 1 = 한밤. 경계에서 `twilight_hours` 동안 이어진다 (해 지는 시각에 0.5).
    pub(crate) fn darkness(&self, hour: f32, cycle: &nexus_sim::DayCycle) -> f32 {
        let half = (self.twilight_hours * 0.5).max(1e-3);
        // 경계에서 지난 시간 (전이면 음수, ±12시간 안 — 자정을 넘겨도 맞게).
        let since = |from: f32| {
            let d = (hour - from).rem_euclid(24.0);
            if d > 12.0 { d - 24.0 } else { d }
        };
        // 경계 앞뒤 `half` 동안 0 → 1.
        let ramp = |d: f32| ((d + half) / (2.0 * half)).clamp(0.0, 1.0);
        let (dusk, dawn) = (since(cycle.dusk_hour), since(cycle.dawn_hour));
        if dusk.abs() <= half {
            ramp(dusk)
        } else if dawn.abs() <= half {
            1.0 - ramp(dawn)
        } else if cycle.is_night_at(hour) {
            1.0
        } else {
            0.0
        }
    }

    /// 이 시각의 주변광 — 한낮 흰색 → 노을 → 밤. 조명을 받지 않는 배경(하늘)은 이 색을 직접 곱한다.
    pub(crate) fn ambient(&self, hour: f32, cycle: &nexus_sim::DayCycle) -> [f32; 3] {
        let t = self.darkness(hour, cycle);
        let mix = |a: (f32, f32, f32), b: (f32, f32, f32), k: f32| {
            [
                a.0 + (b.0 - a.0) * k,
                a.1 + (b.1 - a.1) * k,
                a.2 + (b.2 - a.2) * k,
            ]
        };
        if t < 0.5 {
            mix((1.0, 1.0, 1.0), self.dusk_ambient, t * 2.0)
        } else {
            mix(self.dusk_ambient, self.night_ambient, t * 2.0 - 1.0)
        }
    }

    /// 이 시각의 조명 명령. 한낮이면 비어 있다 (조명을 켜지 않는다).
    /// `player` 는 플레이어 주변 빛의 중심(월드 m) — 시점마다 높이 축이 달라 부르는 쪽이 정한다
    /// ([`Self::light_center`]). 쓰러져 없으면 `None`.
    pub(crate) fn commands(
        &self,
        hour: f32,
        cycle: &nexus_sim::DayCycle,
        player: Option<Vec3>,
    ) -> Vec<RenderCommand> {
        let t = self.darkness(hour, cycle);
        if t <= 0.0 {
            return Vec::new();
        }
        let ambient = self.ambient(hour, cycle);
        let mut out = vec![RenderCommand::SetLighting(Lighting {
            ambient,
            steps: self.steps,
            dither: self.dither,
        })];
        if let Some(at) = player {
            let l = self.player_light;
            out.push(RenderCommand::AddLight(PointLight {
                pos: at,
                color: [l.color.0, l.color.1, l.color.2],
                radius: l.radius,
                intensity: l.intensity * t,
            }));
        }
        out
    }
}

/// HUD 에 보일 시각 — `"21:30 밤"`.
pub(crate) fn clock_text(hour: f32, night: bool) -> String {
    let minutes = (hour.rem_euclid(24.0) * 60.0) as u32;
    format!(
        "{:02}:{:02} {}",
        minutes / 60,
        minutes % 60,
        if night { "밤" } else { "낮" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cycle() -> nexus_sim::DayCycle {
        DayCycleFile::default().to_sim()
    }

    #[test]
    fn darkness_is_zero_by_day_one_by_night_and_half_at_the_boundaries() {
        let look = DayNightLook::default();
        let c = cycle();
        assert_eq!(look.darkness(12.0, &c), 0.0);
        assert_eq!(look.darkness(0.0, &c), 1.0);
        assert!((look.darkness(19.0, &c) - 0.5).abs() < 1e-4);
        assert!((look.darkness(6.0, &c) - 0.5).abs() < 1e-4);
        // 이어진다 — 해 지기 전후로 단조롭게 어두워진다.
        let a = look.darkness(18.6, &c);
        let b = look.darkness(19.3, &c);
        assert!(0.0 < a && a < 0.5 && 0.5 < b && b < 1.0, "{a} {b}");
        assert_eq!(look.darkness(19.6, &c), 1.0);
        assert_eq!(look.darkness(6.6, &c), 0.0);
    }

    #[test]
    fn by_day_nothing_is_lit_and_by_night_only_around_the_player() {
        let look = DayNightLook::default();
        let c = cycle();
        assert!(look.commands(12.0, &c, Some(Vec3::ZERO)).is_empty());

        let night = look.commands(23.0, &c, Some(look.light_center(Vec2::new(3.0, 0.0), true)));
        let RenderCommand::SetLighting(l) = night[0] else {
            panic!("{night:?}")
        };
        let want = [0.12, 0.14, 0.26];
        assert!(
            l.ambient
                .iter()
                .zip(want)
                .all(|(a, b)| (a - b).abs() < 1e-5),
            "{:?}",
            l.ambient
        );
        let RenderCommand::AddLight(p) = night[1] else {
            panic!("{night:?}")
        };
        assert_eq!(p.pos, Vec3::new(3.0, 1.0, 1.0), "횡스크롤 — 몸 한가운데");
        assert!((p.intensity - look.player_light.intensity).abs() < 1e-6);
        // 쓰러져 있으면 주변 빛도 없다.
        assert_eq!(look.commands(23.0, &c, None).len(), 1);
    }

    #[test]
    fn clock_text_reads_like_a_clock() {
        assert_eq!(clock_text(21.5, true), "21:30 밤");
        assert_eq!(clock_text(8.0, false), "08:00 낮");
    }

    #[test]
    fn bad_numbers_are_named() {
        let f = DayCycleFile {
            dusk_hour: 24.0,
            ..DayCycleFile::default()
        };
        assert!(f.check().unwrap_err().contains("시각"));
        let mut l = DayNightLook::default();
        l.player_light.radius = 0.0;
        assert!(l.check().unwrap_err().contains("radius"));
    }
}
