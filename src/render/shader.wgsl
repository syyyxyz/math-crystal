struct Uniforms {
    view_proj: mat4x4<f32>,
    // xyz: direction towards the light (world).
    light_dir: vec4<f32>,
    // xyz: eye position; w = 1 for perspective, 0 for orthographic.
    eye: vec4<f32>,
    // xyz: direction from the scene towards the camera.
    view_dir: vec4<f32>,
    clip_min: vec4<f32>,
    clip_max: vec4<f32>,
    // Viewport size in physical pixels.
    viewport: vec4<f32>,
    // rgb: wireframe colour (linear), a: ambient level.
    wire: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

// The colour target is plain Rgba8Unorm: encode linear colour to sRGB here.
fn encode(rgb: vec3<f32>) -> vec3<f32> {
    let c = clamp(rgb, vec3(0.0), vec3(1.0));
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3(0.0031308));
}

fn outside(p: vec3<f32>) -> bool {
    let e = 1e-3;
    return any(p < u.clip_min.xyz - vec3(e)) || any(p > u.clip_max.xyz + vec3(e));
}

fn to_view(p: vec3<f32>) -> vec3<f32> {
    if u.eye.w > 0.5 {
        return normalize(u.eye.xyz - p);
    }
    return normalize(u.view_dir.xyz);
}

struct MeshIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) grid: vec3<f32>,
};

struct MeshOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) grid: vec3<f32>,
};

fn mesh_out(v: MeshIn) -> MeshOut {
    var o: MeshOut;
    o.clip = u.view_proj * vec4(v.pos, 1.0);
    o.world = v.pos;
    o.normal = v.normal;
    o.color = v.color;
    o.grid = v.grid;
    return o;
}

@vertex
fn vs_mesh(v: MeshIn) -> MeshOut {
    return mesh_out(v);
}

// Translucent overlays (Taylor model, tangent plane) touch the surface to
// high order near P; a small bias towards the camera avoids depth fighting.
@vertex
fn vs_mesh_biased(v: MeshIn) -> MeshOut {
    var o = mesh_out(v);
    o.clip.z = o.clip.z - 0.0004 * o.clip.w;
    return o;
}

@fragment
fn fs_mesh(i: MeshOut) -> @location(0) vec4<f32> {
    // Derivatives must be taken in uniform control flow, before discard.
    let fw = max(fwidth(i.grid.xy), vec2(1e-6));
    if outside(i.world) {
        discard;
    }
    let v = to_view(i.world);
    var n = normalize(i.normal);
    // Two-sided lighting.
    if dot(n, v) < 0.0 {
        n = -n;
    }
    let l = normalize(u.light_dir.xyz);
    let diffuse = max(dot(n, l), 0.0);
    let h = normalize(l + v);
    let spec = pow(max(dot(n, h), 0.0), 60.0) * 0.18;
    let fill = max(dot(n, v), 0.0) * 0.22;
    var rgb = i.color.rgb * (u.wire.a + 0.55 * diffuse + fill) + vec3(spec);
    if i.grid.z > 0.0 {
        let d = abs(fract(i.grid.xy - 0.5) - 0.5) / fw;
        let line = 1.0 - clamp(min(d.x, d.y) - 0.3, 0.0, 1.0);
        rgb = mix(rgb, u.wire.rgb, line * i.grid.z);
    }
    return vec4(encode(rgb), i.color.a);
}

struct LineIn {
    @location(0) a: vec3<f32>,
    @location(1) b: vec3<f32>,
    @location(2) color: vec4<f32>,
    // x: width in pixels, y: clip flag.
    @location(3) params: vec2<f32>,
};

struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) flag: f32,
};

@vertex
fn vs_line(@builtin(vertex_index) vi: u32, l: LineIn) -> LineOut {
    var ts = array<f32, 6>(0.0, 0.0, 1.0, 0.0, 1.0, 1.0);
    var ss = array<f32, 6>(-1.0, 1.0, 1.0, -1.0, 1.0, -1.0);
    let t = ts[vi];
    let s = ss[vi];
    let ca = u.view_proj * vec4(l.a, 1.0);
    let cb = u.view_proj * vec4(l.b, 1.0);
    let half_vp = u.viewport.xy * 0.5;
    let sa = ca.xy / ca.w * half_vp;
    let sb = cb.xy / cb.w * half_vp;
    var dir = sb - sa;
    if length(dir) < 1e-5 {
        dir = vec2(1.0, 0.0);
    }
    dir = normalize(dir);
    let nrm = vec2(-dir.y, dir.x);
    let c = mix(ca, cb, t);
    let w = l.params.x * 0.5;
    // Extend ends by half the width so joints overlap.
    let off = nrm * s * w + dir * (t * 2.0 - 1.0) * w;
    var o: LineOut;
    o.clip = c + vec4(off / half_vp * c.w, 0.0, 0.0);
    // Pull lines slightly towards the camera so they win over the surface
    // they lie on.
    o.clip.z = o.clip.z - 0.0015 * c.w;
    o.world = mix(l.a, l.b, t);
    o.color = l.color;
    o.flag = l.params.y;
    return o;
}

@fragment
fn fs_line(i: LineOut) -> @location(0) vec4<f32> {
    if i.flag > 0.5 && outside(i.world) {
        discard;
    }
    return vec4(encode(i.color.rgb), i.color.a);
}
