use galaxy_render::label::{Candidate, Cause, Grid, Hidden, Labels, Shown, order, place};
use galaxy_render::mesh::{
    Fit, Material, Mesh, Regions, Registry, Vertex, build, count, fitted, forms, reference, region,
};
use galaxy_render::text::{Atlas, Face, Fonts, RESIDUE, SPREAD, distance_field, raster};
use galaxy_scene::{Axis, Colours, DefinedShape, Library, ObjectId, Part, Shape, Solid, Style};

/// Edge thickness of the test positions of the bars.
const THICKNESS: f32 = 0.02;

fn at(vertex: &Vertex) -> [f32; 3] {
    [0, 1, 2].map(|axis| vertex.position[axis] + vertex.offset[axis] * THICKNESS)
}

fn minus(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|axis| a[axis] - b[axis])
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|axis| a[axis] * b[axis]).sum()
}

fn materials(registry: &Registry, mesh: Mesh) -> Vec<u32> {
    let mut slots: Vec<u32> = registry
        .vertices(mesh)
        .iter()
        .map(|vertex| vertex.material[0])
        .collect();
    slots.sort();
    slots.dedup();
    slots
}

#[test]
fn cube_and_cylinder_meshes_have_their_corner_counts() {
    let registry = Registry::new();
    assert_eq!(registry.vertices(Mesh::Cube).len(), 36);
    assert_eq!(registry.vertices(Mesh::CubeEdges).len(), 12 * 36);
    assert_eq!(registry.vertices(Mesh::Cylinder).len(), 2 * 46 * 3 + 48 * 6);
    assert_eq!(registry.vertices(Mesh::CylinderEdges).len(), 2 * 48 * 36);
    assert_eq!(registry.vertices(Mesh::Puck).len(), 2 * 46 * 3 + 48 * 6);
    assert_eq!(registry.vertices(Mesh::SlabEdges).len(), 12 * 36);
}

#[test]
fn each_mesh_has_whole_triangles_in_the_unit_box() {
    let registry = Registry::new();
    for mesh in Mesh::ALL {
        let vertices = registry.vertices(mesh);
        assert!(!vertices.is_empty(), "{mesh:?}");
        assert_eq!(vertices.len() % 3, 0, "{mesh:?}");
        for vertex in vertices {
            assert!(
                vertex
                    .position
                    .iter()
                    .all(|value| value.abs() <= 0.5 + 1e-6),
                "{mesh:?} {vertex:?}"
            );
        }
    }
    let ranges: Vec<_> = Mesh::ALL.map(|mesh| registry.range(mesh)).to_vec();
    for pair in ranges.windows(2) {
        assert_eq!(pair[0].end, pair[1].start);
    }
    assert_eq!(ranges.last().unwrap().end as usize, registry.vertices.len());
}

#[test]
fn each_triangle_turns_counter_clockwise_from_outside() {
    let registry = Registry::new();
    for mesh in Mesh::ALL {
        for triangle in registry.vertices(mesh).chunks(3) {
            let [a, b, c] = [&triangle[0], &triangle[1], &triangle[2]].map(at);
            let turn = cross(minus(b, a), minus(c, a));
            let normal = [0, 1, 2].map(|axis| triangle.iter().map(|v| v.normal[axis]).sum());
            assert!(dot(turn, normal) > 0.0, "{mesh:?} {triangle:?}");
        }
    }
}

#[test]
fn each_solid_mesh_encloses_a_positive_volume() {
    let registry = Registry::new();
    for mesh in Mesh::ALL.into_iter().filter(|mesh| !mesh.is_edges()) {
        let volume: f32 = registry
            .vertices(mesh)
            .chunks(3)
            .map(|triangle| {
                dot(
                    triangle[0].position,
                    cross(triangle[1].position, triangle[2].position),
                ) / 6.0
            })
            .sum();
        let most = if mesh == Mesh::BrickBlock { 2.0 } else { 1.0 };
        assert!(volume > 0.0 && volume <= most + 1e-4, "{mesh:?} {volume}");
    }
    let cube: f32 = registry
        .vertices(Mesh::Cube)
        .chunks(3)
        .map(|triangle| {
            dot(
                triangle[0].position,
                cross(triangle[1].position, triangle[2].position),
            ) / 6.0
        })
        .sum();
    assert!((cube - 1.0).abs() < 1e-5, "{cube}");
}

#[test]
fn symbol_parts_are_only_in_the_meshes_with_a_symbol() {
    let registry = Registry::new();
    let body = Material::Body as u32;
    let symbol = Material::Symbol as u32;
    let brick = Material::Brick as u32;
    let flame = Material::Flame as u32;
    let mortar = Material::Mortar as u32;
    let core = Material::FlameCore as u32;
    assert_eq!(materials(&registry, Mesh::Router), [body, symbol]);
    assert_eq!(materials(&registry, Mesh::Switch), [body, symbol]);
    assert_eq!(
        materials(&registry, Mesh::Firewall),
        [brick, flame, mortar, core]
    );
    assert_eq!(materials(&registry, Mesh::Wall), [brick, flame, core]);
    assert_eq!(materials(&registry, Mesh::AccessPoint), [body, symbol]);
    assert_eq!(materials(&registry, Mesh::Workstation), [body, symbol]);
    let symbols = |mesh| {
        registry
            .vertices(mesh)
            .iter()
            .filter(|vertex| vertex.material[0] == symbol)
            .count()
    };
    assert!(symbols(Mesh::AccessPoint) > symbols(Mesh::Radio));
    for mesh in [
        Mesh::Cube,
        Mesh::Cylinder,
        Mesh::Puck,
        Mesh::Slab,
        Mesh::Monitor,
        Mesh::Cloud,
        Mesh::HeadPosX,
        Mesh::HeadNegY,
    ] {
        assert_eq!(materials(&registry, mesh), [body], "{mesh:?}");
    }
}

#[test]
fn arrowhead_has_its_tip_on_its_middle_line_at_the_end_toward_its_direction() {
    let registry = Registry::new();
    for axis in 0..3 {
        for forward in [true, false] {
            let mesh = Mesh::head(axis, forward);
            let sign = if forward { 1.0 } else { -1.0 };
            let tips: Vec<_> = registry
                .vertices(mesh)
                .iter()
                .filter(|vertex| (vertex.position[axis] - sign * 0.5).abs() < 1e-6)
                .collect();
            assert!(!tips.is_empty(), "{mesh:?}");
            for vertex in tips {
                for other in (0..3).filter(|&other| other != axis && other != 1) {
                    assert!(
                        vertex.position[other].abs() <= 0.2 + 1e-6,
                        "{mesh:?} {vertex:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn each_shape_has_a_full_mesh_a_plain_mesh_and_edges() {
    for shape in Shape::ALL {
        let forms = forms(shape, &Library::default());
        assert!(
            !forms.full.is_edges() && !forms.plain.is_edges(),
            "{shape:?}"
        );
        assert!(forms.edges.is_edges(), "{shape:?}");
        let (axis, part) = forms.symbol;
        assert!(axis < 3 && part > 0.0 && part <= 1.0, "{shape:?}");
    }
    assert_eq!(forms_of(Shape::Router).full, Mesh::Router);
    assert_eq!(forms_of(Shape::Router).plain, Mesh::Puck);
    assert_eq!(forms_of(Shape::Switch).full, Mesh::Switch);
    assert_eq!(forms_of(Shape::Firewall).full, Mesh::Firewall);
    assert_eq!(forms_of(Shape::Firewall).edges, Mesh::WallEdges);
    assert_eq!(forms_of(Shape::Device).full, Mesh::Cube);
    assert_eq!(forms_of(Shape::AccessPoint).full, Mesh::AccessPoint);
    assert_eq!(forms_of(Shape::AccessPoint).plain, Mesh::Radio);
    assert_eq!(forms_of(Shape::Workstation).full, Mesh::Workstation);
    assert_eq!(forms_of(Shape::Cloud).edges, Mesh::CloudEdges);
}

#[test]
fn caps_of_the_flame_cover_the_area_of_its_outline() {
    let registry = Registry::new();
    let flame = Material::Flame as u32;
    let area: f32 = registry
        .vertices(Mesh::Firewall)
        .chunks(3)
        .filter(|triangle| triangle[0].material[0] == flame && triangle[0].normal[2] > 0.5)
        .map(|triangle| {
            let [a, b, c] = [0, 1, 2].map(|corner| triangle[corner].position);
            cross(minus(b, a), minus(c, a))[2] / 2.0
        })
        .sum();
    let outline: f32 = registry
        .vertices(Mesh::Firewall)
        .chunks(3)
        .filter(|triangle| triangle[0].material[0] == flame && triangle[0].normal[2] < -0.5)
        .map(|triangle| {
            let [a, b, c] = [0, 1, 2].map(|corner| triangle[corner].position);
            -cross(minus(b, a), minus(c, a))[2] / 2.0
        })
        .sum();
    assert!(area > 0.05 && area < 0.4, "{area}");
    assert!((area - outline).abs() < 1e-5, "{area} {outline}");
}

fn forms_of(shape: Shape) -> galaxy_render::mesh::Forms {
    forms(shape, &Library::default())
}

#[test]
fn layer3_switch_server_laptop_and_load_balancer_have_their_own_meshes() {
    assert_eq!(
        forms_of(Shape::LayerThreeSwitch).full,
        Mesh::LayerThreeSwitch
    );
    assert_eq!(forms_of(Shape::LayerThreeSwitch).plain, Mesh::Slab);
    assert_eq!(forms_of(Shape::Server).full, Mesh::Server);
    assert_eq!(forms_of(Shape::Server).edges, Mesh::TowerEdges);
    assert_eq!(forms_of(Shape::Laptop).full, Mesh::Laptop);
    assert_eq!(forms_of(Shape::Laptop).plain, Mesh::Notebook);
    assert_eq!(forms_of(Shape::LoadBalancer).full, Mesh::LoadBalancer);
    assert_eq!(forms_of(Shape::LoadBalancer).edges, Mesh::ChamferedEdges);
    let registry = Registry::new();
    let body = Material::Body as u32;
    let symbol = Material::Symbol as u32;
    for mesh in [
        Mesh::LayerThreeSwitch,
        Mesh::Server,
        Mesh::Laptop,
        Mesh::LoadBalancer,
    ] {
        assert_eq!(materials(&registry, mesh), [body, symbol], "{mesh:?}");
    }
    for mesh in [Mesh::Tower, Mesh::Notebook, Mesh::Chamfered] {
        assert_eq!(materials(&registry, mesh), [body], "{mesh:?}");
    }
}

/// The symbol vertices of `mesh` on the front of the unit box, facing +z.
fn front_symbol(registry: &Registry, mesh: Mesh) -> usize {
    registry
        .vertices(mesh)
        .iter()
        .filter(|vertex| {
            vertex.material[0] == Material::Symbol as u32
                && vertex.normal[2] > 0.5
                && vertex.position[2] > 0.49
        })
        .count()
}

#[test]
fn shapes_with_a_symbol_on_their_top_repeat_it_on_their_front() {
    let registry = Registry::new();
    for shape in [
        Shape::Router,
        Shape::Switch,
        Shape::LayerThreeSwitch,
        Shape::AccessPoint,
        Shape::LoadBalancer,
    ] {
        let forms = forms_of(shape);
        assert!(front_symbol(&registry, forms.full) > 0, "{shape:?}");
        assert_eq!(front_symbol(&registry, forms.plain), 0, "{shape:?}");
    }
}

fn part(solid: Solid, at: [f64; 3], size: [f64; 3], axis: Axis, material: &str) -> Part {
    Part {
        solid,
        at,
        size,
        axis,
        material: material.to_owned(),
        edges: true,
    }
}

/// A library with the material `accent` and the shape `lamp`: a box base, a cylinder along x, a
/// prism along y with its points clockwise, and a symbol box on its front.
fn lamp() -> Library {
    let mut library = Library::default();
    library.materials.push(galaxy_scene::Material {
        name: "accent".to_owned(),
        colours: Some(Colours {
            light: [0xE0, 0x7A, 0x1F],
            dark: [0xF0, 0xA0, 0x50],
            lit: false,
        }),
    });
    let mut glass = part(
        Solid::Box,
        [0.0, 0.2, 0.49],
        [0.3, 0.2, 0.02],
        Axis::Y,
        "symbol",
    );
    glass.edges = false;
    library.shapes.push(DefinedShape {
        name: "lamp".to_owned(),
        parts: vec![
            part(
                Solid::Box,
                [0.0, -0.4, 0.0],
                [1.0, 0.2, 1.0],
                Axis::Y,
                "body",
            ),
            part(
                Solid::Cylinder,
                [0.0, 0.0, 0.0],
                [0.8, 0.4, 0.4],
                Axis::X,
                "accent",
            ),
            part(
                Solid::Prism(vec![[-0.5, -0.5], [0.0, 0.5], [0.5, -0.5]]),
                [0.0, 0.35, 0.0],
                [0.6, 0.3, 0.6],
                Axis::Y,
                "accent",
            ),
            glass,
        ],
        symbol: vec![3],
    });
    library
}

#[test]
fn defined_shape_has_its_parts_in_the_unit_box_with_the_index_of_their_material() {
    let library = lamp();
    let registry = Registry::with_library(&library);
    assert_eq!(registry.len(), count(&library));
    assert!(registry.matches(&library));
    assert!(!registry.matches(&Library::default()));
    let forms = forms(Shape::Defined(0), &library);
    assert_eq!(
        [forms.full, forms.plain, forms.edges],
        [Mesh::Defined(0), Mesh::Defined(1), Mesh::Defined(2)]
    );
    assert_eq!(forms.symbol, (1, 0.2));
    assert!(forms.edges.is_edges() && !forms.full.is_edges());
    for mesh in [forms.full, forms.plain, forms.edges] {
        let vertices = registry.vertices(mesh);
        assert!(!vertices.is_empty() && vertices.len() % 3 == 0, "{mesh:?}");
        for vertex in vertices {
            assert!(
                vertex
                    .position
                    .iter()
                    .all(|value| value.abs() <= 0.5 + 1e-6),
                "{mesh:?} {vertex:?}"
            );
        }
        for triangle in vertices.chunks(3) {
            let [a, b, c] = [&triangle[0], &triangle[1], &triangle[2]].map(at);
            let turn = cross(minus(b, a), minus(c, a));
            let normal = [0, 1, 2].map(|axis| triangle.iter().map(|v| v.normal[axis]).sum());
            assert!(dot(turn, normal) > 0.0, "{mesh:?} {triangle:?}");
        }
    }
    let accent = library.material("accent").unwrap() as u32;
    assert_eq!(
        materials(&registry, forms.full),
        [Material::Body as u32, Material::Symbol as u32, accent]
    );
    assert_eq!(
        materials(&registry, forms.plain),
        [Material::Body as u32, accent]
    );
    let volume: f32 = registry
        .vertices(forms.plain)
        .chunks(3)
        .map(|triangle| {
            dot(
                triangle[0].position,
                cross(triangle[1].position, triangle[2].position),
            ) / 6.0
        })
        .sum();
    let parts = 0.2 + std::f32::consts::PI * 0.2 * 0.2 * 0.8 + 0.6 * 0.6 / 2.0 * 0.3;
    assert!((volume - parts).abs() < 0.01, "{volume} {parts}");
}

#[test]
fn defined_shape_index_with_no_shape_gives_the_box() {
    assert_eq!(forms(Shape::Defined(3), &lamp()).full, Mesh::Cube);
    assert_eq!(Registry::new().range(Mesh::Defined(0)), 0..0);
}

#[test]
fn registry_matches_its_library_until_a_shape_or_a_material_name_changes() {
    let library = lamp();
    let registry = Registry::with_library(&library);
    assert!(registry.matches(&library));
    let mut recoloured = library.clone();
    let accent = recoloured.material("accent").unwrap();
    recoloured.materials[accent].colours = Some(Colours {
        light: [0x12, 0x34, 0x56],
        dark: [0x65, 0x43, 0x21],
        lit: true,
    });
    assert!(registry.matches(&recoloured));
    let mut redefined = library.clone();
    redefined.shapes[0].parts[0].size[1] = 0.1;
    assert!(!registry.matches(&redefined));
    let mut added = library.clone();
    added.shapes.push(DefinedShape {
        name: "slab".to_owned(),
        ..library.shapes[0].clone()
    });
    assert!(!registry.matches(&added));
    let mut named = library;
    named.materials.push(galaxy_scene::Material {
        name: "glass".to_owned(),
        colours: None,
    });
    assert!(!registry.matches(&named));
}

#[test]
fn symbol_parts_that_keep_their_aspect_have_the_fit_of_their_face() {
    let registry = Registry::new();
    let symbol = Material::Symbol as u32;
    for (mesh, fits) in [
        (Mesh::Router, [Fit::Y, Fit::Puck]),
        (Mesh::Switch, [Fit::Y, Fit::Z]),
        (Mesh::LayerThreeSwitch, [Fit::Y, Fit::Z]),
        (Mesh::AccessPoint, [Fit::Y, Fit::Z]),
        (Mesh::LoadBalancer, [Fit::Y, Fit::Z]),
    ] {
        let body = match mesh {
            Mesh::Router => Fit::Stadium,
            Mesh::AccessPoint => Fit::Round,
            _ => Fit::None,
        };
        for vertex in registry.vertices(mesh) {
            let fit = vertex.material[2];
            if vertex.material[0] != symbol {
                assert_eq!(fit, body as u32, "{mesh:?} {vertex:?}");
            } else if vertex.position[1] > 0.355 {
                assert_eq!(fit, fits[0] as u32, "{mesh:?} {vertex:?}");
            } else {
                assert_eq!(fit, fits[1] as u32, "{mesh:?} {vertex:?}");
            }
        }
    }
    let flame = [Material::Flame as u32, Material::FlameCore as u32];
    for mesh in [Mesh::Firewall, Mesh::Wall] {
        for vertex in registry.vertices(mesh) {
            let fit = if flame.contains(&vertex.material[0]) {
                Fit::Z
            } else {
                Fit::None
            };
            assert_eq!(vertex.material[2], fit as u32, "{mesh:?} {vertex:?}");
        }
    }
    for mesh in [Mesh::Workstation, Mesh::Server, Mesh::Laptop, Mesh::Slab] {
        assert!(
            registry
                .vertices(mesh)
                .iter()
                .all(|vertex| vertex.material[2] == Fit::None as u32),
            "{mesh:?}"
        );
    }
    assert_eq!(forms_of(Shape::Switch).fit, Fit::Y);
    assert_eq!(forms_of(Shape::Firewall).fit, Fit::None);
}

#[test]
fn reference_size_is_the_default_size_of_a_built_in_shape_and_the_rule_size_of_a_defined_shape() {
    let style = Style::default();
    let mut fitted_kinds = 0;
    for rule in style.rules.kinds.values() {
        if let Some(size) = reference(rule.shape, [2.0, 0.5, 1.0]) {
            assert_eq!(size, rule.size, "{rule:?}");
            fitted_kinds += 1;
        }
    }
    assert_eq!(fitted_kinds, 6);
    assert_eq!(reference(Shape::Box, [1.6, 0.4, 1.6]), None);
    assert_eq!(reference(Shape::Workstation, [1.0, 1.0, 1.0]), None);
    assert_eq!(
        reference(Shape::Defined(0), [2.0, 0.5, 1.0]),
        Some([2.0, 0.5, 1.0])
    );
}

#[test]
fn fitted_scale_keeps_the_aspect_of_the_reference_on_the_face() {
    let reference = [1.6, 0.35, 1.6];
    let whole = [1.0; 3];
    let long = fitted([4.0, 0.4, 0.9], reference, 1, whole);
    assert!(
        (long[0] - 0.9).abs() < 1e-9 && (long[2] - 0.9).abs() < 1e-9,
        "{long:?}"
    );
    assert!((long[1] - 0.35 * 0.9 / 1.6).abs() < 1e-9, "{long:?}");
    let front = fitted([0.7, 1.4, 0.7], reference, 2, whole);
    let factor = 0.7 / 1.6;
    assert!((front[0] - 0.7).abs() < 1e-9, "{front:?}");
    assert!((front[1] - 0.35 * factor).abs() < 1e-9, "{front:?}");
    assert!((front[2] - 0.7).abs() < 1e-9, "{front:?}");
    assert_eq!(fitted(reference, reference, 1, whole), reference);
}

#[test]
fn fitted_scale_fills_the_face_with_the_region_of_the_symbol() {
    let reference = [1.6, 0.35, 1.6];
    let half = [0.5, 1.0, 0.5];
    let long = fitted([4.0, 0.4, 0.9], reference, 1, half);
    let factor = 0.9 / (1.6 * 0.5);
    assert!((long[0] - 1.6 * factor).abs() < 1e-9, "{long:?}");
    assert!((long[2] - 1.6 * factor).abs() < 1e-9, "{long:?}");
    assert!((long[1] - 0.35 * factor).abs() < 1e-9, "{long:?}");
    assert_eq!(fitted(reference, reference, 1, half), reference);
    let small = fitted([0.8, 0.175, 0.8], reference, 1, half);
    assert!((small[0] - 0.8).abs() < 1e-9, "{small:?}");
}

#[test]
fn symbol_regions_lie_in_the_unit_box_and_the_body_of_the_router_is_a_stadium() {
    for mesh in [
        Mesh::Router,
        Mesh::Switch,
        Mesh::LayerThreeSwitch,
        Mesh::AccessPoint,
        Mesh::LoadBalancer,
    ] {
        for fit in [Fit::Y, Fit::Z] {
            let region = region(mesh, fit);
            assert!(
                region.iter().all(|side| *side > 0.0 && *side <= 1.0),
                "{mesh:?} {region:?}"
            );
        }
    }
    for mesh in [Mesh::Puck, Mesh::PuckEdges] {
        assert!(
            build(mesh)
                .iter()
                .all(|vertex| vertex.material[2] == Fit::Stadium as u32
                    && vertex.anchor[0] == 0.0
                    && vertex.anchor[2] == 0.0)
        );
    }
    assert!(
        build(Mesh::Radio)
            .iter()
            .all(|vertex| vertex.material[2] == Fit::Round as u32)
    );
    let corners: Vec<_> = build(Mesh::Radio)
        .iter()
        .map(|vertex| [vertex.anchor[0].abs(), vertex.anchor[2].abs()])
        .collect();
    assert!(corners.iter().all(|[x, z]| *x == 0.5 && *z > 0.49));
}

#[test]
fn symbol_part_of_a_defined_shape_fits_the_face_across_its_smallest_side() {
    let library = lamp();
    let registry = Registry::with_library(&library);
    let forms = forms(Shape::Defined(0), &library);
    assert_eq!(forms.fit, Fit::Z);
    let symbol = Material::Symbol as u32;
    for vertex in registry.vertices(forms.full) {
        if vertex.material[0] == symbol {
            assert_eq!(vertex.material[2], Fit::Z as u32, "{vertex:?}");
            for (axis, value) in [0.0, 0.2, 0.48].into_iter().enumerate() {
                assert!((vertex.anchor[axis] - value).abs() < 1e-6, "{vertex:?}");
            }
        } else {
            assert_eq!(vertex.material[2], Fit::None as u32, "{vertex:?}");
        }
    }
}

#[test]
fn symbol_parts_of_a_defined_shape_on_1_face_have_1_region() {
    let mut library = lamp();
    let mut mark = part(
        Solid::Box,
        [0.3, -0.1, 0.49],
        [0.1, 0.1, 0.02],
        Axis::Y,
        "symbol",
    );
    mark.edges = false;
    library.shapes[0].parts.push(mark);
    library.shapes[0].symbol = vec![3, 4];
    let registry = Registry::with_library(&library);
    let regions = Regions::new(&library);
    let front = regions.get(Mesh::Defined(0), Fit::Z);
    let expected = [0.7, 0.65, 0.24];
    for axis in 0..3 {
        assert!((front[axis] - expected[axis]).abs() < 1e-6, "{front:?}");
    }
    let symbol = Material::Symbol as u32;
    let extents: Vec<[f32; 3]> = registry
        .vertices(Mesh::Defined(0))
        .iter()
        .filter(|vertex| vertex.material[0] == symbol)
        .map(|vertex| vertex.extent)
        .collect();
    assert!(!extents.is_empty());
    assert!(
        extents.iter().all(|extent| extent.map(f64::from) == front),
        "{extents:?}"
    );
    assert_eq!(regions.get(Mesh::Defined(0), Fit::Y), [1.0; 3]);
    assert_eq!(regions.get(Mesh::Defined(1), Fit::Z), [1.0; 3]);
    assert_eq!(regions.get(Mesh::Defined(3), Fit::Z), [1.0; 3]);
    assert_eq!(
        regions.get(Mesh::Switch, Fit::Z),
        region(Mesh::Switch, Fit::Z)
    );
}

#[test]
fn defined_shape_with_sides_below_the_resolution_of_f32_has_finite_vertices() {
    let prism = || Solid::Prism(vec![[-0.5, -0.5], [0.0, 0.5], [0.5, -0.5]]);
    let mut library = Library::default();
    library.shapes.push(DefinedShape {
        name: "thin".to_owned(),
        parts: vec![
            part(
                Solid::Box,
                [0.25, 0.0, 0.0],
                [1e-12, 0.5, 0.5],
                Axis::Y,
                "body",
            ),
            part(
                Solid::Box,
                [-0.25, 0.0, 0.0],
                [0.4, 1e-50, 1e-50],
                Axis::Y,
                "body",
            ),
            part(
                Solid::Cylinder,
                [0.0, 0.25, 0.0],
                [1e-12, 0.3, 0.3],
                Axis::Y,
                "body",
            ),
            part(
                Solid::Cylinder,
                [0.0, -0.25, 0.0],
                [1e-12, 0.3, 1e-12],
                Axis::Y,
                "body",
            ),
            part(
                prism(),
                [0.0, 0.0, 0.25],
                [0.4, 1e-12, 0.4],
                Axis::Y,
                "body",
            ),
            part(
                prism(),
                [0.0, 0.0, -0.25],
                [0.4, 1e-12, 0.4],
                Axis::Y,
                "symbol",
            ),
        ],
        symbol: vec![5],
    });
    let registry = Registry::with_library(&library);
    for mesh in [Mesh::Defined(0), Mesh::Defined(1), Mesh::Defined(2)] {
        let vertices = registry.vertices(mesh);
        assert!(!vertices.is_empty(), "{mesh:?}");
        for vertex in vertices {
            let values = [
                vertex.position,
                vertex.normal,
                vertex.offset,
                vertex.local,
                vertex.extent,
                vertex.anchor,
            ];
            assert!(
                values.iter().flatten().all(|value| value.is_finite()),
                "{mesh:?} {vertex:?}"
            );
            assert!(
                dot(vertex.normal, vertex.normal) > 0.9,
                "{mesh:?} {vertex:?}"
            );
        }
    }
    let top = Regions::new(&library).get(Mesh::Defined(0), Fit::Y);
    assert!(top.iter().all(|side| side.is_finite()), "{top:?}");
}

/// The lowest and highest position of the vertices of `mesh` with the fit `fit`.
fn bounds(mesh: Mesh, fit: Fit) -> [[f32; 3]; 2] {
    let mut low = [f32::MAX; 3];
    let mut high = [f32::MIN; 3];
    for vertex in build(mesh) {
        if vertex.material[2] == fit as u32 {
            for axis in 0..3 {
                low[axis] = low[axis].min(vertex.position[axis]);
                high[axis] = high[axis].max(vertex.position[axis]);
            }
        }
    }
    [low, high]
}

#[test]
fn front_symbol_of_the_load_balancer_has_the_aspect_of_its_top_symbol_and_room_in_its_face() {
    let size = [1.6_f32, 0.4, 1.2];
    let [low, high] = bounds(Mesh::LoadBalancer, Fit::Y);
    let top = (high[0] - low[0]) * size[0] / ((high[2] - low[2]) * size[2]);
    let [low, high] = bounds(Mesh::LoadBalancer, Fit::Z);
    let front = (high[0] - low[0]) * size[0] / ((high[1] - low[1]) * size[1]);
    assert!((front / top - 1.0).abs() < 0.1, "{top} {front}");
    assert!(
        low[1] > -0.5 + 0.05 && high[1] < 0.38 - 0.05,
        "{low:?} {high:?}"
    );
}

#[test]
fn front_symbol_of_the_switch_is_2_arrows_with_room_above_and_below() {
    let [low, high] = bounds(Mesh::Switch, Fit::Z);
    let room = 0.15 * (0.38 + 0.5);
    assert!(
        low[1] >= -0.5 + room && high[1] <= 0.38 - room,
        "{low:?} {high:?}"
    );
}

#[test]
fn brick_block_fills_the_unit_box_with_bricks_on_mortar() {
    let registry = Registry::new();
    assert_eq!(
        materials(&registry, Mesh::BrickBlock),
        [Material::Brick as u32, Material::Mortar as u32]
    );
    let [low, high] = bounds(Mesh::BrickBlock, Fit::None);
    assert_eq!((low, high), ([-0.5; 3], [0.5; 3]));
}

#[test]
fn distance_of_a_pixel_on_the_outline_of_the_master_raster_is_one_half_within_one_sixteenth() {
    let fonts = Fonts::new();
    for face in [Face::Label, Face::Title] {
        for character in ['a', 'g', 'W', '0', '@'] {
            let raster = raster(&fonts, face, character).unwrap();
            assert_eq!(raster.coverage.len(), raster.width * raster.height);
            let mut outline = 0;
            for (coverage, field) in raster.coverage.iter().zip(&raster.field) {
                if *coverage > RESIDUE && *coverage < 1.0 - RESIDUE {
                    outline += 1;
                    assert!((field - 0.5).abs() <= 1.0 / 16.0, "{character} {field}");
                } else if *coverage >= 0.5 {
                    assert!(*field > 0.5, "{character} {field}");
                } else {
                    assert!(*field < 0.5, "{character} {field}");
                }
            }
            assert!(outline > 0, "{character}");
            let border = SPREAD * 4;
            for column in [0, raster.width - 1] {
                for row in 0..raster.height {
                    assert_eq!(raster.coverage[row * raster.width + column], 0.0);
                }
            }
            assert!(raster.width > 2 * border && raster.height > 2 * border);
        }
    }
}

#[test]
fn coverage_residue_inside_and_outside_a_ring_gives_the_distance_to_the_edge() {
    let size = 64;
    let coverage: Vec<f32> = (0..size * size)
        .map(|pixel| {
            let [x, y] = [pixel % size, pixel / size];
            let outer = (8..56).contains(&x) && (8..56).contains(&y);
            let hole = (20..44).contains(&x) && (20..44).contains(&y);
            match (outer, hole) {
                (true, false) => 1.0 - 1e-6,
                (true, true) => 1e-6,
                _ => 0.0,
            }
        })
        .collect();
    let field = distance_field(&coverage, size, size);
    let range = (2 * SPREAD * 4) as f32;
    let centre = field[31 * size + 31];
    assert!((centre - (0.5 - 11.5 / range)).abs() < 1e-6, "{centre}");
    let ring = field[31 * size + 14];
    assert!((ring - (0.5 + 5.5 / range)).abs() < 1e-6, "{ring}");
}

#[test]
fn field_at_the_centre_of_the_counter_of_o_is_2_texels_or_more_outside_the_edge() {
    let fonts = Fonts::new();
    for face in [Face::Label, Face::Value, Face::Title, Face::Name] {
        let raster = raster(&fonts, face, 'o').unwrap();
        let row = (0..raster.height)
            .filter(|&row| raster.coverage[row * raster.width + raster.width / 2] >= 0.5)
            .collect::<Vec<_>>();
        let middle = (row[0] + row[row.len() - 1]) / 2;
        let field = raster.field[middle * raster.width + raster.width / 2];
        assert!(field < 0.5 - 2.0 / (2 * SPREAD) as f32, "{face:?} {field}");
    }
}

#[test]
fn full_atlas_clears_and_places_a_glyph_as_an_empty_atlas() {
    let fonts = Fonts::new();
    let mut fresh = Atlas::with_size(256);
    let cell = fresh.glyph(&fonts, Face::Label, 'g').unwrap().unwrap();
    let texels = fresh.cell_texels(&cell);
    assert!(texels.iter().any(|&texel| texel > 128));

    let mut atlas = Atlas::with_size(256);
    let mut full = false;
    'fill: for face in [Face::Title, Face::Name, Face::Value, Face::Label] {
        for character in ('A'..='Z').chain('a'..='z').chain('0'..='9') {
            if atlas.glyph(&fonts, face, character).is_err() {
                full = true;
                break 'fill;
            }
        }
    }
    assert!(full);
    assert!(atlas.glyph(&fonts, Face::Label, '#').is_err());
    atlas.clear();
    let again = atlas.glyph(&fonts, Face::Label, 'g').unwrap().unwrap();
    assert_eq!(again, cell);
    assert_eq!(atlas.cell_texels(&again), texels);
    assert_eq!(atlas.texels, fresh.texels);

    let mut large = Atlas::new();
    large.glyph(&fonts, Face::Name, 'Q').unwrap();
    let moved = large.glyph(&fonts, Face::Label, 'g').unwrap().unwrap();
    assert_ne!(moved.corner, cell.corner);
    assert_eq!(large.cell_texels(&moved), texels);
}

#[test]
fn glyph_with_no_outline_has_no_cell() {
    let fonts = Fonts::new();
    let mut atlas = Atlas::with_size(256);
    assert_eq!(atlas.glyph(&fonts, Face::Label, ' '), Ok(None));
}

#[test]
fn width_of_a_label_is_the_sum_of_the_advances_of_its_characters() {
    let fonts = Fonts::new();
    let one = fonts.width(Face::Label, "a", 11.0);
    assert!(one > 5.0 && one < 8.0, "{one}");
    assert!((fonts.width(Face::Label, "abcd", 11.0) - 4.0 * one).abs() < 1e-4);
    assert!((fonts.width(Face::Label, "abcd", 22.0) - 8.0 * one).abs() < 1e-4);
    let accent = fonts.width(Face::Label, "\u{e9}", 11.0);
    assert!((fonts.width(Face::Label, "ab\u{e9}", 11.0) - (2.0 * one + accent)).abs() < 1e-4);
}

fn id(name: &str) -> ObjectId {
    ObjectId::new(name).unwrap()
}

#[test]
fn labels_take_their_places_in_priority_order_with_the_first_overlapped_label_as_the_cause() {
    let rects = [
        [0.0, 0.0, 100.0, 20.0],
        [50.0, 10.0, 150.0, 30.0],
        [100.0, 0.0, 200.0, 20.0],
        [10.0, 5.0, 20.0, 15.0],
        [60.0, 100.0, 300.0, 200.0],
        [290.0, 190.0, 310.0, 210.0],
        [0.0, 0.0, 400.0, 300.0],
    ];
    assert_eq!(
        place(&rects, 400.0, 300.0, 64.0),
        vec![None, Some(0), None, Some(0), None, Some(4), Some(0)]
    );
    assert_eq!(
        place(&rects, 400.0, 300.0, 1000.0),
        place(&rects, 400.0, 300.0, 64.0)
    );
    assert_eq!(Grid::new(2560.0, 1440.0, 64.0).dimensions(), [40, 23]);
    assert_eq!(Grid::new(2560.0, 1440.0, 128.0).dimensions(), [20, 12]);
}

#[test]
fn order_puts_the_selected_node_then_the_path_then_the_larger_then_the_id() {
    let candidates: Vec<Candidate> = [
        ("b", 50.0),
        ("a", 50.0),
        ("big", 200.0),
        ("hop-2", 10.0),
        ("hop-1", 60.0),
        ("chosen", 5.0),
    ]
    .into_iter()
    .map(|(name, size)| Candidate {
        node: id(name),
        size,
    })
    .collect();
    let path = [id("hop-1"), id("hop-2"), id("hop-1")];
    assert_eq!(
        order(&candidates, Some(&id("chosen")), &path),
        vec![5, 4, 3, 2, 1, 0]
    );
    assert_eq!(order(&candidates, None, &[]), vec![2, 4, 1, 0, 3, 5]);
}

#[test]
fn scene_text_has_the_count_line_and_256_hidden_lines_at_most() {
    let hidden = |count: usize| -> Vec<Hidden> {
        (0..count)
            .map(|number| Hidden {
                node: id(&format!("n{number}")),
                cause: match number % 3 {
                    0 => Cause::Overlap(id("top")),
                    1 => Cause::Atlas,
                    _ => Cause::Limit,
                },
            })
            .collect()
    };
    let labels = Labels {
        shown: vec![Shown {
            node: id("top"),
            rect: [0.0, 0.0, 10.0, 10.0],
        }],
        hidden: hidden(300),
    };
    let lines = labels.lines();
    assert_eq!(lines.len(), 1 + 256 + 1);
    assert_eq!(lines[0], "labels shown=1 hidden=300");
    assert_eq!(lines[1], "label n0 hidden overlap=top");
    assert_eq!(lines[2], "label n1 hidden atlas");
    assert_eq!(lines[3], "label n2 hidden limit");
    assert_eq!(lines[256], "label n255 hidden overlap=top");
    assert_eq!(lines[257], "labels unlisted=44");
    let few = Labels {
        shown: Vec::new(),
        hidden: hidden(256),
    };
    assert_eq!(few.lines().len(), 257);
    assert_eq!(
        Labels::default().lines(),
        vec!["labels shown=0 hidden=0".to_owned()]
    );
}
