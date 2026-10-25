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
//! 액터 타입 · 레벨·경험치 · 현재 HP · 가방(칸 번호 그대로) · 장착 · 마지막 존과 위치 ·
//! 진행 플래그(R1 — 캐릭터 범위와 월드 범위를 따로). 플래그는 0 이 아닌 것만 적는다.
//! **게임 수치는 담지 않는다** — HP 상한·공격력은 `rules.ron` 에서 다시 계산된다.
//! 그래서 데이터를 고치면 저장 파일을 지우지 않아도 새 수치가 적용된다.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nexus_core::Vec2;
use nexus_sim::{ClassId, CurrencyId, FlagId, ItemId, QuestId, SkillId};
use serde::{Deserialize, Serialize};

use crate::scene::ActorId;

/// 지금 쓰는 형식 번호.
pub(crate) const FORMAT_VERSION: u32 = 1;
/// 1번 칸 저장 파일 — 저장 칸이 하나뿐이던 때의 경로를 그대로 쓴다 (예전 저장이 1번 칸이 된다).
pub(crate) const DEFAULT_SAVE: &str = "saves/player.save.ron";
/// 저장 칸 수.
pub(crate) const SLOTS: u8 = 3;

/// 퀘스트 하나의 기록 `(퀘스트, 진행 중이면 (단계, 센 수), 완료 횟수)` (R5).
pub(crate) type QuestRecord = (QuestId, Option<(u16, Vec<u32>)>, u32);

/// 읽어 들인 저장 데이터 — 앱 안에서 쓰는 형태.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SaveData {
    /// 조작하던 액터 타입.
    pub(crate) actor: ActorId,
    /// 직업 (R2). `None` = 직업이 생기기 전의 저장 파일 — 기본 직업으로 이어 한다.
    pub(crate) class: Option<ClassId>,
    /// 배운 스킬과 레벨 (R3). `None` = 스킬이 생기기 전의 저장 — 직업의 시작 스킬로 이어 한다.
    pub(crate) skills: Option<Vec<(SkillId, u32)>>,
    /// 단축키 배치 (R3) — 칸마다 스킬 또는 빈 칸. `None` = 예전 저장 — 시작 스킬을 차례로.
    pub(crate) hotbar: Option<Vec<Option<SkillId>>>,
    pub(crate) level: u32,
    pub(crate) exp: u32,
    pub(crate) hp: u32,
    /// 현재 MP. `None` = MP 가 생기기 전의 저장 파일 — 가득 찬 채로 시작한다.
    pub(crate) mp: Option<u32>,
    /// 저장할 때 열어 둔 존 파일 (`zones/village.zone.ron`). 없으면 빈 문자열.
    pub(crate) zone: String,
    /// 저장할 때의 위치. **다른 존에서 이어 하면 `None` 으로 지우고** 스폰 지점에서 시작한다.
    pub(crate) pos: Option<Vec2>,
    /// 가방 내용 — `(칸 번호, 아이템, 개수)`. 칸 번호는 프로토콜에 실리므로 그대로 되살린다.
    pub(crate) bags: Vec<(u16, ItemId, u32)>,
    /// 장착한 장비. 자리는 아이템 정의에서 나오므로 아이템 번호만 적는다.
    pub(crate) equipped: Vec<ItemId>,
    /// 캐릭터 범위 진행 플래그 (R1) — 번호 순, 0 은 없다.
    pub(crate) flags: Vec<(FlagId, i32)>,
    /// 월드 범위 진행 플래그 (R1). 싱글에서는 캐릭터와 같은 파일에 둔다 — MMO 에서는 서버 몫이다.
    pub(crate) world_flags: Vec<(FlagId, i32)>,
    /// 지갑 (R4). `None` = 재화가 생기기 전의 저장 — 시작 재화로 이어 한다.
    pub(crate) wallet: Option<Vec<(CurrencyId, u64)>>,
    /// 창고 `(칸, 아이템, 개수)` (R4-2).
    pub(crate) storage: Vec<(u16, ItemId, u32)>,
    /// 퀘스트 기록 (R5).
    pub(crate) quests: Vec<QuestRecord>,
}

// ─────────────────────────────────────────────────────────────────────────────
// 파일 형식
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveFile {
    version: u32,
    actor: u32,
    /// 직업 (R2). 없는 예전 파일도 읽힌다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    class: Option<u32>,
    /// 배운 스킬 `번호: 레벨` (R3). 없는 예전 파일도 읽힌다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    skills: Option<BTreeMap<u32, u32>>,
    /// 단축키 칸 (R3) — `[Some(5), None, Some(6)]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hotbar: Option<Vec<Option<u32>>>,
    level: u32,
    exp: u32,
    hp: u32,
    /// 현재 MP. 이 필드가 없는 예전 파일도 읽힌다 (그때는 가득 찬 채로 시작).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mp: Option<u32>,
    /// 마지막으로 플레이한 존 파일. 새 씬이면 빈 문자열.
    #[serde(default)]
    zone: String,
    pos: [f32; 2],
    #[serde(default)]
    bags: Vec<SlotFile>,
    #[serde(default)]
    equipped: Vec<u32>,
    /// 캐릭터 플래그 `번호: 값`. 없는 예전 파일도 읽힌다 (전부 0).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    flags: BTreeMap<u32, i32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    world_flags: BTreeMap<u32, i32>,
    /// 지갑 `재화: 금액` (R4). 없는 예전 파일도 읽힌다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    wallet: Option<BTreeMap<u32, u64>>,
    /// 창고 (R4-2) — 가방과 같은 모양.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    storage: Vec<SlotFile>,
    /// 퀘스트 기록 (R5) — `번호: (stage: Some(1), counts: [2], completed: 0)`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    quests: BTreeMap<u32, QuestSaveFile>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QuestSaveFile {
    /// 진행 중이면 단계 (끝까지 채웠으면 단계 수 — 보고 가능).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stage: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    counts: Vec<u32>,
    #[serde(default)]
    completed: u32,
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
        class: save.class.map(|c| c.0),
        skills: save
            .skills
            .as_ref()
            .map(|list| list.iter().map(|(s, l)| (s.0, *l)).collect()),
        hotbar: save
            .hotbar
            .as_ref()
            .map(|h| h.iter().map(|s| s.map(|s| s.0)).collect()),
        level: save.level,
        exp: save.exp,
        hp: save.hp,
        mp: save.mp,
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
        flags: save.flags.iter().map(|(f, v)| (f.0, *v)).collect(),
        world_flags: save.world_flags.iter().map(|(f, v)| (f.0, *v)).collect(),
        wallet: save
            .wallet
            .as_ref()
            .map(|w| w.iter().map(|(c, a)| (c.0, *a)).collect()),
        storage: save
            .storage
            .iter()
            .map(|&(slot, item, count)| SlotFile {
                slot,
                item: item.0,
                count,
            })
            .collect(),
        quests: save
            .quests
            .iter()
            .map(|(q, active, completed)| {
                (
                    q.0,
                    QuestSaveFile {
                        stage: active.as_ref().map(|a| a.0),
                        counts: active.as_ref().map(|a| a.1.clone()).unwrap_or_default(),
                        completed: *completed,
                    },
                )
            })
            .collect(),
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
        class: file.class.map(ClassId),
        skills: file.skills.map(|m| {
            m.into_iter()
                .filter(|(_, l)| *l > 0)
                .map(|(s, l)| (SkillId(s), l))
                .collect()
        }),
        hotbar: file
            .hotbar
            .map(|h| h.into_iter().map(|s| s.map(SkillId)).collect()),
        level: file.level,
        exp: file.exp,
        hp: file.hp,
        mp: file.mp,
        zone: file.zone,
        pos: Some(Vec2::from_array(file.pos)),
        bags: file
            .bags
            .iter()
            .map(|s| (s.slot, ItemId(s.item), s.count))
            .collect(),
        equipped: file.equipped.iter().map(|&i| ItemId(i)).collect(),
        // 0 은 "없음" 과 같다 — 손으로 적은 0 은 버린다.
        flags: flag_list(&file.flags),
        world_flags: flag_list(&file.world_flags),
        wallet: file
            .wallet
            .map(|w| w.into_iter().map(|(c, a)| (CurrencyId(c), a)).collect()),
        storage: file
            .storage
            .iter()
            .map(|s| (s.slot, ItemId(s.item), s.count))
            .collect(),
        quests: file
            .quests
            .into_iter()
            .map(|(q, f)| (QuestId(q), f.stage.map(|s| (s, f.counts)), f.completed))
            .collect(),
    })
}

fn flag_list(map: &BTreeMap<u32, i32>) -> Vec<(FlagId, i32)> {
    map.iter()
        .filter(|(_, v)| **v != 0)
        .map(|(f, v)| (FlagId(*f), *v))
        .collect()
}

/// `slot` 번 칸의 저장 파일 — 1번은 `saves/player.save.ron`, 2번부터 `saves/player2.save.ron` …
/// 범위 밖의 번호는 1~[`SLOTS`] 로 자른다.
pub(crate) fn slot_path(slot: u8) -> PathBuf {
    match slot.clamp(1, SLOTS) {
        1 => PathBuf::from(DEFAULT_SAVE),
        n => PathBuf::from(format!("saves/player{n}.save.ron")),
    }
}

/// 저장 칸 한 줄 요약 — 메뉴·이어 하기 창용. 비었으면 "비어 있음", 깨졌으면 그 이유.
pub(crate) fn summary(path: &Path) -> String {
    match load(path) {
        Ok(Some(s)) if s.zone.is_empty() => format!("레벨 {} · (새 존)", s.level),
        Ok(Some(s)) => format!("레벨 {} · {}", s.level, s.zone),
        Ok(None) => String::from("비어 있음"),
        Err(e) => format!("읽을 수 없음 — {e}"),
    }
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
            class: Some(ClassId(2)),
            skills: Some(vec![(SkillId(6), 1), (SkillId(8), 3)]),
            hotbar: Some(vec![Some(SkillId(8)), None, Some(SkillId(6))]),
            level: 3,
            exp: 240,
            hp: 180,
            mp: Some(25),
            zone: String::from("zones/village.zone.ron"),
            pos: Some(Vec2::new(-4.5, 7.25)),
            bags: vec![(0, ItemId(501), 3), (7, ItemId(909), 12)],
            equipped: vec![ItemId(1101)],
            flags: vec![(FlagId(1), 4), (FlagId(2), 1)],
            world_flags: vec![(FlagId(30), -1)],
            wallet: Some(vec![(CurrencyId(1), 250)]),
            storage: vec![(4, ItemId(909), 30)],
            quests: vec![(QuestId(1), Some((0, vec![2])), 0), (QuestId(2), None, 3)],
        }
    }

    #[test]
    fn a_save_from_before_flags_opens_with_no_flags() {
        let old = SaveData {
            flags: Vec::new(),
            world_flags: Vec::new(),
            ..sample()
        };
        let text = to_ron(&old);
        assert!(!text.contains("flags"), "빈 플래그는 적지 않는다: {text}");
        let back = from_ron(&text).unwrap();
        assert!(back.flags.is_empty() && back.world_flags.is_empty());
        // 손으로 적은 0 은 "없음" 과 같다.
        let zero = to_ron(&sample()).replace("1: 4", "1: 0");
        assert_eq!(from_ron(&zero).unwrap().flags, [(FlagId(2), 1)]);
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
    fn a_save_from_before_mp_still_opens_and_starts_full() {
        let old = SaveData {
            mp: None,
            ..sample()
        };
        let text = to_ron(&old);
        assert!(!text.contains("mp:"), "없는 값은 적지 않는다: {text}");
        assert_eq!(from_ron(&text).unwrap().mp, None);
        assert!(to_ron(&sample()).contains("mp: Some(25)"));
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

    #[test]
    fn slots_have_their_own_files_and_slot_one_is_the_old_path() {
        assert_eq!(
            slot_path(1),
            PathBuf::from(DEFAULT_SAVE),
            "예전 저장이 1번 칸"
        );
        assert_eq!(slot_path(2), PathBuf::from("saves/player2.save.ron"));
        assert_eq!(slot_path(3), PathBuf::from("saves/player3.save.ron"));
        // 범위 밖은 자른다.
        assert_eq!(slot_path(0), slot_path(1));
        assert_eq!(slot_path(99), slot_path(SLOTS));
        let all: std::collections::BTreeSet<_> = (1..=SLOTS).map(slot_path).collect();
        assert_eq!(all.len(), usize::from(SLOTS), "칸마다 다른 파일");
        // 경로는 소문자·'/' — 두 OS 에서 같은 파일.
        for p in &all {
            let s = p.to_string_lossy();
            assert_eq!(s, s.to_lowercase());
            assert!(!s.contains(char::from(92)), "역슬래시 없음");
        }
    }

    #[test]
    fn a_slot_summary_says_level_and_zone_or_empty() {
        let dir = std::env::temp_dir().join(format!("nexus-slot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("player2.save.ron");
        assert_eq!(summary(&path), "비어 있음");
        save(&path, &sample()).unwrap();
        let s = sample();
        assert_eq!(summary(&path), format!("레벨 {} · {}", s.level, s.zone));
        std::fs::write(&path, "깨진 파일").unwrap();
        assert!(summary(&path).starts_with("읽을 수 없음"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
