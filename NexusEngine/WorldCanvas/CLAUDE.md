# WorldCanvas

WorldEditor 용 픽셀 스프라이트 저작 도구. **원본은 텍스트(`.canvas`)** 이고,
여기서 시트 PNG 와 WorldEditor 시트 정의(`.sheet.ron`)를 만든다.

왜 텍스트인가: 한 글자 = 한 픽셀이라 사람과 Claude 가 같은 파일을 읽고 고칠 수 있고,
git diff 로 어느 픽셀이 바뀌었는지 보인다. 프레임은 다른 프레임에 연산을 쌓아 **파생**시킬 수 있어서
(방향 반전, 걷기 프레임의 다리 이동, 색 변종) 바탕을 고치면 파생 프레임이 함께 바뀐다.

## 빌드·검사

```bash
cargo build                                  # target/debug/worldcanvas
cargo test                                   # 단위 테스트 + 결과물 동기화 테스트
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

툴체인은 WorldEditor 와 같은 1.95.0 으로 고정 (`rust-toolchain.toml`). 외부 의존은 `png` 하나다.

## 명령

| 명령 | 하는 일 |
|---|---|
| `check <f.canvas>` | 문법·참조·순환 검사 |
| `list <f.canvas>` | 팔레트·그림 목록·시트 배치 요약 |
| `show <f.canvas> <이름>` | 계산된 그림을 좌표 눈금과 함께 텍스트로 |
| `preview <f.canvas> [-o out.png] [--scale N] [--grid] [--all \| --frames a,b]` | 확대 미리보기. 기본은 시트 배치, `./preview/<이름>.png` |
| `build <f.canvas> [--out 폴더]` | 시트 PNG + `.sheet.ron`, 또는 아틀라스 PNG + 그림별 픽셀 사각형 출력. 경로는 `output` 블록, `--out` 은 시험 출력 |
| `import <시트.png> --cell WxH -o <f.canvas> [--directions N] [--clip 상태:프레임:ms:loop\|once]…` | 기존 PNG 를 원본으로 가져오기. 기존 파일은 덮어쓰지 않는다 |

## 작업 절차 (Claude)

1. `.canvas` 를 고친다 (Edit 도구로 직접).
2. `check` — 오류는 `파일:N줄: …` 로 나온다.
3. `preview --grid --scale 8 --frames …` 후 PNG 를 Read 로 **눈으로 확인한다.**
   격자는 1픽셀마다 어둡게, 8픽셀마다 밝게 그어져 있어 좌표를 그림에서 바로 읽는다.
   좌표를 정확히 확인할 때는 `show` 가 더 정확하다.
4. `build` — WorldEditor 의 에셋이 갱신된다. 에디터는 **재시작해야** 새 그림을 올린다.
5. `cargo test` — `tests/assets_in_sync.rs` 가 원본과 결과물 PNG 가 일치하는지 지킨다.
   원본만 고치고 `build` 를 잊으면 여기서 깨진다.

`preview/` 는 gitignore 대상이다. 미리보기는 커밋하지 않는다.

## 원본 형식 (`canvas 1`)

줄 단위. `//` 부터 줄 끝까지 주석, 빈 줄 무시, 블록은 `end` 로 닫는다. 들여쓰기는 자유.
좌표는 **좌상단 (0,0)**, x 오른쪽, y 아래. 범위 밖 픽셀은 조용히 잘린다.

```
canvas 1                  // 첫 줄 고정 — 형식 버전
cell 32 48                // 프레임(시트 칸) 크기. frame 보다 먼저

palette
    K #1c1c22             // <문자> #rrggbb 또는 #rrggbbaa
    W #ffffff
end

frame idle_s_0            // 투명에서 시작
    grid                  // 칸 크기와 정확히 같아야 한다
        ....KK....
        ...KWWK...
    end
end

frame idle_n_0 = idle_s_0 // 바탕을 복사한 뒤 연산을 쌓는다 (크기가 같아야 함)
    flip_h
    patch 10 4            // 조각을 (10, 4) 에 덮어쓴다. ~ 는 그대로 둠
        K~K
    end
end

part eye 2 2              // 임의 크기 부품. 시트에는 못 들어가고 stamp 로 붙인다
    rect 0 0 2 2 K
end

sheet
    directions 4          // 필수. 엔진 방향 순서: 0=동, 반시계 (4방향: 동·북·서·남)
    tinted true           // 무채색 시트면 true (에디터가 종류 색을 곱한다)
    // direction_rows 1 2 3 0   // 선택. 엔진 방향 → 시트 행 오프셋
    // pixels_per_meter 16      // 선택. 생략하면 에디터 기본(32)
    clip Idle 250 loop    // <상태> <프레임 ms> loop|once
        row idle_e_0 idle_e_1  // 방향 0 — 행 수 = directions, 행마다 프레임 수가 같아야 함
        row idle_n_0 idle_n_1
        row idle_w_0 idle_w_1
        row idle_s_0 idle_s_1
    end
end

// sheet 대신 atlas — 크기가 제각각인 정적 오브젝트(건물·나무)를 한 장에.
// 행마다 왼쪽부터 붙이고 행 높이는 가장 큰 그림. frame·part 모두 넣을 수 있다.
// build 가 `이름: (x, y, 폭, 높이)` 를 출력한다 → WorldEditor data/terrain.ron 의 props px.
// atlas 만 있는 문서는 cell 이 필요 없다. sheet 와 atlas 는 한 문서에 함께 못 둔다.
// atlas
//     row house tree_oak
//     row rock
// end

output                    // 경로는 이 .canvas 파일 폴더 기준
    image ../../WorldEditor/assets/sprites/foo.png
    sheet ../../WorldEditor/assets/sprites/foo.sheet.ron   // sheet 블록이 있을 때만
    // image_ref ""       // 선택. .sheet.ron 의 image 값. 생략하면 정의 파일 기준 상대 경로,
                          // "" 는 에디터 내장 그림 (markers 전용)
end
```

**예약 문자:** `.` 투명(팔레트에 적지 않는다), `~` 유지(`grid`/`patch` 안에서만).
`/`·`#`·`=` 와 공백은 팔레트 문자로 못 쓴다.
`end` 라는 그림 행은 블록 끝으로 읽히므로 `e`·`n`·`d` 를 모두 팔레트에 쓸 때 주의.

**상태:** `Idle Walk Attack Cast Hit Die` (엔진 `AnimState` 와 같음). 클립은 적은 순서대로
`directions` 행씩 시트를 차지하고, 시트 열 수는 가장 긴 클립의 프레임 수다.

### 연산

| 연산 | 뜻 |
|---|---|
| `grid` … `end` | 그림 전체 (칸 크기 정확히) |
| `patch X Y` … `end` | 텍스트 조각을 덮어씀. `~` 는 유지 |
| `px X Y C` | 점 하나 |
| `rect X Y W H C` | 꽉 찬 사각형 (`C` 가 `.` 이면 지우기) |
| `box X Y W H C` | 사각형 테두리 1px |
| `line X0 Y0 X1 Y1 C` | 직선 (양 끝 포함) |
| `fill X Y C` | 4방향 연결된 같은 색 영역 칠하기 |
| `flip_h` / `flip_v` | 좌우 / 상하 뒤집기 |
| `shift DX DY` | 통째로 이동. 밖으로 나간 픽셀은 버림 |
| `move X Y W H DX DY` | 영역을 떼어 옮김. 떠난 자리는 투명 — 걷기 프레임의 팔다리용 |
| `swap A B [X Y W H]` | 색 A 를 B 로 — 색 변종. 사각형을 주면 그 안에서만 (한쪽 그늘, 모양 안 줄무늬) |
| `mirror` | 왼쪽 절반을 오른쪽에 대칭 복사 — 정면·뒷면·건물은 반만 그린다 |
| `outline C` | 불투명 픽셀에 상하좌우로 닿은 투명 픽셀을 C 로 — 외곽선 |
| `stamp NAME X Y` | 다른 그림(부품·프레임)을 투명 부분 빼고 얹음 |

바탕(`= base`)과 `stamp` 의 순환은 `순환 참조: a → b → a` 로 보고된다.

## 코드 구조 (`src/`)

| 파일 | 역할 |
|---|---|
| `color.rs` | `Rgba`, `Palette`, 예약 문자 |
| `bitmap.rs` | 팔레트 문자 비트맵과 그리기 연산 |
| `doc.rs` | 문서 모델 (`Document`, `ImageDef`, `Op`, `SheetDef`, `OutputDef`) |
| `parse.rs` | 텍스트 → 문서, `validate`(이름·크기·팔레트·참조·시트 배치) |
| `resolve.rs` | 바탕·연산을 따라 최종 비트맵 계산, 캐시, 순환 검출 |
| `export.rs` | 시트 PNG, `.sheet.ron`, PNG 입출력, 상대 경로 |
| `preview.rs` | 확대 미리보기 (배경·틈·격자) |
| `import.rs` | PNG → 원본. 같은 칸·좌우 반전 칸은 `= base` 로 줄인다 |
| `main.rs` | CLI |

## 지켜야 할 것

- `.sheet.ron` 은 WorldEditor `apps/world-editor/src/sprites.rs` 의 `SheetFile` 스키마를 따른다.
  저쪽 스키마가 바뀌면 `export::sheet_ron` 도 바꾼다.
- 결과물 `.sheet.ron` 은 생성 파일이다 — 손으로 고치지 말고 원본을 고친다.
- 그림 배율은 정수 배여야 한다 (WorldEditor 규칙: 스프라이트 1픽셀 = 화면 1픽셀).
  캐릭터를 키우려면 `cell` 을 키운다.
- 무채색 시트(`tinted true`)는 에디터가 선형 공간에서 색을 곱하므로 **밝게** 그려야 색이 산다.

## 그리는 요령 (해 보고 얻은 것)

- **대칭은 반만 그린다.** `patch 0 Y` 로 왼쪽 12열만 적고 `mirror`. 눈 하이라이트처럼 대칭이면
  어색한 것은 `mirror` 뒤에 `px` 로 따로 찍는다 (두 눈 모두 오른쪽 위).
- **겹쳐 그릴 것은 부품으로.** 몽둥이처럼 손에 쥐는 물건은 부품을 먼저 `stamp` 하고 몸을 그 위에
  `stamp` 한다 — 손이 손잡이를 덮는다. 나무 잎은 잎 덩어리 부품을 위→아래 순서로 겹쳐 찍고
  마지막에 `outline o` 로 바깥 윤곽만 두른다 (안쪽 경계는 짙은 녹색이라 덩어리 결이 남는다).
- **걷기·숨쉬기는 파생으로.** `move 0 0 24 27 0 1` (상체 1px 내려앉음) + 한쪽 다리 `move … 0 -1`.
  옆모습 보폭은 다리 칸을 `rect … .` 로 지운 뒤 `~` 섞인 `patch` 로 다시 그린다 — 다른 픽셀을 지키려고.
- **`patch` 의 `.` 는 지운다.** 남겨야 할 자리는 `~`.
- **도형 안 줄무늬는 영역 swap.** 지붕처럼 윤곽을 긋고 `fill` 한 뒤 `swap r R 0 Y 64 1` 로 한 줄씩 —
  윤곽 밖으로 새지 않는다.
- 빛은 왼쪽 위. 밝은 면 왼쪽·위, 그늘 오른쪽·아래. 외곽선은 순흑 대신 아주 짙은 갈색·녹색.

## 원본 목록 (`sprites/`)

| 원본 | 결과물 | 비고 |
|---|---|---|
| `player.canvas` | `WorldEditor/assets/worldcanvas/player.png` + `.sheet.ron` | 모험가. 24×32, 16px/m, 4방향, Idle 2 + Walk 4 |
| `goblin.canvas` | `WorldEditor/assets/worldcanvas/goblin.png` + `.sheet.ron` | 고블린(액터 102 자리). 몽둥이 부품, 구성 동일 |
| `props.canvas` | `WorldEditor/assets/worldcanvas/props.png` (atlas) | 집 64×64, 참나무 32×48, 소나무 24×48, 바위 16×12, 덤불 20×13 |
| `markers.canvas` | `WorldEditor/assets/sprites/markers.png` (+ `.sheet.ron`) | 에디터 내장 플레이스홀더. `import` 로 가져옴 — 픽셀 일치 확인됨. `build` 하면 손으로 쓴 `markers.sheet.ron` 주석이 생성 주석으로 바뀐다 |

## 다음 단계 (미구현)

- 애니메이션 미리보기 (GIF 또는 프레임 스트립).
