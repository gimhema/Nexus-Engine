//! `sprites/*.canvas` 와 그것이 가리키는 결과물(PNG)이 어긋나지 않았는지.
//!
//! 원본만 고치고 `build` 를 잊으면 에디터는 옛 그림을 보여 준다. 반대로 PNG 를 다른 툴로
//! 고치면 원본이 낡는다. 어느 쪽이든 여기서 잡는다 — 알파 0 픽셀은 RGB 와 무관하게 같은 투명으로 본다.

use std::path::Path;

use worldcanvas::export::{build_sheet, decode_png};
use worldcanvas::parse::parse;

fn normalized(data: &[u8]) -> Vec<[u8; 4]> {
    data.chunks_exact(4)
        .map(|p| {
            if p[3] == 0 {
                [0; 4]
            } else {
                [p[0], p[1], p[2], p[3]]
            }
        })
        .collect()
}

#[test]
fn built_sheets_match_the_shipped_pictures() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("sprites");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "canvas") {
            continue;
        }
        let name = path.display().to_string();
        let doc = parse(&std::fs::read_to_string(&path).unwrap())
            .unwrap_or_else(|e| panic!("{name}:{e}"));
        let Some(output) = &doc.output else { continue };

        let built = build_sheet(&doc).unwrap_or_else(|e| panic!("{name}:{e}"));
        let shipped_path = dir.join(&output.image);
        let shipped = std::fs::read(&shipped_path)
            .unwrap_or_else(|e| panic!("{name}: {} — {e}", shipped_path.display()));
        let shipped = decode_png(&shipped).unwrap();

        assert_eq!(
            (built.width, built.height),
            (shipped.width, shipped.height),
            "{name}: 크기가 다름 — `worldcanvas build` 를 다시 돌릴 것"
        );
        let (a, b) = (normalized(&built.data), normalized(&shipped.data));
        if let Some(i) = a.iter().zip(&b).position(|(x, y)| x != y) {
            let (x, y) = (i as u32 % built.width, i as u32 / built.width);
            panic!(
                "{name}: ({x}, {y}) 픽셀이 다름 — 원본 {:?}, 결과물 {:?}. `worldcanvas build` 를 다시 돌릴 것",
                a[i], b[i]
            );
        }
        checked += 1;
    }
    assert!(checked > 0, "검사한 원본이 없음");
}
