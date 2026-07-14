Texture2D<float4> source_frame : register(t0);
RWTexture2D<float4> output_patch : register(u0);

cbuffer RegionConstants : register(b0) {
    uint source_width;
    uint source_height;
    uint region_x;
    uint region_y;
    uint region_width;
    uint region_height;
    uint axis;
    uint near_available;
    uint far_available;
    uint reserved0;
    uint reserved1;
    uint reserved2;
};

[numthreads(8, 8, 1)]
void main(uint3 dispatch_id : SV_DispatchThreadID) {
    uint dx = dispatch_id.x;
    uint dy = dispatch_id.y;
    if (dx >= region_width || dy >= region_height) {
        return;
    }

    uint2 near_coord;
    uint2 far_coord;
    float t;
    if (axis == 0) {
        near_coord = near_available != 0
            ? uint2(region_x + dx, region_y - 1 - dy)
            : uint2(0, 0);
        far_coord = far_available != 0
            ? uint2(region_x + dx, region_y + 2 * region_height - 1 - dy)
            : uint2(0, 0);
        t = 1.0 - ((float)dy / max(1.0, (float)region_height - 1.0));
    } else {
        near_coord = near_available != 0
            ? uint2(region_x - 1 - dx, region_y + dy)
            : uint2(0, 0);
        far_coord = far_available != 0
            ? uint2(region_x + 2 * region_width - 1 - dx, region_y + dy)
            : uint2(0, 0);
        t = 1.0 - ((float)dx / max(1.0, (float)region_width - 1.0));
    }

    float4 near_pixel = float4(0.0, 0.0, 0.0, 1.0);
    float4 far_pixel = float4(0.0, 0.0, 0.0, 1.0);
    if (near_available != 0) {
        near_pixel = source_frame.Load(int3(near_coord, 0));
    }
    if (far_available != 0) {
        far_pixel = source_frame.Load(int3(far_coord, 0));
    }

    float4 result;
    if (near_available != 0 && far_available != 0) {
        result = lerp(far_pixel, near_pixel, t);
    } else if (near_available != 0) {
        result = near_pixel;
    } else {
        result = far_pixel;
    }
    result.a = 1.0;
    output_patch[uint2(dx, dy)] = result;
}
