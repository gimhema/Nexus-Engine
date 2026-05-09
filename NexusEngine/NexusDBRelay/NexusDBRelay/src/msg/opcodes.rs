/// DB Relay RPC opcode 정의 (0x1000번대)
///
/// C++ 대응 헤더: protocol_shared/DBRelayOpcodes.h (추후 추가)
/// 네이밍: Req* = 게임서버→Relay, Res* = Relay→게임서버
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DbOpcode {
    // ── 계정 (0x1101~) ───────────────────────────────────────────────────────
    ReqCheckAccount = 0x1101, // [u64 sessionId][string accountName][string token]
    ResCheckAccount = 0x1102, // [u8 success][u64 sessionId][u32 accountId][string message]

    // ── 캐릭터 (0x1001~) ─────────────────────────────────────────────────────
    ReqCreateCharacter = 0x1001, // [u64 sessionId][u32 accountId][string name]
    ResCreateCharacter = 0x1002, // [u8 success][u64 sessionId][u32 charId][string message]
    ReqLoadCharacter   = 0x1003, // [u64 sessionId][u32 charId]
    ResLoadCharacter   = 0x1004, // [u8 success][u64 sessionId][CharacterData | string error]
    ReqSaveCharacter   = 0x1005, // [u64 sessionId][CharacterData]
    ResSaveCharacter   = 0x1006, // [u8 success][u64 sessionId][string message]
}

impl DbOpcode {
    pub fn from_u16(v: u16) -> Option<Self> {
        match v {
            0x1101 => Some(Self::ReqCheckAccount),
            0x1102 => Some(Self::ResCheckAccount),
            0x1001 => Some(Self::ReqCreateCharacter),
            0x1002 => Some(Self::ResCreateCharacter),
            0x1003 => Some(Self::ReqLoadCharacter),
            0x1004 => Some(Self::ResLoadCharacter),
            0x1005 => Some(Self::ReqSaveCharacter),
            0x1006 => Some(Self::ResSaveCharacter),
            _      => None,
        }
    }
}
