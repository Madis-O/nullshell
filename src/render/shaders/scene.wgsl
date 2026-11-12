// Scene pass: every visible thing is an instanced quad.
//
// params.x selects how the quad is filled:
//   0  solid color
//   1  glyph from the atlas, alpha mask tinted with `color`
//   2  color glyph from the atlas (emoji), not tinted
//   3  arc stroke: uv.xy = circle center (px), params.y = radius, params.z = thickness
//   4  rectangle with 45 degree cut corners:
//        params.y = cut size, params.z = border thickness (0 = filled),
//        params.w = corner bit mask (1 TL, 2 TR, 4 BR, 8 BL)
//
// Output is premultiplied alpha.

struct Globals {
    screen: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

struct Instance {
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) uv: vec4<f32>,
    @location(3) color: vec4<f32>,
    @location(4) params: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) world: vec2<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) @interpolate(flat) color: vec4<f32>,
    @location(4) @interpolate(flat) params: vec4<f32>,
    @location(5) @interpolate(flat) size: vec2<f32>,
    @location(6) @interpolate(flat) raw_uv: vec4<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32, inst: Instance) -> VsOut {
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u));
    let world = inst.pos + corner * inst.size;
    var out: VsOut;
    out.clip = vec4<f32>(world / globals.screen * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    out.local = corner * inst.size;
    out.world = world;
    out.uv = mix(inst.uv.xy, inst.uv.zw, corner);
    out.color = inst.color;
    out.params = inst.params;
    out.size = inst.size;
    out.raw_uv = inst.uv;
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let mode = i32(in.params.x + 0.5);
    var alpha = 1.0;

    if mode == 1 {
        alpha = textureSampleLevel(atlas, atlas_sampler, in.uv, 0.0).a;
    } else if mode == 2 {
        let t = textureSampleLevel(atlas, atlas_sampler, in.uv, 0.0);
        return vec4<f32>(t.rgb * t.a, t.a) * in.color.a;
    } else if mode == 3 {
        let d = abs(distance(in.world, in.raw_uv.xy) - in.params.y);
        alpha = clamp(in.params.z * 0.5 + 0.5 - d, 0.0, 1.0);
    } else if mode == 4 {
        let p = in.local;
        let s = in.size;
        let cut = in.params.y;
        let border = in.params.z;
        let mask = u32(in.params.w + 0.5);
        let k = 0.70710678;
        var d = min(min(p.x, p.y), min(s.x - p.x, s.y - p.y));
        if (mask & 1u) != 0u { d = min(d, (p.x + p.y - cut) * k); }
        if (mask & 2u) != 0u { d = min(d, ((s.x - p.x) + p.y - cut) * k); }
        if (mask & 4u) != 0u { d = min(d, ((s.x - p.x) + (s.y - p.y) - cut) * k); }
        if (mask & 8u) != 0u { d = min(d, (p.x + (s.y - p.y) - cut) * k); }
        alpha = clamp(d + 0.5, 0.0, 1.0);
        if border > 0.0 {
            alpha = alpha * clamp(border - d + 0.5, 0.0, 1.0);
        }
    }

    alpha = alpha * in.color.a;
    return vec4<f32>(in.color.rgb * alpha, alpha);
}
