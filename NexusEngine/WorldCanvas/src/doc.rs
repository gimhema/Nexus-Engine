//! `.canvas` 문서 모델.
//!
//! 문서는 **그리는 방법**을 담는다 — 결과 픽셀이 아니라. 프레임은 다른 프레임을 바탕으로
//! (`frame b = a`) 연산을 쌓아 만들 수 있고, 바탕을 고치면 파생 프레임이 함께 바뀐다.
//! 결과 픽셀은 [`crate::resolve`] 가 계산한다.

use std::fmt;

use crate::color::Palette;

/// 문서 형식 버전.
pub const VERSION: u32 = 1;

/// 엔진(`nexus-assets::AnimState`)이 아는 애니메이션 상태.
pub const STATES: [&str; 6] = ["Idle", "Walk", "Attack", "Cast", "Hit", "Die"];

#[derive(Clone, Debug, Default)]
pub struct Document {
    /// 프레임 한 칸의 픽셀 크기 — 시트 칸 크기와 같다.
    pub cell: (u32, u32),
    pub palette: Palette,
    /// 명도 단계 묶음 — `shade`·`light` 가 따라 오르내린다.
    pub ramps: Vec<Ramp>,
    /// 프레임과 부품. 적은 순서를 유지한다.
    pub images: Vec<ImageDef>,
    pub sheet: Option<SheetDef>,
    pub atlas: Option<AtlasDef>,
    pub output: Option<OutputDef>,
}

impl Document {
    #[must_use]
    pub fn image(&self, name: &str) -> Option<&ImageDef> {
        self.images.iter().find(|i| i.name == name)
    }

    #[must_use]
    pub fn ramp(&self, name: &str) -> Option<&Ramp> {
        self.ramps.iter().find(|r| r.name == name)
    }
}

/// 한 재질의 명도 단계 — 팔레트 문자를 **밝은 것부터** 늘어놓은 것.
///
/// `ramp hair WwvxD` 처럼 팔레트에 적는다. 같은 재질의 색을 단계로 묶어 두면 그늘을 "한 단계
/// 어둡게" 처럼 칠할 수 있고, 어느 색이 어느 재질인지 원본에 남는다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ramp {
    pub name: String,
    pub keys: Vec<u8>,
    pub line: usize,
}

impl Ramp {
    /// 문자의 단계 (0 이 가장 밝다).
    #[must_use]
    pub fn index(&self, key: u8) -> Option<usize> {
        self.keys.iter().position(|&k| k == key)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageKind {
    /// 시트 한 칸 크기. 시트에 배치할 수 있다.
    Frame,
    /// 임의 크기 조각 (머리·팔 같은). `stamp` 로 프레임에 붙인다. 시트에는 못 들어간다.
    Part,
}

#[derive(Clone, Debug)]
pub struct ImageDef {
    pub name: String,
    pub kind: ImageKind,
    pub size: (u32, u32),
    /// 바탕 그림. 없으면 투명에서 시작한다.
    pub base: Option<String>,
    pub ops: Vec<OpAt>,
    /// 정의가 시작된 줄 (오류 보고용).
    pub line: usize,
}

/// 연산과 그 줄 번호.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpAt {
    pub op: Op,
    pub line: usize,
}

/// 그리기 연산. 색 인자는 팔레트 문자다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// 그림 전체를 텍스트로. 크기가 정확히 맞아야 한다.
    Grid(Vec<Vec<u8>>),
    /// 텍스트 조각을 `(x, y)` 에 덮어쓴다.
    Patch {
        x: i32,
        y: i32,
        rows: Vec<Vec<u8>>,
    },
    Px {
        x: i32,
        y: i32,
        c: u8,
    },
    Rect {
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        c: u8,
    },
    Box {
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        c: u8,
    },
    Line {
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        c: u8,
    },
    Fill {
        x: i32,
        y: i32,
        c: u8,
    },
    FlipH,
    FlipV,
    Shift {
        dx: i32,
        dy: i32,
    },
    Move {
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        dx: i32,
        dy: i32,
    },
    /// 색 교체. `region` 이 있으면 그 사각형 `(x, y, 가로, 세로)` 안에서만 — 한쪽 그늘, 모양 안 줄무늬.
    Swap {
        from: u8,
        to: u8,
        region: Option<(i32, i32, u32, u32)>,
    },
    Outline {
        c: u8,
    },
    /// 색 있는 외곽선. 색 `c` 픽셀이 안쪽 색 `from` 에 맞닿아 있으면 `to` 로 바꾼다.
    Selout {
        c: u8,
        pairs: Vec<(u8, u8)>,
    },
    /// 왼쪽 절반을 오른쪽에 좌우 대칭으로 복사한다. 가로가 홀수면 가운데 열은 그대로.
    Mirror,
    /// `(x, y, 가로, 세로)` 사각형에 내접하는 채운 타원.
    Ellipse {
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        c: u8,
    },
    /// 채운 다각형. 꼭짓점은 픽셀 좌표 — 픽셀 중심이 안에 들면 칠한다.
    Poly {
        c: u8,
        points: Vec<(i32, i32)>,
    },
    Stamp {
        name: String,
        x: i32,
        y: i32,
    },
    /// 돌려서 올린다 (RotSprite). `pivot` 은 부품 안 회전 중심 픽셀 — 없으면 부품 한가운데.
    /// 돌리지 않은 `stamp` 와 같은 자리에 피벗이 온다. `deg` 양수 = 시계 방향.
    StampRot {
        name: String,
        x: i32,
        y: i32,
        deg: i32,
        pivot: Option<(i32, i32)>,
    },
    /// 램프 색 픽셀을 `steps` 단계 옮긴다 (양수 = 어둡게). 끝에서 멈춘다.
    Shade {
        ramp: String,
        steps: i32,
        region: Option<(i32, i32, u32, u32)>,
    },
    /// 왼쪽 위 빛으로 램프 색 덩어리에 명암을 자동으로 깐다.
    ///
    /// 램프 색 픽셀을 덩어리로 보고 반경 `radius`(0 = 두께에 맞춰 자동)로 흐려 높이로 삼은 뒤, 빛을 등진 쪽은
    /// 최대 `dark` 단계 어둡게, 빛을 받는 쪽은 최대 `light` 단계 밝게 한다. 밑칠 — 손으로 다듬는다.
    Light {
        ramp: String,
        radius: u32,
        dark: u32,
        light: u32,
    },
    /// 안쪽 연산을 **원래 `keys` 색이던 픽셀에만** 적용한다 — 재질 안에서만 결·주름·무늬.
    Mask {
        keys: Vec<u8>,
        ops: Vec<OpAt>,
    },
    /// 부품을 그림 전체에 타일로 깐다 (`(x, y)` 는 무늬 시작점). 보통 `mask` 안에서 쓴다.
    Pattern {
        name: String,
        x: i32,
        y: i32,
    },
    /// 외곽선 계단 안쪽 모서리 — 안쪽 색 `from` 픽셀이 외곽선 `c` 에 세로·가로로 함께 닿으면 `to` 로.
    Aa {
        c: u8,
        pairs: Vec<(u8, u8)>,
    },
    /// 색 `a` 픽셀 중 체크무늬 자리를 `b` 로.
    Dither {
        a: u8,
        b: u8,
        region: Option<(i32, i32, u32, u32)>,
    },
    /// 색 `a` 픽셀 중 `percent`% 를 `b` 로 — 좌표 해시라 늘 같은 자리. 돌·가죽·천의 결.
    Speckle {
        a: u8,
        b: u8,
        percent: u32,
        seed: u32,
    },
}

impl Op {
    /// 이 연산이 쓰는 색 문자들 — 팔레트 검사용.
    #[must_use]
    pub fn colors(&self) -> Vec<u8> {
        match self {
            Self::Grid(rows) | Self::Patch { rows, .. } => rows.concat(),
            Self::Px { c, .. }
            | Self::Rect { c, .. }
            | Self::Box { c, .. }
            | Self::Line { c, .. }
            | Self::Fill { c, .. }
            | Self::Ellipse { c, .. }
            | Self::Poly { c, .. }
            | Self::Outline { c } => vec![*c],
            Self::Swap { from, to, .. } => vec![*from, *to],
            Self::Selout { c, pairs } | Self::Aa { c, pairs } => std::iter::once(*c)
                .chain(pairs.iter().flat_map(|(from, to)| [*from, *to]))
                .collect(),
            Self::Dither { a, b, .. } | Self::Speckle { a, b, .. } => vec![*a, *b],
            Self::Mask { keys, ops } => keys
                .iter()
                .copied()
                .chain(ops.iter().flat_map(|at| at.op.colors()))
                .collect(),
            Self::FlipH
            | Self::FlipV
            | Self::Shift { .. }
            | Self::Move { .. }
            | Self::Mirror
            | Self::Stamp { .. }
            | Self::StampRot { .. }
            | Self::Shade { .. }
            | Self::Light { .. }
            | Self::Pattern { .. } => Vec::new(),
        }
    }
}

/// 문서 문법 그대로 쓴다 — `import` 가 만든 문서를 다시 텍스트로 내보낼 때.
impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ch = |c: &u8| *c as char;
        let block = |f: &mut fmt::Formatter<'_>, head: &str, rows: &[Vec<u8>]| {
            writeln!(f, "{head}")?;
            for r in rows {
                writeln!(f, "    {}", String::from_utf8_lossy(r))?;
            }
            write!(f, "end")
        };
        match self {
            Self::Grid(rows) => block(f, "grid", rows),
            Self::Patch { x, y, rows } => block(f, &format!("patch {x} {y}"), rows),
            Self::Px { x, y, c } => write!(f, "px {x} {y} {}", ch(c)),
            Self::Rect { x, y, w, h, c } => write!(f, "rect {x} {y} {w} {h} {}", ch(c)),
            Self::Box { x, y, w, h, c } => write!(f, "box {x} {y} {w} {h} {}", ch(c)),
            Self::Line { x0, y0, x1, y1, c } => write!(f, "line {x0} {y0} {x1} {y1} {}", ch(c)),
            Self::Fill { x, y, c } => write!(f, "fill {x} {y} {}", ch(c)),
            Self::FlipH => write!(f, "flip_h"),
            Self::FlipV => write!(f, "flip_v"),
            Self::Shift { dx, dy } => write!(f, "shift {dx} {dy}"),
            Self::Move { x, y, w, h, dx, dy } => write!(f, "move {x} {y} {w} {h} {dx} {dy}"),
            Self::Swap { from, to, region } => {
                write!(f, "swap {} {}", ch(from), ch(to))?;
                match region {
                    Some((x, y, w, h)) => write!(f, " {x} {y} {w} {h}"),
                    None => Ok(()),
                }
            }
            Self::Outline { c } => write!(f, "outline {}", ch(c)),
            Self::Selout { c, pairs } => {
                write!(f, "selout {}", ch(c))?;
                for (from, to) in pairs {
                    write!(f, " {}{}", ch(from), ch(to))?;
                }
                Ok(())
            }
            Self::Mirror => write!(f, "mirror"),
            Self::Ellipse { x, y, w, h, c } => write!(f, "ellipse {x} {y} {w} {h} {}", ch(c)),
            Self::Poly { c, points } => {
                write!(f, "poly {}", ch(c))?;
                for (x, y) in points {
                    write!(f, " {x} {y}")?;
                }
                Ok(())
            }
            Self::Stamp { name, x, y } => write!(f, "stamp {name} {x} {y}"),
            Self::StampRot {
                name,
                x,
                y,
                deg,
                pivot,
            } => {
                write!(f, "stamp_rot {name} {x} {y} {deg}")?;
                match pivot {
                    Some((px, py)) => write!(f, " {px} {py}"),
                    None => Ok(()),
                }
            }
            Self::Shade {
                ramp,
                steps,
                region,
            } => {
                write!(f, "shade {ramp} {steps}")?;
                match region {
                    Some((x, y, w, h)) => write!(f, " {x} {y} {w} {h}"),
                    None => Ok(()),
                }
            }
            Self::Light {
                ramp,
                radius,
                dark,
                light,
            } => write!(f, "light {ramp} {radius} {dark} {light}"),
            Self::Mask { keys, ops } => {
                writeln!(f, "mask {}", String::from_utf8_lossy(keys))?;
                for at in ops {
                    writeln!(f, "{}", at.op)?;
                }
                write!(f, "end")
            }
            Self::Pattern { name, x, y } => write!(f, "pattern {name} {x} {y}"),
            Self::Aa { c, pairs } => {
                write!(f, "aa {}", ch(c))?;
                for (from, to) in pairs {
                    write!(f, " {}{}", ch(from), ch(to))?;
                }
                Ok(())
            }
            Self::Dither { a, b, region } => {
                write!(f, "dither {} {}", ch(a), ch(b))?;
                match region {
                    Some((x, y, w, h)) => write!(f, " {x} {y} {w} {h}"),
                    None => Ok(()),
                }
            }
            Self::Speckle {
                a,
                b,
                percent,
                seed,
            } => {
                write!(f, "speckle {} {} {percent} {seed}", ch(a), ch(b))
            }
        }
    }
}

/// 시트 배치와 메타데이터 — `.sheet.ron` 으로 나간다.
#[derive(Clone, Debug)]
pub struct SheetDef {
    pub directions: u32,
    pub direction_rows: Option<Vec<u32>>,
    pub pixels_per_meter: Option<f32>,
    pub tinted: bool,
    pub clips: Vec<ClipDef>,
    pub line: usize,
}

impl SheetDef {
    /// 시트 열 수 — 가장 긴 클립의 프레임 수.
    #[must_use]
    pub fn columns(&self) -> u32 {
        self.clips.iter().map(ClipDef::frames).max().unwrap_or(0)
    }

    /// 시트 행 수.
    #[must_use]
    pub fn rows(&self) -> u32 {
        self.clips.len() as u32 * self.directions
    }

    /// 클립 `i` 의 시작 행.
    #[must_use]
    pub fn clip_row(&self, i: usize) -> u32 {
        i as u32 * self.directions
    }
}

/// 클립 하나. `rows[d]` 가 방향 `d` 의 프레임 이름들이다.
#[derive(Clone, Debug)]
pub struct ClipDef {
    pub state: String,
    pub frame_ms: u64,
    pub looping: bool,
    pub rows: Vec<Vec<String>>,
    pub line: usize,
}

impl ClipDef {
    #[must_use]
    pub fn frames(&self) -> u32 {
        self.rows.first().map_or(0, Vec::len) as u32
    }
}

/// 크기가 제각각인 그림(건물·나무 같은 정적 오브젝트)을 한 장에 모은다.
///
/// 행마다 왼쪽부터 붙이고, 행 높이는 그 행에서 가장 큰 그림을 따른다. 애니메이션 시트가 아니므로
/// `.sheet.ron` 은 없고, 대신 `build` 가 그림마다 픽셀 사각형을 출력한다 (WorldEditor `terrain.ron`
/// 의 `px` 에 그대로 넣는다).
#[derive(Clone, Debug)]
pub struct AtlasDef {
    pub rows: Vec<Vec<String>>,
    pub line: usize,
}

/// 아틀라스 안 그림 하나의 자리.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placed {
    pub name: String,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl AtlasDef {
    /// 배치 계산 — 그림 크기만 보면 되므로 픽셀을 계산하기 전에 할 수 있다.
    #[must_use]
    pub fn place(&self, doc: &Document) -> (Vec<Placed>, u32, u32) {
        let mut out = Vec::new();
        let (mut y, mut width) = (0, 0);
        for row in &self.rows {
            let (mut x, mut row_h) = (0, 0);
            for name in row {
                let (w, h) = doc.image(name).map_or((0, 0), |i| i.size);
                out.push(Placed {
                    name: name.clone(),
                    x,
                    y,
                    w,
                    h,
                });
                x += w;
                row_h = row_h.max(h);
            }
            width = width.max(x);
            y += row_h;
        }
        (out, width, y)
    }
}

/// 결과물 경로. `.canvas` 파일 폴더 기준.
#[derive(Clone, Debug)]
pub struct OutputDef {
    pub image: String,
    /// 시트 정의 경로 — `sheet` 블록이 있을 때만.
    pub sheet: Option<String>,
    /// `.sheet.ron` 의 `image` 필드에 쓸 값. 없으면 정의 파일 기준 상대 경로를 계산한다.
    /// 빈 문자열이면 에디터 내장 그림을 뜻한다.
    pub image_ref: Option<String>,
}
