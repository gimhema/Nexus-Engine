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

// ── 장면 조명 ────────────────────────────────────────────────────────────────
// 픽셀아트와 섞기 위해 (1) 빛을 **텍셀 중심**에서 계산하고 (2) 밝기를 단계로 끊는다.
// 그래서 빛의 경계가 그림 1픽셀 단위로 계단지고, 손으로 칠한 명암처럼 띠가 진다.
const MAX_LIGHTS: u32 = 64u;

struct Light {
    pos_radius: vec4<f32>, // 월드 위치 (m), 반지름 (m)
    color: vec4<f32>,      // 선형 색 × 세기
};

struct Lighting {
    ambient: vec4<f32>, // 선형 주변광, w = 1 이면 조명 켜짐
    params: vec4<f32>,  // x = 밝기 단계 수 (0 = 끊지 않음), y = 광원 수, z = 디더 (0/1)
    lights: array<Light, MAX_LIGHTS>,
};

@group(0) @binding(1) var<uniform> lighting: Lighting;

@group(1) @binding(0) var t_color: texture_2d<f32>;
@group(1) @binding(1) var s_color: sampler;
// 발광 마스크 — 불투명 픽셀은 어둠과 상관없이 그 색으로 빛난다. 없는 텍스처는 1×1 투명.
@group(1) @binding(2) var t_emissive: texture_2d<f32>;

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
    @location(10) depth_span: vec2<f32>, // 층의 NDC 깊이 구간 (시작, 크기)
    @location(11) lit: f32,               // 1 = 장면 조명을 받는다 (지면·오브젝트 층의 월드 그림)
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv:    vec2<f32>,
    @location(2) world: vec3<f32>,
    @location(3) lit:   f32,
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

    // 정사영이라 w = 1 이고 clip.z 가 곧 NDC 깊이다 (0..1).
    // ① 층이 쓰는 구간으로 눌러 넣는다 — 지면이 오브젝트를 가리지 못하게 하는 장치다.
    // ② depth_bias 로 같은 층 안의 앞뒤를 정한다. 화면 위치(x, y)는 건드리지 않으므로
    //    월드 Z 와 달리 표시가 대상에서 떨어지지 않는다.
    out.clip.z = inst.depth_span.x + out.clip.z * inst.depth_span.y - inst.depth_bias;
    out.color = inst.color;
    out.uv = uv;
    out.world = world;
    out.lit = inst.lit;
    return out;
}

// 4×4 베이어 행렬 (0..15) / 16.
fn bayer(p: vec2<u32>) -> f32 {
    let m = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    return m[(p.y % 4u) * 4u + (p.x % 4u)] / 16.0;
}

// 이 프래그먼트가 받는 빛 (선형 배수). 조명이 꺼졌거나 받지 않는 그림이면 1.
//
// 텍셀 중심 맞추기: 프래그먼트의 UV 를 텍셀 중심으로 옮기고, 그만큼 월드 위치도 옮긴다.
// UV→월드 변환은 화면 미분(dpdx/dpdy)으로 푼다 — 눕는 쿼드·서는 빌보드 모두 같은 식.
// 미분은 분기 전에 구해야 해서(균일 제어 흐름) 호출자가 먼저 넘긴다.
fn scene_light(world: vec3<f32>, uv: vec2<f32>, lit: f32,
               dwx: vec3<f32>, dwy: vec3<f32>, dux: vec2<f32>, duy: vec2<f32>) -> vec3<f32> {
    if (lighting.ambient.w < 0.5 || lit < 0.5) {
        return vec3<f32>(1.0);
    }
    let dims = vec2<f32>(textureDimensions(t_color));
    var p = world;
    var texel = vec2<u32>(0u, 0u);
    if (dims.x > 1.0 || dims.y > 1.0) {
        let su = (floor(uv * dims) + 0.5) / dims;
        let d = su - uv;
        let det = dux.x * duy.y - duy.x * dux.y;
        if (abs(det) > 1e-12) {
            let a = (d.x * duy.y - duy.x * d.y) / det;
            let b = (dux.x * d.y - d.x * dux.y) / det;
            p = world + dwx * a + dwy * b;
        }
        texel = vec2<u32>(floor(uv * dims));
    }
    // 광원이 더하는 빛만 단계로 끊는다 — 주변광은 매끈하게 둬야 빛이 닿지 않는 곳에 디더 잡티가 없다.
    var add = vec3<f32>(0.0);
    let count = min(u32(lighting.params.y), MAX_LIGHTS);
    for (var i = 0u; i < count; i = i + 1u) {
        let light = lighting.lights[i];
        let r = light.pos_radius.w;
        let dist = distance(p, light.pos_radius.xyz);
        if (dist < r) {
            let t = 1.0 - dist / r;
            add = add + light.color.rgb * (t * t);
        }
    }
    // 지각 명도(감마 2.2)에서 끊어야 단계가 고르게 보인다. 색 비율은 지킨다.
    let steps = lighting.params.x;
    let k = max(max(add.r, add.g), add.b);
    if (steps > 0.5 && k > 1e-4) {
        let dither = select(0.5, bayer(texel), lighting.params.z > 0.5);
        let kp = pow(min(k, 2.0), 1.0 / 2.2);
        let q = max(floor(kp * steps + dither), 0.0) / steps;
        add = add * (pow(q, 2.2) / k);
    }
    let l = lighting.ambient.rgb + add;
    return min(l, vec3<f32>(2.0));
}

fn emissive_at(uv: vec2<f32>, lit: f32) -> vec3<f32> {
    let e = textureSample(t_emissive, s_color, uv);
    if (lighting.ambient.w < 0.5 || lit < 0.5) {
        return vec3<f32>(0.0);
    }
    return e.rgb * e.a;
}

// 알파 블렌딩용. 그리드·에디터 표시처럼 반투명이 필요한 것에 쓴다.
@fragment
fn fs_blend(in: VsOut) -> @location(0) vec4<f32> {
    let dwx = dpdx(in.world);
    let dwy = dpdy(in.world);
    let dux = dpdx(in.uv);
    let duy = dpdy(in.uv);
    let c = textureSample(t_color, s_color, in.uv) * in.color;
    let e = emissive_at(in.uv, in.lit);
    let l = scene_light(in.world, in.uv, in.lit, dwx, dwy, dux, duy);
    return vec4<f32>(max(c.rgb * l, e), c.a);
}

// 알파 컷아웃용. 스프라이트 전용 — 깊이 쓰기를 켠 채로도 정렬이 깨지지 않는다.
@fragment
fn fs_cutout(in: VsOut) -> @location(0) vec4<f32> {
    // 미분·샘플링은 discard 앞에서 — 균일 제어 흐름이어야 한다.
    let dwx = dpdx(in.world);
    let dwy = dpdy(in.world);
    let dux = dpdx(in.uv);
    let duy = dpdy(in.uv);
    let c = textureSample(t_color, s_color, in.uv) * in.color;
    let e = emissive_at(in.uv, in.lit);
    if (c.a < ALPHA_CUTOFF) {
        discard;
    }
    let l = scene_light(in.world, in.uv, in.lit, dwx, dwy, dux, duy);
    // 통과한 픽셀은 완전 불투명으로 취급한다. 반투명 가장자리를 남기면
    // 블렌딩이 꺼져 있어 배경색이 아니라 쓰레기 값이 섞인다.
    return vec4<f32>(max(c.rgb * l, e), 1.0);
}
