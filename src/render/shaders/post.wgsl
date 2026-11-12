// Post pass: takes the finished scene texture and applies the screen effects.
// This is the file to edit (or replace) for custom visuals.
//
// Colors are premultiplied alpha and stay that way on output.

struct Post {
    // xy = resolution in pixels, z = time in seconds, w = glitch amount 0..1
    frame: vec4<f32>,
    // x = scanlines, y = glow, z = chromatic aberration, w = vignette
    fx_a: vec4<f32>,
    // x = scan beam, y = noise, z = ui scale, w = unused
    fx_b: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Post;
@group(0) @binding(1) var scene: texture_2d<f32>;
@group(0) @binding(2) var scene_sampler: sampler;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> VsOut {
    // One oversized triangle covering the screen.
    let p = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    var out: VsOut;
    out.clip = vec4<f32>(p * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    out.uv = p;
    return out;
}

fn hash(p: vec2<f32>) -> f32 {
    var q = fract(vec3<f32>(p.xyx) * 0.1031);
    q = q + dot(q, q.yzx + 33.33);
    return fract((q.x + q.y) * q.z);
}

fn tap(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(scene, scene_sampler, uv, 0.0);
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let res = u.frame.xy;
    let time = u.frame.z;
    let glitch = u.frame.w;
    let px = 1.0 / res;
    let scale = max(u.fx_b.z, 1.0);
    var uv = in.uv;

    // Glitch: random horizontal slices tear sideways for a moment.
    if glitch > 0.0 {
        let slice = floor(uv.y * 36.0);
        let tick = floor(time * 24.0);
        let n = hash(vec2<f32>(slice, tick));
        if n > 1.0 - 0.45 * glitch {
            uv.x = uv.x + (hash(vec2<f32>(tick, slice)) - 0.5) * 0.08 * glitch;
        }
    }

    // Chromatic aberration: zero in the middle so text there stays sharp.
    let center = uv - vec2<f32>(0.5, 0.5);
    let edge = dot(center, center);
    let shift = (u.fx_a.z * 4.0 * edge * scale + glitch * 3.0) * px.x;
    let base = tap(uv);
    var col = base;
    if shift > 0.0 {
        col.r = tap(uv + vec2<f32>(shift, 0.0)).r;
        col.b = tap(uv - vec2<f32>(shift, 0.0)).b;
    }

    // Glow: bright pixels bleed into their surroundings.
    if u.fx_a.y > 0.0 {
        var glow = vec3<f32>(0.0);
        let radius = 5.0 * scale;
        for (var i = 0; i < 16; i = i + 1) {
            let fi = f32(i);
            let r = sqrt((fi + 0.5) / 16.0) * radius;
            let a = fi * 2.39996323;
            let s = tap(uv + vec2<f32>(cos(a), sin(a)) * r * px).rgb;
            glow = glow + max(s - vec3<f32>(0.22), vec3<f32>(0.0));
        }
        col = vec4<f32>(col.rgb + glow / 16.0 * u.fx_a.y, col.a);
    }

    // Scanlines, three pixel period.
    if u.fx_a.x > 0.0 {
        let wave = 0.5 - 0.5 * cos(in.clip.y / scale * 2.0943951);
        col = vec4<f32>(col.rgb * (1.0 - u.fx_a.x * wave), col.a);
    }

    // Scan beam: a faint band sweeping down the screen.
    if u.fx_b.x > 0.0 {
        let beam_y = fract(time * 0.11) * 1.3 - 0.15;
        let d = uv.y - beam_y;
        let band = exp(-d * d * 700.0);
        col = vec4<f32>(col.rgb + vec3<f32>(0.0, 0.94, 1.0) * band * u.fx_b.x, col.a);
    }

    // Film grain.
    if u.fx_b.y > 0.0 {
        let n = hash(in.clip.xy + vec2<f32>(time * 61.0, time * 37.0)) - 0.5;
        col = vec4<f32>(col.rgb + vec3<f32>(n * u.fx_b.y) * max(col.a, 0.25), col.a);
    }

    // Vignette.
    if u.fx_a.w > 0.0 {
        let v = smoothstep(0.30, 0.80, length(center));
        col = vec4<f32>(col.rgb * (1.0 - u.fx_a.w * v), col.a);
    }

    // During a glitch the whole image flashes slightly.
    col = vec4<f32>(col.rgb * (1.0 + glitch * 0.25), col.a);

    return max(col, vec4<f32>(0.0));
}
