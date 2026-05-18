// Mirror-blend inpainter — WGSL compute shader.
// One workgroup per region. Each thread fills one output pixel with a
// cross-faded mirror sample from the surrounding bands.

struct RegionUniform {
    rect_min: vec2<f32>,    // pixel coords, top-left
    rect_size: vec2<f32>,   // width, height
    frame_size: vec2<f32>,  // frame width, height
    axis: u32,              // 0 = vertical mirror, 1 = horizontal mirror
    _pad: u32,
};

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_samp: sampler;
@group(0) @binding(2) var<uniform> region: RegionUniform;
@group(0) @binding(3) var dst_tex: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(8, 8, 1)
fn cs_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let local = vec2<f32>(f32(gid.x), f32(gid.y));
    if (local.x >= region.rect_size.x || local.y >= region.rect_size.y) {
        return;
    }
    let pix = region.rect_min + local;

    // Two reflection samples — "near" and "far" sides of the region.
    var near_uv: vec2<f32>;
    var far_uv: vec2<f32>;
    var t: f32;

    if (region.axis == 0u) {
        // Vertical mirror: sample from above (y < rect_min.y) and below
        // (y > rect_min.y + rect_size.y), reflected across each adjacent edge.
        let dy = local.y;                            // distance from top of rect
        let dy_far = region.rect_size.y - local.y;   // distance from bottom of rect
        near_uv = vec2<f32>(pix.x, region.rect_min.y - dy);
        far_uv = vec2<f32>(pix.x, region.rect_min.y + region.rect_size.y + dy_far);
        t = local.y / region.rect_size.y;            // 0 at top → near, 1 at bottom → far
    } else {
        let dx = local.x;
        let dx_far = region.rect_size.x - local.x;
        near_uv = vec2<f32>(region.rect_min.x - dx, pix.y);
        far_uv = vec2<f32>(region.rect_min.x + region.rect_size.x + dx_far, pix.y);
        t = local.x / region.rect_size.x;
    }

    let near_norm = near_uv / region.frame_size;
    let far_norm = far_uv / region.frame_size;

    let near_color = textureSampleLevel(src_tex, src_samp, near_norm, 0.0);
    let far_color = textureSampleLevel(src_tex, src_samp, far_norm, 0.0);

    let color = mix(near_color, far_color, t);
    textureStore(dst_tex, vec2<i32>(i32(local.x), i32(local.y)), color);
}
