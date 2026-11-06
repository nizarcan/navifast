struct Backdrop {
    top: vec4<f32>,
    bottom: vec4<f32>,
    viewport: vec4<f32>,
}
@group(0) @binding(0) var<uniform> backdrop: Backdrop;

@vertex fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let points = array(vec2<f32>(-1.0,-1.0), vec2<f32>(3.0,-1.0), vec2<f32>(-1.0,3.0));
    return vec4<f32>(points[index],0.0,1.0);
}

@fragment fn fragment(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let height = backdrop.viewport.y;
    let shade = mix(backdrop.top, backdrop.bottom, p.y / height);
    // Interleaved gradient noise (Jimenez, 2014). The reference GL framebuffer
    // has its origin at the bottom left, including at fractional display zooms.
    let dot_xy = 0.06711056 * p.x + 0.00583715 * (height - p.y);
    let noise = (fract(52.9829189 * fract(dot_xy)) - 0.5) * (0.95 / 255.0);
    let rgb = clamp(shade.rgb + noise, vec3<f32>(0.0), vec3<f32>(1.0));
    if backdrop.viewport.z > 0.5 {
        return vec4<f32>(rgb, shade.a);
    }
    let straight = clamp(rgb / max(shade.a, 1e-6), vec3<f32>(0.0), vec3<f32>(1.0));
    let linear = select(pow((straight + 0.055) / 1.055, vec3<f32>(2.4)), straight / 12.92, straight <= vec3<f32>(0.04045));
    return vec4<f32>(linear * shade.a, shade.a);
}
