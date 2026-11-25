// Glyphs of the distance-field atlas on quads in pixels of the frame, with y down from the top
// edge. The output colour is linear and premultiplied by its alpha.

// SPREAD of text.rs.
const SPREAD: f32 = 8.0;
// The width in pixels of the ramp of the coverage across the edge of a glyph and of a halo.
const RAMP: f32 = 0.7;

struct Screen {
    // The width and the height of the target in pixels, and the size of the atlas in texels.
    size: vec4<f32>,
}

@group(0) @binding(0) var<uniform> screen: Screen;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var glyphs: sampler;

struct Quad {
    @location(0) rect: vec4<f32>,
    @location(1) cell: vec4<f32>,
    @location(2) colour: vec4<f32>,
    @location(3) halo: vec4<f32>,
    // The width of the halo in pixels and the pixels of 1 texel of the atlas.
    @location(4) size: vec2<f32>,
}

struct Glyph {
    @builtin(position) position: vec4<f32>,
    @location(0) texel: vec2<f32>,
    @location(1) colour: vec4<f32>,
    @location(2) halo: vec4<f32>,
    @location(3) size: vec2<f32>,
}

// The linear value of each channel of an sRGB colour.
fn linear(srgb: vec3<f32>) -> vec3<f32> {
    let low = srgb / 12.92;
    let high = pow((srgb + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, srgb <= vec3<f32>(0.04045));
}

@vertex
fn text_vertex(@builtin(vertex_index) index: u32, quad: Quad) -> Glyph {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
    );
    let corner = corners[index];
    let pixel = mix(quad.rect.xy, quad.rect.zw, corner);
    var out: Glyph;
    out.position = vec4<f32>(
        pixel.x / screen.size.x * 2.0 - 1.0,
        1.0 - pixel.y / screen.size.y * 2.0,
        0.0,
        1.0,
    );
    out.texel = quad.cell.xy + corner * quad.cell.zw;
    out.colour = quad.colour;
    out.halo = quad.halo;
    out.size = quad.size;
    return out;
}

// The coverage of a pixel at `distance` pixels inside an edge.
fn ramp(distance: f32) -> f32 {
    return clamp(distance / RAMP + 0.5, 0.0, 1.0);
}

// The glyph covers the pixels within its edge, and the halo covers the pixels within its width
// outside the edge. The glyph mixes with the halo in sRGB values by its coverage. The halo is
// opaque under each pixel of the glyph for a width of RAMP or more.
@fragment
fn text_fragment(glyph: Glyph) -> @location(0) vec4<f32> {
    let value = textureSampleLevel(atlas, glyphs, glyph.texel / screen.size.z, 0.0).r;
    let distance = (value - 0.5) * 2.0 * SPREAD * glyph.size.y;
    let ink = ramp(distance) * glyph.colour.a;
    let ring = ramp(distance + glyph.size.x) * glyph.halo.a * (1.0 - ink);
    let alpha = ink + ring;
    if alpha <= 0.0 {
        return vec4<f32>(0.0);
    }
    let srgb = (glyph.colour.rgb * ink + glyph.halo.rgb * ring) / alpha;
    return vec4<f32>(linear(srgb) * alpha, alpha);
}
