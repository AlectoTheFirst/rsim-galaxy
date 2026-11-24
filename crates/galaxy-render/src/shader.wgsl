// The material table holds the linear colour of each material after the body slot by its index.
// A `w` of 1 is lit by the sun, 0 is not lit.
struct Frame {
    view_projection: mat4x4<f32>,
    sun: vec4<f32>,
    eye: vec4<f32>,
    materials: array<vec4<f32>, 64>,
}

@group(0) @binding(0) var<uniform> frame: Frame;

// A vertex of a mesh in the unit box: its position, its normal, its shift as a multiple of the
// edge thickness, its position in the box of its part from -1 to 1, the size of that box, the
// anchor of its symbol, and its material index, bevel and fit. The fit is 0 for none, 1, 2 and
// 3 for the face across x, y and z, 4 for the curved front of the puck, 5 for a round body and 6
// for the body of the puck.
// The size of the box of the part of a vertex with a fit on a face is the region of its symbol.
struct Corner {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) offset: vec3<f32>,
    @location(3) local: vec3<f32>,
    @location(4) extent: vec3<f32>,
    @location(5) anchor: vec3<f32>,
    @location(6) material: vec3<u32>,
}

// An instance of a mesh: the centre and the size of its box, its edge thickness in `size.w`, the
// linear colour of the body slot, its pick value and the reference size of its symbol. A
// `colour.a` of 1 is lit by the sun, 0 is not lit. A reference of 0 is no fit, a negative
// `reference.y` hides the symbol on the face across y, and a negative `reference.z` the symbol on
// the face across z.
struct Instance {
    @location(7) centre: vec4<f32>,
    @location(8) size: vec4<f32>,
    @location(9) colour: vec4<f32>,
    @location(10) pick: u32,
    @location(11) reference: vec3<f32>,
}

// The depth range of the viewport of the world, as `DEPTH_RANGE` of the renderer.
const DEPTH_RANGE: vec2<f32> = vec2<f32>(0.05, 1.0);
// Distance of the depth of an edge of an open node in front of the face where the line of sight
// leaves its box.
const SHELL_FRONT: f32 = 0.002;

// Half the depth of the puck of the router in the unit box.
const PUCK_DEPTH: f32 = 0.494;

// The place of a vertex relative to the centre of its instance, its normal, and the size of the
// unit box on each axis for the vertex.
struct Fitted {
    offset: vec3<f32>,
    normal: vec3<f32>,
    scale: vec3<f32>,
}

// The front of the puck on z in the unit box at `x`.
fn surface(x: f32) -> f32 {
    return PUCK_DEPTH * sqrt(max(1.0 - 4.0 * x * x, 1e-6));
}

// Half the length of the straight sides of the stadium of the puck in a box of `size`.
fn straight(size: vec3<f32>) -> f32 {
    return abs(size.x - size.z) * 0.5;
}

// The place of `corner` in `instance`. A vertex with no fit scales with the size of the
// instance. A vertex of a symbol with a fit scales around its anchor with the reference size
// times the largest factor that keeps the region of the symbol, `corner.extent`, on the 2 axes
// of its face inside the size of the instance, at most the factor that fits the reference size
// on 1 axis of the face, and across the face with the smaller of that and the size. A vertex on
// the curved front of the puck fits the round puck of the instance, as wide and as deep as the
// smaller of its width and depth, and keeps its distance on z to the curve at its place on x. A
// body vertex with the round fit scales on x and z around its anchor by the factor that keeps x
// and z of the reference size inside the instance, and keeps its place on y. A vertex on the
// face across y of an instance with a negative `reference.y`, and a vertex on the face across z
// of an instance with a negative `reference.z`, is on the centre. A body vertex of the puck is on
// the stadium that fills the instance on x and z: the circle of the smaller of its width and
// depth, with each half moved to its end of the longer side. A vertex on the curved front of the
// puck follows that stadium.
fn fit(corner: Corner, instance: Instance) -> Fitted {
    let size = instance.size.xyz;
    let reference = abs(instance.reference);
    let kind = corner.material.z;
    var out: Fitted;
    out.offset = corner.position * size;
    out.scale = size;
    if kind == 6u {
        let round = min(size.x, size.z);
        out.scale = vec3<f32>(round, size.y, round);
        out.offset = corner.position * out.scale;
        let reach = straight(size);
        if size.x >= size.z {
            out.offset.x = out.offset.x + select(-reach, reach, corner.position.x >= 0.0);
        } else {
            out.offset.z = out.offset.z + select(-reach, reach, corner.position.z >= 0.0);
        }
        out.normal = corner.normal / max(out.scale, vec3<f32>(1e-6));
        return out;
    }
    if kind == 0u || min(reference.x, min(reference.y, reference.z)) <= 0.0 {
        out.normal = corner.normal / max(size, vec3<f32>(1e-6));
        return out;
    }
    if (kind == 2u && instance.reference.y < 0.0)
        || ((kind == 3u || kind == 4u) && instance.reference.z < 0.0) {
        out.offset = vec3<f32>(0.0);
        out.normal = corner.normal;
        return out;
    }
    var room = size;
    if kind == 4u {
        let round = min(size.x, size.z);
        room = vec3<f32>(round, size.y, round);
    }
    var across = min(kind, 3u) - 1u;
    var region = max(corner.extent, vec3<f32>(0.05));
    if kind == 5u {
        across = 1u;
        region = vec3<f32>(1.0);
    }
    let ratio = room / reference;
    let fill = ratio / region;
    var factor: f32;
    var most: f32;
    if across == 0u {
        factor = min(fill.y, fill.z);
        most = max(ratio.y, ratio.z);
    } else if across == 1u {
        factor = min(fill.x, fill.z);
        most = max(ratio.x, ratio.z);
    } else {
        factor = min(fill.x, fill.y);
        most = max(ratio.x, ratio.y);
    }
    factor = min(factor, most);
    var scale = reference * factor;
    scale[across] = min(scale[across], room[across]);
    if kind == 5u {
        scale.y = size.y;
    }
    out.scale = scale;
    out.offset = corner.anchor * room + (corner.position - corner.anchor) * scale;
    out.normal = corner.normal / max(scale, vec3<f32>(1e-6));
    if kind == 4u {
        var x = out.offset.x / max(room.x, 1e-6);
        var shift = 0.0;
        if size.x >= size.z {
            let along = max(abs(out.offset.x) - straight(size), 0.0);
            x = sign(out.offset.x) * along / max(room.x, 1e-6);
        } else {
            shift = straight(size);
        }
        let z = surface(x) + corner.position.z - surface(corner.position.x);
        out.offset.z = z * room.z + shift;
        if abs(corner.normal.z) > 0.5 {
            let cap = vec3<f32>(PUCK_DEPTH * PUCK_DEPTH * 4.0 * x / surface(x), 0.0, 1.0)
                * sign(corner.normal.z);
            out.normal = cap / max(room, vec3<f32>(1e-6));
        }
    }
    return out;
}

struct Fragment {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) colour: vec4<f32>,
    @location(2) @interpolate(flat) pick: u32,
    @location(3) local: vec3<f32>,
    @location(4) @interpolate(flat) size: vec3<f32>,
    @location(5) world: vec3<f32>,
    @location(6) @interpolate(flat) bevel: u32,
    @location(7) @interpolate(flat) low: vec3<f32>,
    @location(8) @interpolate(flat) high: vec3<f32>,
    @location(9) @interpolate(flat) plate: f32,
}

@vertex
fn vertex_main(corner: Corner, instance: Instance) -> Fragment {
    let fitted = fit(corner, instance);
    return place(corner, instance, instance.centre.xyz + fitted.offset
        + corner.offset * instance.size.w, fitted);
}

// A vertex of a casing of a flow line: the corner of the box moved away from the eye by
// `size.w`.
@vertex
fn casing_main(corner: Corner, instance: Instance) -> Fragment {
    let fitted = fit(corner, instance);
    let world = instance.centre.xyz + fitted.offset;
    return place(corner, instance, world + normalize(world - frame.eye.xyz) * instance.size.w,
        fitted);
}

fn place(corner: Corner, instance: Instance, world: vec3<f32>, fitted: Fitted) -> Fragment {
    var out: Fragment;
    out.position = frame.view_projection * vec4<f32>(world, 1.0);
    out.normal = fitted.normal;
    if corner.material.x == 0u {
        out.colour = instance.colour;
    } else {
        out.colour = frame.materials[corner.material.x];
    }
    out.pick = instance.pick;
    out.local = corner.local;
    out.size = fitted.scale * corner.extent;
    out.world = world;
    out.bevel = corner.material.y;
    out.low = instance.centre.xyz - instance.size.xyz * 0.5;
    out.high = instance.centre.xyz + instance.size.xyz * 0.5;
    out.plate = instance.reference.y;
    return out;
}

// The lit colour: the light of the sky from above and of the ground from below, the sun on the
// face, a darker band along the edges of the face of a part with a bevel, a darker band near the
// floor, and the highlight of the sun. The band of a round part runs along the top and bottom
// rims: on a side face by the distance on y, on a cap by the distance from the rim.
@fragment
fn fragment_main(fragment: Fragment) -> @location(0) vec4<f32> {
    let base = fragment.colour.rgb;
    if fragment.colour.a < 0.5 || frame.sun.w >= 1.0 {
        return vec4<f32>(base, 1.0);
    }
    let normal = normalize(fragment.normal);
    let sun = frame.sun.xyz;
    let ambient = frame.sun.w;
    let sky = mix(ambient * 0.78, ambient * 1.06, normal.y * 0.5 + 0.5);
    let direct = max(dot(normal, sun), 0.0) * (1.0 - ambient) * 1.1;

    let size = fragment.size;
    var bevel = 1.0;
    if fragment.bevel != 0u {
        var edge: f32;
        if fragment.bevel == 2u {
            if abs(normal.y) > 0.5 {
                edge = (1.0 - length(fragment.local.xz)) * 0.5 * min(size.x, size.z);
            } else {
                edge = (1.0 - abs(fragment.local.y)) * size.y * 0.5;
            }
        } else {
            let face = abs(normal);
            let away = (vec3<f32>(1.0) - abs(fragment.local)) * size * 0.5 + face * 1e3;
            edge = min(away.x, min(away.y, away.z));
        }
        let band = min(0.06, 0.12 * min(size.x, min(size.y, size.z)));
        bevel = mix(0.86, 1.0, smoothstep(0.0, band, edge));
    }

    var contact = 1.0;
    if frame.eye.w > 0.5 {
        contact = mix(0.72, 1.0, smoothstep(0.0, 0.45, fragment.world.y));
    }

    let view = normalize(frame.eye.xyz - fragment.world);
    let half = normalize(sun + view);
    let highlight = pow(max(dot(normal, half), 0.0), 48.0) * 0.12;

    let light = (sky + direct) * bevel * contact;
    return vec4<f32>(base * light + vec3<f32>(highlight), 1.0);
}

// The glow of a line segment: the alpha falls from the axis of the segment to its surface. On a
// side face, the fall runs across the face. On an end face, it runs from the centre of the face.
@fragment
fn glow_main(fragment: Fragment) -> @location(0) vec4<f32> {
    let size = fragment.size;
    let normal = abs(normalize(fragment.normal));
    var across: vec2<f32>;
    var facing: vec2<f32>;
    if size.x >= size.y && size.x >= size.z {
        across = fragment.local.yz;
        facing = normal.yz;
    } else if size.y >= size.z {
        across = fragment.local.xz;
        facing = normal.xz;
    } else {
        across = fragment.local.xy;
        facing = normal.xy;
    }
    var distance = length(across);
    if facing.x > 0.5 {
        distance = abs(across.y);
    } else if facing.y > 0.5 {
        distance = abs(across.x);
    }
    let fall = clamp(1.0 - distance, 0.0, 1.0);
    return vec4<f32>(fragment.colour.rgb, fall * fall * 0.55);
}

// The soft shadow on the floor: the alpha falls from the opacity in `colour.a` in the middle of
// the box to 0 at its sides on x and z.
@fragment
fn shadow_main(fragment: Fragment) -> @location(0) vec4<f32> {
    let away = abs(fragment.local.xz);
    let fall = (1.0 - smoothstep(0.35, 1.0, away.x)) * (1.0 - smoothstep(0.35, 1.0, away.y));
    return vec4<f32>(fragment.colour.rgb, fall * fragment.colour.a);
}

struct Placed {
    @builtin(frag_depth) depth: f32,
    @location(0) colour: vec4<f32>,
}

// An edge of the box of an open node in its colour, at the depth of the point in front of the
// face where the line of sight from the eye through the fragment leaves the box above its floor
// plate, of the thickness `plate`, the `reference.y` of the instance: behind each node in the box
// and in front of each node behind the box.
@fragment
fn shell_main(fragment: Fragment) -> Placed {
    var low = fragment.low;
    low.y = low.y + max(fragment.plate, 0.0);
    let direction = normalize(fragment.world - frame.eye.xyz);
    let safe = select(direction, vec3<f32>(1e-6), abs(direction) < vec3<f32>(1e-6));
    let first = (low - fragment.world) / safe;
    let second = (fragment.high - fragment.world) / safe;
    let far = max(first, second);
    let leave = max(min(far.x, min(far.y, far.z)) - SHELL_FRONT, 0.0);
    let point = frame.view_projection * vec4<f32>(fragment.world + direction * leave, 1.0);
    var out: Placed;
    out.depth = clamp(
        DEPTH_RANGE.x + point.z / point.w * (DEPTH_RANGE.y - DEPTH_RANGE.x),
        DEPTH_RANGE.x,
        DEPTH_RANGE.y,
    );
    out.colour = vec4<f32>(fragment.colour.rgb, 1.0);
    return out;
}

@fragment
fn pick_main(fragment: Fragment) -> @location(0) u32 {
    return fragment.pick;
}
