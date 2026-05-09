use anyhow::{bail, Result};

/// 수신 payload 바이트 순차 읽기.
/// 기존 NexusEngine PacketReader/NexusPacketParser 와 동일한 Little-Endian 포맷.
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
            bail!(
                "PacketReader 오버런: pos={} need={} len={}",
                self.pos, n, self.data.len()
            );
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

    /// [u16 len][UTF-8 bytes]
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
