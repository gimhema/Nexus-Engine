//! 소리 재생 — 효과음과 반복 배경음악 (T8).
//!
//! **파일 시스템도 게임 규칙도 모른다.** 입력은 바이트이고, 무엇을 언제 낼지는 앱이 정한다
//! (`nexus-assets` 와 같은 원칙). 재생만 `rodio` 에 맡기므로 크로스플랫폼 분기가 이 크레이트에 없다 —
//! Windows 는 WASAPI, 리눅스는 ALSA.
//!
//! 소리 장치가 없는 머신에서도 게임은 돌아야 한다 — [`Audio::open`] 의 실패는 앱이 "소리 없음" 으로 넘긴다.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};

/// 디코더가 읽는 바이트 — 효과음을 낼 때마다 복사하지 않게 공유한다.
#[derive(Clone)]
struct Clip(Arc<[u8]>);

impl AsRef<[u8]> for Clip {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// 소리 장치 하나와 읽어 둔 소리들.
pub struct Audio {
    sink: MixerDeviceSink,
    clips: HashMap<String, Clip>,
    /// 지금 도는 배경음악. 버리면 멈춘다.
    music: Option<(String, Player)>,
    effect_volume: f32,
    music_volume: f32,
}

impl std::fmt::Debug for Audio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Audio")
            .field("clips", &self.clips.len())
            .field("music", &self.music.as_ref().map(|(key, _)| key))
            .finish_non_exhaustive()
    }
}

impl Audio {
    /// 기본 소리 장치를 연다.
    pub fn open() -> Result<Self, String> {
        let mut sink = DeviceSinkBuilder::open_default_sink().map_err(|e| e.to_string())?;
        // 끝날 때 표준 오류에 안내를 찍지 않게 한다 — 정상 종료다.
        sink.log_on_drop(false);
        Ok(Self {
            sink,
            clips: HashMap::new(),
            music: None,
            effect_volume: 1.0,
            music_volume: 1.0,
        })
    }

    /// 음량 (0~1). 도는 배경음악에도 바로 적용한다.
    pub fn set_volumes(&mut self, effect: f32, music: f32) {
        self.effect_volume = effect.clamp(0.0, 1.0);
        self.music_volume = music.clamp(0.0, 1.0);
        if let Some((_, player)) = &self.music {
            player.set_volume(self.music_volume);
        }
    }

    #[must_use]
    pub fn is_loaded(&self, key: &str) -> bool {
        self.clips.contains_key(key)
    }

    /// 소리를 읽어 둔다. 재생할 수 없는 바이트면 이유를 돌려주고 등록하지 않는다.
    pub fn load(&mut self, key: &str, bytes: Vec<u8>) -> Result<(), String> {
        let clip = Clip(bytes.into());
        Decoder::new(Cursor::new(clip.clone())).map_err(|e| e.to_string())?;
        self.clips.insert(key.to_owned(), clip);
        Ok(())
    }

    /// 효과음을 한 번 낸다 — 겹쳐도 된다. 읽어 두지 않은 이름이면 아무 일도 없다.
    pub fn play(&self, key: &str) {
        let Some(clip) = self.clips.get(key) else {
            return;
        };
        let Ok(source) = Decoder::new(Cursor::new(clip.clone())) else {
            return;
        };
        let player = Player::connect_new(self.sink.mixer());
        player.set_volume(self.effect_volume);
        player.append(source);
        player.detach();
    }

    /// 배경음악을 바꾼다. 같은 곡이면 처음으로 돌리지 않고, `None` 이면 멈춘다.
    pub fn play_music(&mut self, key: Option<&str>) {
        if self.music.as_ref().map(|(k, _)| k.as_str()) == key {
            return;
        }
        self.music = None;
        let Some(key) = key else { return };
        let Some(clip) = self.clips.get(key) else {
            return;
        };
        let Ok(source) = Decoder::new_looped(Cursor::new(clip.clone())) else {
            return;
        };
        let player = Player::connect_new(self.sink.mixer());
        player.set_volume(self.music_volume);
        player.append(source);
        self.music = Some((key.to_owned(), player));
    }
}
