//! 소리 — 게임의 일을 `display.ron` 에 적힌 파일로 잇는다 (T8).
//!
//! ```ron
//! sound: (
//!     effect_volume: 0.8,
//!     music_volume: 0.5,
//!     events: { "hit": "assets/sounds/hit.ogg", "level_up": "assets/sounds/level.ogg" },
//!     skills: { 9: "assets/sounds/fireball.ogg" },   // 적으면 그 스킬은 "hit" 대신 이것
//! ),
//! ```
//!
//! 레벨의 배경음악은 레벨 파일의 `bgm`. 소리는 **표시 데이터**다 — 규칙은 소리를 모른다.
//!
//! - 처음 낼 때 파일을 읽는다. 없거나 깨진 파일은 **한 번만** 알리고 그 소리만 빠진다 — 게임은 계속된다.
//! - 소리 장치가 없으면 조용히 돈다. `NEXUS_MUTE=1` 이면 장치를 열지 않는다 (창 없는 검증용).

use std::collections::BTreeSet;

use nexus_audio::Audio;

const ENV_MUTE: &str = "NEXUS_MUTE";

/// 게임의 일 — `events` 표의 이름.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SoundEvent {
    Hit,
    Death,
    LevelUp,
    Pickup,
    Heal,
    Buff,
    Trade,
    Quest,
    Travel,
    Click,
}

impl SoundEvent {
    pub(crate) const ALL: [Self; 10] = [
        Self::Hit,
        Self::Death,
        Self::LevelUp,
        Self::Pickup,
        Self::Heal,
        Self::Buff,
        Self::Trade,
        Self::Quest,
        Self::Travel,
        Self::Click,
    ];

    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Hit => "hit",
            Self::Death => "death",
            Self::LevelUp => "level_up",
            Self::Pickup => "pickup",
            Self::Heal => "recover", // "heal" 은 표시 파일에 쓰지 않는 낱말이다 (수치 분리 시험)
            Self::Buff => "buff",
            Self::Trade => "trade",
            Self::Quest => "quest",
            Self::Travel => "travel",
            Self::Click => "click",
        }
    }
}

/// 소리 파일 경로 규칙 — 에셋·스크립트와 같다. 리눅스에서 "파일 없음" 이 되는 것을 미리 막는다.
pub(crate) fn sound_path_ok(path: &str) -> bool {
    (path.ends_with(".ogg") || path.ends_with(".wav"))
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && !path.split('/').any(|part| part.is_empty() || part == "..")
        && !path.chars().any(char::is_uppercase)
}

/// 앱이 드는 소리 — 장치가 없으면 `audio` 가 `None` 이고 전부 아무 일도 하지 않는다.
#[derive(Debug, Default)]
pub(crate) struct Sounds {
    audio: Option<Audio>,
    /// 읽지 못한 파일 — 다시 시도하지도, 다시 알리지도 않는다.
    failed: BTreeSet<String>,
}

impl Sounds {
    /// 소리 장치를 연다. 못 열면 이유를 돌려주고 조용히 돈다.
    pub(crate) fn open() -> (Self, Option<String>) {
        if std::env::var_os(ENV_MUTE).is_some() {
            return (Self::default(), None);
        }
        match Audio::open() {
            Ok(audio) => (
                Self {
                    audio: Some(audio),
                    failed: BTreeSet::new(),
                },
                None,
            ),
            Err(e) => (Self::default(), Some(format!("소리 없이 실행합니다 — {e}"))),
        }
    }

    pub(crate) fn set_volumes(&mut self, effect: f32, music: f32) {
        if let Some(audio) = &mut self.audio {
            audio.set_volumes(effect, music);
        }
    }

    /// 파일을 읽어 둔다 (처음 한 번). 알릴 오류가 있으면 돌려준다.
    fn ensure(&mut self, path: &str) -> Option<String> {
        let audio = self.audio.as_mut()?;
        if audio.is_loaded(path) || self.failed.contains(path) {
            return None;
        }
        let loaded = std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| audio.load(path, bytes));
        match loaded {
            Ok(()) => None,
            Err(e) => {
                self.failed.insert(path.to_owned());
                Some(format!("소리 {path}: {e}"))
            }
        }
    }

    /// 효과음. 처음 실패한 파일이면 알릴 글을 돌려준다.
    pub(crate) fn play(&mut self, path: &str) -> Option<String> {
        let error = self.ensure(path);
        if let Some(audio) = &self.audio {
            audio.play(path);
        }
        error
    }

    /// 배경음악 — 같은 곡이면 이어 간다. `None` 이면 멈춘다.
    pub(crate) fn music(&mut self, path: Option<&str>) -> Option<String> {
        let error = path.and_then(|p| self.ensure(p));
        if let Some(audio) = &mut self.audio {
            audio.play_music(path);
        }
        error
    }
}
