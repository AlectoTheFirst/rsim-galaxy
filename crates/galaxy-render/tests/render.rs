use std::collections::BTreeMap;

use galaxy_render::{Camera, Image, Renderer, Selection, View};
use galaxy_scene::{
    Arrangement, Axis, Colours, DefinedShape, Library, Lines, Look, ObjectId, Outcome, OutcomeKind,
    Part, Projection, Relations, Rule, RuleSet, Scheme, Shape, Solid, Space, World,
};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 200;
const ASPECT: f32 = WIDTH as f32 / HEIGHT as f32;

fn id(name: &str) -> ObjectId {
    ObjectId::new(name).unwrap()
}

fn world() -> World {
    let rule = |arrangement, size| Rule {
        arrangement,
        shape: Shape::Box,
        size,
        gap: 0.5,
        margin: 0.5,
    };
    let rules = RuleSet {
        root: rule(Arrangement::Row, [0.0; 3]),
        default: rule(Arrangement::Row, [1.0, 1.0, 1.0]),
        kinds: BTreeMap::from([
            ("site".to_owned(), rule(Arrangement::Grid, [4.0, 1.0, 4.0])),
            (
                "device".to_owned(),
                rule(Arrangement::Stack, [3.0, 1.0, 2.0]),
            ),
        ]),
    };
    let mut relations = Relations::new();
    for (name, kind, parent) in [
        ("site-a", "site", None),
        ("fw-01", "device", Some("site-a")),
        ("client", "endpoint", Some("site-a")),
        ("dns", "endpoint", Some("site-a")),
        ("fw-01.in", "function", Some("fw-01")),
        ("fw-01.out", "function", Some("fw-01")),
    ] {
        relations.add(id(name), kind, parent.map(id)).unwrap();
    }
    let path = ["client", "fw-01.in", "fw-01.out", "dns"].map(id).to_vec();
    relations.add_flow(id("client-dns"), path).unwrap();
    World::expand(&relations, &rules)
}

/// The renderer of the system, or `None` on a system with no GPU adapter.
fn renderer() -> Option<Renderer> {
    match Renderer::new() {
        Ok(renderer) => Some(renderer),
        Err(error) => {
            eprintln!("render test not run: {error}");
            None
        }
    }
}

fn render(renderer: &Renderer, look: &Look, selection: Option<&Selection>) -> Image {
    let world = world();
    let camera = Camera::home(&world, look, ASPECT);
    let view = View {
        world: &world,
        look,
        selection,
        camera: &camera,
        details: &[],
    };
    renderer.render(&view, WIDTH, HEIGHT).unwrap()
}

fn count(image: &Image, rgb: [u8; 3]) -> usize {
    image
        .rgba
        .chunks(4)
        .filter(|pixel| pixel[..3] == rgb)
        .count()
}

/// The pixel of the centre of the node `name` from the home camera.
fn pixel_of(world: &World, look: &Look, name: &str) -> [u32; 2] {
    let index = world.index(&id(name)).unwrap();
    let (_, placed, _) = world.node(index);
    let [x, y] = Camera::home(world, look, ASPECT)
        .project(look, placed.centre, WIDTH, HEIGHT)
        .unwrap();
    [x as u32, y as u32]
}

#[test]
fn same_world_and_camera_give_the_same_image() {
    let Some(renderer) = renderer() else { return };
    let look = Look::default();
    assert_eq!(
        render(&renderer, &look, None),
        render(&renderer, &look, None)
    );
    assert_eq!(
        render(&renderer, &look, None),
        render(&Renderer::new().unwrap(), &look, None)
    );
}

#[test]
fn image_has_the_ground_colour_in_its_corner_and_the_flow_colour_in_view() {
    let Some(renderer) = renderer() else { return };
    let image = render(&renderer, &Look::default(), None);
    assert_eq!((image.width, image.height), (WIDTH, HEIGHT));
    assert_eq!(image.pixel(WIDTH - 1, 0), [0xF4, 0xF5, 0xF8, 0xFF]);
    assert!(count(&image, [0x00, 0x6C, 0x78]) > 0);
}

#[test]
fn look_with_ground_has_the_grid_and_the_floating_look_has_none() {
    let Some(renderer) = renderer() else { return };
    let grid = [0xDD, 0xE1, 0xE7];
    let ground = render(&renderer, &Look::default(), None);
    let floating = Look {
        space: Space::Floating,
        ..Look::default()
    };
    assert!(count(&ground, grid) > 0);
    assert_eq!(count(&render(&renderer, &floating, None), grid), 0);
}

#[test]
fn compass_is_in_the_bottom_left_corner() {
    let Some(renderer) = renderer() else { return };
    let image = render(&renderer, &Look::default(), None);
    let ground = [0xF4, 0xF5, 0xF8, 0xFF];
    let corner = (16..112)
        .flat_map(|x| (HEIGHT - 112..HEIGHT - 16).map(move |y| (x, y)))
        .filter(|&(x, y)| image.pixel(x, y) != ground)
        .count();
    assert!(corner > 0);
}

#[test]
fn two_looks_of_the_same_world_give_two_images() {
    let Some(renderer) = renderer() else { return };
    let dark = Look {
        projection: Projection::Isometric,
        lines: Lines::Glow,
        scheme: Scheme::Dark,
        ..Look::default()
    };
    let light = render(&renderer, &Look::default(), None);
    let image = render(&renderer, &dark, None);
    assert_ne!(image, light);
    assert_eq!(image.pixel(WIDTH - 1, 0), [0x0F, 0x13, 0x18, 0xFF]);
    assert!(count(&image, [0x5F, 0xD0, 0xD8]) > 0);
    assert_eq!(world().broken(), Vec::new());
}

#[test]
fn pick_at_the_pixel_of_a_node_gives_the_node_or_an_ancestor_in_each_projection() {
    let Some(renderer) = renderer() else { return };
    let world = world();
    for look in [
        Look::default(),
        Look {
            projection: Projection::Isometric,
            ..Look::default()
        },
    ] {
        let camera = Camera::home(&world, &look, ASPECT);
        let view = View {
            world: &world,
            look: &look,
            selection: None,
            camera: &camera,
            details: &[],
        };
        for name in ["client", "dns"] {
            let [x, y] = pixel_of(&world, &look, name);
            let picked = renderer.pick(&view, WIDTH, HEIGHT, x, y).unwrap();
            assert_eq!(picked, Some(Selection::Node(id(name))), "{name}");
        }
        let [x, y] = pixel_of(&world, &look, "fw-01.in");
        let picked = renderer.pick(&view, WIDTH, HEIGHT, x, y).unwrap();
        let drawn = ["fw-01.in", "fw-01", "site-a"].map(|name| Selection::Node(id(name)));
        assert!(drawn.contains(picked.as_ref().unwrap()), "{picked:?}");
        assert_eq!(
            renderer.pick(&view, WIDTH, HEIGHT, WIDTH - 1, 0).unwrap(),
            None
        );
        assert_eq!(renderer.pick(&view, WIDTH, HEIGHT, WIDTH, 0).unwrap(), None);
    }
}

#[test]
fn selected_node_has_edges_in_the_selection_colour() {
    let Some(renderer) = renderer() else { return };
    let selection = Selection::Node(id("client"));
    let look = Look::default();
    let selected = render(&renderer, &look, Some(&selection));
    assert!(count(&selected, [0x2A, 0x52, 0xD6]) > 0);
    assert_eq!(
        count(&render(&renderer, &look, None), [0x2A, 0x52, 0xD6]),
        0
    );
}

#[test]
fn far_camera_draws_the_site_as_1_closed_box_with_no_flow() {
    let Some(renderer) = renderer() else { return };
    let world = world();
    let look = Look::default();
    let home = Camera::home(&world, &look, ASPECT);
    let far = Camera {
        eye: [0, 1, 2].map(|axis| home.target[axis] + (home.eye[axis] - home.target[axis]) * 8.0),
        ..home
    };
    let view = |camera| View {
        world: &world,
        look: &look,
        selection: None,
        camera,
        details: &[],
    };
    let near_count = renderer.count(&view(&home), WIDTH, HEIGHT);
    let far_count = renderer.count(&view(&far), WIDTH, HEIGHT);
    assert!(far_count < near_count);
    let image = renderer.render(&view(&far), WIDTH, HEIGHT).unwrap();
    assert_eq!(count(&image, [0x00, 0x6C, 0x78]), 0);
}

#[test]
fn panel_of_the_selection_shows_its_lines_in_the_text_colour() {
    let Some(renderer) = renderer() else { return };
    let world = world();
    let look = Look::default();
    let camera = Camera::home(&world, &look, ASPECT);
    let selection = Selection::Node(id("client"));
    let details = [
        (String::new(), "client".to_owned()),
        ("ip".to_owned(), "192.0.2.10".to_owned()),
    ];
    let frame = |details: &[(String, String)]| {
        let view = View {
            world: &world,
            look: &look,
            selection: Some(&selection),
            camera: &camera,
            details,
        };
        renderer.render(&view, WIDTH, HEIGHT).unwrap()
    };
    let text = [0x15, 0x19, 0x1F];
    let (without, with) = (count(&frame(&[]), text), count(&frame(&details), text));
    assert!(with > without + 50, "{without} {with}");
}

#[test]
fn node_of_a_cylinder_kind_leaves_the_corners_of_its_box_empty() {
    let Some(renderer) = renderer() else { return };
    let look = Look {
        light: galaxy_scene::Light::Flat,
        space: Space::Floating,
        ..Look::default()
    };
    let surface = [0xFF, 0xFF, 0xFF];
    let frame = |shape: Shape| {
        let rules = RuleSet {
            root: Rule {
                arrangement: Arrangement::Row,
                shape: Shape::Box,
                size: [0.0; 3],
                gap: 0.5,
                margin: 0.5,
            },
            default: Rule {
                arrangement: Arrangement::Row,
                shape,
                size: [2.0, 1.0, 2.0],
                gap: 0.5,
                margin: 0.5,
            },
            kinds: BTreeMap::new(),
        };
        let mut relations = Relations::new();
        relations.add(id("router"), "router", None).unwrap();
        let world = World::expand(&relations, &rules);
        let camera = Camera::home(&world, &look, ASPECT);
        let view = View {
            world: &world,
            look: &look,
            selection: None,
            camera: &camera,
            details: &[],
        };
        count(&renderer.render(&view, WIDTH, HEIGHT).unwrap(), surface)
    };
    let (cube, cylinder) = (frame(Shape::Box), frame(Shape::Cylinder));
    assert!(cylinder > 0 && cylinder < cube, "{cylinder} {cube}");
}

/// The image of 1 node of `shape` with the size of `size`, from the home camera moved away by
/// `factor`, in the flat light with no ground.
fn one_node(renderer: &Renderer, shape: Shape, size: [f64; 3], factor: f32) -> Image {
    node_frame(renderer, shape, size, Library::default(), |home| Camera {
        eye: [0, 1, 2]
            .map(|axis| home.target[axis] + (home.eye[axis] - home.target[axis]) * factor),
        ..home
    })
}

/// The image of 1 node of `shape` with the size of `size` in a world with `library`, from the
/// camera that `camera` gives for the home camera, in the flat light with no ground.
fn node_frame(
    renderer: &Renderer,
    shape: Shape,
    size: [f64; 3],
    library: Library,
    camera: impl Fn(Camera) -> Camera,
) -> Image {
    let look = flat();
    let world = node_world(shape, size, library);
    let camera = camera(Camera::home(&world, &look, ASPECT));
    let view = View {
        world: &world,
        look: &look,
        selection: None,
        camera: &camera,
        details: &[],
    };
    renderer.render(&view, WIDTH, HEIGHT).unwrap()
}

/// The look with the flat light and no ground.
fn flat() -> Look {
    Look {
        light: galaxy_scene::Light::Flat,
        space: Space::Floating,
        ..Look::default()
    }
}

/// The world of the node node-a of `shape` with the size of `size` and `library`.
fn node_world(shape: Shape, size: [f64; 3], library: Library) -> World {
    let rule = |shape, size| Rule {
        arrangement: Arrangement::Row,
        shape,
        size,
        gap: 0.5,
        margin: 0.5,
    };
    let rules = RuleSet {
        root: rule(Shape::Box, [0.0; 3]),
        default: rule(shape, size),
        kinds: BTreeMap::new(),
    };
    let mut relations = Relations::new();
    relations.add(id("node-a"), "node", None).unwrap();
    World::expand(&relations, &rules).with_library(library)
}

/// `library` with the colours `rgb` in both schemes and no light for the material `name`, added
/// when it is not in `library`.
fn coloured(mut library: Library, name: &str, rgb: [u8; 3]) -> Library {
    let colours = Some(Colours {
        light: rgb,
        dark: rgb,
        lit: false,
    });
    match library.material(name) {
        Some(index) => library.materials[index].colours = colours,
        None => library.materials.push(galaxy_scene::Material {
            name: name.to_owned(),
            colours,
        }),
    }
    library
}

#[test]
fn defined_shape_shows_the_colour_of_its_material() {
    let Some(renderer) = renderer() else { return };
    let accent = [0xE0, 0x7A, 0x1F];
    let mut library = coloured(Library::default(), "accent", accent);
    library.shapes.push(DefinedShape {
        name: "block".to_owned(),
        parts: vec![Part {
            solid: Solid::Box,
            at: [0.0, 0.0, 0.0],
            size: [1.0, 0.5, 1.0],
            axis: Axis::Y,
            material: "accent".to_owned(),
            edges: true,
        }],
        symbol: Vec::new(),
    });
    let image = node_frame(
        &renderer,
        Shape::Defined(0),
        [1.0, 1.0, 1.0],
        library,
        |home| home,
    );
    assert!(count(&image, accent) > 1000, "{}", count(&image, accent));
}

#[test]
fn pick_at_the_centre_of_a_node_of_a_defined_shape_gives_the_node() {
    let Some(renderer) = renderer() else { return };
    let mut library = Library::default();
    library.shapes.push(DefinedShape {
        name: "block".to_owned(),
        parts: vec![Part {
            solid: Solid::Box,
            at: [0.0, 0.0, 0.0],
            size: [1.0, 0.5, 1.0],
            axis: Axis::Y,
            material: "body".to_owned(),
            edges: true,
        }],
        symbol: Vec::new(),
    });
    let world = node_world(Shape::Defined(0), [1.0, 1.0, 1.0], library);
    let look = flat();
    let camera = Camera::home(&world, &look, ASPECT);
    let view = View {
        world: &world,
        look: &look,
        selection: None,
        camera: &camera,
        details: &[],
    };
    assert_eq!(
        renderer
            .pick(&view, WIDTH, HEIGHT, WIDTH / 2, HEIGHT / 2)
            .unwrap(),
        Some(Selection::Node(id("node-a")))
    );
}

#[test]
fn set_colours_of_the_symbol_and_the_body_show() {
    let Some(renderer) = renderer() else { return };
    let (symbol, body) = ([0x12, 0xA0, 0x55], [0x40, 0x80, 0xC0]);
    let library = coloured(coloured(Library::default(), "symbol", symbol), "body", body);
    let image = node_frame(&renderer, Shape::Router, [1.6, 0.4, 1.6], library, |home| {
        home
    });
    assert!(count(&image, symbol) > 100, "{}", count(&image, symbol));
    assert!(count(&image, body) > 1000, "{}", count(&image, body));
    assert_eq!(count(&image, [0x2E, 0x3B, 0x4E]), 0);
}

#[test]
fn front_symbol_shows_from_the_front() {
    let Some(renderer) = renderer() else { return };
    let symbol = [0x2E, 0x3B, 0x4E];
    let front = |home: Camera| Camera {
        eye: [home.target[0], home.target[1], home.target[2] + 1.8],
        ..home
    };
    for (shape, size) in [
        (Shape::Router, [1.6, 0.4, 1.6]),
        (Shape::Switch, [1.6, 0.35, 1.6]),
        (Shape::LayerThreeSwitch, [1.6, 0.35, 1.6]),
        (Shape::AccessPoint, [1.4, 0.35, 1.0]),
        (Shape::LoadBalancer, [1.6, 0.4, 1.2]),
    ] {
        let image = node_frame(&renderer, shape, size, Library::default(), front);
        assert!(
            count(&image, symbol) > 20,
            "{shape:?} {}",
            count(&image, symbol)
        );
    }
    let plain = node_frame(
        &renderer,
        Shape::Box,
        [1.6, 0.35, 1.6],
        Library::default(),
        front,
    );
    assert_eq!(count(&plain, symbol), 0);
}

#[test]
fn router_shows_its_arrows_near_and_no_arrows_far() {
    let Some(renderer) = renderer() else { return };
    let symbol = [0x2E, 0x3B, 0x4E];
    let near = one_node(&renderer, Shape::Router, [1.6, 0.4, 1.6], 1.0);
    let far = one_node(&renderer, Shape::Router, [1.6, 0.4, 1.6], 14.0);
    assert!(count(&near, symbol) > 100, "{}", count(&near, symbol));
    assert_eq!(count(&far, symbol), 0);
    assert!(count(&far, [0xFF, 0xFF, 0xFF]) > 0);
}

#[test]
fn firewall_has_bricks_and_a_flame_in_their_colours() {
    let Some(renderer) = renderer() else { return };
    let image = one_node(&renderer, Shape::Firewall, [1.6, 1.2, 0.8], 1.0);
    for rgb in [
        [0xB5, 0x4A, 0x36],
        [0xE8, 0x60, 0x2C],
        [0xFF, 0xC9, 0x3C],
        [0xD9, 0xD3, 0xC9],
    ] {
        assert!(count(&image, rgb) > 50, "{rgb:?} {}", count(&image, rgb));
    }
}

#[test]
fn cap_of_a_cylinder_has_the_light_of_the_top_of_a_box_at_its_centre() {
    let Some(renderer) = renderer() else { return };
    let look = Look::default();
    let frame = |shape: Shape| {
        let rule = |shape, size| Rule {
            arrangement: Arrangement::Row,
            shape,
            size,
            gap: 0.5,
            margin: 0.5,
        };
        let rules = RuleSet {
            root: rule(Shape::Box, [0.0; 3]),
            default: rule(shape, [2.0, 0.4, 2.0]),
            kinds: BTreeMap::new(),
        };
        let mut relations = Relations::new();
        relations.add(id("node-a"), "node", None).unwrap();
        let world = World::expand(&relations, &rules);
        let camera = Camera::home(&world, &look, ASPECT);
        let view = View {
            world: &world,
            look: &look,
            selection: None,
            camera: &camera,
            details: &[],
        };
        let [x, y] = camera
            .project(&look, [0.0, 0.4, 0.0], WIDTH, HEIGHT)
            .unwrap();
        renderer
            .render(&view, WIDTH, HEIGHT)
            .unwrap()
            .pixel(x as u32, y as u32)
    };
    let (cube, cylinder) = (frame(Shape::Box), frame(Shape::Cylinder));
    for channel in 0..3 {
        assert!(
            cube[channel].abs_diff(cylinder[channel]) <= 1,
            "{cube:?} {cylinder:?}"
        );
    }
}

/// 2 endpoints in a row with 1 flow from the first to the second for each of `outcomes`.
fn pair(outcomes: &[Option<OutcomeKind>]) -> World {
    let rule = |size| Rule {
        arrangement: Arrangement::Row,
        shape: Shape::Box,
        size,
        gap: 2.0,
        margin: 1.0,
    };
    let rules = RuleSet {
        root: rule([0.0; 3]),
        default: rule([1.0, 1.0, 1.0]),
        kinds: BTreeMap::new(),
    };
    let mut relations = Relations::new();
    for name in ["client", "server"] {
        relations.add(id(name), "endpoint", None).unwrap();
    }
    for (number, kind) in outcomes.iter().enumerate() {
        let outcome = kind.map(|kind| Outcome { kind, reason: None });
        relations
            .add_flow_with_data(
                id(&format!("flow-{number}")),
                vec![id("client"), id("server")],
                outcome,
                Vec::new(),
            )
            .unwrap();
    }
    World::expand(&relations, &rules)
}

fn frame(renderer: &Renderer, world: &World, look: &Look) -> Image {
    let camera = Camera::home(world, look, ASPECT);
    let view = View {
        world,
        look,
        selection: None,
        camera: &camera,
        details: &[],
    };
    renderer.render(&view, WIDTH, HEIGHT).unwrap()
}

#[test]
fn two_flows_on_the_same_nodes_are_2_lines_with_a_gap() {
    let Some(renderer) = renderer() else { return };
    let look = Look::default();
    let world = pair(&[Some(OutcomeKind::Exit), Some(OutcomeKind::Exit)]);
    let image = frame(&renderer, &world, &look);
    let camera = Camera::home(&world, &look, ASPECT);
    let flows = world.flows();
    let middle = |points: &[[f64; 3]]| {
        let place = points.len() / 2;
        [0, 1, 2].map(|axis| (points[place - 1][axis] + points[place][axis]) / 2.0)
    };
    let (first, second) = (middle(&flows[0].points), middle(&flows[1].points));
    let between = [0, 1, 2].map(|axis| (first[axis] + second[axis]) / 2.0);
    let colour = |point: [f64; 3]| {
        let [x, y] = camera.project(&look, point, WIDTH, HEIGHT).unwrap();
        image.pixel(x as u32, y as u32)
    };
    let (flow, other) = ([0x00, 0x6C, 0x78, 0xFF], [0x1F, 0x8A, 0x5C, 0xFF]);
    assert_eq!(colour(first), flow);
    assert_eq!(colour(second), other);
    assert_ne!(colour(between), flow);
    assert_ne!(colour(between), other);
}

#[test]
fn selected_flow_keeps_the_colour_of_its_outcome_inside_a_halo() {
    let Some(renderer) = renderer() else { return };
    let look = Look::default();
    let world = pair(&[Some(OutcomeKind::Drop)]);
    let camera = Camera::home(&world, &look, ASPECT);
    let selection = Selection::Flow(id("flow-0"));
    let view = View {
        world: &world,
        look: &look,
        selection: Some(&selection),
        camera: &camera,
        details: &[],
    };
    let image = renderer.render(&view, WIDTH, HEIGHT).unwrap();
    let plain = frame(&renderer, &world, &look);
    let (drop, selected) = ([0xC8, 0x26, 0x1E], [0x2A, 0x52, 0xD6]);
    assert!(count(&image, drop) > 100, "{}", count(&image, drop));
    assert!(count(&image, selected) > 100, "{}", count(&image, selected));
    assert_eq!(count(&plain, selected), 0);
}

#[test]
fn line_has_the_colour_of_its_outcome_in_each_scheme() {
    let Some(renderer) = renderer() else { return };
    for (scheme, flow, drop, looped, unknown) in [
        (
            Scheme::Light,
            [0x00, 0x6C, 0x78],
            [0xC8, 0x26, 0x1E],
            [0xC2, 0x7C, 0x0E],
            [0x7A, 0x84, 0x91],
        ),
        (
            Scheme::Dark,
            [0x5F, 0xD0, 0xD8],
            [0xFF, 0x5C, 0x50],
            [0xF5, 0xB0, 0x3C],
            [0x9A, 0xA4, 0xB2],
        ),
    ] {
        let look = Look {
            scheme,
            ..Look::default()
        };
        for (kind, rgb) in [
            (Some(OutcomeKind::Deliver), flow),
            (Some(OutcomeKind::Exit), flow),
            (None, flow),
            (Some(OutcomeKind::Drop), drop),
            (Some(OutcomeKind::Loop), looped),
            (Some(OutcomeKind::Unknown), unknown),
        ] {
            let image = frame(&renderer, &pair(&[kind]), &look);
            assert!(count(&image, rgb) > 100, "{scheme:?} {kind:?}");
            for other in [flow, drop, looped, unknown] {
                if other != rgb {
                    assert_eq!(count(&image, other), 0, "{scheme:?} {kind:?} {other:?}");
                }
            }
        }
    }
}

#[test]
fn line_of_a_dropped_flow_ends_in_a_stop_mark_clear_of_its_last_node() {
    let Some(renderer) = renderer() else { return };
    let look = Look::default();
    let world = pair(&[Some(OutcomeKind::Drop)]);
    let (_, server, _) = world.node(world.index(&id("server")).unwrap());
    let top = [
        server.centre[0],
        server.centre[1] + server.size[1] / 2.0,
        server.centre[2],
    ];
    let level = [top[0], top[1] + 0.15, top[2]];
    let camera = Camera {
        eye: [level[0] as f32, level[1] as f32, (level[2] + 4.0) as f32],
        target: level.map(|value| value as f32),
        fov_y_degrees: 40.0,
    };
    let view = View {
        world: &world,
        look: &look,
        selection: None,
        camera: &camera,
        details: &[],
    };
    let image = renderer.render(&view, WIDTH, HEIGHT).unwrap();
    let [x, y] = camera.project(&look, top, WIDTH, HEIGHT).unwrap();
    let drop = [0xC8, 0x26, 0x1E];
    let marked: Vec<u32> = (0..y as u32)
        .filter(|&row| image.pixel(x as u32, row)[..3] == drop)
        .collect();
    let lowest = marked.iter().copied().max().unwrap();
    assert!(y as u32 - lowest >= 3, "{y} {lowest}");
}

#[test]
fn home_camera_fills_the_frame_with_the_world() {
    let world = world();
    for look in [
        Look::default(),
        Look {
            projection: Projection::Isometric,
            ..Look::default()
        },
    ] {
        let camera = Camera::home(&world, &look, ASPECT);
        let mut low = [f32::MAX; 2];
        let mut high = [f32::MIN; 2];
        for &index in world.top() {
            let (_, placed, _) = world.node(index);
            for corner in 0..8 {
                let point = [0, 1, 2].map(|axis| {
                    let side = if corner & (1 << axis) == 0 { -0.5 } else { 0.5 };
                    placed.centre[axis] + side * placed.size[axis]
                });
                let pixel = camera.project(&look, point, WIDTH, HEIGHT).unwrap();
                for slot in 0..2 {
                    low[slot] = low[slot].min(pixel[slot]);
                    high[slot] = high[slot].max(pixel[slot]);
                }
            }
        }
        assert!(low[0] >= 0.0 && low[1] >= 0.0, "{low:?}");
        assert!(
            high[0] <= WIDTH as f32 && high[1] <= HEIGHT as f32,
            "{high:?}"
        );
        let fill = ((high[0] - low[0]) / WIDTH as f32).max((high[1] - low[1]) / HEIGHT as f32);
        assert!(fill > 0.85, "{look:?} {fill}");
    }
}

#[test]
fn flow_line_past_the_camera_keeps_its_width_near_the_camera() {
    let Some(renderer) = renderer() else { return };
    let rule = |size, gap| Rule {
        arrangement: Arrangement::Row,
        shape: Shape::Box,
        size,
        gap,
        margin: 0.5,
    };
    let rules = RuleSet {
        root: rule([0.0; 3], 40.0),
        default: rule([1.0, 1.0, 1.0], 0.5),
        kinds: BTreeMap::new(),
    };
    let mut relations = Relations::new();
    relations.add(id("near"), "endpoint", None).unwrap();
    relations.add(id("far"), "endpoint", None).unwrap();
    relations
        .add_flow(id("near-far"), ["near", "far"].map(id).to_vec())
        .unwrap();
    let world = World::expand(&relations, &rules);
    let line = &world.flows()[0].points;
    let [start, end] = [line[0], line[line.len() - 1]];
    let at = |part: f64| [0, 1, 2].map(|axis| start[axis] + (end[axis] - start[axis]) * part);
    let near = at(0.1);
    let look = Look::default();
    let camera = Camera {
        eye: [near[0] as f32, near[1] as f32 + 0.6, near[2] as f32 + 0.6],
        target: at(0.9).map(|value| value as f32),
        fov_y_degrees: 45.0,
    };
    let view = View {
        world: &world,
        look: &look,
        selection: None,
        camera: &camera,
        details: &[],
    };
    let image = renderer.render(&view, WIDTH, HEIGHT).unwrap();
    let line = count(&image, [0x00, 0x6C, 0x78]);
    let frame = (WIDTH * HEIGHT) as usize;
    assert!(line > 0 && line < frame / 20, "{line} of {frame}");
}

/// The colour of the symbol in the light scheme.
const SYMBOL: [u8; 3] = [0x2E, 0x3B, 0x4E];

/// The ratio of the width to the height of the box of the pixels of `rgb` in `image`.
fn aspect(image: &Image, rgb: [u8; 3]) -> f32 {
    let mut low = [u32::MAX; 2];
    let mut high = [0; 2];
    for y in 0..image.height {
        for x in 0..image.width {
            if image.pixel(x, y)[..3] == rgb {
                low = [low[0].min(x), low[1].min(y)];
                high = [high[0].max(x), high[1].max(y)];
            }
        }
    }
    assert!(
        high[0] >= low[0] && high[1] >= low[1],
        "no pixel of {rgb:?}"
    );
    (high[0] + 1 - low[0]) as f32 / (high[1] + 1 - low[1]) as f32
}

/// The image of 1600 by 1000 pixels of 1 node of `shape` with the size of `size`, in the isometric
/// projection with the flat light and no ground, from `direction` at 1.4 times the distance of
/// the home camera.
fn node_from(renderer: &Renderer, shape: Shape, size: [f64; 3], direction: [f32; 3]) -> Image {
    let look = Look {
        projection: Projection::Isometric,
        ..flat()
    };
    let world = node_world(shape, size, Library::default());
    let home = Camera::home(&world, &look, 1.6);
    let distance = (0..3)
        .map(|axis| (home.eye[axis] - home.target[axis]).powi(2))
        .sum::<f32>()
        .sqrt()
        * 1.4;
    let length = direction
        .iter()
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt();
    let camera = Camera {
        eye: [0, 1, 2].map(|axis| home.target[axis] + direction[axis] / length * distance),
        ..home
    };
    let view = View {
        world: &world,
        look: &look,
        selection: None,
        camera: &camera,
        details: &[],
    };
    renderer.render(&view, 1600, 1000).unwrap()
}

#[test]
fn symbols_keep_their_aspect_on_a_long_flat_node_and_on_a_tall_narrow_node() {
    let Some(renderer) = renderer() else { return };
    let above = [0.0, 1.0, -0.3];
    let front = [0.0, -0.6, 1.0];
    for (shape, reference) in [
        (Shape::Switch, [1.6, 0.35, 1.6]),
        (Shape::Router, [1.6, 0.4, 1.6]),
        (Shape::LayerThreeSwitch, [1.6, 0.35, 1.6]),
        (Shape::AccessPoint, [1.4, 0.35, 1.0]),
        (Shape::LoadBalancer, [1.6, 0.4, 1.2]),
    ] {
        for direction in [above, front] {
            let expected = aspect(&node_from(&renderer, shape, reference, direction), SYMBOL);
            for size in [[4.0, 0.4, 0.9], [0.7, 1.4, 0.7]] {
                let image = node_from(&renderer, shape, size, direction);
                let ratio = aspect(&image, SYMBOL) / expected;
                assert!(
                    (ratio - 1.0).abs() < 0.15,
                    "{shape:?} {size:?} {direction:?} {ratio}"
                );
            }
        }
    }
}

/// The world of the rack rack-a with the arrangement stack and a gap of 1 between its children,
/// with the switches sw-01 and sw-02 from the bottom.
fn stack() -> World {
    let rule = |arrangement, shape, size| Rule {
        arrangement,
        shape,
        size,
        gap: 1.0,
        margin: 0.2,
    };
    let rules = RuleSet {
        root: rule(Arrangement::Row, Shape::Box, [0.0; 3]),
        default: rule(Arrangement::Stack, Shape::Box, [1.0; 3]),
        kinds: BTreeMap::from([(
            "switch".to_owned(),
            rule(Arrangement::Stack, Shape::Switch, [1.6, 0.35, 1.6]),
        )]),
    };
    let mut relations = Relations::new();
    relations.add(id("rack-a"), "rack", None).unwrap();
    for name in ["sw-01", "sw-02"] {
        relations
            .add(id(name), "switch", Some(id("rack-a")))
            .unwrap();
    }
    World::expand(&relations, &rules)
}

/// The image of `world` from `eye` toward `target` in the flat light with no ground.
fn seen(renderer: &Renderer, world: &World, eye: [f64; 3], target: [f64; 3]) -> Image {
    let look = flat();
    let camera = Camera {
        eye: eye.map(|value| value as f32),
        target: target.map(|value| value as f32),
        fov_y_degrees: 45.0,
    };
    let view = View {
        world,
        look: &look,
        selection: None,
        camera: &camera,
        details: &[],
    };
    renderer.render(&view, WIDTH, HEIGHT).unwrap()
}

#[test]
fn device_below_another_device_of_a_stack_shows_no_top_symbol_and_keeps_its_front_symbol() {
    let Some(renderer) = renderer() else { return };
    let world = stack();
    let placed = |name: &str| *world.get(&id(name)).unwrap();
    let (lower, upper) = (placed("sw-01"), placed("sw-02"));
    assert!(upper.centre[1] > lower.centre[1]);
    let top = |node: &galaxy_scene::Placed| {
        [
            node.centre[0],
            node.centre[1] + node.size[1] / 2.0,
            node.centre[2],
        ]
    };
    let shifted = |point: [f64; 3], by: [f64; 3]| [0, 1, 2].map(|axis| point[axis] + by[axis]);
    let gap = seen(
        &renderer,
        &world,
        shifted(top(&lower), [0.0, 0.4, -3.0]),
        top(&lower),
    );
    assert_eq!(count(&gap, SYMBOL), 0);
    let above = seen(
        &renderer,
        &world,
        shifted(top(&upper), [0.0, 3.0, 1.0]),
        top(&upper),
    );
    assert!(count(&above, SYMBOL) > 100, "{}", count(&above, SYMBOL));
    let front = seen(
        &renderer,
        &world,
        shifted(lower.centre, [0.0, 0.0, 1.6]),
        lower.centre,
    );
    let middle = (0..WIDTH)
        .flat_map(|x| (HEIGHT / 2 - 20..HEIGHT / 2 + 20).map(move |y| [x, y]))
        .filter(|&[x, y]| front.pixel(x, y)[..3] == SYMBOL)
        .count();
    assert!(middle > 20, "{middle}");
}

/// The world of the nodes a, b and c in a row with the gap `gap`, b of `shape` with the size of
/// `size`, and 2 flows from a through b to c.
fn through(shape: Shape, size: [f64; 3], gap: f64) -> World {
    let rule = |shape, size| Rule {
        arrangement: Arrangement::Row,
        shape,
        size,
        gap,
        margin: 0.5,
    };
    let rules = RuleSet {
        root: rule(Shape::Box, [0.0; 3]),
        default: rule(Shape::Box, [1.0; 3]),
        kinds: BTreeMap::from([("middle".to_owned(), rule(shape, size))]),
    };
    let mut relations = Relations::new();
    relations.add(id("a"), "end", None).unwrap();
    relations.add(id("b"), "middle", None).unwrap();
    relations.add(id("c"), "end", None).unwrap();
    for name in ["one", "two"] {
        relations
            .add_flow(id(name), ["a", "b", "c"].map(id).to_vec())
            .unwrap();
    }
    World::expand(&relations, &rules)
}

#[test]
fn flow_line_is_hidden_inside_a_node_on_its_path() {
    let Some(renderer) = renderer() else { return };
    let world = through(Shape::Firewall, [1.6, 1.2, 0.8], 1.0);
    let b = *world.get(&id("b")).unwrap();
    let low = b.centre[1] - b.size[1] / 2.0 + 0.1;
    let inside = world.flows().iter().flat_map(|flow| {
        flow.points.windows(2).filter(move |pair| {
            pair.iter().all(|point| {
                (point[0] - b.centre[0]).abs() < b.size[0] / 2.0
                    && (point[2] - b.centre[2]).abs() < b.size[2] / 2.0
                    && point[1] > low
            })
        })
    });
    assert!(inside.count() > 0);
    let image = seen(
        &renderer,
        &world,
        [b.centre[0], b.centre[1], b.centre[2] + 3.0],
        b.centre,
    );
    let window = (WIDTH / 2 - 30..WIDTH / 2 + 30)
        .flat_map(|x| (HEIGHT / 2 - 20..HEIGHT / 2 + 20).map(move |y| [x, y]))
        .filter(|&[x, y]| {
            let pixel = image.pixel(x, y);
            pixel[..3] == [0x00, 0x6C, 0x78] || pixel[..3] == [0x1F, 0x8A, 0x5C]
        })
        .count();
    assert_eq!(window, 0);
}

#[test]
fn open_node_of_the_cloud_shape_is_the_outline_of_its_cloud_with_no_floor_plate() {
    let Some(renderer) = renderer() else { return };
    let world = |shape| {
        let rule = |shape, size| Rule {
            arrangement: Arrangement::Row,
            shape,
            size,
            gap: 0.5,
            margin: 0.5,
        };
        let rules = RuleSet {
            root: rule(Shape::Box, [0.0; 3]),
            default: rule(Shape::Box, [1.0; 3]),
            kinds: BTreeMap::from([("network".to_owned(), rule(shape, [1.8, 1.1, 0.7]))]),
        };
        let mut relations = Relations::new();
        relations.add(id("internet"), "network", None).unwrap();
        relations
            .add(id("client"), "end", Some(id("internet")))
            .unwrap();
        World::expand(&relations, &rules)
    };
    let look = Look {
        light: galaxy_scene::Light::Flat,
        space: Space::Ground,
        ..Look::default()
    };
    let counts = [Shape::Cloud, Shape::Box].map(|shape| {
        let world = world(shape);
        let camera = Camera::home(&world, &look, ASPECT);
        let view = View {
            world: &world,
            look: &look,
            selection: None,
            camera: &camera,
            details: &[],
        };
        renderer.count(&view, WIDTH, HEIGHT)
    });
    assert_eq!(counts[0] + 1, counts[1]);
}

/// The ratio of the width to the height of the box of the pixels of `image` outside the ground
/// colour.
fn extent(image: &Image) -> f32 {
    let ground = image.pixel(0, 0);
    let mut low = [u32::MAX; 2];
    let mut high = [0; 2];
    for y in 0..image.height {
        for x in 0..image.width {
            if image.pixel(x, y) != ground {
                low = [low[0].min(x), low[1].min(y)];
                high = [high[0].max(x), high[1].max(y)];
            }
        }
    }
    assert!(high[0] >= low[0] && high[1] >= low[1], "no node");
    (high[0] + 1 - low[0]) as f32 / (high[1] + 1 - low[1]) as f32
}

#[test]
fn router_of_a_long_box_is_a_stadium_through_the_box() {
    let Some(renderer) = renderer() else { return };
    let above = [0.0, 1.0, -0.3];
    let size = [4.0, 0.4, 0.9];
    let router = extent(&node_from(&renderer, Shape::Router, size, above));
    let cube = extent(&node_from(&renderer, Shape::Box, size, above));
    assert!((router / cube - 1.0).abs() < 0.05, "{router} {cube}");
}

#[test]
fn front_symbol_of_a_node_in_a_stack_shows_by_its_size_on_the_screen_as_alone() {
    let Some(renderer) = renderer() else { return };
    let stacked = stack();
    let lone = node_world(Shape::Switch, [1.6, 0.35, 1.6], Library::default());
    let fronts = |distance: f64| {
        [(&stacked, "sw-01"), (&lone, "node-a")].map(|(world, name)| {
            let centre = world.get(&id(name)).unwrap().centre;
            let eye = [centre[0], centre[1], centre[2] + distance];
            count(&seen(&renderer, world, eye, centre), SYMBOL)
        })
    };
    let near = fronts(4.0);
    assert!(near[0] > 0 && near[1] > 0, "{near:?}");
    assert_eq!(fronts(8.0), [0, 0]);
}

/// The world of the node plate-a of the kind plate with the shape `shape` of `library` and the
/// rule size [2, 0.5, 1], for `child` with 1 child of the size [1, 0.5, 1] at a margin of 0.5.
fn plate(library: Library, child: bool) -> World {
    let rule = |shape, size| Rule {
        arrangement: Arrangement::Row,
        shape,
        size,
        gap: 0.5,
        margin: 0.5,
    };
    let rules = RuleSet {
        root: rule(Shape::Box, [0.0; 3]),
        default: rule(Shape::Box, [1.0, 0.5, 1.0]),
        kinds: BTreeMap::from([("plate".to_owned(), rule(Shape::Defined(0), [2.0, 0.5, 1.0]))]),
    };
    let mut relations = Relations::new();
    relations.add(id("plate-a"), "plate", None).unwrap();
    if child {
        relations
            .add(id("part-a"), "part", Some(id("plate-a")))
            .unwrap();
    }
    World::expand(&relations, &rules).with_library(library)
}

/// The library with the shape plate: a body and a mark of the material symbol on its top,
/// with the mark as its symbol for `symbol`.
fn plate_library(symbol: bool) -> Library {
    let part = |at, size, material: &str| Part {
        solid: Solid::Box,
        at,
        size,
        axis: Axis::Y,
        material: material.to_owned(),
        edges: false,
    };
    let mut library = Library::default();
    library.shapes.push(DefinedShape {
        name: "plate".to_owned(),
        parts: vec![
            part([0.0, -0.1, 0.0], [1.0, 0.8, 1.0], "body"),
            part([0.0, 0.4, 0.0], [0.5, 0.2, 0.5], "symbol"),
        ],
        symbol: if symbol { vec![1] } else { Vec::new() },
    });
    library
}

#[test]
fn symbol_of_a_defined_shape_keeps_its_aspect_at_the_rule_size_and_on_a_node_of_another_size() {
    let Some(renderer) = renderer() else { return };
    let from_above = |world: World| {
        let centre = world.get(&id("plate-a")).unwrap().centre;
        let eye = [centre[0], centre[1] + 6.0, centre[2] + 1.2];
        let image = seen(&renderer, &world, eye, centre);
        let middle = Image {
            width: WIDTH / 2,
            height: HEIGHT / 2,
            rgba: (HEIGHT / 4..HEIGHT * 3 / 4)
                .flat_map(|y| (WIDTH / 4..WIDTH * 3 / 4).map(move |x| (x, y)))
                .flat_map(|(x, y)| image.pixel(x, y))
                .collect(),
        };
        aspect(&middle, SYMBOL)
    };
    let larger = plate(plate_library(true), true);
    assert_eq!(larger.get(&id("plate-a")).unwrap().size, [2.0, 1.5, 2.0]);
    let expected = from_above(plate(plate_library(false), false));
    assert!(expected > 1.6, "{expected}");
    for (world, name) in [
        (plate(plate_library(true), false), "rule size"),
        (larger, "larger"),
    ] {
        let ratio = from_above(world) / expected;
        assert!((ratio - 1.0).abs() < 0.1, "{name} {ratio}");
    }
}
