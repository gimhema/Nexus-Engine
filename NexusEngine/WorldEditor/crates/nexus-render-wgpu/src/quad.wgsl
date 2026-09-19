// 인스턴스 기반 텍스처 쿼드.
//
// 그리드 선·오브젝트·스프라이트를 모두 이 셰이더 하나로 그린다 — 선은 얇은 쿼드다.
// 정점 버퍼는 없다: 단위 쿼드를 vertex_index 로 생성한다.
//
// 단색 쿼드는 1×1 흰색 텍스처를 샘플링한다. 그래서 "텍스처 있는 경로"와
// "없는 경로"로 셰이더가 갈리지 않는다 — 분기 대신 데이터로 처리한다.
//
// 좌표는 월드 공간(m, Z-up)이며 view_proj 가 정사영/원근 어느 쪽이든 동작한다.
// S3 에서 빌보드 정점 생성이 추가되어도 프래그먼트 쪽은 그대로다.

struct Camera {
    view_proj: mat4x4<f32>,
    // 빌보드 축. w 성분은 정렬용 패딩이다 (유니폼 버퍼는 16바이트 정렬).
    right: vec4<f32>,
    up:    vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;

@group(1) @binding(0) var t_color: texture_2d<f32>;
@group(1) @binding(1) var s_color: sampler;

struct Instance {
    @location(0) center:   vec2<f32>, // 월드 중심 (m)
    @location(1) size:     vec2<f32>, // 월드 크기 (m). x 가 회전 방향 쪽 길이
    @location(2) z:        f32,       // 높이 = 깊이 정렬 기준
    @location(3) rotation: f32,       // Z 축 회전 (라디안, 반시계 +)
    @location(4) color:    vec4<f32>, // 선형 색 공간. 텍스처에 곱해진다
    @location(5) uv_min:   vec2<f32>, // 아틀라스 영역 좌상단 (정규화)
    @location(6) uv_max:   vec2<f32>, // 아틀라스 영역 우하단 (정규화)
    @location(7) depth_bias: f32,     // NDC 깊이 편향. 양수가 앞. 화면 위치에는 영향 없음
    @location(8) billboard:  f32,     // 0 = 지면에 눕는 쿼드, 1 = 카메라를 향해 서는 빌보드
    @location(9) anchor:     f32,     // 빌보드 전용. 0 = 중앙, 0.5 = 발밑
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv:    vec2<f32>,
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

// 알파 컷아웃 기준. nexus_render::ALPHA_CUTOFF 와 같은 값이어야 한다.
const ALPHA_CUTOFF: f32 = 0.5;

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: Instance) -> VsOut {
    let corner = CORNERS[vi];

    var world: vec3<f32>;
    if (inst.billboard > 0.5) {
        // 카메라 축으로 세운다. 두 축 모두 시선과 직교하므로 쿼드 전체의 깊이가
        // inst.center/z 하나로 정해진다 — 발밑 위치로만 정렬된다.
        let anchored = corner.y + inst.anchor;
        world = vec3<f32>(inst.center, inst.z)
              + camera.right.xyz * (corner.x * inst.size.x)
              + camera.up.xyz    * (anchored * inst.size.y);
    } else {
        // 지면(XY 평면)에 눕는다. 크기를 먼저 곱한 뒤 회전한다 —
        // 순서가 바뀌면 직사각형이 찌그러진다.
        let local = corner * inst.size;
        let c = cos(inst.rotation);
        let s = sin(inst.rotation);
        let offset = vec2<f32>(local.x * c - local.y * s, local.x * s + local.y * c);
        world = vec3<f32>(inst.center + offset, inst.z);
    }

    // 코너 오프셋(-0.5~0.5) → 0~1 → UV.
    // V 는 뒤집는다: 월드 +Y(쿼드 위쪽)가 이미지의 **윗줄**(uv_min.y)이어야 한다.
    // 뒤집지 않으면 스프라이트가 상하 반전된다.
    let t = corner + vec2<f32>(0.5, 0.5);
    let uv = vec2<f32>(
        mix(inst.uv_min.x, inst.uv_max.x, t.x),
        mix(inst.uv_max.y, inst.uv_min.y, t.y),
    );

    var out: VsOut;
    out.clip = camera.view_proj * vec4<f32>(world, 1.0);
    // 정사영이라 w = 1 이므로 clip.z 가 곧 NDC 깊이다. 빼면 앞으로 당겨진다.
    // 화면 위치(x, y)는 건드리지 않는다 — 월드 Z 와 달리 표시가 대상에서 떨어지지 않는다.
    out.clip.z = out.clip.z - inst.depth_bias;
    out.color = inst.color;
    out.uv = uv;
    return out;
}

// 알파 블렌딩용. 그리드·에디터 표시처럼 반투명이 필요한 것에 쓴다.
@fragment
fn fs_blend(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(t_color, s_color, in.uv) * in.color;
}

// 알파 컷아웃용. 스프라이트 전용 — 깊이 쓰기를 켠 채로도 정렬이 깨지지 않는다.
@fragment
fn fs_cutout(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(t_color, s_color, in.uv) * in.color;
    if (c.a < ALPHA_CUTOFF) {
        discard;
    }
    // 통과한 픽셀은 완전 불투명으로 취급한다. 반투명 가장자리를 남기면
    // 블렌딩이 꺼져 있어 배경색이 아니라 쓰레기 값이 섞인다.
    return vec4<f32>(c.rgb, 1.0);
}
