// 고블린 — 3초마다 주변을 둘러보고, 적이 있으면 돌진해서 문다.
//
// 액터 스크립트 예시 (data/rules.ron 의 actors.102.script 가 이 파일을 가리킨다).
// 고친 뒤 에디터에서 F5 만 다시 누르면 적용된다 — 빌드할 필요 없다.
//
// 규칙:
//   - 함수만 정의한다. 엔진이 부르는 훅: on_spawn / on_tick / on_attack / on_damaged / on_death
//   - 상태는 this 에 둔다 (액터마다 따로). on_spawn 에서 초기화한다.
//   - 월드는 읽기만 하고, 하고 싶은 일은 move_to / attack / stop 으로 "요청" 한다.
//     사거리·쿨타임·진영 판정은 AI·플레이어와 똑같이 받는다.

// 둘러보는 간격(초)과 거리(m).
fn look_every() { 3.0 }
fn look_range() { 10.0 }

fn on_spawn(me) {
    this.timer = look_every();   // 태어나자마자 한 번 둘러본다
    this.target = ();
}

fn on_tick(me, dt) {
    this.timer += dt;
    if this.timer >= look_every() {
        this.timer = 0.0;
        let found = nearest_enemy(me, look_range());
        if found != () && this.target == () {
            print("적 발견 — 돌진!");
        }
        this.target = found;
    }

    if this.target == () { return; }
    if !this.target.alive {
        this.target = ();
        stop(me);
        return;
    }

    if distance(me, this.target) <= attack_range(me) {
        stop(me);
        attack(me, this.target);   // 쿨타임 중이면 조용히 거절된다
    } else {
        move_to(me, this.target);
    }
}

fn on_damaged(me, attacker, amount) {
    // 맞으면 둘러보기를 기다리지 않고 바로 상대에게 달려든다.
    this.target = attacker;
}

fn on_death(me, killer) {
    print("끄아악…");
}
