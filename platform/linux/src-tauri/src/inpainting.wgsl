struct Params {
    frame_width: u32,
    frame_height: u32,
    region_count: u32,
    _padding: u32,
};

struct Region {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    output_offset: u32,
    axis: u32,
    far_available: u32,
    _padding: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> source_pixels: array<u32>;
@group(0) @binding(2) var<storage, read> regions: array<Region>;
@group(0) @binding(3) var<storage, read_write> output_pixels: array<u32>;

fn channel(pixel: u32, shift: u32) -> f32 {
    return f32((pixel >> shift) & 0xffu);
}

fn blend_pixel(near: u32, far: u32, amount: f32) -> u32 {
    let inverse = 1.0 - amount;
    let blue = u32(clamp(channel(near, 0u) * amount + channel(far, 0u) * inverse, 0.0, 255.0));
    let green = u32(clamp(channel(near, 8u) * amount + channel(far, 8u) * inverse, 0.0, 255.0));
    let red = u32(clamp(channel(near, 16u) * amount + channel(far, 16u) * inverse, 0.0, 255.0));
    return blue | (green << 8u) | (red << 16u) | 0xff000000u;
}

fn sample_source(x: u32, y: u32) -> u32 {
    return source_pixels[y * params.frame_width + x] | 0xff000000u;
}

@compute @workgroup_size(8, 8, 1)
fn inpaint(@builtin(global_invocation_id) invocation: vec3<u32>) {
    if (invocation.z >= params.region_count) {
        return;
    }
    let region = regions[invocation.z];
    let dx = invocation.x;
    let dy = invocation.y;
    if (dx >= region.width || dy >= region.height) {
        return;
    }

    var near_x = region.x + dx;
    var near_y = region.y - region.height + (region.height - 1u - dy);
    var far_x = region.x + dx;
    var far_y = region.y + region.height + (region.height - 1u - dy);
    var blend_axis_offset = dy;
    var blend_axis_size = region.height;

    if (region.axis == 1u) {
        near_x = region.x - region.width + (region.width - 1u - dx);
        near_y = region.y + dy;
        far_x = region.x + region.width + (region.width - 1u - dx);
        far_y = region.y + dy;
        blend_axis_offset = dx;
        blend_axis_size = region.width;
    }

    let near = sample_source(near_x, near_y);
    var result = near;
    if (region.far_available == 1u) {
        let far = sample_source(far_x, far_y);
        let denominator = max(f32(blend_axis_size) - 1.0, 1.0);
        let amount = 1.0 - f32(blend_axis_offset) / denominator;
        result = blend_pixel(near, far, amount);
    }
    output_pixels[region.output_offset + dy * region.width + dx] = result;
}
