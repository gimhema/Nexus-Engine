// 인스턴스 기반 단색 쿼드.
//
// 그리드 선과 오브젝트를 모두 이 파이프라인 하나로 그린다 — 선은 얇은 쿼드다.
// 정점 버퍼는 없다: 단위 쿼드를 vertex_index 로 생성한다.
//
// 좌표는 월드 공간(m, Z-up)이며 view_proj 가 정사영/원근 어느 쪽이든 동작한다.
// M8 에서 3D 로 갈 때 이 셰이더는 바뀌지 않는다.

struct Camera {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;

struct Instance {
    @location(0) center:   vec2<f32>, // 월드 중심 (m)
    @location(1) size:     vec2<f32>, // 월드 크기 (m). x 가 회전 방향 쪽 길이
    @location(2) z:        f32,       // 높이 = 깊이 정렬 기준
    @location(3) rotation: f32,       // Z 축 회전 (라디안, 반시계 +)
    @location(4) color:    vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

// 두 삼각형으로 이루어진 단위 쿼드의 코너 오프셋 (-0.5 ~ +0.5)
const CORNERS = array<vec2<f32>, 6>(
    vec2<f32>(-0.5, -0.5),
    vec2<f32>( 0.5, -0.5),
    vec2<f32>( 0.5,  0.5),
    vec2<f32>(-0.5, -0.5),
    vec2<f32>( 0.5,  0.5),
    vec2<f32>(-0.5,  0.5),
);

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: Instance) -> VsOut {
    // 크기를 먼저 곱한 뒤 회전한다 — 순서가 바뀌면 직사각형이 찌그러진다.
    let local = CORNERS[vi] * inst.size;
    let c = cos(inst.rotation);
    let s = sin(inst.rotation);
    let offset = vec2<f32>(local.x * c - local.y * s, local.x * s + local.y * c);
    let world = vec3<f32>(inst.center + offset, inst.z);

    var out: VsOut;
    out.clip = camera.view_proj * vec4<f32>(world, 1.0);
    out.color = inst.color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}
