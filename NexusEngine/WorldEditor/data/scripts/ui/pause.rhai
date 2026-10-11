// 일시정지 화면의 버튼 스크립트 — ui/pause.ui.ron 의 "포션 마시고 계속" 버튼이 drink() 를 부른다.
//
// 버튼 스크립트(UI 스크립트)는 액터 스크립트와 따로 돈다: 버튼을 누를 때 한 번,
// 인자 없는 함수 하나를 부른다. 하고 싶은 일은 명령으로 낸다 —
//   open_screen / close / open_level / open_start_level / resume / quit / notify / use_item
// 읽기: playing() · hp() · max_hp() · mp() · max_mp() · level() · item_count(번호) · level_id()
// 고치고 저장하면 다음 클릭부터 적용된다 (다시 시작할 필요 없음).

// 빨간 포션 (data/rules.ron 의 items.501)
fn potion() { 501 }

fn drink() {
    if !playing() { return; }
    if item_count(potion()) == 0 {
        notify("빨간 포션이 없습니다");
        return;
    }
    if hp() >= max_hp() {
        notify("체력이 가득합니다");
        return;
    }
    use_item(potion());
    resume();
}
