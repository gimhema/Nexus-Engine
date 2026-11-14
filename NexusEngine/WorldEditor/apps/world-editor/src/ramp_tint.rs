//! 램프 매핑 색 입히기 — 회색 그림을 고른 색의 **명도 단계(램프)** 로 다시 칠한다.
//!
//! 무채색 시트에 색을 곱하면(`tinted`) 그늘이 그 색의 어두운 판 — 회색빛으로 탁해진다. 손으로 그린
//! 도트는 그늘로 갈수록 색상을 돌린다(따뜻한 색은 붉은·보라, 차가운 색은 푸른 보라) — WorldCanvas
//! `ramp … from #색` 과 같은 규칙. 시트 정의에 `ramp: true` 가 있으면 곱하는 대신 이렇게 칠한다.
//!
//! 밝기는 **곱한 결과와 같게** 둔다 — 색 고르기 감각(색 묶음)이 그대로다. 그 위에 회색이 어두울수록
//! (흰색에서 멀수록) 색상을 그늘 쪽으로 돌리고 채도를 조금 올린다. 흰색은 고른 색 그대로.
//!
//! GPU 셰이더는 곱하기 하나라 건드리지 않는다 — 외형이 정해질 때 CPU 에서 한 번 칠해 텍스처로 올린다
//! (`SpriteLibrary::ensure_ramps`).

/// 그늘 쪽으로 끌려가는 색상각 — 푸른 보라 (WorldCanvas `color.rs` 와 같다).
const SHADOW_HUE: f32 = 285.0;
/// 가장 어두운 쪽(흰색에서 OKLab 명도 1.0 만큼 어두움)의 색상 회전.
const TURN_PER_L: f32 = 70.0;
/// 같은 기준의 채도 증가.
const CHROMA_PER_L: f32 = 0.6;

/// 회색 RGBA(sRGB) 를 `base`(sRGB, 0~1) 램프로 칠한 RGBA. 투명은 그대로.
pub(crate) fn recolor(rgba: &[u8], base: [f32; 4]) -> Vec<u8> {
    let tint = [lin(base[0]), lin(base[1]), lin(base[2])];
    let mut out = rgba.to_vec();
    for px in out.chunks_exact_mut(4) {
        if px[3] == 0 {
            continue;
        }
        let grey = [lin8(px[0]), lin8(px[1]), lin8(px[2])];
        // 흰색에서 얼마나 어두운가 — 그림의 명도
        let dark = (1.0 - oklab(grey).0).clamp(0.0, 1.0);
        // 곱한 결과 — 밝기는 이것을 따른다
        let mult = oklab([grey[0] * tint[0], grey[1] * tint[1], grey[2] * tint[2]]);
        let (l, c, h) = lch(mult);
        let diff = (SHADOW_HUE - h + 540.0).rem_euclid(360.0) - 180.0;
        let turn = diff.signum() * diff.abs().min(TURN_PER_L * dark);
        let rgb = to_srgb_clipped(l, c * (1.0 + CHROMA_PER_L * dark), h + turn);
        px[..3].copy_from_slice(&rgb);
    }
    out
}

fn lin(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn lin8(c: u8) -> f32 {
    lin(f32::from(c) / 255.0)
}

/// 선형 RGB → OKLab (L, a, b).
fn oklab([r, g, b]: [f32; 3]) -> (f32, f32, f32) {
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    (
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    )
}

fn lch((l, a, b): (f32, f32, f32)) -> (f32, f32, f32) {
    (l, a.hypot(b), b.atan2(a).to_degrees().rem_euclid(360.0))
}

/// OKLCH → sRGB 8bit. 범위를 벗어나면 채도를 줄여 맞춘다 (색상·명도는 지킨다).
fn to_srgb_clipped(l: f32, c: f32, h: f32) -> [u8; 3] {
    let (s, co) = h.to_radians().sin_cos();
    let mut c = c;
    loop {
        let (a, b) = (c * co, c * s);
        let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
        let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
        let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
        let rgb = [
            4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_,
            -1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_,
            -0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_,
        ];
        if rgb.iter().all(|v| (-0.001..=1.001).contains(v)) || c <= 0.0 {
            return rgb.map(|v| {
                let v = v.clamp(0.0, 1.0);
                let e = if v <= 0.003_130_8 {
                    v * 12.92
                } else {
                    1.055 * v.powf(1.0 / 2.4) - 0.055
                };
                (e * 255.0).round() as u8
            });
        }
        c = (c - 0.005).max(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hue_of(rgb: &[u8]) -> f32 {
        lch(oklab([lin8(rgb[0]), lin8(rgb[1]), lin8(rgb[2])])).2
    }

    #[test]
    fn white_stays_the_chosen_colour_and_alpha_is_kept() {
        let base = [0.8, 0.3, 0.2, 1.0];
        let out = recolor(&[255, 255, 255, 255, 0, 0, 0, 0], base);
        let want = [204, 77, 51];
        for (got, want) in out[..3].iter().zip(want) {
            assert!(got.abs_diff(want) <= 2, "{out:?}");
        }
        assert_eq!(&out[4..], &[0, 0, 0, 0], "투명은 그대로");
    }

    #[test]
    fn shadows_turn_toward_violet_unlike_multiply() {
        // 따뜻한 주황 — 곱하면 그늘도 주황(색상 그대로), 램프면 붉은·보라 쪽으로 돈다.
        let base = [0.95, 0.6, 0.25, 1.0];
        let grey = 110;
        let ramp = recolor(&[grey, grey, grey, 255], base);
        let g = lin8(grey);
        let mult = oklab([g * lin(base[0]), g * lin(base[1]), g * lin(base[2])]);
        let (h_ramp, h_mult) = (hue_of(&ramp), lch(mult).2);
        let toward = ((SHADOW_HUE - h_mult + 540.0).rem_euclid(360.0) - 180.0).signum();
        let moved = ((h_ramp - h_mult + 540.0).rem_euclid(360.0) - 180.0) * toward;
        assert!(moved > 5.0, "램프 {h_ramp} · 곱하기 {h_mult}");
        // 밝기는 곱한 결과와 비슷하다 — 색 고르기 감각이 그대로
        let l_ramp = oklab([lin8(ramp[0]), lin8(ramp[1]), lin8(ramp[2])]).0;
        assert!((l_ramp - mult.0).abs() < 0.02, "{l_ramp} vs {}", mult.0);
    }
}
