//! 에디터 실행 파일 — 내용은 라이브러리에 있다.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    world_editor::run_editor()
}
