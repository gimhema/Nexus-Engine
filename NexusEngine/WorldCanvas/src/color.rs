//! 색과 팔레트.
//!
//! 그림은 RGBA 가 아니라 **팔레트 문자**로 저장한다. 한 글자가 한 픽셀이라 원본을 텍스트로
//! 읽고 고칠 수 있고, 색을 바꿀 때는 팔레트 한 줄만 고치면 된다.

use std::fmt;

/// 투명 — 모든 문서에서 예약되어 있고 팔레트에 적지 않는다.
pub const TRANSPARENT: u8 = b'.';
/// "그대로 둔다" — `grid` / `patch` 안에서만 쓴다.
pub const KEEP: u8 = b'~';

/// sRGB 8bit RGBA.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Rgba(pub [u8; 4]);

impl Rgba {
    pub const CLEAR: Self = Self([0, 0, 0, 0]);

    /// `#rrggbb` 또는 `#rrggbbaa`.
    pub fn parse_hex(s: &str) -> Result<Self, String> {
        let hex = s
            .strip_prefix('#')
            .ok_or_else(|| format!("색은 #rrggbb 형식이어야 함: {s}"))?;
        if !(hex.len() == 6 || hex.len() == 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("색은 #rrggbb 또는 #rrggbbaa 형식이어야 함: {s}"));
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0);
        let a = if hex.len() == 8 { byte(6) } else { 0xFF };
        Ok(Self([byte(0), byte(2), byte(4), a]))
    }

    /// 알파가 0 이면 RGB 와 무관하게 같은 투명으로 본다.
    #[must_use]
    pub fn normalized(self) -> Self {
        if self.0[3] == 0 { Self::CLEAR } else { self }
    }
}

impl fmt::Display for Rgba {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [r, g, b, a] = self.0;
        if a == 0xFF {
            write!(f, "#{r:02x}{g:02x}{b:02x}")
        } else {
            write!(f, "#{r:02x}{g:02x}{b:02x}{a:02x}")
        }
    }
}

/// 팔레트 문자로 쓸 수 있는가.
///
/// `.`(투명)·`~`(유지)는 예약, `/` 는 주석(`//`)과, `#` 은 색 표기와 헷갈리므로 막는다.
#[must_use]
pub fn is_palette_key(c: u8) -> bool {
    c.is_ascii_graphic() && !matches!(c, TRANSPARENT | KEEP | b'/' | b'#' | b'=')
}

/// 문자 → 색. 적은 순서를 유지한다 (다시 쓸 때 순서가 바뀌지 않게).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Palette {
    entries: Vec<(u8, Rgba)>,
}

impl Palette {
    pub fn insert(&mut self, key: u8, color: Rgba) -> Result<(), String> {
        if !is_palette_key(key) {
            return Err(format!("'{}' 는 팔레트 문자로 쓸 수 없음", key as char));
        }
        if self.get(key).is_some() {
            return Err(format!("팔레트 문자 '{}' 가 두 번 나옴", key as char));
        }
        self.entries.push((key, color));
        Ok(())
    }

    /// 문자의 색. 투명 문자는 항상 [`Rgba::CLEAR`].
    #[must_use]
    pub fn get(&self, key: u8) -> Option<Rgba> {
        if key == TRANSPARENT {
            return Some(Rgba::CLEAR);
        }
        self.entries
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, c)| *c)
    }

    /// 그림 안에 올 수 있는 문자인가 (팔레트 문자 또는 투명).
    #[must_use]
    pub fn contains(&self, key: u8) -> bool {
        self.get(key).is_some()
    }

    pub fn iter(&self) -> impl Iterator<Item = (u8, Rgba)> + '_ {
        self.entries.iter().copied()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let c = Rgba::parse_hex("#1a2b3c").unwrap();
        assert_eq!(c, Rgba([0x1a, 0x2b, 0x3c, 0xff]));
        assert_eq!(c.to_string(), "#1a2b3c");
        let t = Rgba::parse_hex("#10203080").unwrap();
        assert_eq!(t.to_string(), "#10203080");
        assert!(Rgba::parse_hex("1a2b3c").is_err());
        assert!(Rgba::parse_hex("#12345").is_err());
        assert!(Rgba::parse_hex("#gg0000").is_err());
    }

    #[test]
    fn reserved_keys_are_rejected() {
        let mut p = Palette::default();
        for c in [b'.', b'~', b'/', b'#', b' '] {
            assert!(p.insert(c, Rgba::CLEAR).is_err(), "{}", c as char);
        }
        p.insert(b'K', Rgba::CLEAR).unwrap();
        assert!(p.insert(b'K', Rgba::CLEAR).is_err(), "중복");
        assert!(p.contains(b'.'), "투명은 항상 있다");
    }
}
