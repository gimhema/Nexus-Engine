/// 응답 패킷 직렬화 빌더.
/// 소유권 이동(consuming) 체이닝 — .write_*().finalize() 까지 자연스럽게 연결.
///
/// 포맷: [u16 total_size][u16 opcode][payload...]  (Little-Endian)
pub struct PacketWriter {
    buf: Vec<u8>,
}

impl PacketWriter {
    pub fn new(opcode: u16) -> Self {
        // 헤더 4바이트 예약: [u16 size=0 placeholder][u16 opcode]
        let mut buf = vec![0u8; 4];
        buf[2..4].copy_from_slice(&opcode.to_le_bytes());
        Self { buf }
    }

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

    /// [u16 len][UTF-8 bytes]
    pub fn write_string(mut self, s: &str) -> Self {
        let bytes = s.as_bytes();
        self.buf.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
        self.buf.extend_from_slice(bytes);
        self
    }

    /// size 필드를 확정하고 완성된 버퍼 반환
    pub fn finalize(mut self) -> Vec<u8> {
        let size = self.buf.len() as u16;
        self.buf[0..2].copy_from_slice(&size.to_le_bytes());
        self.buf
    }
}

/// 실패 응답 공통 헬퍼
pub fn error_response(res_opcode: u16, session_id: u64, msg: &str) -> Vec<u8> {
    PacketWriter::new(res_opcode)
        .write_u8(0)
        .write_u64(session_id)
        .write_string(msg)
        .finalize()
}
