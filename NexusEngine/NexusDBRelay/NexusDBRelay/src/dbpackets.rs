// ─────────────────────────────────────────────────────────────────────────────
// dbpackets.rs — RPC 프로토콜 정의
//
// 게임서버(C++) ↔ NexusDBRelay(Rust) 간 바이너리 프로토콜.
// 기존 NexusEngine 패킷 포맷과 동일한 Little-Endian 바이너리:
//   TCP: [u16 total_size][u16 opcode][payload...]
//   문자열: [u16 len][UTF-8 bytes]
//
// C++ 대응 헤더: protocol_shared/DBRelayOpcodes.h (추후 추가)
// ─────────────────────────────────────────────────────────────────────────────

use anyhow::{bail, Result};

// ─────────────────────────────────────────────────────────────────────────────
// Opcode 정의 (0x1000번대 — 게임서버 패킷과 충돌 없음)
// ─────────────────────────────────────────────────────────────────────────────
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DbOpcode {
    // ── 캐릭터 (0x1001~) ──────────────────────────────────────────────────────
    ReqCreateCharacter = 0x1001, // C→R [u64 sessionId][u32 accountId][string name]
    ResCreateCharacter = 0x1002, // R→C [u8 success][u64 sessionId][u32 charId][string message]
    ReqLoadCharacter   = 0x1003, // C→R [u64 sessionId][u32 charId]
    ResLoadCharacter   = 0x1004, // R→C [u8 success][u64 sessionId][CharacterData | string error]
    ReqSaveCharacter   = 0x1005, // C→R [u64 sessionId][CharacterData]
    ResSaveCharacter   = 0x1006, // R→C [u8 success][u64 sessionId][string message]
    // ── 계정 (0x1101~) ───────────────────────────────────────────────────────
    ReqCheckAccount    = 0x1101, // C→R [u64 sessionId][string accountName][string token]
    ResCheckAccount    = 0x1102, // R→C [u8 success][u64 sessionId][u32 accountId][string message]
}

impl DbOpcode {
    pub fn from_u16(v: u16) -> Option<Self> {
        match v {
            0x1001 => Some(Self::ReqCreateCharacter),
            0x1002 => Some(Self::ResCreateCharacter),
            0x1003 => Some(Self::ReqLoadCharacter),
            0x1004 => Some(Self::ResLoadCharacter),
            0x1005 => Some(Self::ReqSaveCharacter),
            0x1006 => Some(Self::ResSaveCharacter),
            0x1101 => Some(Self::ReqCheckAccount),
            0x1102 => Some(Self::ResCheckAccount),
            _      => None,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// PacketReader — payload 바이트 순차 읽기
// ─────────────────────────────────────────────────────────────────────────────
pub struct PacketReader<'a> {
    data: &'a [u8],
    pos:  usize,
}

impl<'a> PacketReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn ensure(&self, n: usize) -> Result<()> {
        if self.pos + n > self.data.len() {
            bail!("PacketReader 오버런: pos={} need={} len={}", self.pos, n, self.data.len());
        }
        Ok(())
    }

    pub fn read_u8(&mut self) -> Result<u8> {
        self.ensure(1)?;
        let v = self.data[self.pos];
        self.pos += 1;
        Ok(v)
    }

    pub fn read_u16(&mut self) -> Result<u16> {
        self.ensure(2)?;
        let v = u16::from_le_bytes(self.data[self.pos..self.pos + 2].try_into()?);
        self.pos += 2;
        Ok(v)
    }

    pub fn read_u32(&mut self) -> Result<u32> {
        self.ensure(4)?;
        let v = u32::from_le_bytes(self.data[self.pos..self.pos + 4].try_into()?);
        self.pos += 4;
        Ok(v)
    }

    pub fn read_u64(&mut self) -> Result<u64> {
        self.ensure(8)?;
        let v = u64::from_le_bytes(self.data[self.pos..self.pos + 8].try_into()?);
        self.pos += 8;
        Ok(v)
    }

    pub fn read_f32(&mut self) -> Result<f32> {
        self.ensure(4)?;
        let v = f32::from_le_bytes(self.data[self.pos..self.pos + 4].try_into()?);
        self.pos += 4;
        Ok(v)
    }

    // [u16 len][UTF-8 bytes]
    pub fn read_string(&mut self) -> Result<String> {
        let len = self.read_u16()? as usize;
        self.ensure(len)?;
        let s = std::str::from_utf8(&self.data[self.pos..self.pos + len])
            .map_err(|e| anyhow::anyhow!("UTF-8 디코딩 실패: {}", e))?
            .to_string();
        self.pos += len;
        Ok(s)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// PacketWriter — 응답 패킷 직렬화
// ─────────────────────────────────────────────────────────────────────────────
pub struct PacketWriter {
    buf: Vec<u8>,
}

impl PacketWriter {
    pub fn new(opcode: u16) -> Self {
        // 헤더 4바이트 예약: [u16 size=0][u16 opcode]
        let mut buf = vec![0u8; 4];
        buf[2..4].copy_from_slice(&opcode.to_le_bytes());
        Self { buf }
    }

    // 소유권 이동(consuming) 빌더 패턴 — finalize() 까지 체이닝 가능
    pub fn write_u8(mut self, v: u8) -> Self {
        self.buf.push(v);
        self
    }

    pub fn write_u32(mut self, v: u32) -> Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub fn write_u64(mut self, v: u64) -> Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub fn write_f32(mut self, v: f32) -> Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    // [u16 len][UTF-8 bytes]
    pub fn write_string(mut self, s: &str) -> Self {
        let bytes = s.as_bytes();
        let len = bytes.len() as u16;
        self.buf.extend_from_slice(&len.to_le_bytes());
        self.buf.extend_from_slice(bytes);
        self
    }

    // size 필드 확정 후 버퍼 반환
    pub fn finalize(mut self) -> Vec<u8> {
        let size = self.buf.len() as u16;
        self.buf[0..2].copy_from_slice(&size.to_le_bytes());
        self.buf
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 입력 검증 헬퍼
// ─────────────────────────────────────────────────────────────────────────────

pub fn validate_character_name(name: &str) -> Result<()> {
    if name.len() < 2 || name.len() > 20 {
        bail!("캐릭터명 길이 오류 (2~20자): {}", name.len());
    }
    if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        bail!("캐릭터명 허용 문자: 영숫자, 언더스코어");
    }
    Ok(())
}

pub fn validate_account_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 64 {
        bail!("계정명 길이 오류 (1~64자): {}", name.len());
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// 요청 구조체 — decode + validate
// ─────────────────────────────────────────────────────────────────────────────

pub struct CreateCharacterReq {
    pub session_id:     u64,
    pub account_id:     u32,
    pub character_name: String,
}

impl CreateCharacterReq {
    pub fn decode(r: &mut PacketReader) -> Result<Self> {
        Ok(Self {
            session_id:     r.read_u64()?,
            account_id:     r.read_u32()?,
            character_name: r.read_string()?,
        })
    }
    pub fn validate(&self) -> Result<()> {
        validate_character_name(&self.character_name)
    }
}

pub struct LoadCharacterReq {
    pub session_id:   u64,
    pub character_id: u32,
}

impl LoadCharacterReq {
    pub fn decode(r: &mut PacketReader) -> Result<Self> {
        Ok(Self {
            session_id:   r.read_u64()?,
            character_id: r.read_u32()?,
        })
    }
}

// 게임 서버에서 저장 요청 시 보내는 캐릭터 전체 상태
pub struct SaveCharacterReq {
    pub session_id:   u64,
    pub character_id: u32,
    pub level:        u32,
    pub experience:   u64,
    pub hp:           u32,
    pub zone_id:      u32,
    pub pos_x:        f32,
    pub pos_y:        f32,
    pub pos_z:        f32,
    pub orientation:  f32,
}

impl SaveCharacterReq {
    pub fn decode(r: &mut PacketReader) -> Result<Self> {
        Ok(Self {
            session_id:   r.read_u64()?,
            character_id: r.read_u32()?,
            level:        r.read_u32()?,
            experience:   r.read_u64()?,
            hp:           r.read_u32()?,
            zone_id:      r.read_u32()?,
            pos_x:        r.read_f32()?,
            pos_y:        r.read_f32()?,
            pos_z:        r.read_f32()?,
            orientation:  r.read_f32()?,
        })
    }
    pub fn validate(&self) -> Result<()> {
        if self.level == 0 || self.level > 9999 {
            bail!("레벨 범위 오류: {}", self.level);
        }
        if self.hp == 0 {
            bail!("hp는 0 이상이어야 합니다");
        }
        Ok(())
    }
}

pub struct CheckAccountReq {
    pub session_id:   u64,
    pub account_name: String,
    pub token:        String,
}

impl CheckAccountReq {
    pub fn decode(r: &mut PacketReader) -> Result<Self> {
        Ok(Self {
            session_id:   r.read_u64()?,
            account_name: r.read_string()?,
            token:        r.read_string()?,
        })
    }
    pub fn validate(&self) -> Result<()> {
        validate_account_name(&self.account_name)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 에러 응답 헬퍼
// ─────────────────────────────────────────────────────────────────────────────
pub fn error_response(res_opcode: u16, session_id: u64, msg: &str) -> Vec<u8> {
    PacketWriter::new(res_opcode)
        .write_u8(0)           // success = false
        .write_u64(session_id)
        .write_string(msg)
        .finalize()
}
