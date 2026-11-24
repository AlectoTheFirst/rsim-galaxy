//! Writes the frame of a site with a packet flow to the PNG file of the first argument. Each
//! further argument is a value of the look, for example `isometric dark glow floating flat`.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufWriter;

use galaxy_render::{Camera, Renderer, View};
use galaxy_scene::{
    Arrangement, Light, Lines, Look, ObjectId, Projection, Relations, Rule, RuleSet, Scheme, Shape,
    Space, World,
};

fn id(name: &str) -> ObjectId {
    ObjectId::new(name).expect("object name")
}

fn rule(arrangement: Arrangement, size: [f64; 3]) -> Rule {
    Rule {
        arrangement,
        shape: Shape::Box,
        size,
        gap: 0.5,
        margin: 0.5,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("argument: path of the PNG file")?;
    let rules = RuleSet {
        root: rule(Arrangement::Row, [0.0; 3]),
        default: rule(Arrangement::Row, [1.0, 1.0, 1.0]),
        kinds: BTreeMap::from([
            ("site".to_owned(), rule(Arrangement::Grid, [4.0, 1.0, 4.0])),
            (
                "device".to_owned(),
                rule(Arrangement::Stack, [3.0, 1.0, 2.0]),
            ),
            (
                "function".to_owned(),
                rule(Arrangement::Row, [2.0, 0.5, 1.0]),
            ),
        ]),
    };
    let mut relations = Relations::new();
    let nodes = [
        ("site-a", "site", None),
        ("core-01", "device", Some("site-a")),
        ("fw-01", "device", Some("site-a")),
        ("ips-01", "device", Some("site-a")),
        ("dns", "endpoint", Some("site-a")),
        ("client", "endpoint", Some("site-a")),
        ("core-01.in", "function", Some("core-01")),
        ("core-01.route", "function", Some("core-01")),
        ("core-01.out", "function", Some("core-01")),
        ("fw-01.in", "function", Some("fw-01")),
        ("fw-01.zone", "function", Some("fw-01")),
        ("fw-01.policy", "function", Some("fw-01")),
        ("fw-01.nat", "function", Some("fw-01")),
        ("fw-01.out", "function", Some("fw-01")),
        ("ips-01.in", "function", Some("ips-01")),
        ("ips-01.out", "function", Some("ips-01")),
    ];
    for (name, kind, parent) in nodes {
        relations.add(id(name), kind, parent.map(id))?;
    }
    let path_nodes = [
        "client",
        "fw-01.in",
        "fw-01.zone",
        "fw-01.policy",
        "fw-01.nat",
        "fw-01.out",
        "ips-01.in",
        "ips-01.out",
        "core-01.in",
        "core-01.route",
        "core-01.out",
        "dns",
    ];
    relations.add_flow(id("client-dns"), path_nodes.map(id).to_vec())?;
    let world = World::expand(&relations, &rules);
    let mut look = Look::default();
    for word in std::env::args().skip(2) {
        if let Some(value) = Projection::parse(&word) {
            look.projection = value;
        } else if let Some(value) = Space::parse(&word) {
            look.space = value;
        } else if let Some(value) = Light::parse(&word) {
            look.light = value;
        } else if let Some(value) = Lines::parse(&word) {
            look.lines = value;
        } else if let Some(value) = Scheme::parse(&word) {
            look.scheme = value;
        } else {
            return Err(format!("look: no value {word}").into());
        }
    }
    let camera = Camera::home(&world, &look, 1.6);
    let view = View {
        world: &world,
        look: &look,
        selection: None,
        camera: &camera,
        details: &[],
    };
    let image = Renderer::new()?.render(&view, 1600, 1000)?;
    image.write_png(BufWriter::new(File::create(path)?))?;
    Ok(())
}
