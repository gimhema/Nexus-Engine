# WorldCanvas

WorldEditor 용 픽셀 스프라이트 저작 도구. **원본은 텍스트(`.canvas`)** 이고,
여기서 시트 PNG 와 WorldEditor 시트 정의(`.sheet.ron`)를 만든다.

왜 텍스트인가: 한 글자 = 한 픽셀이라 사람과 Claude 가 같은 파일을 읽고 고칠 수 있고,
git diff 로 어느 픽셀이 바뀌었는지 보인다. 프레임은 다른 프레임에 연산을 쌓아 **파생**시킬 수 있어서
(방향 반전, 걷기 프레임의 다리 이동, 색 변종) 바탕을 고치면 파생 프레임이 함께 바뀐다.

## 그린 그림 확인하기 — 뷰어 실행

`WorldCanvas/` 폴더에서:

```bash
cargo run --release --bin worldcanvas-view
```

창 왼쪽에 `sprites/` 의 원본 목록이 뜬다. 하나를 고르면:

- **애니메이션** 탭 — 클립(Idle·Walk)마다 동·북·서·남 네 방향을 동시에 재생
- **시트 / 아틀라스** 탭 — 게임이 읽는 결과 그림 한 장 (오브젝트는 그림 경계와 `px` 목록)
- **그림** 탭 — 프레임·부품 전부를 이름과 함께

위쪽 막대에서 배율(1~16배), 배경(어두움·밝음·체커), 재생/멈춤을 바꾼다.
**창을 열어 둔 채로 원본이 바뀌면 0.4초 안에 다시 그린다** — Claude 가 그리는 동안 켜 두면 된다.

- 첫 실행은 그래픽 의존성(eframe·wgpu)을 빌드하느라 몇 분 걸린다. 이후는 바로 뜬다.
- 다른 폴더나 파일 하나만 보려면 `cargo run --release --bin worldcanvas-view -- 경로`.
- Linux 에서 창이 안 뜨거나 Wayland 오류가 나면 X11 로: `env -u WAYLAND_DISPLAY DISPLAY=:0 cargo run --release --bin worldcanvas-view`
- 한글이 네모로 나오면 폰트를 지정: `WORLDCANVAS_FONT=/경로/폰트.ttf cargo run …`

## 빌드·검사

```bash
cargo build                                  # target/debug/worldcanvas
cargo test                                   # 단위 테스트 + 결과물 동기화 테스트
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

툴체인은 WorldEditor 와 같은 1.95.0 으로 고정 (`rust-toolchain.toml`).
의존은 `png`, 그리고 뷰어용 `eframe` 0.36 (WorldEditor 와 같은 egui 0.36 + wgpu 계열).
뷰어는 `view` 기능(기본 켜짐)이다 — CLI 만 필요하면 `cargo build --no-default-features`.

## 뷰어 (`worldcanvas-view`)

```bash
cargo run --bin worldcanvas-view            # sprites/ 의 원본 전부
cargo run --bin worldcanvas-view -- 경로    # 다른 폴더 또는 .canvas 파일 하나
```

보기 전용이다 (편집 없음). 원본 목록 / **애니메이션**(클립마다 모든 방향을 동시에 재생) /
**시트·아틀라스**(결과 그림, 아틀라스는 그림 경계와 `px` 목록) / **그림**(프레임·부품 전부, 이름 표시).
배율 1~16, 배경 어두움·밝음·체커, 재생/멈춤.

원본을 0.4초마다 확인해서 **바뀌면 다시 읽는다** — 사용자가 열어 두면 Claude 가 고치는 결과가 바로 보인다.
읽기에 실패하면 마지막으로 성공한 그림을 유지하고 오류(줄 번호 포함)를 빨갛게 띄운다.

자동 확인용 환경 변수 (WorldEditor 의 `NEXUS_SCREENSHOT` 방식):

| 변수 | 뜻 |
|---|---|
| `WORLDCANVAS_SELECT=player` | 시작 원본 (파일 이름, 확장자 없이) |
| `WORLDCANVAS_TAB=anim\|sheet\|images` | 시작 탭 (생략하면 문서에 맞춤) |
| `WORLDCANVAS_ZOOM=4` | 시작 배율 |
| `WORLDCANVAS_SCREENSHOT=out.png` | 캡처 후 자동 종료 |
| `WORLDCANVAS_SCREENSHOT_FRAME=15` | 캡처 프레임. 실행 중 파일 변경을 확인할 때 늘린다 |
| `WORLDCANVAS_FONT=/path/font.ttc` | 한글 폰트 직접 지정 |

Linux 에서 띄울 때는 WorldEditor 와 같이 `env -u WAYLAND_DISPLAY DISPLAY=:0` 를 붙인다.

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
| `ellipse X Y W H C` | 사각형에 내접하는 채운 타원 — 머리·견갑·몸통 같은 둥근 덩어리 |
| `poly C x1 y1 x2 y2 …` | 채운 다각형 (꼭짓점 3개 이상, 픽셀 중심 좌표) — 날개막·망토·지붕·꼬리 |

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
| `view/main.rs` | 뷰어 (`view` 기능) |

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
- **큰 그림은 도형으로 쌓는다.** 덩어리를 `ellipse`/`poly` 로 깔고, 같은 도형을 한 치수 크게
  짙은 색으로 **먼저** 깔면 덩어리 사이에 윤곽이 생긴다 (드래곤 몸통과 날개가 섞이지 않게).
  작은 밝은 도형을 왼쪽 위로 비켜 겹치면 둥근 입체감이 난다 (기사 견갑).
- **대칭으로 그린 뒤 그늘 쪽만 어둡게** — `mirror` 다음에 오른쪽 영역에 `swap 밝은색 어두운색 X Y W H`
  를 단계별로 (반사광→밝은 면, 밝은 면→중간, 중간→그늘).
- **바뀌는 부위는 부품으로 갈아 끼운다.** 드래곤 날개 세 자세(위·가운데·아래), 기사 옆모습 다리
  세 자세를 부품으로 두고 프레임마다 다른 부품을 `stamp`. 옆모습 외곽선이 두 겹이 되지 않도록
  부품마다 자기 `outline` 을 갖는다.
- **반복 선이 많으면 생성한다.** 기와 골·석재 줄눈처럼 규칙적인 선은 짧은 스크립트로 연산 줄을
  만들어 원본에 넣는다 (`palace_gate.canvas`). 원본은 여전히 평범한 연산 목록이다.
- 빛은 왼쪽 위. 밝은 면 왼쪽·위, 그늘 오른쪽·아래. 외곽선은 순흑 대신 아주 짙은 갈색·녹색.

## 원본 목록 (`sprites/`)

| 원본 | 결과물 | 비고 |
|---|---|---|
| `player.canvas` | `WorldEditor/assets/worldcanvas/player.png` + `.sheet.ron` | 모험가. 24×32, 16px/m, 4방향, Idle 2 + Walk 4 |
| `goblin.canvas` | `WorldEditor/assets/worldcanvas/goblin.png` + `.sheet.ron` | 고블린(액터 102 자리). 몽둥이 부품, 구성 동일 |
| `props.canvas` | `WorldEditor/assets/worldcanvas/props.png` (atlas) | 집 64×64, 참나무 32×48, 소나무 24×48, 바위 16×12, 덤불 20×13 |
| `knight.canvas` | `WorldEditor/assets/worldcanvas/knight.png` + `.sheet.ron` | 성기사 (`reference/kinght.webp` 풍). 32×40, 4방향, Idle 2 + Walk 4, 망치 번개가 프레임마다 바뀐다 |
| `dragon.canvas` | `WorldEditor/assets/worldcanvas/dragon.png` + `.sheet.ron` | 붉은 드래곤 (`reference/dragon.webp` 풍). 64×64, 4방향, Idle 2 + Walk 4(날갯짓) |
| `palace_gate.canvas` | `WorldEditor/assets/worldcanvas/palace_gate.png` (atlas) | 궁궐 정문 (`reference/castle.webp` 광화문 풍). 128×96 = 8×6m |

`reference/` 는 화풍 참고용 원화다 (WebP). 그대로 베끼지 않고 구성·색·분위기만 가져온다.
knight·dragon·palace_gate 는 **아직 WorldEditor 데이터에 연결하지 않았다** — 기사·드래곤은
`data/rules.ron` 에 액터가 있어야 `display.ron` 에 걸 수 있고, 정문은 `terrain.ron` props 에 추가하면 된다.

**WorldEditor 연결:** `data/display.ron` 의 액터 1(플레이어)·102(고블린)가 위 시트를 쓴다.
`data/terrain.ron` 에 타일셋 `worldcanvas` 와 props 104 기와집·105 참나무·106 소나무·107 바위·108 덤불.
props 의 `px` 는 `build` 출력값을 손으로 옮긴 것이다 — **아틀라스 배치를 바꾸면 terrain.ron 도 고친다.**
| `markers.canvas` | `WorldEditor/assets/sprites/markers.png` (+ `.sheet.ron`) | 에디터 내장 플레이스홀더. `import` 로 가져옴 — 픽셀 일치 확인됨. `build` 하면 손으로 쓴 `markers.sheet.ron` 주석이 생성 주석으로 바뀐다 |

## 다음 단계 (미구현)

- 공격·피격·사망 클립 (지금은 Idle·Walk 만). 엔진은 없는 클립을 Idle 로 대체한다.
