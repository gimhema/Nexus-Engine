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
#[derive(Clone, Debug)]
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
    /// 왼쪽 절반을 오른쪽에 좌우 대칭으로 복사한다. 가로가 홀수면 가운데 열은 그대로.
    Mirror,
    Stamp {
        name: String,
        x: i32,
        y: i32,
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
            | Self::Outline { c } => vec![*c],
            Self::Swap { from, to, .. } => vec![*from, *to],
            Self::FlipH
            | Self::FlipV
            | Self::Shift { .. }
            | Self::Move { .. }
            | Self::Mirror
            | Self::Stamp { .. } => Vec::new(),
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
            Self::Mirror => write!(f, "mirror"),
            Self::Stamp { name, x, y } => write!(f, "stamp {name} {x} {y}"),
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
