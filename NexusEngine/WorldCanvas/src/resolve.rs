//! 문서 → 픽셀. 바탕을 따라 올라가 연산을 차례로 적용한다.
//!
//! 한 번 계산한 그림은 캐시한다 — 걷기 프레임 여럿이 같은 바탕을 쓰는 게 보통이다.
//! 바탕·스탬프가 돌고 돌면(`a = b`, `b = a`) 오류로 끊는다.

use std::collections::HashMap;

use crate::bitmap::Bitmap;
use crate::doc::{Document, Op};
use crate::error::{Error, Result};

#[derive(Debug)]
pub struct Resolver<'a> {
    doc: &'a Document,
    done: HashMap<String, Bitmap>,
    /// 지금 계산 중인 이름들 — 순환 검출용 경로.
    stack: Vec<String>,
}

impl<'a> Resolver<'a> {
    #[must_use]
    pub fn new(doc: &'a Document) -> Self {
        Self {
            doc,
            done: HashMap::new(),
            stack: Vec::new(),
        }
    }

    /// 이름의 최종 그림.
    pub fn get(&mut self, name: &str) -> Result<Bitmap> {
        if let Some(b) = self.done.get(name) {
            return Ok(b.clone());
        }
        let doc = self.doc;
        let def = doc
            .image(name)
            .ok_or_else(|| Error::new(format!("`{name}` 이(가) 없음")))?;
        if self.stack.iter().any(|s| s == name) {
            let mut path = self.stack.clone();
            path.push(name.to_owned());
            return Err(Error::at(
                def.line,
                format!("순환 참조: {}", path.join(" → ")),
            ));
        }
        self.stack.push(name.to_owned());

        let result = (|| {
            let mut bm = match &def.base {
                Some(base) => self.get(base)?,
                None => Bitmap::new(def.size.0, def.size.1),
            };
            for at in &def.ops {
                self.apply(&mut bm, &at.op)
                    .map_err(|e| Error::at(at.line, format!("{name}: {}", e.msg)))?;
            }
            Ok(bm)
        })();

        self.stack.pop();
        let bm = result?;
        self.done.insert(name.to_owned(), bm.clone());
        Ok(bm)
    }

    fn apply(&mut self, bm: &mut Bitmap, op: &Op) -> Result<()> {
        match op {
            Op::Grid(rows) => bm.patch(0, 0, rows),
            Op::Patch { x, y, rows } => bm.patch(*x, *y, rows),
            Op::Px { x, y, c } => bm.set(*x, *y, *c),
            Op::Rect { x, y, w, h, c } => bm.rect(*x, *y, *w, *h, *c),
            Op::Box { x, y, w, h, c } => bm.frame_rect(*x, *y, *w, *h, *c),
            Op::Line { x0, y0, x1, y1, c } => bm.line(*x0, *y0, *x1, *y1, *c),
            Op::Fill { x, y, c } => bm.fill(*x, *y, *c),
            Op::FlipH => bm.flip_h(),
            Op::FlipV => bm.flip_v(),
            Op::Shift { dx, dy } => bm.shift(*dx, *dy),
            Op::Move { x, y, w, h, dx, dy } => bm.move_region(*x, *y, *w, *h, *dx, *dy),
            Op::Swap { from, to, region } => match region {
                Some((x, y, w, h)) => bm.swap_in(*from, *to, *x, *y, *w, *h),
                None => bm.swap(*from, *to),
            },
            Op::Outline { c } => bm.outline(*c),
            Op::Mirror => bm.mirror(),
            Op::Stamp { name, x, y } => {
                let src = self.get(name)?;
                bm.stamp(&src, *x, *y);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;

    fn doc(body: &str) -> Document {
        parse(&format!(
            "canvas 1\ncell 3 2\npalette\n  A #ff0000\n  B #0000ff\nend\n{body}"
        ))
        .unwrap()
    }

    #[test]
    fn derived_frames_follow_their_base() {
        let d = doc("
frame a
  grid
    AB.
    ...
  end
end
frame b = a
  flip_h
  swap A B
end
");
        let mut r = Resolver::new(&d);
        assert_eq!(r.get("b").unwrap().rows(), [".BB", "..."]);
        assert_eq!(r.get("a").unwrap().rows(), ["AB.", "..."], "바탕은 그대로");
    }

    #[test]
    fn stamps_compose_parts() {
        let d = doc("
part dot 1 1
  px 0 0 B
end
frame a
  stamp dot 1 1
  stamp dot 2 1
end
");
        assert_eq!(Resolver::new(&d).get("a").unwrap().rows(), ["...", ".BB"]);
    }

    #[test]
    fn cycles_are_reported_with_their_path() {
        let d = doc("
frame a = b
end
frame b = a
end
");
        let e = Resolver::new(&d).get("a").unwrap_err().to_string();
        assert!(e.contains("a → b → a"), "{e}");

        let d = doc("
frame a
  stamp a 0 0
end
");
        let e = Resolver::new(&d).get("a").unwrap_err().to_string();
        assert!(e.contains("순환"), "{e}");
    }
}
