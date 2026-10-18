struct Frame {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    light: vec4<f32>,
    // x, y — размер области в пикселях; z — 1, если цель не sRGB (нужна гамма вручную)
    viewport: vec4<f32>,
    bg_top: vec4<f32>,
    bg_bottom: vec4<f32>,
    edge_color: vec4<f32>,
}
@group(0) @binding(0) var<uniform> frame: Frame;
// цвет каждого тела (альфа 0 — тело скрыто)
@group(0) @binding(1) var<storage, read> body_colors: array<vec4<f32>>;

fn encode(c: vec3<f32>) -> vec3<f32> {
    if frame.viewport.z > 0.5 {
        // линейный -> sRGB
        let lo = c * 12.92;
        let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
        return select(hi, lo, c <= vec3<f32>(0.0031308));
    }
    return c;
}

// ---------------------------------------------------------------- фон

struct BgOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) t: f32,
}

@vertex
fn vs_bg(@builtin(vertex_index) i: u32) -> BgOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: BgOut;
    o.clip = vec4<f32>(uv * 2.0 - 1.0, 1.0, 1.0);
    o.t = uv.y; // 0 внизу, 1 вверху
    return o;
}

@fragment
fn fs_bg(i: BgOut) -> @location(0) vec4<f32> {
    let k = clamp(i.t, 0.0, 1.0);
    return vec4<f32>(encode(mix(frame.bg_bottom.rgb, frame.bg_top.rgb, k)), 1.0);
}

// ---------------------------------------------------------------- грани тел

struct FaceIn {
    @location(0) pos: vec3<f32>,
    @location(1) nrm: vec3<f32>,
    @location(2) slot: u32,
}
struct FaceOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) nrm: vec3<f32>,
    @location(1) world: vec3<f32>,
    @location(2) color: vec4<f32>,
}

@vertex
fn vs_face(v: FaceIn) -> FaceOut {
    var o: FaceOut;
    o.color = body_colors[v.slot];
    o.clip = frame.view_proj * vec4<f32>(v.pos, 1.0);
    if abs(o.color.a) < 0.001 {
        o.clip = vec4<f32>(0.0, 0.0, -2.0, 1.0); // скрытое тело: выбрасываем вершину
    }
    o.nrm = v.nrm;
    o.world = v.pos;
    return o;
}

@fragment
fn fs_face(i: FaceOut) -> @location(0) vec4<f32> {
    var n = normalize(i.nrm);
    let v = normalize(frame.eye.xyz - i.world);
    if dot(n, v) < 0.0 {
        n = -n; // оборотная сторона (например, при разрезе) освещается так же
    }
    let l = normalize(frame.light.xyz);
    let diff = max(dot(n, l), 0.0);
    let h = normalize(l + v);
    let cutter = i.color.a < 0.0;
    let reflection = select(clamp(i.color.a, 0.0, 1.0), 0.12, cutter);
    let gloss_power = mix(12.0, 96.0, reflection);
    let spec = pow(max(dot(n, h), 0.0), gloss_power) * mix(0.03, 0.62, reflection);
    // мягкий «небесный» свет сверху
    let sky = 0.5 + 0.5 * n.z;
    // «окружение»: небо сверху, тёмная земля снизу; на скользящих углах отражение сильнее
    let r = reflect(-v, n);
    let env = mix(vec3<f32>(0.035, 0.04, 0.05), vec3<f32>(0.75, 0.82, 0.95), smoothstep(-0.25, 0.85, r.z));
    let fres = pow(1.0 - max(dot(n, v), 0.0), 3.0);
    let c = i.color.rgb * (0.22 + 0.16 * sky + 0.62 * diff) + env * (0.03 + reflection * (0.34 + 0.34 * fres)) + vec3<f32>(spec);
    return vec4<f32>(encode(c), select(1.0, 0.32, cutter));
}

// ---------------------------------------------------------------- толстые линии

struct LineIn {
    @builtin(vertex_index) vi: u32,
    @location(0) p0: vec3<f32>,
    @location(1) p1: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) width: f32,
    // индекс тела для рёбер (0xFFFFFFFF — не тело)
    @location(4) slot: u32,
}
struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
}

fn clip_near(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    // конец за камерой — подтягиваем к ближней плоскости, иначе направление на экране ломается
    let eps = 1e-4;
    if a.w < eps {
        let t = (eps - a.w) / (b.w - a.w);
        return mix(a, b, t);
    }
    return a;
}

@vertex
fn vs_line(v: LineIn) -> LineOut {
    var o: LineOut;
    var color = v.color;
    if v.slot != 0xffffffffu {
        let bc = body_colors[v.slot];
        if abs(bc.a) < 0.001 {
            o.clip = vec4<f32>(0.0, 0.0, -2.0, 1.0);
            return o;
        }
        color = frame.edge_color;
    }
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0),
    );
    let c = corners[v.vi];
    var a = frame.view_proj * vec4<f32>(v.p0, 1.0);
    var b = frame.view_proj * vec4<f32>(v.p1, 1.0);
    if a.w < 1e-4 && b.w < 1e-4 {
        o.clip = vec4<f32>(0.0, 0.0, -2.0, 1.0);
        return o;
    }
    let a2 = clip_near(a, b);
    let b2 = clip_near(b, a);
    let half = frame.viewport.xy * 0.5;
    let sa = a2.xy / a2.w * half;
    let sb = b2.xy / b2.w * half;
    var dir = sb - sa;
    let len = length(dir);
    if len < 1e-6 {
        dir = vec2<f32>(1.0, 0.0);
    } else {
        dir = dir / len;
    }
    let nrm = vec2<f32>(-dir.y, dir.x);
    let w = v.width * 0.5;
    let along = select(-1.0, 1.0, c.x > 0.5);
    let off_px = nrm * c.y * w + dir * along * w * 0.5;
    var p = select(a2, b2, c.x > 0.5);
    p = vec4<f32>(p.xy + off_px / half * p.w, p.z, p.w);
    o.clip = p;
    o.color = color;
    return o;
}

@fragment
fn fs_line(i: LineOut) -> @location(0) vec4<f32> {
    return vec4<f32>(encode(i.color.rgb), i.color.a);
}

// ---------------------------------------------------------------- полупрозрачные треугольники

struct TriIn {
    @location(0) pos: vec3<f32>,
    @location(1) color: vec4<f32>,
}

@vertex
fn vs_tri(v: TriIn) -> LineOut {
    var o: LineOut;
    o.clip = frame.view_proj * vec4<f32>(v.pos, 1.0);
    o.color = v.color;
    return o;
}
