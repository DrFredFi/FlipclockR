// Scene shading: PBR tiles and dots, PCF shadows, textured background.

const PI: f32 = 3.14159265;

const MODE_TILE: f32 = 0.0;
const MODE_PLAIN: f32 = 1.0;
const MODE_BACKGROUND: f32 = 2.0;

struct Globals {
    view_proj: mat4x4<f32>,
    light_view_proj: mat4x4<f32>,
    // xyz camera position
    camera: vec4<f32>,
    // xyz direction towards the light, w intensity
    light: vec4<f32>,
    // rgb digit color (linear), w ambient
    digit: vec4<f32>,
    // x shadow texel size (uv), y normal offset (world), z background dim, w shadow strength
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;
@group(0) @binding(3) var atlas: texture_2d<f32>;
@group(0) @binding(4) var linear_sampler: sampler;
@group(0) @binding(5) var background: texture_2d<f32>;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) kind: f32,
    @location(4) m0: vec4<f32>,
    @location(5) m1: vec4<f32>,
    @location(6) m2: vec4<f32>,
    @location(7) m3: vec4<f32>,
    // rgb base color (linear), w mode
    @location(8) color: vec4<f32>,
    // x metallic, y roughness, z front digit, w back digit
    @location(9) material: vec4<f32>,
    // background uv scale (xy) and offset (zw)
    @location(10) extra: vec4<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) @interpolate(flat) kind: f32,
    @location(4) @interpolate(flat) color: vec4<f32>,
    @location(5) @interpolate(flat) material: vec4<f32>,
    @location(6) @interpolate(flat) extra: vec4<f32>,
};

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    let model = mat4x4<f32>(v.m0, v.m1, v.m2, v.m3);
    let world = model * vec4<f32>(v.position, 1.0);
    var out: VertexOut;
    out.clip = g.view_proj * world;
    out.world = world.xyz;
    out.normal = normalize((model * vec4<f32>(v.normal, 0.0)).xyz);
    out.uv = v.uv;
    out.kind = v.kind;
    out.color = v.color;
    out.material = v.material;
    out.extra = v.extra;
    return out;
}

struct ShadowIn {
    @location(0) position: vec3<f32>,
    @location(4) m0: vec4<f32>,
    @location(5) m1: vec4<f32>,
    @location(6) m2: vec4<f32>,
    @location(7) m3: vec4<f32>,
};

@vertex
fn vs_shadow(v: ShadowIn) -> @builtin(position) vec4<f32> {
    let model = mat4x4<f32>(v.m0, v.m1, v.m2, v.m3);
    return g.light_view_proj * model * vec4<f32>(v.position, 1.0);
}

fn shadow_factor(world: vec3<f32>, normal: vec3<f32>) -> f32 {
    let p = g.light_view_proj * vec4<f32>(world + normal * g.params.y, 1.0);
    let ndc = p.xyz / p.w;
    let uv = ndc.xy * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5, 0.5);
    if (any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) || ndc.z > 1.0) {
        return 1.0;
    }
    let texel = g.params.x;
    var sum = 0.0;
    for (var y = -2; y <= 2; y++) {
        for (var x = -2; x <= 2; x++) {
            let offset = vec2<f32>(f32(x), f32(y)) * texel * 1.5;
            sum += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + offset, ndc.z);
        }
    }
    return sum / 25.0;
}

fn distribution_ggx(n_dot_h: f32, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

fn geometry_smith(n_dot_v: f32, n_dot_l: f32, roughness: f32) -> f32 {
    let r = roughness + 1.0;
    let k = r * r / 8.0;
    let gv = n_dot_v / (n_dot_v * (1.0 - k) + k);
    let gl = n_dot_l / (n_dot_l * (1.0 - k) + k);
    return gv * gl;
}

fn fresnel_schlick(cos_theta: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}

fn shade(albedo: vec3<f32>, metallic: f32, roughness: f32, n: vec3<f32>, world: vec3<f32>, shadow: f32) -> vec3<f32> {
    let v = normalize(g.camera.xyz - world);
    let l = normalize(g.light.xyz);
    let h = normalize(v + l);
    let n_dot_l = max(dot(n, l), 0.0);
    let n_dot_v = max(dot(n, v), 1e-4);
    let n_dot_h = max(dot(n, h), 0.0);
    let v_dot_h = max(dot(v, h), 0.0);

    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    let f = fresnel_schlick(v_dot_h, f0);
    let d = distribution_ggx(n_dot_h, roughness);
    let gs = geometry_smith(n_dot_v, n_dot_l, roughness);
    let specular = d * gs * f / (4.0 * n_dot_v * max(n_dot_l, 1e-4) + 1e-4);
    let kd = (vec3<f32>(1.0) - f) * (1.0 - metallic);
    let radiance = g.light.w * 2.6;
    let direct = (kd * albedo / PI + specular) * radiance * n_dot_l * shadow;

    // Soft ambient plus a faint studio-like reflection for glossy finishes.
    let ambient = g.digit.w;
    let r = reflect(-v, n);
    let env = mix(vec3<f32>(0.05), vec3<f32>(0.6), clamp(r.y * 0.5 + 0.5, 0.0, 1.0));
    let fv = fresnel_schlick(n_dot_v, f0);
    let gloss = (1.0 - roughness) * (1.0 - roughness);
    let indirect = albedo * ambient * (1.0 - metallic * 0.7) + env * fv * gloss * 0.5 + f0 * ambient * 0.5;
    return direct + indirect;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let mode = in.color.w;
    let n = normalize(in.normal);
    let shadow = mix(1.0, shadow_factor(in.world, n), g.params.w);

    if (mode > 1.5) {
        let uv = in.uv * in.extra.xy + in.extra.zw;
        let tex = textureSampleLevel(background, linear_sampler, uv, 0.0).rgb;
        let albedo = tex * in.color.rgb * (1.0 - g.params.z);
        return vec4<f32>(albedo * mix(0.42, 1.0, shadow), 1.0);
    }

    var albedo = in.color.rgb;
    if (mode < 0.5 && in.kind > 0.5) {
        let digit = select(in.material.z, in.material.w, in.kind > 1.5);
        let atlas_uv = vec2<f32>((digit + clamp(in.uv.x, 0.0, 1.0)) / 10.0, in.uv.y);
        let coverage = textureSampleLevel(atlas, linear_sampler, atlas_uv, 0.0).r;
        albedo = mix(albedo, g.digit.rgb, coverage);
    }
    let color = shade(albedo, in.material.x, clamp(in.material.y, 0.04, 1.0), n, in.world, shadow);
    return vec4<f32>(min(color, vec3<f32>(1.0)), 1.0);
}
