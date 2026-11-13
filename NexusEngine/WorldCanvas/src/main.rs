//! `worldcanvas` 명령줄.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use worldcanvas::analyze;
use worldcanvas::doc::{Document, ImageKind};
use worldcanvas::error::{Error, Result};
use worldcanvas::export::{self, Rgba8};
use worldcanvas::import::{self, ClipSpec};
use worldcanvas::lint;
use worldcanvas::parse::{parse, parse_file};
use worldcanvas::preview;
use worldcanvas::resolve::Resolver;

const USAGE: &str = "\
worldcanvas — 텍스트 원본(.canvas)으로 픽셀 스프라이트 시트를 만든다

  check   <원본.canvas>                         문법·참조·순환 검사
  list    <원본.canvas>                         팔레트·그림·시트 배치 요약
  show    <원본.canvas> <이름>                   계산된 그림을 텍스트로 출력
  preview <원본.canvas> [-o 파일.png] [--scale N] [--grid] [--all | --frames a,b,c]
          [--value | --silhouette] [--actual] [--onion]
                                                확대 미리보기 (기본: 시트 배치, ./preview/<이름>.png)
                                                명도·실루엣 보기, 1·2배 덧붙임, 이전 프레임 잔상
  lint    <원본.canvas> [--frames a,b] [--detail] [--limit N] [-o 표시.png] [--scale N]
                                                도트 품질 검사 — 계단·밴딩·줄무늬·고립 픽셀·명도·팔레트·움직임
  build   <원본.canvas> [--out 폴더]             시트 PNG + .sheet.ron 출력 (경로는 output 블록)
  analyze <그림.png> [--crop x,y,w,h] [--colors N]
                                                참고 그림의 색 수·재질별 램프·단계 폭·색상 회전, 팔레트 초안
  import  <시트.png> --cell WxH -o <원본.canvas> [--directions N] [--clip 상태:프레임:ms:loop|once]...
                                                기존 PNG 를 원본 형식으로 가져오기
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("오류: {e}");
            ExitCode::FAILURE
        }
    }
}

/// 위치 인자와 `--플래그 값` 을 나눈다. 값 없는 플래그는 `BOOL_FLAGS` 에 적는다.
#[derive(Debug, Default)]
struct Args {
    positional: Vec<String>,
    flags: Vec<(String, String)>,
}

const BOOL_FLAGS: &[&str] = &[
    "--grid",
    "--all",
    "--detail",
    "--value",
    "--silhouette",
    "--actual",
    "--onion",
];

impl Args {
    fn parse(raw: &[String]) -> Result<Self> {
        let mut out = Self::default();
        let mut it = raw.iter();
        while let Some(a) = it.next() {
            if a.starts_with('-') && a.len() > 1 {
                if BOOL_FLAGS.contains(&a.as_str()) {
                    out.flags.push((a.clone(), String::new()));
                } else {
                    let v = it
                        .next()
                        .ok_or_else(|| Error::new(format!("{a} 뒤에 값이 없음")))?;
                    out.flags.push((a.clone(), v.clone()));
                }
            } else {
                out.positional.push(a.clone());
            }
        }
        Ok(out)
    }

    fn flag(&self, names: &[&str]) -> Option<&str> {
        self.flags
            .iter()
            .rev()
            .find(|(k, _)| names.contains(&k.as_str()))
            .map(|(_, v)| v.as_str())
    }

    fn all(&self, name: &str) -> Vec<&str> {
        self.flags
            .iter()
            .filter(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|(k, _)| k == name)
    }

    fn pos(&self, i: usize, what: &str) -> Result<&str> {
        self.positional
            .get(i)
            .map(String::as_str)
            .ok_or_else(|| Error::new(format!("{what} 가 필요함\n\n{USAGE}")))
    }
}

fn run(raw: &[String]) -> Result<()> {
    let Some((cmd, rest)) = raw.split_first() else {
        print!("{USAGE}");
        return Ok(());
    };
    let args = Args::parse(rest)?;
    match cmd.as_str() {
        "check" => cmd_check(&args),
        "list" => cmd_list(&args),
        "show" => cmd_show(&args),
        "preview" => cmd_preview(&args),
        "lint" => cmd_lint(&args),
        "build" => cmd_build(&args),
        "import" => cmd_import(&args),
        "analyze" => cmd_analyze(&args),
        "help" | "-h" | "--help" => {
            print!("{USAGE}");
            Ok(())
        }
        other => Err(Error::new(format!("알 수 없는 명령: {other}\n\n{USAGE}"))),
    }
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| Error::new(format!("{}: {e}", dir.display())))?;
    }
    std::fs::write(path, bytes).map_err(|e| Error::new(format!("{}: {e}", path.display())))
}

/// 원본을 읽고 모든 그림을 한 번 계산해 본다. 오류에 파일 이름을 붙인다.
fn load(path: &str) -> Result<Document> {
    let tag = |e: Error| Error::new(format!("{path}:{e}"));
    let doc = parse_file(Path::new(path)).map_err(tag)?;
    export::resolve_all(&doc).map_err(tag)?;
    Ok(doc)
}

fn cmd_check(a: &Args) -> Result<()> {
    let path = a.pos(0, "원본 파일")?;
    let doc = load(path)?;
    let frames = doc
        .images
        .iter()
        .filter(|i| i.kind == ImageKind::Frame)
        .count();
    println!(
        "{path}: 문제 없음 — 칸 {}x{}, 색 {}, 프레임 {frames}, 부품 {}",
        doc.cell.0,
        doc.cell.1,
        doc.palette.len(),
        doc.images.len() - frames
    );
    Ok(())
}

fn cmd_list(a: &Args) -> Result<()> {
    let path = a.pos(0, "원본 파일")?;
    let doc = load(path)?;
    println!("칸 {}x{}", doc.cell.0, doc.cell.1);
    println!("\n팔레트 ({}색)", doc.palette.len());
    for (k, c) in doc.palette.iter() {
        println!("  {} {c}", k as char);
    }
    println!("\n그림 ({})", doc.images.len());
    for img in &doc.images {
        let kind = match img.kind {
            ImageKind::Frame => "frame",
            ImageKind::Part => "part ",
        };
        let base = img
            .base
            .as_ref()
            .map(|b| format!(" = {b}"))
            .unwrap_or_default();
        println!(
            "  {kind} {}{base}  ({}x{}, 연산 {}, {}줄)",
            img.name,
            img.size.0,
            img.size.1,
            img.ops.len(),
            img.line
        );
    }
    if let Some(sheet) = &doc.sheet {
        println!(
            "\n시트 {}열 × {}행, 방향 {}, tinted {}",
            sheet.columns(),
            sheet.rows(),
            sheet.directions,
            sheet.tinted
        );
        for (k, c) in sheet.clips.iter().enumerate() {
            let mode = if c.looping { "loop" } else { "once" };
            println!(
                "  {} — {}행부터, {}프레임, {}ms {mode}",
                c.state,
                sheet.clip_row(k),
                c.frames(),
                c.frame_ms
            );
            for (d, row) in c.rows.iter().enumerate() {
                println!("    방향 {d}: {}", row.join(" "));
            }
        }
    }
    Ok(())
}

fn cmd_show(a: &Args) -> Result<()> {
    let path = a.pos(0, "원본 파일")?;
    let name = a.pos(1, "그림 이름")?;
    let doc = load(path)?;
    let bm = Resolver::new(&doc).get(name)?;
    // 열 번호 눈금 — 좌표를 세기 쉽게 10 단위로.
    let w = bm.width() as usize;
    let tens: String = (0..w)
        .map(|x| {
            if x % 10 == 0 {
                char::from(b'0' + (x / 10 % 10) as u8)
            } else {
                ' '
            }
        })
        .collect();
    let ones: String = (0..w).map(|x| char::from(b'0' + (x % 10) as u8)).collect();
    println!("    {tens}\n    {ones}");
    for (y, row) in bm.rows().iter().enumerate() {
        println!("{y:>3} {row}");
    }
    Ok(())
}

fn cmd_preview(a: &Args) -> Result<()> {
    let path = a.pos(0, "원본 파일")?;
    let doc = load(path)?;
    let layout = if let Some(list) = a.flag(&["--frames"]) {
        vec![list.split(',').map(|s| Some(s.trim().to_owned())).collect()]
    } else if a.has("--all") {
        preview::all_layout(&doc, 8)
    } else if let Some(atlas) = &doc.atlas {
        atlas
            .rows
            .iter()
            .map(|r| r.iter().cloned().map(Some).collect())
            .collect()
    } else {
        preview::sheet_layout(&doc).unwrap_or_else(|| preview::all_layout(&doc, 8))
    };
    let scale = match a.flag(&["--scale"]) {
        Some(s) => s
            .parse()
            .map_err(|_| Error::new(format!("--scale 은 정수: {s}")))?,
        None => 4,
    };
    let opt = preview::Options {
        scale,
        grid: a.has("--grid"),
        mode: if a.has("--value") {
            preview::Mode::Value
        } else if a.has("--silhouette") {
            preview::Mode::Silhouette
        } else {
            preview::Mode::Color
        },
        onion: a.has("--onion"),
    };
    let mut img = preview::render(&doc, &layout, opt)?;
    if a.has("--actual") {
        img = preview::with_actual(img, &doc, &layout, opt)?;
    }
    let out = match a.flag(&["-o", "--out"]) {
        Some(o) => PathBuf::from(o),
        None => {
            let stem = Path::new(path)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy();
            PathBuf::from("preview").join(format!("{stem}.png"))
        }
    };
    write(&out, &export::encode_png(&img)?)?;
    println!("{} ({}x{})", out.display(), img.width, img.height);
    Ok(())
}

fn cmd_lint(a: &Args) -> Result<()> {
    let path = a.pos(0, "원본 파일")?;
    let doc = load(path)?;
    let names: Vec<String> = match a.flag(&["--frames"]) {
        Some(list) => list.split(',').map(|s| s.trim().to_owned()).collect(),
        None => lint::targets(&doc),
    };
    let limit = match a.flag(&["--limit"]) {
        Some(n) => n
            .parse()
            .map_err(|_| Error::new(format!("--limit 은 정수: {n}")))?,
        None => 6,
    };
    let report = lint::lint(&doc, &names).map_err(|e| Error::new(format!("{path}:{e}")))?;
    // 좌표는 그림을 좁혔을 때(--frames)나 --detail 일 때만 — 시트 전체는 개수만.
    let detail = a.has("--frames") || a.has("--detail");
    print!("{}", lint::format(&report, &doc.palette, limit, detail));

    // 표시 그림 — 걸린 픽셀에 종류별 색 테두리. 시트 배치(없으면 검사한 순서 8개씩).
    if let Some(out) = a.flag(&["-o", "--out"]) {
        let mut marks = preview::Marks::new();
        for (name, found) in &report.images {
            let list = marks.entry(name.clone()).or_default();
            for f in found {
                list.extend(f.pixels().map(|(x, y)| (x, y, f.kind.mark())));
            }
        }
        let layout = if a.has("--frames") {
            names
                .chunks(8)
                .map(|c| c.iter().cloned().map(Some).collect())
                .collect()
        } else if let Some(atlas) = &doc.atlas {
            atlas
                .rows
                .iter()
                .map(|r| r.iter().cloned().map(Some).collect())
                .collect()
        } else {
            preview::sheet_layout(&doc).unwrap_or_else(|| {
                names
                    .chunks(8)
                    .map(|c| c.iter().cloned().map(Some).collect())
                    .collect()
            })
        };
        let scale = match a.flag(&["--scale"]) {
            Some(s) => s
                .parse()
                .map_err(|_| Error::new(format!("--scale 은 정수: {s}")))?,
            None => 6,
        };
        let img = preview::render_marked(&doc, &layout, preview::Options::new(scale), &marks)?;
        write(Path::new(out), &export::encode_png(&img)?)?;
        println!("\n표시: {out} (청록 계단 · 노랑 밴딩 · 초록 줄무늬 · 자홍 고립)");
    }
    Ok(())
}

fn cmd_build(a: &Args) -> Result<()> {
    let path = a.pos(0, "원본 파일")?;
    let doc = load(path)?;
    let output = doc
        .output
        .as_ref()
        .ok_or_else(|| Error::new(format!("{path}: output 블록이 없어 어디에 쓸지 모름")))?;

    // `--out` 은 시험 출력 — 파일 이름만 따와 다른 폴더에 쓴다.
    let dir = Path::new(path).parent().unwrap_or(Path::new(""));
    let place = |p: &str| match a.flag(&["--out"]) {
        Some(out) => Path::new(out).join(Path::new(p).file_name().unwrap_or_default()),
        None => dir.join(p),
    };
    let image_path = place(&output.image);
    let tag = |e: Error| Error::new(format!("{path}:{e}"));

    if let Some(atlas) = &doc.atlas {
        let (img, placed) = export::build_atlas(&doc, atlas).map_err(tag)?;
        write(&image_path, &export::encode_png(&img)?)?;
        println!("{} ({}x{})", image_path.display(), img.width, img.height);
        println!("\n// 그림 안 픽셀 사각형 (x, y, 폭, 높이) — terrain.ron 의 px 에 쓴다");
        for p in placed {
            println!("{}: ({}, {}, {}, {})", p.name, p.x, p.y, p.w, p.h);
        }
        return Ok(());
    }

    let (Some(sheet_def), Some(sheet_out)) = (&doc.sheet, &output.sheet) else {
        return Err(Error::new(format!("{path}: sheet 블록이 없음")));
    };
    let sheet_path = place(sheet_out);
    let image_ref = match &output.image_ref {
        Some(r) => r.clone(),
        None => export::relative(&image_path, sheet_path.parent().unwrap_or(Path::new(""))),
    };
    let img: Rgba8 = export::build_sheet(&doc).map_err(tag)?;
    let source = Path::new(path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    write(&image_path, &export::encode_png(&img)?)?;
    write(
        &sheet_path,
        export::sheet_ron(&doc, sheet_def, &image_ref, &source).as_bytes(),
    )?;
    println!("{} ({}x{})", image_path.display(), img.width, img.height);
    println!("{}", sheet_path.display());
    Ok(())
}

fn cmd_analyze(a: &Args) -> Result<()> {
    let png = a.pos(0, "PNG 파일")?;
    let bytes = std::fs::read(png).map_err(|e| Error::new(format!("{png}: {e}")))?;
    let img = export::decode_png(&bytes).map_err(|e| Error::new(format!("{png}: {e}")))?;
    let crop = match a.flag(&["--crop"]) {
        Some(c) => {
            let v: Vec<u32> = c.split(',').filter_map(|s| s.trim().parse().ok()).collect();
            match v.as_slice() {
                [x, y, w, h] => Some((*x, *y, *w, *h)),
                _ => return Err(Error::new(format!("--crop 은 x,y,w,h: {c}"))),
            }
        }
        None => None,
    };
    let colors = match a.flag(&["--colors"]) {
        Some(n) => n
            .parse()
            .map_err(|_| Error::new(format!("--colors 는 정수: {n}")))?,
        None => 24,
    };
    let result = analyze::analyze(&img, &analyze::Options { crop, colors });
    print!("{}", analyze::format(&result));
    Ok(())
}

fn cmd_import(a: &Args) -> Result<()> {
    let png = a.pos(0, "PNG 파일")?;
    let out = a
        .flag(&["-o", "--out"])
        .ok_or_else(|| Error::new("-o <원본.canvas> 가 필요함"))?;
    let cell = a
        .flag(&["--cell"])
        .ok_or_else(|| Error::new("--cell WxH 가 필요함"))?;
    let cell = cell
        .split_once('x')
        .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
        .ok_or_else(|| Error::new(format!("--cell 은 WxH: {cell}")))?;
    let directions = match a.flag(&["--directions"]) {
        Some(d) => d
            .parse()
            .map_err(|_| Error::new(format!("--directions 는 정수: {d}")))?,
        None => 1,
    };
    let clips = a
        .all("--clip")
        .into_iter()
        .map(ClipSpec::parse)
        .collect::<Result<_>>()?;

    let bytes = std::fs::read(png).map_err(|e| Error::new(format!("{png}: {e}")))?;
    let img = export::decode_png(&bytes).map_err(|e| Error::new(format!("{png}: {e}")))?;
    let text = import::import(
        &img,
        &import::Options {
            cell,
            directions,
            clips,
            source: png.to_owned(),
        },
    )?;
    // 만든 원본이 바로 읽혀야 한다.
    parse(&text).map_err(|e| Error::new(format!("가져온 원본이 읽히지 않음 (버그): {e}")))?;
    if Path::new(out).exists() {
        return Err(Error::new(format!(
            "{out} 이(가) 이미 있음 — 덮어쓰지 않는다"
        )));
    }
    write(Path::new(out), text.as_bytes())?;
    println!("{out}");
    Ok(())
}
