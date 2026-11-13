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

    /// OKLab — 눈에 보이는 차이가 거리와 비례하는 색 공간. 명도 `L` 은 0~1.
    #[must_use]
    pub fn oklab(self) -> Oklab {
        let lin = |c: u8| {
            let c = f32::from(c) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let [r, g, b] = [lin(self.0[0]), lin(self.0[1]), lin(self.0[2])];
        let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
        let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
        let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
        Oklab {
            l: 0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
            a: 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
            b: 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
        }
    }
}

/// OKLab 색. 램프 생성과 `lint` 의 색 비교에 쓴다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklab {
    pub l: f32,
    pub a: f32,
    pub b: f32,
}

impl Oklab {
    #[must_use]
    pub fn distance(self, o: Self) -> f32 {
        ((self.l - o.l).powi(2) + (self.a - o.a).powi(2) + (self.b - o.b).powi(2)).sqrt()
    }

    /// 채도·색상각(도) — OKLCH.
    #[must_use]
    pub fn lch(self) -> (f32, f32, f32) {
        let c = self.a.hypot(self.b);
        let h = self.b.atan2(self.a).to_degrees().rem_euclid(360.0);
        (self.l, c, h)
    }

    #[must_use]
    pub fn from_lch(l: f32, c: f32, h: f32) -> Self {
        let (s, co) = h.to_radians().sin_cos();
        Self {
            l,
            a: c * co,
            b: c * s,
        }
    }

    /// sRGB 로. 범위를 벗어나면 채도를 줄여 맞춘다 (색상·명도는 지킨다).
    #[must_use]
    pub fn to_rgba(self) -> Rgba {
        let (l, c, h) = self.lch();
        let mut c = c;
        loop {
            if let Some(rgb) = Self::from_lch(l, c, h).to_srgb() {
                return Rgba([rgb[0], rgb[1], rgb[2], 0xFF]);
            }
            if c <= 0.0 {
                let v = (l.clamp(0.0, 1.0) * 255.0).round() as u8;
                return Rgba([v, v, v, 0xFF]);
            }
            c = (c - 0.005).max(0.0);
        }
    }

    fn to_srgb(self) -> Option<[u8; 3]> {
        let l = (self.l + 0.396_337_78 * self.a + 0.215_803_76 * self.b).powi(3);
        let m = (self.l - 0.105_561_346 * self.a - 0.063_854_17 * self.b).powi(3);
        let s = (self.l - 0.089_484_18 * self.a - 1.291_485_5 * self.b).powi(3);
        let lin = [
            4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
            -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
            -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
        ];
        let mut out = [0; 3];
        for (o, v) in out.iter_mut().zip(lin) {
            if !(-0.001..=1.001).contains(&v) {
                return None;
            }
            let v = v.clamp(0.0, 1.0);
            let e = if v <= 0.003_130_8 {
                v * 12.92
            } else {
                1.055 * v.powf(1.0 / 2.4) - 0.055
            };
            *o = (e * 255.0).round() as u8;
        }
        Some(out)
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

/// 그늘 쪽으로 갈수록 끌려가는 색상각 — 푸른 보라. 밝은 쪽은 노랑으로 끌린다.
const SHADOW_HUE: f32 = 285.0;
const LIGHT_HUE: f32 = 95.0;

/// 램프 생성 — `base` 를 `at` 번째(0 이 가장 밝음)에 두고 `n` 단계를 만든다.
///
/// 한 단계마다 OKLab 명도를 `step` 만큼 바꾸고, 색상각을 `hue` 도씩 그늘 쪽은 푸른 보라로,
/// 밝은 쪽은 노랑으로 돌린다 (목표를 넘어가지는 않는다). 명도만 바꾼 램프는 그늘이 탁해진다 —
/// 색상을 돌리고 채도를 지켜야 그늘도 색을 가진다.
#[must_use]
pub fn generate_ramp(base: Rgba, n: usize, at: usize, step: f32, hue: f32) -> Vec<Rgba> {
    let (l0, c0, h0) = base.oklab().lch();
    (0..n)
        .map(|i| {
            let k = i as f32 - at as f32; // 양수 = 더 어둡다
            let target = if k > 0.0 { SHADOW_HUE } else { LIGHT_HUE };
            let diff = (target - h0 + 540.0).rem_euclid(360.0) - 180.0;
            let turn = diff.signum() * diff.abs().min(hue * k.abs());
            // 그늘은 채도를 조금 올리고(탁해지지 않게), 하이라이트는 빛에 바래게 내린다.
            // 아주 어두운 단계는 sRGB 범위가 좁아 to_rgba 가 알아서 채도를 줄인다.
            let c = if k > 0.0 {
                c0 * (1.0 + 0.05 * k)
            } else {
                c0 * (1.0 + 0.12 * k).max(0.2)
            };
            let l = (l0 - step * k).clamp(0.02, 0.99);
            let mut out = Oklab::from_lch(l, c, h0 + turn).to_rgba();
            out.0[3] = base.0[3];
            out
        })
        .collect()
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
