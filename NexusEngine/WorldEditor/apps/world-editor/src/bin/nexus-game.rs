//! 게임 실행 파일 (T7) — 에디터 UI 없이 시작 레벨부터 돈다.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    world_editor::run_game()
}
