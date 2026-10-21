// Separable gaussian blur of the scene behind frosted glass: one fullscreen triangle per direction.
struct Blur {
    // xy: step between taps in uv units (x only for the horizontal pass, y only for the vertical one)
    dir: vec4<f32>,
}
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> blur: Blur;

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_blur(@builtin(vertex_index) i: u32) -> Out {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: Out;
    o.clip = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return o;
}

@fragment
fn fs_blur(i: Out) -> @location(0) vec4<f32> {
    // 9 bilinear taps stand for a 17-tap gaussian
    var offs = array<f32, 5>(0.0, 1.3846153846, 3.2307692308, 5.0769230769, 6.9230769231);
    var w = array<f32, 5>(0.2270270270, 0.3162162162, 0.0702702703, 0.0054054054, 0.0001);
    var c = textureSampleLevel(src, samp, i.uv, 0.0).rgb * w[0];
    for (var k = 1; k < 5; k = k + 1) {
        let d = blur.dir.xy * offs[k];
        c += textureSampleLevel(src, samp, i.uv + d, 0.0).rgb * w[k];
        c += textureSampleLevel(src, samp, i.uv - d, 0.0).rgb * w[k];
    }
    return vec4<f32>(c, 1.0);
}
