// Screen-space text quad (the date line).

struct Overlay {
    // x, y, w, h in normalized device coordinates (x, y = top-left corner)
    rect: vec4<f32>,
    // linear rgb, a opacity
    color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> o: Overlay;
@group(0) @binding(1) var mask: texture_2d<f32>;
@group(0) @binding(2) var mask_sampler: sampler;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VertexOut {
    let corner = vec2<f32>(f32(i & 1u), f32((i >> 1u) & 1u));
    var out: VertexOut;
    out.clip = vec4<f32>(o.rect.x + corner.x * o.rect.z, o.rect.y - corner.y * o.rect.w, 0.0, 1.0);
    out.uv = corner;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let coverage = textureSampleLevel(mask, mask_sampler, in.uv, 0.0).r;
    return vec4<f32>(o.color.rgb, o.color.a * coverage);
}
