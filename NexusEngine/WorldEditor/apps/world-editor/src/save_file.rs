//! 저장 파일 — 플레이어 한 명의 진행 상황 (단계 2 P3).
//!
//! `saves/player.save.ron`. 형식과 규칙은 **존 파일과 같다**: 파일 전용 타입만 직렬화하고,
//! `version` 으로 관리하고, 줄바꿈은 LF 로 고정하고, 임시 파일에 쓴 뒤 이름을 바꾼다.
//!
//! # 언제 저장하나 *(사용자 결정 2026-09-21)*
//!
//! **수동 저장**(플레이 메뉴)과 **정상 종료 시 저장**(플레이 정지·창 닫기) 둘뿐이다.
//! 크래시 대비 자동 저장은 하지 않는다.
//!
//! # 담는 것
//!
//! 액터 타입 · 레벨·경험치 · 현재 HP · 가방(칸 번호 그대로) · 장착 · 마지막 존과 위치.
//! **게임 수치는 담지 않는다** — HP 상한·공격력은 `rules.ron` 에서 다시 계산된다.
//! 그래서 데이터를 고치면 저장 파일을 지우지 않아도 새 수치가 적용된다.

use std::path::{Path, PathBuf};

use nexus_core::Vec2;
use nexus_sim::ItemId;
use serde::{Deserialize, Serialize};

use crate::scene::ActorId;

/// 지금 쓰는 형식 번호.
pub(crate) const FORMAT_VERSION: u32 = 1;
/// 한 칸뿐인 기본 저장 파일.
pub(crate) const DEFAULT_SAVE: &str = "saves/player.save.ron";

/// 읽어 들인 저장 데이터 — 앱 안에서 쓰는 형태.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SaveData {
    /// 조작하던 액터 타입.
    pub(crate) actor: ActorId,
    pub(crate) level: u32,
    pub(crate) exp: u32,
    pub(crate) hp: u32,
    /// 저장할 때 열어 둔 존 파일 (`zones/village.zone.ron`). 없으면 빈 문자열.
    pub(crate) zone: String,
    /// 저장할 때의 위치. **다른 존에서 이어 하면 `None` 으로 지우고** 스폰 지점에서 시작한다.
    pub(crate) pos: Option<Vec2>,
    /// 가방 내용 — `(칸 번호, 아이템, 개수)`. 칸 번호는 프로토콜에 실리므로 그대로 되살린다.
    pub(crate) bags: Vec<(u16, ItemId, u32)>,
    /// 장착한 장비. 자리는 아이템 정의에서 나오므로 아이템 번호만 적는다.
    pub(crate) equipped: Vec<ItemId>,
}

// ─────────────────────────────────────────────────────────────────────────────
// 파일 형식
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveFile {
    version: u32,
    actor: u32,
    level: u32,
    exp: u32,
    hp: u32,
    /// 마지막으로 플레이한 존 파일. 새 씬이면 빈 문자열.
    #[serde(default)]
    zone: String,
    pos: [f32; 2],
    #[serde(default)]
    bags: Vec<SlotFile>,
    #[serde(default)]
    equipped: Vec<u32>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SlotFile {
    slot: u16,
    item: u32,
    count: u32,
}

/// 저장 데이터를 RON 문자열로. 줄바꿈은 항상 LF.
pub(crate) fn to_ron(save: &SaveData) -> String {
    let file = SaveFile {
        version: FORMAT_VERSION,
        actor: save.actor.raw(),
        level: save.level,
        exp: save.exp,
        hp: save.hp,
        zone: save.zone.clone(),
        pos: save.pos.unwrap_or(Vec2::ZERO).to_array(),
        bags: save
            .bags
            .iter()
            .map(|&(slot, item, count)| SlotFile {
                slot,
                item: item.0,
                count,
            })
            .collect(),
        equipped: save.equipped.iter().map(|i| i.0).collect(),
    };
    let config = ron::ser::PrettyConfig::new()
        .new_line("\n")
        .indentor("    ");
    let body = ron::ser::to_string_pretty(&file, config).expect("저장 데이터 직렬화 실패");
    format!("// Nexus WorldEditor 저장 파일 — 형식 {FORMAT_VERSION}. 위치 단위: 미터.\n{body}\n")
}

/// RON 문자열을 읽는다. 무엇이 틀렸는지 한국어로 알려 준다.
pub(crate) fn from_ron(text: &str) -> Result<SaveData, String> {
    let file: SaveFile =
        ron::from_str(text).map_err(|e| format!("저장 파일을 읽을 수 없음 — {e}"))?;
    if file.version != FORMAT_VERSION {
        return Err(format!(
            "형식 {} 은(는) 읽을 수 없음 (이 에디터는 {FORMAT_VERSION})",
            file.version
        ));
    }
    if !file.pos.iter().all(|v| v.is_finite()) {
        return Err(String::from("pos 가 수가 아님"));
    }
    if file.level == 0 {
        return Err(String::from("level 은 1 이상이어야 함"));
    }
    if file.hp == 0 {
        return Err(String::from(
            "hp 는 1 이상이어야 함 — 죽은 채로 이어 할 수 없다",
        ));
    }
    for s in &file.bags {
        if s.count == 0 {
            return Err(format!("bags[{}]: 개수가 0", s.slot));
        }
    }
    Ok(SaveData {
        actor: ActorId::new(file.actor),
        level: file.level,
        exp: file.exp,
        hp: file.hp,
        zone: file.zone,
        pos: Some(Vec2::from_array(file.pos)),
        bags: file
            .bags
            .iter()
            .map(|s| (s.slot, ItemId(s.item), s.count))
            .collect(),
        equipped: file.equipped.iter().map(|&i| ItemId(i)).collect(),
    })
}

/// 기본 저장 파일 경로.
pub(crate) fn default_path() -> PathBuf {
    PathBuf::from(DEFAULT_SAVE)
}

/// 저장한다 — 임시 파일에 쓴 뒤 이름 바꾸기. 폴더가 없으면 만든다.
pub(crate) fn save(path: &Path, data: &SaveData) -> Result<(), String> {
    let fail = |e: std::io::Error| format!("{}: {e}", path.display());
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(fail)?;
    }
    let tmp = path.with_extension("ron.tmp");
    std::fs::write(&tmp, to_ron(data)).map_err(fail)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        fail(e)
    })
}

/// 읽는다. 파일이 **없으면 `Ok(None)`** — 새로 시작하는 것이 정상이다.
pub(crate) fn load(path: &Path) -> Result<Option<SaveData>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => from_ron(&text)
            .map(Some)
            .map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// 저장 파일을 지운다. 없으면 아무것도 하지 않는다.
pub(crate) fn delete(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SaveData {
        SaveData {
            actor: ActorId::new(1),
            level: 3,
            exp: 240,
            hp: 180,
            zone: String::from("zones/village.zone.ron"),
            pos: Some(Vec2::new(-4.5, 7.25)),
            bags: vec![(0, ItemId(501), 3), (7, ItemId(909), 12)],
            equipped: vec![ItemId(1101)],
        }
    }

    #[test]
    fn a_save_survives_a_round_trip() {
        let text = to_ron(&sample());
        assert_eq!(from_ron(&text).unwrap(), sample());
        assert!(!text.contains('\r'), "줄바꿈은 LF 고정");
        assert_eq!(
            to_ron(&from_ron(&text).unwrap()),
            text,
            "두 번 써도 같은 바이트"
        );
    }

    #[test]
    fn broken_saves_say_what_is_wrong() {
        let text = to_ron(&sample());
        for (find, replace, expect) in [
            ("version: 1", "version: 2", "형식 2"),
            ("level: 3", "level: 0", "level 은 1 이상"),
            ("hp: 180", "hp: 0", "hp 는 1 이상"),
            ("count: 3", "count: 0", "개수가 0"),
            ("pos: (-4.5, 7.25)", "pos: (-4.5, inf)", "수가 아님"),
        ] {
            let broken = text.replace(find, replace);
            assert_ne!(broken, text, "시험 전제: '{find}' 를 찾았어야 한다");
            let err = from_ron(&broken).unwrap_err();
            assert!(err.contains(expect), "{expect} 를 알려야 한다: {err}");
        }
        // 필드 이름 오타가 기본값으로 조용히 넘어가지 않는다.
        assert!(from_ron(&text.replace("level:", "levle:")).is_err());
    }

    #[test]
    fn a_missing_file_is_not_an_error() {
        let path = std::env::temp_dir().join("nexus-no-such.save.ron");
        let _ = std::fs::remove_file(&path);
        assert_eq!(load(&path), Ok(None), "없으면 새로 시작한다");
    }

    #[test]
    fn saving_uses_a_temp_file_and_deleting_is_idempotent() {
        let dir = std::env::temp_dir().join(format!("nexus-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("player.save.ron");
        save(&path, &sample()).unwrap();
        assert_eq!(load(&path).unwrap().as_ref(), Some(&sample()));
        assert!(!dir.join("player.save.ron.tmp").exists());
        delete(&path).unwrap();
        delete(&path).unwrap();
        assert_eq!(load(&path), Ok(None));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
