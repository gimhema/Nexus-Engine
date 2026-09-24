//! RON 텍스트 부분 고치기 — 손으로 쓴 주석을 지키며 **항목 하나만** 바꾼다 (P8).
//!
//! `data/rules.ron`·`display.ron` 은 사람이 주석을 달아 쓰는 표다. 편집기가 액터 하나를
//! 저장할 때 파일을 통째로 다시 쓰면 다른 항목의 주석까지 다 사라진다. 그래서 지형 팔레트의
//! 자동 추가(`terrain.rs`)와 같은 방식으로, **그 항목의 줄만** 갈아 끼운다.
//!
//! - 찾는 단위: `이름: {` 로 여는 맵 블록 안의 `번호: (` 항목. 블록 밖의 같은 번호
//!   (`items` 의 `1:` 등)는 건드리지 않는다.
//! - **항목 안의 주석은 남지 않는다** — 그 줄들은 다시 쓰이기 때문이다. 항목 **위**의 주석은 남는다.
//! - 결과가 올바른 RON 인지는 여기서 보지 않는다 — 부르는 쪽이 다시 파싱해 검증한 뒤에 쓴다.

/// 맵 블록 `name: {` … `},` — `(여는 줄, 닫는 줄, 들여쓰기)`.
fn map_block(lines: &[&str], name: &str) -> Option<(usize, usize, usize)> {
    let head = format!("{name}:");
    let start = lines.iter().position(|l| {
        let t = l.trim();
        t.starts_with(&head) && t.ends_with('{')
    })?;
    let indent = indent_of(lines[start]);
    let end = lines[start + 1..]
        .iter()
        .position(|l| {
            let t = l.trim();
            (t == "}," || t == "}") && indent_of(l) == indent
        })
        .map(|i| start + 1 + i)?;
    Some((start, end, indent))
}

/// 블록 안 항목의 들여쓰기 — 첫 항목에서 읽는다. 비어 있으면 블록보다 4칸 안쪽.
fn child_indent(lines: &[&str], (start, end, indent): (usize, usize, usize)) -> usize {
    lines[start + 1..end]
        .iter()
        .find(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with("//")
        })
        .map_or(indent + 4, |l| indent_of(l))
}

/// 줄이 `번호:` 로 시작하는 항목의 머리면 그 번호.
fn entry_key(line: &str) -> Option<u32> {
    let t = line.trim_start();
    let colon = t.find(':')?;
    t[..colon].trim().parse().ok()
}

/// 항목의 끝 줄 — 한 줄짜리면 그 줄, 아니면 같은 들여쓰기의 `),` 줄.
///
/// 여는 괄호가 `(` 면 `),`, `[` 면 `],` 를 찾는다 (드롭 표는 목록이다). 줄 끝 주석은 무시한다 —
/// 아이템·스킬은 `1: (…),   // 이름` 처럼 한 줄에 주석을 단다.
fn entry_end(lines: &[&str], at: usize, indent: usize, block_end: usize) -> Option<usize> {
    let head = code_part(lines[at]).trim_end();
    let close = if head.ends_with('(') {
        ")"
    } else if head.ends_with('[') {
        "]"
    } else {
        return Some(at); // 한 줄짜리
    };
    lines[at + 1..block_end]
        .iter()
        .position(|l| {
            let t = code_part(l).trim();
            (t == close || t == format!("{close},")) && indent_of(l) == indent
        })
        .map(|i| at + 1 + i)
}

/// 줄에서 주석을 뺀 부분 — 문자열 안의 `//` 는 주석이 아니다.
fn code_part(line: &str) -> &str {
    let mut in_string = false;
    let mut escaped = false;
    let bytes = line.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            _ if escaped => escaped = false,
            b'\\' if in_string => escaped = true,
            b'"' => in_string = !in_string,
            b'/' if !in_string && bytes.get(i + 1) == Some(&b'/') => return &line[..i],
            _ => {}
        }
    }
    line
}

/// 줄 끝 주석 (`// 이름`) — 한 줄짜리 항목을 다시 쓸 때 붙여 준다.
fn trailing_comment(line: &str) -> Option<&str> {
    let code = code_part(line);
    let rest = line[code.len()..].trim();
    (!rest.is_empty() && !code.trim().is_empty()).then_some(rest)
}

/// 맵 `map` 의 `id` 항목을 `entry` 로 바꾼다. 없으면 **번호 순서에 맞는 자리**에 넣는다.
///
/// `entry` 는 들여쓰기 없이 쓴 항목 텍스트다 (`"102: (\n    name: \"고블린\",\n),"`).
/// 각 줄에 블록 안쪽 들여쓰기를 붙여 끼운다.
pub(crate) fn upsert_entry(text: &str, map: &str, id: u32, entry: &str) -> Result<String, String> {
    let lines: Vec<&str> = text.lines().collect();
    let block = map_block(&lines, map).ok_or_else(|| format!("'{map}: {{' 블록을 찾지 못함"))?;
    let (start, end, _) = block;
    let inner = child_indent(&lines, block);
    let pad = " ".repeat(inner);
    let body: Vec<String> = entry
        .lines()
        .map(|l| {
            if l.is_empty() {
                String::new()
            } else {
                format!("{pad}{l}")
            }
        })
        .collect();

    // 같은 번호의 항목 — 블록 안, 항목 들여쓰기에서만 찾는다.
    let existing =
        (start + 1..end).find(|&i| indent_of(lines[i]) == inner && entry_key(lines[i]) == Some(id));

    let mut body = body;
    let (from, to) = match existing {
        Some(at) => {
            let last = entry_end(&lines, at, inner, end)
                .ok_or_else(|| format!("'{map}' 의 {id} 항목이 닫히지 않음"))?;
            // 한 줄짜리 항목의 줄 끝 주석은 살린다 — 그 주석이 이름 노릇을 하는 표가 있다.
            if at == last
                && body.len() == 1
                && let Some(comment) = trailing_comment(lines[at])
            {
                body[0] = format!("{}   {comment}", body[0]);
            }
            (at, last + 1)
        }
        None => {
            // 번호가 더 큰 첫 항목 앞에 — 그 항목 위의 주석은 그 항목 것이므로 주석 위에 넣는다.
            let next = (start + 1..end).find(|&i| {
                indent_of(lines[i]) == inner && entry_key(lines[i]).is_some_and(|k| k > id)
            });
            let mut at = next.unwrap_or(end);
            while next.is_some() && at > start + 1 && lines[at - 1].trim_start().starts_with("//") {
                at -= 1;
            }
            (at, at)
        }
    };

    let mut out: Vec<String> = Vec::with_capacity(lines.len() + body.len());
    out.extend(lines[..from].iter().map(|l| (*l).to_owned()));
    out.extend(body);
    out.extend(lines[to..].iter().map(|l| (*l).to_owned()));
    let mut text = out.join("\n");
    text.push('\n');
    Ok(text)
}

/// 맵 `map` 의 `id` 항목을 뺀다 — `(남은 텍스트, 뺀 항목 텍스트)`. 항목 바로 위의 주석도
/// 그 항목 것이므로 함께 뺀다 (남기면 다음 항목의 설명처럼 읽힌다).
pub(crate) fn remove_entry(text: &str, map: &str, id: u32) -> Result<(String, String), String> {
    let lines: Vec<&str> = text.lines().collect();
    let block = map_block(&lines, map).ok_or_else(|| format!("'{map}: {{' 블록을 찾지 못함"))?;
    let (start, end, _) = block;
    let inner = child_indent(&lines, block);
    let at = (start + 1..end)
        .find(|&i| indent_of(lines[i]) == inner && entry_key(lines[i]) == Some(id))
        .ok_or_else(|| format!("'{map}' 에 {id} 항목이 없음"))?;
    let last = entry_end(&lines, at, inner, end)
        .ok_or_else(|| format!("'{map}' 의 {id} 항목이 닫히지 않음"))?;
    let mut from = at;
    while from > start + 1 && lines[from - 1].trim_start().starts_with("//") {
        from -= 1;
    }
    let removed = lines[from..=last].join("\n");
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    out.extend(&lines[..from]);
    out.extend(&lines[last + 1..]);
    let mut text = out.join("\n");
    text.push('\n');
    Ok((text, removed))
}

/// 첫 `field:` 줄의 값을 바꾼다 — `startup_level: "main",` 같은 한 줄짜리 필드.
/// 들여쓰기와 다른 줄(주석 포함)은 그대로 둔다.
pub(crate) fn replace_field(text: &str, field: &str, value: &str) -> Result<String, String> {
    let head = format!("{field}:");
    let mut found = false;
    let out: Vec<String> = text
        .lines()
        .map(|line| {
            if !found && line.trim_start().starts_with(&head) {
                found = true;
                let pad = " ".repeat(indent_of(line));
                format!("{pad}{field}: {value},")
            } else {
                line.to_owned()
            }
        })
        .collect();
    if !found {
        return Err(format!("'{field}' 줄을 찾지 못함"));
    }
    let mut text = out.join("\n");
    text.push('\n');
    Ok(text)
}

/// 필드 블록 `field: [` … `],` (또는 `{` … `},`) 의 **안쪽을 통째로** `entries` 로 바꾼다.
///
/// 블록 **안**의 주석은 남지 않는다 (다시 쓰는 줄이다) — 블록 위의 주석은 남는다.
/// `field: [],` 처럼 한 줄로 빈 블록이어도 되고, `entries` 가 비면 한 줄 빈 블록으로 쓴다.
/// 블록이 아예 없으면 파일 맨 끝 `)` 바로 앞에 새로 넣는다 (`#[serde(default)]` 필드).
pub(crate) fn replace_block(
    text: &str,
    field: &str,
    open: char,
    entries: &[String],
) -> Result<String, String> {
    let close = match open {
        '[' => ']',
        '{' => '}',
        _ => return Err(format!("'{open}' 은(는) 블록 괄호가 아님")),
    };
    let lines: Vec<&str> = text.lines().collect();
    let head = format!("{field}:");
    let start = lines.iter().position(|l| {
        let t = code_part(l).trim();
        t.starts_with(&head) && t.contains(open)
    });
    let (start, end, indent) = match start {
        Some(start) => {
            let indent = indent_of(lines[start]);
            let t = code_part(lines[start]).trim();
            let end = if t.ends_with(&format!("{close},")) || t.ends_with(close) {
                start // 한 줄짜리 (`field: [],`)
            } else {
                lines[start + 1..]
                    .iter()
                    .position(|l| {
                        let t = code_part(l).trim();
                        (t == close.to_string() || t == format!("{close},"))
                            && indent_of(l) == indent
                    })
                    .map(|i| start + 1 + i)
                    .ok_or_else(|| format!("'{field}' 블록이 닫히지 않음"))?
            };
            (start, end, indent)
        }
        None => {
            // 없는 블록 — 맨 끝 `)` 앞에 넣는다. 들여쓰기는 최상위 필드와 같은 4칸.
            let at = lines
                .iter()
                .rposition(|l| l.trim() == ")")
                .ok_or_else(|| format!("'{field}' 을 넣을 자리(맨 끝 ')')를 찾지 못함"))?;
            (at, at - 1, 4)
        }
    };
    let pad = " ".repeat(indent);
    let mut block = Vec::new();
    if entries.is_empty() {
        block.push(format!("{pad}{field}: {open}{close},"));
    } else {
        block.push(format!("{pad}{field}: {open}"));
        block.extend(entries.iter().map(|e| format!("{pad}    {e}")));
        block.push(format!("{pad}{close},"));
    }
    let mut out: Vec<String> = lines[..start].iter().map(|l| (*l).to_owned()).collect();
    out.extend(block);
    // 새로 넣은 경우(end < start)는 맨 끝 `)` 부터 그대로 잇는다.
    let rest = if end < start { start } else { end + 1 };
    out.extend(lines[rest..].iter().map(|l| (*l).to_owned()));
    let mut text = out.join("\n");
    text.push('\n');
    Ok(text)
}

/// RON 문자열 값 — 따옴표와 역슬래시를 막는다.
pub(crate) fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "\
// 머리 주석
(
    items: {
        1: (name: \"칼\"),
    },
    actors: {
        1: (
            max_hp: 200,
            // 항목 안 주석
            attack: 20,
        ),
        // 10 번 설명 — 이 주석은 10 번 것이다
        10: (
            max_hp: 300,
        ),
    },
)
";

    #[test]
    fn an_existing_entry_is_replaced_and_other_comments_survive() {
        let out = upsert_entry(TABLE, "actors", 1, "1: (\n    max_hp: 250,\n),").unwrap();
        assert!(out.contains("            max_hp: 250,"));
        assert!(!out.contains("max_hp: 200"));
        assert!(
            !out.contains("항목 안 주석"),
            "고친 항목 안의 주석은 다시 쓰인다"
        );
        assert!(out.contains("// 머리 주석"));
        assert!(out.contains("// 10 번 설명"), "다른 항목의 주석은 남는다");
        assert!(
            out.contains("1: (name: \"칼\")"),
            "다른 맵의 같은 번호는 건드리지 않는다"
        );
        // 두 번 해도 같은 결과.
        assert_eq!(
            upsert_entry(&out, "actors", 1, "1: (\n    max_hp: 250,\n),").unwrap(),
            out
        );
    }

    #[test]
    fn a_new_entry_goes_in_number_order_above_the_next_ones_comment() {
        let out = upsert_entry(TABLE, "actors", 5, "5: (\n    max_hp: 1,\n),").unwrap();
        let five = out.find("5: (").unwrap();
        let comment = out.find("// 10 번 설명").unwrap();
        let ten = out.find("10: (").unwrap();
        assert!(
            five < comment && comment < ten,
            "5 는 10 의 주석 위에 들어간다"
        );

        let out = upsert_entry(TABLE, "actors", 99, "99: (\n    max_hp: 1,\n),").unwrap();
        let ninety_nine = out.find("99: (").unwrap();
        assert!(ninety_nine > out.find("10: (").unwrap(), "가장 크면 맨 뒤");
        assert!(out.contains("        99: ("), "항목 들여쓰기에 맞춘다");
    }

    #[test]
    fn single_line_entries_are_replaced_whole() {
        let out = upsert_entry(TABLE, "items", 1, "1: (name: \"창\"),").unwrap();
        assert!(out.contains("1: (name: \"창\"),"));
        assert!(!out.contains("칼"));
    }

    const INLINE: &str = "\
(
    skills: {
        1: (range: 2.0, cooldown_ms: 800),   // 플레이어 베기
        2: (range: 1.5, cooldown_ms: 1200),  // 슬라임 물기
    },
    loot: {
        1: [
            (item: 909, count: 1),
        ],
        2: [(item: 1, count: 1)],
    },
    items: {
        3: (name: \"a // b\"),
    },
)
";

    #[test]
    fn one_line_entries_keep_their_end_of_line_comment() {
        let out = upsert_entry(INLINE, "skills", 1, "1: (range: 3.0, cooldown_ms: 800),").unwrap();
        assert!(
            out.contains("        1: (range: 3.0, cooldown_ms: 800),   // 플레이어 베기"),
            "{out}"
        );
        assert!(out.contains("// 슬라임 물기"), "옆 항목은 그대로");
        // 새로 넣는 항목에는 주석이 없다.
        let out = upsert_entry(INLINE, "skills", 5, "5: (range: 1.0, cooldown_ms: 1),").unwrap();
        assert!(out.contains("        5: (range: 1.0, cooldown_ms: 1),\n"));
    }

    #[test]
    fn list_entries_are_found_by_their_closing_bracket() {
        let out = upsert_entry(INLINE, "loot", 1, "1: [\n    (item: 501, count: 2),\n],").unwrap();
        assert!(out.contains("(item: 501, count: 2)"));
        assert!(!out.contains("item: 909"));
        assert!(
            out.contains("2: [(item: 1, count: 1)],"),
            "한 줄 목록은 그대로"
        );
        let (out, removed) = remove_entry(INLINE, "loot", 2).unwrap();
        assert!(removed.contains("2: [(item: 1, count: 1)]"));
        assert!(out.contains("1: ["));
    }

    #[test]
    fn slashes_inside_strings_are_not_comments() {
        assert_eq!(
            code_part("3: (name: \"a // b\"), // 진짜"),
            "3: (name: \"a // b\"), "
        );
        assert_eq!(trailing_comment("3: (name: \"a // b\"),"), None);
        let out = upsert_entry(INLINE, "items", 3, "3: (name: \"c\"),").unwrap();
        assert!(
            out.contains("3: (name: \"c\"),\n"),
            "문자열 속 // 를 주석으로 붙이지 않는다"
        );
    }

    #[test]
    fn removing_an_entry_takes_its_comment_and_leaves_the_rest() {
        let (out, removed) = remove_entry(TABLE, "actors", 10).unwrap();
        assert!(!out.contains("10: ("));
        assert!(!out.contains("10 번 설명"), "항목 위 주석은 그 항목 것");
        assert!(removed.contains("// 10 번 설명") && removed.contains("max_hp: 300"));
        assert!(
            out.contains("1: (\n            max_hp: 200,"),
            "다른 항목은 그대로"
        );
        assert!(remove_entry(TABLE, "actors", 77).is_err(), "없는 번호");
    }

    #[test]
    fn missing_blocks_are_reported() {
        assert!(upsert_entry(TABLE, "skills", 1, "1: (),").is_err());
    }

    #[test]
    fn a_field_line_is_replaced_in_place() {
        let text = "(\n    // 시작\n    startup_level: \"main\",\n)\n";
        let out = replace_field(text, "startup_level", &quote("village")).unwrap();
        assert!(out.contains("    startup_level: \"village\","));
        assert!(out.contains("// 시작"));
        assert!(replace_field(text, "없는필드", "1").is_err());
    }

    #[test]
    fn quoting_escapes_quotes_and_backslashes() {
        assert_eq!(quote("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }
}
