// Glyphs of the glyph atlas on quads in pixels of the frame, with y down from the top edge.

struct Screen {
    size: vec4<f32>,
}

@group(0) @binding(0) var<uniform> screen: Screen;
@group(0) @binding(1) var atlas: texture_2d<f32>;

struct Quad {
    @location(0) rect: vec4<f32>,
    @location(1) cell: vec4<f32>,
    @location(2) colour: vec4<f32>,
}

struct Glyph {
    @builtin(position) position: vec4<f32>,
    @location(0) texel: vec2<f32>,
    @location(1) colour: vec4<f32>,
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
    out.texel = quad.cell.xy + corner * (quad.rect.zw - quad.rect.xy);
    out.colour = quad.colour;
    return out;
}

@fragment
fn text_fragment(glyph: Glyph) -> @location(0) vec4<f32> {
    let coverage = textureLoad(atlas, vec2<i32>(floor(glyph.texel)), 0).r;
    return vec4<f32>(glyph.colour.rgb, glyph.colour.a * coverage);
}
