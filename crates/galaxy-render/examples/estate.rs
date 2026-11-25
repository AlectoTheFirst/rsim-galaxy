//! Builds an estate of 100 sites with 4 rooms each, 25 routers and 125 endpoints in each room,
//! and 1,000 flows, then prints the time of the expansion, of the frames and of the label pass at
//! 2560 by 1440 pixels from the home view, from a view of 1 site and from a view of 1 room, with
//! the counts of the shown and the hidden labels.

use std::time::Instant;

use galaxy_render::{Camera, Renderer, View};
use galaxy_scene::{Look, ObjectId, Relations, Style, World};

const SITES: usize = 100;
const ROOMS: usize = 4;
const ROUTERS: usize = 25;
const ENDPOINTS: usize = 5;
const FLOWS: usize = 1_000;
const WIDTH: u32 = 2560;
const HEIGHT: u32 = 1440;
const FRAMES: u32 = 20;

fn id(name: String) -> ObjectId {
    ObjectId::new(&name).expect("object name")
}

fn estate() -> Result<Relations, Box<dyn std::error::Error>> {
    let mut relations = Relations::new();
    for site in 0..SITES {
        relations.add(id(format!("s{site}")), "site", None)?;
        for room in 0..ROOMS {
            let room_id = id(format!("s{site}-r{room}"));
            relations.add(room_id.clone(), "room", Some(id(format!("s{site}"))))?;
            for router in 0..ROUTERS {
                let router_id = format!("s{site}-r{room}-rt{router}");
                relations.add(id(router_id.clone()), "device", Some(room_id.clone()))?;
                for endpoint in 0..ENDPOINTS {
                    let fields = vec![(
                        "ip".to_owned(),
                        format!(
                            "2001:db8:{site:x}:{:x}::{:x}",
                            room * ROUTERS + router,
                            endpoint + 10
                        ),
                    )];
                    relations.add_with_fields(
                        id(format!("{router_id}-e{endpoint}")),
                        "endpoint",
                        Some(room_id.clone()),
                        fields,
                    )?;
                }
            }
        }
    }
    let endpoints = SITES * ROOMS * ROUTERS * ENDPOINTS;
    for flow in 0..FLOWS {
        let name = |number: usize| {
            let endpoint = number % ENDPOINTS;
            let router = number / ENDPOINTS % ROUTERS;
            let room = number / (ENDPOINTS * ROUTERS) % ROOMS;
            let site = number / (ENDPOINTS * ROUTERS * ROOMS);
            (
                id(format!("s{site}-r{room}-rt{router}-e{endpoint}")),
                id(format!("s{site}-r{room}-rt{router}")),
            )
        };
        let (source, source_router) = name(flow * 47 % endpoints);
        let (target, target_router) = name((flow * 7919 + 13) % endpoints);
        if source_router == target_router {
            continue;
        }
        let path = vec![source, source_router, target_router, target];
        relations.add_flow(id(format!("f{flow}")), path)?;
    }
    Ok(relations)
}

fn frames(renderer: &Renderer, view: &View, label: &str) -> Result<(), Box<dyn std::error::Error>> {
    let texture = renderer.device().create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let target = texture.create_view(&Default::default());
    renderer.draw(view, &target, WIDTH, HEIGHT);
    renderer.wait()?;
    let start = Instant::now();
    for _ in 0..FRAMES {
        renderer.draw(view, &target, WIDTH, HEIGHT);
        renderer.wait()?;
    }
    let frame = start.elapsed() / FRAMES;
    println!(
        "{label}: {} instances, {:.2} ms for each frame",
        renderer.count(view, WIDTH, HEIGHT),
        frame.as_secs_f64() * 1000.0
    );
    let start = Instant::now();
    for _ in 0..FRAMES {
        renderer.labels(view, WIDTH, HEIGHT);
    }
    let pass = start.elapsed() / FRAMES;
    let labels = renderer.labels(view, WIDTH, HEIGHT);
    println!(
        "{label}: {} labels shown, {} hidden, {:.3} ms for each label pass",
        labels.shown.len(),
        labels.hidden.len(),
        pass.as_secs_f64() * 1000.0
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let relations = estate()?;
    println!(
        "relations: {} nodes, {} flows, {:.1} ms",
        relations.nodes().len(),
        relations.flows().len(),
        start.elapsed().as_secs_f64() * 1000.0
    );
    let style = Style::default();
    let start = Instant::now();
    let world = World::expand(&relations, &style.rules);
    println!(
        "expansion: {:.1} ms",
        start.elapsed().as_secs_f64() * 1000.0
    );
    let renderer = Renderer::new()?;
    let look = Look::default();
    let home = Camera::home(&world, &look, WIDTH as f32 / HEIGHT as f32);
    let focus = |name: &str, distance: f32| {
        let index = world.index(&id(name.to_owned())).expect("node");
        let (_, placed, _) = world.node(index);
        let target = placed.centre.map(|value| value as f32);
        let direction = [0.45_f32, 0.55, 0.7];
        Camera {
            eye: [0, 1, 2].map(|axis| target[axis] + direction[axis] * distance),
            target,
            fov_y_degrees: home.fov_y_degrees,
        }
    };
    for (label, camera) in [
        ("home", home),
        ("site", focus("s42", 60.0)),
        ("room", focus("s42-r1", 22.0)),
    ] {
        let view = View {
            world: &world,
            look: &look,
            selection: None,
            camera: &camera,
            details: &[],
            scale: 1.0,
        };
        frames(&renderer, &view, label)?;
    }
    Ok(())
}
