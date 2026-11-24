use std::collections::BTreeMap;

use galaxy_scene::{
    Arrangement, Broken, Library, ObjectId, RelationRefusal, Relations, Rule, RuleSet, Shape,
    Style, World,
};

fn id(name: &str) -> ObjectId {
    ObjectId::new(name).unwrap()
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

fn rules() -> RuleSet {
    RuleSet {
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
    }
}

fn relations(nodes: &[(&str, &str, Option<&str>)]) -> Relations {
    let mut relations = Relations::new();
    for &(name, kind, parent) in nodes {
        relations.add(id(name), kind, parent.map(id)).unwrap();
    }
    relations
}

/// 1 site with 3 devices, their functions and 2 endpoints.
fn site() -> Relations {
    relations(&[
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
    ])
}

fn centre(world: &World, name: &str) -> [f64; 3] {
    world.get(&id(name)).unwrap().centre
}

fn size(world: &World, name: &str) -> [f64; 3] {
    world.get(&id(name)).unwrap().size
}

#[test]
fn world_of_a_site_breaks_no_rule() {
    let world = World::expand(&site(), &rules());
    assert_eq!(world.broken(), Vec::new());
}

#[test]
fn row_places_the_children_along_x_in_the_order_of_the_relations() {
    let world = World::expand(
        &relations(&[
            ("group", "group", None),
            ("b", "endpoint", Some("group")),
            ("a", "endpoint", Some("group")),
        ]),
        &rules(),
    );
    assert_eq!(size(&world, "group"), [3.5, 2.0, 2.0]);
    assert_eq!(centre(&world, "group"), [0.0, 1.0, 0.0]);
    assert_eq!(centre(&world, "b"), [-0.75, 1.0, 0.0]);
    assert_eq!(centre(&world, "a"), [0.75, 1.0, 0.0]);
}

#[test]
fn stack_places_the_children_along_y_from_the_bottom() {
    let world = World::expand(
        &relations(&[
            ("fw-01", "device", None),
            ("fw-01.in", "function", Some("fw-01")),
            ("fw-01.policy", "function", Some("fw-01")),
            ("fw-01.out", "function", Some("fw-01")),
        ]),
        &rules(),
    );
    assert_eq!(size(&world, "fw-01"), [3.0, 3.5, 2.0]);
    assert_eq!(centre(&world, "fw-01.in"), [0.0, 0.75, 0.0]);
    assert_eq!(centre(&world, "fw-01.policy"), [0.0, 1.75, 0.0]);
    assert_eq!(centre(&world, "fw-01.out"), [0.0, 2.75, 0.0]);
}

#[test]
fn grid_of_5_children_has_3_columns_and_2_rows() {
    let world = World::expand(
        &relations(&[
            ("site-a", "site", None),
            ("e1", "endpoint", Some("site-a")),
            ("e2", "endpoint", Some("site-a")),
            ("e3", "endpoint", Some("site-a")),
            ("e4", "endpoint", Some("site-a")),
            ("e5", "endpoint", Some("site-a")),
        ]),
        &rules(),
    );
    assert_eq!(size(&world, "site-a"), [5.0, 2.0, 3.5]);
    assert_eq!(centre(&world, "e1"), [-1.5, 1.0, -0.75]);
    assert_eq!(centre(&world, "e3"), [1.5, 1.0, -0.75]);
    assert_eq!(centre(&world, "e4"), [-0.75, 1.0, 0.75]);
    assert_eq!(centre(&world, "e5"), [0.75, 1.0, 0.75]);
}

#[test]
fn node_with_no_child_has_the_size_of_its_rule() {
    let world = World::expand(&site(), &rules());
    assert_eq!(size(&world, "fw-01.policy"), [2.0, 0.5, 1.0]);
}

#[test]
fn kind_with_no_rule_gets_the_default_rule() {
    let world = World::expand(&site(), &rules());
    assert_eq!(size(&world, "dns"), [1.0, 1.0, 1.0]);
}

#[test]
fn same_relations_and_rule_set_give_the_same_world() {
    let first = World::expand(&site(), &rules());
    let second = World::expand(&site(), &rules());
    assert_eq!(first, second);
    assert_eq!(first.describe(), second.describe());
}

fn other_rules() -> RuleSet {
    let mut other = rules();
    other.kinds.get_mut("site").unwrap().arrangement = Arrangement::Row;
    other.kinds.get_mut("device").unwrap().arrangement = Arrangement::Grid;
    other
}

#[test]
fn another_rule_set_gives_another_world_that_breaks_no_rule() {
    let other = other_rules();
    let world = World::expand(&site(), &other);
    assert_ne!(world, World::expand(&site(), &rules()));
    assert_eq!(world.broken(), Vec::new());
}

#[test]
fn world_text_has_one_line_for_each_node_in_name_order() {
    let world = World::expand(
        &relations(&[
            ("group", "group", None),
            ("b", "endpoint", Some("group")),
            ("a", "endpoint", Some("group")),
        ]),
        &rules(),
    );
    assert_eq!(
        world.describe(),
        "a endpoint in=group at=0.75,1,0 size=1,1,1\n\
         b endpoint in=group at=-0.75,1,0 size=1,1,1\n\
         group group at=0,1,0 size=3.5,2,2\n"
    );
}

#[test]
fn rule_with_a_gap_below_0_gives_siblings_that_intersect() {
    let mut rules = rules();
    rules.default.gap = -0.5;
    let world = World::expand(
        &relations(&[
            ("group", "group", None),
            ("a", "endpoint", Some("group")),
            ("b", "endpoint", Some("group")),
        ]),
        &rules,
    );
    assert_eq!(
        world.broken(),
        vec![Broken::Intersect {
            first: id("a"),
            second: id("b")
        }]
    );
}

#[test]
fn rule_with_a_margin_below_0_gives_a_child_outside_its_parent() {
    let mut rules = rules();
    rules.default.margin = -0.25;
    let world = World::expand(
        &relations(&[("group", "group", None), ("a", "endpoint", Some("group"))]),
        &rules,
    );
    assert_eq!(
        world.broken(),
        vec![Broken::Outside {
            child: id("a"),
            parent: id("group")
        }]
    );
}

#[test]
fn node_with_a_name_in_use_or_with_no_parent_node_is_refused() {
    let mut relations = site();
    let before = relations.clone();
    let refusal = relations.add(id("dns"), "endpoint", None).unwrap_err();
    assert_eq!(
        refusal,
        RelationRefusal::Exists {
            id: id("dns"),
            flow: false
        }
    );
    assert_eq!(refusal.to_string(), "node dns exists");

    let refusal = relations
        .add(id("printer"), "endpoint", Some(id("site-b")))
        .unwrap_err();
    assert_eq!(refusal.to_string(), "in: no node site-b");
    assert_eq!(relations, before);
}

fn with_flow(mut relations: Relations, name: &str, path: &[&str]) -> Relations {
    relations
        .add_flow(id(name), path.iter().copied().map(id).collect())
        .unwrap();
    relations
}

fn points(world: &World, name: &str) -> Vec<[f64; 3]> {
    let flow = world.flows().iter().find(|flow| flow.id == id(name));
    flow.unwrap().points.clone()
}

#[test]
fn flow_between_2_siblings_goes_through_the_doors_and_the_corridor_of_the_parent() {
    let relations = relations(&[
        ("group", "group", None),
        ("a", "endpoint", Some("group")),
        ("b", "endpoint", Some("group")),
    ]);
    let world = World::expand(&with_flow(relations, "a-b", &["a", "b"]), &rules());
    assert_eq!(
        points(&world, "a-b"),
        vec![
            [-0.75, 1.0, 0.0],
            [-0.75, 1.5, 0.0],
            [-0.75, 1.75, 0.0],
            [0.75, 1.75, 0.0],
            [0.75, 1.5, 0.0],
            [0.75, 1.0, 0.0],
        ]
    );
}

#[test]
fn flow_between_functions_of_a_stack_goes_through_its_corridor_beside_them() {
    let relations = relations(&[
        ("fw-01", "device", None),
        ("fw-01.in", "function", Some("fw-01")),
        ("fw-01.policy", "function", Some("fw-01")),
        ("fw-01.out", "function", Some("fw-01")),
    ]);
    let world = World::expand(
        &with_flow(relations, "pass", &["fw-01.in", "fw-01.out"]),
        &rules(),
    );
    assert_eq!(
        points(&world, "pass"),
        vec![
            [0.0, 0.75, 0.0],
            [1.0, 0.75, 0.0],
            [1.25, 0.75, 0.0],
            [1.25, 2.75, 0.0],
            [1.0, 2.75, 0.0],
            [0.0, 2.75, 0.0],
        ]
    );
    assert_eq!(world.broken(), Vec::new());
}

#[test]
fn packet_flow_through_the_devices_of_a_site_crosses_no_other_node() {
    let relations = with_flow(
        site(),
        "client-dns",
        &[
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
        ],
    );
    for rules in [rules(), other_rules()] {
        let world = World::expand(&relations, &rules);
        assert_eq!(world.broken(), Vec::new());
        let line = points(&world, "client-dns");
        assert_eq!(line.first(), Some(&centre(&world, "client")));
        assert_eq!(line.last(), Some(&centre(&world, "dns")));
    }
}

#[test]
fn flow_between_devices_starts_and_ends_outside_their_functions() {
    let world = World::expand(
        &with_flow(site(), "fw-core", &["fw-01", "ips-01", "core-01"]),
        &rules(),
    );
    assert_eq!(world.broken(), Vec::new());
    let [x, y, z] = centre(&world, "fw-01");
    let width = size(&world, "fw-01")[0];
    assert_eq!(points(&world, "fw-core")[0], [x + width / 2.0 - 0.25, y, z]);
}

#[test]
fn flow_from_a_site_to_a_node_in_it_and_flow_between_2_sites_cross_no_other_node() {
    let mut relations = site();
    relations.add(id("site-b"), "site", None).unwrap();
    relations
        .add(id("web"), "endpoint", Some(id("site-b")))
        .unwrap();
    let relations = with_flow(relations, "down", &["site-a", "dns"]);
    let relations = with_flow(relations, "across", &["fw-01.out", "web"]);
    let world = World::expand(&relations, &rules());
    assert_eq!(world.broken(), Vec::new());
}

#[test]
fn flow_line_through_a_node_off_its_path_is_a_broken_rule() {
    let mut rules = rules();
    rules.kinds.get_mut("device").unwrap().margin = -1.0;
    let world = World::expand(
        &with_flow(site(), "pass", &["fw-01.in", "fw-01.out"]),
        &rules,
    );
    assert!(world.broken().contains(&Broken::Crosses {
        flow: id("pass"),
        node: id("fw-01.zone")
    }));
}

#[test]
fn world_text_has_one_line_for_each_flow_after_the_nodes() {
    let relations = relations(&[
        ("group", "group", None),
        ("a", "endpoint", Some("group")),
        ("b", "endpoint", Some("group")),
    ]);
    let world = World::expand(&with_flow(relations, "a-b", &["a", "b"]), &rules());
    assert!(world.describe().ends_with(
        "group group at=0,1,0 size=3.5,2,2\n\
         a-b flow path=a,b points=-0.75,1,0;-0.75,1.5,0;-0.75,1.75,0;0.75,1.75,0;0.75,1.5,0;0.75,1,0\n"
    ));
}

#[test]
fn flow_with_a_name_in_use_a_short_path_no_node_or_a_repeated_node_is_refused() {
    let mut relations = with_flow(site(), "f", &["client", "dns"]);
    let before = relations.clone();
    let cases = [
        ("dns", vec!["client", "dns"], "node dns exists"),
        ("f", vec!["client", "dns"], "flow f exists"),
        ("g", vec!["client"], "path: 1 nodes, valid: 2 or more"),
        ("g", vec!["client", "printer"], "path: no node printer"),
        ("g", vec!["client", "dns", "dns"], "path: dns follows dns"),
    ];
    for (name, path, text) in cases {
        let path = path.into_iter().map(id).collect();
        let refusal = relations.add_flow(id(name), path).unwrap_err();
        assert_eq!(refusal.to_string(), text);
    }
    assert_eq!(relations, before);
    let refusal = relations.add(id("f"), "endpoint", None).unwrap_err();
    assert_eq!(
        refusal,
        RelationRefusal::Exists {
            id: id("f"),
            flow: true
        }
    );
    assert_eq!(refusal.to_string(), "flow f exists");
}

#[test]
fn each_segment_of_a_flow_line_is_parallel_to_1_axis() {
    let mut relations = with_flow(
        site(),
        "client-dns",
        &["client", "fw-01.policy", "ips-01.in", "core-01.out", "dns"],
    );
    relations.add(id("site-b"), "site", None).unwrap();
    relations
        .add(id("web"), "endpoint", Some(id("site-b")))
        .unwrap();
    let relations = with_flow(relations, "across", &["fw-01.out", "web"]);
    for rules in [rules(), other_rules()] {
        let world = World::expand(&relations, &rules);
        assert_eq!(world.broken(), Vec::new());
        for flow in world.flows() {
            for pair in flow.points.windows(2) {
                let changed = (0..3)
                    .filter(|&axis| pair[0][axis] != pair[1][axis])
                    .count();
                assert_eq!(changed, 1, "{} {:?}", flow.id, pair);
            }
        }
    }
}

#[test]
fn grid_child_keeps_its_own_size_next_to_a_wide_sibling() {
    let mut rules = rules();
    rules
        .kinds
        .insert("wide".to_owned(), rule(Arrangement::Row, [10.0, 1.0, 1.0]));
    let mut nodes = vec![("site-a", "site", None), ("big", "wide", Some("site-a"))];
    let names = ["e1", "e2", "e3", "e4", "e5", "e6", "e7", "e8"];
    nodes.extend(names.iter().map(|name| (*name, "endpoint", Some("site-a"))));
    let world = World::expand(&relations(&nodes), &rules);
    assert_eq!(world.broken(), Vec::new());
    let [width, _, depth] = size(&world, "site-a");
    assert!(width <= 11.0 + 1e-9, "{width}");
    assert!(depth <= 5.5 + 1e-9, "{depth}");
    assert_eq!(size(&world, "e1"), [1.0, 1.0, 1.0]);
}

/// 3 sources, 2 access points, 1 switch, 1 router and 1 destination, with 3 flows at the root
/// in the arrangement layers.
fn layered() -> (World, RuleSet) {
    let mut rules = rules();
    rules.root = rule(Arrangement::Layers, [0.0; 3]);
    rules.kinds.insert(
        "router".to_owned(),
        Rule {
            shape: Shape::Cylinder,
            ..rule(Arrangement::Stack, [2.0, 1.0, 2.0])
        },
    );
    rules
        .kinds
        .insert("ap".to_owned(), rule(Arrangement::Stack, [1.0, 0.25, 1.0]));
    let mut relations = relations(&[
        ("a", "endpoint", None),
        ("ap-1", "ap", None),
        ("switch", "default", None),
        ("router", "router", None),
        ("dns", "endpoint", None),
        ("b", "endpoint", None),
        ("c", "endpoint", None),
        ("ap-2", "ap", None),
    ]);
    for (name, path) in [
        ("one", &["a", "ap-1", "switch", "router", "dns"][..]),
        ("two", &["b", "router", "dns"][..]),
        ("three", &["c", "ap-2", "switch", "router"][..]),
    ] {
        relations = with_flow(relations, name, path);
    }
    (World::expand(&relations, &rules), rules)
}

#[test]
fn layers_place_each_step_of_a_flow_in_a_column_further_along_x() {
    let (world, _) = layered();
    assert_eq!(world.broken(), Vec::new());
    let x = |name: &str| centre(&world, name)[0];
    for name in ["b", "c"] {
        assert_eq!(x(name), x("a"));
    }
    assert_eq!(x("ap-1"), x("ap-2"));
    for pair in [
        ("a", "ap-1"),
        ("ap-1", "switch"),
        ("switch", "router"),
        ("router", "dns"),
    ] {
        assert!(x(pair.0) < x(pair.1), "{pair:?}");
    }
    assert_ne!(centre(&world, "a")[2], centre(&world, "b")[2]);
}

#[test]
fn flows_in_layers_stay_below_the_top_of_the_lowest_node() {
    let (world, _) = layered();
    assert_eq!(world.broken(), Vec::new());
    let low = centre(&world, "ap-1")[1] + size(&world, "ap-1")[1] / 2.0;
    for flow in world.flows() {
        for point in &flow.points {
            assert!(point[1] < low, "{} {point:?}", flow.id);
        }
        for pair in flow.points.windows(2) {
            let changed = (0..3)
                .filter(|&axis| pair[0][axis] != pair[1][axis])
                .count();
            assert_eq!(changed, 1, "{} {:?}", flow.id, pair);
        }
    }
}

#[test]
fn flow_past_a_column_goes_through_a_free_stretch_of_the_column() {
    let (world, _) = layered();
    let column = |name: &str| {
        let [x, _, _] = centre(&world, name);
        let [width, _, _] = size(&world, name);
        [x - width / 2.0, x + width / 2.0]
    };
    let [left, right] = column("switch");
    let [_, _, z] = centre(&world, "switch");
    let [_, _, depth] = size(&world, "switch");
    let across: Vec<[f64; 3]> = points(&world, "two")
        .windows(2)
        .filter(|pair| pair[0][0].min(pair[1][0]) < left && pair[0][0].max(pair[1][0]) > right)
        .map(|pair| pair[0])
        .collect();
    assert_eq!(across.len(), 1, "{:?}", points(&world, "two"));
    assert!((across[0][2] - z).abs() > depth / 2.0, "{across:?}");
}

#[test]
fn lanes_of_flows_that_join_keep_the_side_from_which_each_comes() {
    let (world, _) = layered();
    let entry = |name: &str| {
        let [x, _, _] = centre(&world, "router");
        let [width, _, _] = size(&world, "router");
        let door = x - width / 2.0;
        points(&world, name)
            .windows(2)
            .find(|pair| pair[0][0] < door && pair[1][0] >= door)
            .map(|pair| pair[0][2])
            .unwrap()
    };
    let side = (centre(&world, "ap-2")[2] - centre(&world, "ap-1")[2]).signum();
    assert_eq!((entry("three") - entry("one")).signum(), side);
}

#[test]
fn lanes_of_a_bundle_fit_the_room_of_its_own_segments() {
    let mut layered = rules();
    layered.root = rule(Arrangement::Layers, [0.0; 3]);
    layered
        .kinds
        .insert("narrow".to_owned(), rule(Arrangement::Row, [1.0, 1.0, 0.2]));
    let mut relations = relations(&[
        ("a", "endpoint", None),
        ("b", "endpoint", None),
        ("c", "narrow", None),
        ("d", "narrow", None),
    ]);
    for (name, path) in [
        ("wide-1", ["a", "b"]),
        ("wide-2", ["a", "b"]),
        ("narrow-1", ["c", "d"]),
        ("narrow-2", ["c", "d"]),
        ("narrow-3", ["c", "d"]),
    ] {
        relations = with_flow(relations, name, &path);
    }
    let world = World::expand(&relations, &layered);
    assert_eq!(world.broken(), Vec::new());
    let spacing = |name: &str| {
        let flow = world.flows().iter().find(|flow| flow.id == id(name));
        flow.unwrap().spacing
    };
    assert!(spacing("narrow-1") < spacing("wide-1"));
    assert_eq!(spacing("wide-1"), spacing("wide-2"));
    let [_, _, z] = centre(&world, "d");
    for name in ["narrow-1", "narrow-2", "narrow-3"] {
        let end = points(&world, name).last().copied().unwrap();
        assert!((end[2] - z).abs() < 0.1, "{name} {end:?}");
    }
}

#[test]
fn lanes_of_any_paths_give_each_path_its_own_line() {
    let (world, _) = layered();
    let path = |names: &[&str]| -> Vec<usize> {
        names
            .iter()
            .map(|name| world.index(&id(name)).unwrap())
            .collect()
    };
    let paths = [path(&["a", "router"]), path(&["a", "router"])];
    let lines = world.lanes(&paths);
    assert_eq!(lines.len(), 2);
    assert!(!common_stretch(&lines[0].points, &lines[1].points));
    assert_eq!(lines[0].spacing, lines[1].spacing);
}

#[test]
fn shape_of_a_node_comes_from_the_rule_of_its_kind() {
    let (world, _) = layered();
    let shape = |name: &str| world.shape(world.index(&id(name)).unwrap());
    assert_eq!(shape("router"), Shape::Cylinder);
    assert_eq!(shape("switch"), Shape::Box);
}

#[test]
fn default_style_gives_each_device_kind_its_shape() {
    let kinds = [
        "device",
        "router",
        "switch",
        "firewall",
        "access-point",
        "workstation",
        "network",
        "endpoint",
        "room",
        "l3-switch",
        "server",
        "laptop",
        "load-balancer",
    ];
    let mut relations = Relations::new();
    for kind in kinds {
        relations
            .add(id(&format!("{kind}-01")), kind, None)
            .unwrap();
    }
    let world = World::expand(&relations, &Style::default().rules);
    let shapes = kinds.map(|kind| {
        world
            .shape(world.index(&id(&format!("{kind}-01"))).unwrap())
            .name()
            .unwrap()
    });
    assert_eq!(
        shapes,
        [
            "device",
            "router",
            "switch",
            "firewall",
            "access-point",
            "workstation",
            "cloud",
            "workstation",
            "box",
            "layer3-switch",
            "server",
            "laptop",
            "load-balancer"
        ]
    );
    assert_eq!(world.broken(), Vec::new());
}

#[test]
fn flow_leaves_a_router_through_its_side_toward_the_next_node() {
    let mut rules = rules();
    rules.kinds.insert(
        "router".to_owned(),
        Rule {
            shape: Shape::Router,
            ..rule(Arrangement::Row, [1.6, 0.4, 1.6])
        },
    );
    let relations = relations(&[("client", "endpoint", None), ("r1", "router", None)]);
    let world = World::expand(&with_flow(relations, "f", &["r1", "client"]), &rules);
    let [x, y, z] = centre(&world, "r1");
    let [width, height, _] = size(&world, "r1");
    assert!(centre(&world, "client")[0] < x);
    let door = x - width / 2.0 - rules.root.gap / 4.0;
    assert_eq!(
        points(&world, "f")[..3],
        [[x, y, z], [door, y, z], [door, y + height / 2.0, z]]
    );
    assert_eq!(world.broken(), Vec::new());
}

/// `true` when a segment of `first` and a segment of `second` lie on 1 line along 1 axis with a
/// common stretch.
fn common_stretch(first: &[[f64; 3]], second: &[[f64; 3]]) -> bool {
    let along = |pair: &[[f64; 3]]| (0..3).find(|&axis| pair[0][axis] != pair[1][axis]);
    first.windows(2).any(|one| {
        second.windows(2).any(|other| {
            let Some(axis) = along(one) else {
                return false;
            };
            if along(other) != Some(axis) {
                return false;
            }
            let level = (0..3)
                .filter(|&other_axis| other_axis != axis)
                .all(|other_axis| (one[0][other_axis] - other[0][other_axis]).abs() < 1e-9);
            let low = one[0][axis]
                .min(one[1][axis])
                .max(other[0][axis].min(other[1][axis]));
            let high = one[0][axis]
                .max(one[1][axis])
                .min(other[0][axis].max(other[1][axis]));
            level && low + 1e-9 < high
        })
    })
}

#[test]
fn flows_over_the_same_nodes_have_lines_in_distinct_lanes_with_no_common_stretch() {
    let mut layered = rules();
    layered.root = rule(Arrangement::Layers, [0.0; 3]);
    let worlds = [
        (
            relations(&[
                ("group", "group", None),
                ("a", "endpoint", Some("group")),
                ("b", "endpoint", Some("group")),
            ]),
            rules(),
            ["b", "a"],
        ),
        (
            relations(&[("a", "endpoint", None), ("b", "endpoint", None)]),
            layered,
            ["a", "b"],
        ),
    ];
    for (relations, rules, third) in worlds {
        let mut relations = relations;
        for (name, path) in [("one", ["a", "b"]), ("two", ["a", "b"]), ("three", third)] {
            relations = with_flow(relations, name, &path);
        }
        let world = World::expand(&relations, &rules);
        assert_eq!(world.broken(), Vec::new());
        for flow in world.flows() {
            assert!(flow.spacing > 0.0, "{}", flow.id);
        }
        for flow in world.flows() {
            for pair in flow.points.windows(2) {
                let changed = (0..3)
                    .filter(|&axis| pair[0][axis] != pair[1][axis])
                    .count();
                assert_eq!(changed, 1, "{} {:?}", flow.id, pair);
            }
        }
        let flows = world.flows();
        for (place, first) in flows.iter().enumerate() {
            for second in &flows[place + 1..] {
                assert!(
                    !common_stretch(&first.points, &second.points),
                    "{} {}",
                    first.id,
                    second.id
                );
            }
        }
    }
}

#[test]
fn flow_with_no_common_stretch_keeps_the_line_through_the_centres() {
    let relations = relations(&[
        ("a", "endpoint", None),
        ("b", "endpoint", None),
        ("c", "endpoint", None),
        ("d", "endpoint", None),
    ]);
    let mut layered = rules();
    layered.root = rule(Arrangement::Layers, [0.0; 3]);
    let relations = with_flow(relations, "first", &["a", "b"]);
    let relations = with_flow(relations, "second", &["c", "d"]);
    let world = World::expand(&relations, &layered);
    for flow in world.flows() {
        assert_eq!(flow.points, world.line(&flow.nodes), "{}", flow.id);
    }
    assert_eq!(world.broken(), Vec::new());
}

/// `true` when a segment of `first` crosses a segment of `second` on 1 plane at a point inside
/// both segments.
fn cross(first: &[[f64; 3]], second: &[[f64; 3]]) -> bool {
    let along = |pair: &[[f64; 3]]| (0..3).find(|&axis| pair[0][axis] != pair[1][axis]);
    let between = |value: f64, pair: &[[f64; 3]], axis: usize| {
        value > pair[0][axis].min(pair[1][axis]) + 1e-9
            && value < pair[0][axis].max(pair[1][axis]) - 1e-9
    };
    first.windows(2).any(|one| {
        second.windows(2).any(|other| {
            let (Some(a), Some(b)) = (along(one), along(other)) else {
                return false;
            };
            a != b
                && (one[0][3 - a - b] - other[0][3 - a - b]).abs() < 1e-9
                && between(other[0][a], one, a)
                && between(one[0][b], other, b)
        })
    })
}

#[test]
fn flows_in_layers_cross_no_other_flow() {
    let mut rules = Style::default().rules;
    rules.root = Rule {
        gap: 1.6,
        margin: 1.0,
        ..rule(Arrangement::Layers, [0.0; 3])
    };
    let mut relations = relations(&[
        ("client-a", "workstation", None),
        ("ap-1", "access-point", None),
        ("switch-1", "switch", None),
        ("router-1", "router", None),
        ("internet", "network", None),
        ("client-b", "workstation", None),
        ("client-c", "workstation", None),
        ("ap-2", "access-point", None),
    ]);
    for (name, path) in [
        (
            "one",
            &["client-a", "ap-1", "switch-1", "router-1", "internet"][..],
        ),
        ("two", &["client-b", "router-1", "internet"][..]),
        ("three", &["client-c", "ap-2", "switch-1", "router-1"][..]),
    ] {
        relations = with_flow(relations, name, path);
    }
    let world = World::expand(&relations, &rules);
    assert_eq!(world.broken(), Vec::new());
    let flows = world.flows();
    for (place, first) in flows.iter().enumerate() {
        for second in &flows[place + 1..] {
            assert!(
                !cross(&first.points, &second.points),
                "{} {}",
                first.id,
                second.id
            );
        }
    }
}

#[test]
fn flow_past_columns_turns_once_before_its_last_column() {
    let (world, _) = layered();
    let line = points(&world, "two");
    let router = centre(&world, "router")[0] - size(&world, "router")[0] / 2.0;
    let turns: Vec<&[[f64; 3]]> = line
        .windows(2)
        .filter(|pair| pair[0][0] < router && pair[0][2] != pair[1][2])
        .collect();
    assert_eq!(turns.len(), 1, "{line:?}");
    let switch = centre(&world, "switch")[0] + size(&world, "switch")[0] / 2.0;
    assert!(turns[0][0][0] > switch, "{line:?}");
}

#[test]
fn each_segment_keeps_the_lane_gap_of_its_own_bundle() {
    let (world, _) = layered();
    for flow in world.flows() {
        assert_eq!(flow.gaps.len(), flow.points.len() - 1, "{}", flow.id);
        let least = flow.gaps.iter().copied().fold(f64::MAX, f64::min);
        assert_eq!(least, flow.spacing, "{}", flow.id);
    }
    let one = world.flows().iter().find(|flow| flow.id == id("one"));
    let gaps = &one.unwrap().gaps;
    assert!(gaps[0] > gaps.iter().copied().fold(f64::MAX, f64::min));
}

#[test]
fn lanes_at_a_node_with_a_top_symbol_stay_outside_its_top_face() {
    let mut relations = relations(&[
        ("site-a", "site", None),
        ("room-1", "room", Some("site-a")),
        ("r1", "router", Some("room-1")),
        ("sw1", "switch", Some("room-1")),
        ("ap1", "access-point", Some("room-1")),
        ("fw1", "firewall", Some("room-1")),
        ("pc1", "endpoint", Some("room-1")),
        ("net", "network", Some("site-a")),
    ]);
    for (name, path) in [
        ("f1", &["pc1", "ap1", "sw1", "r1", "fw1", "net"][..]),
        ("f2", &["pc1", "ap1", "sw1", "r1", "fw1"][..]),
        ("f3", &["pc1", "ap1", "sw1", "r1"][..]),
        ("f4", &["net", "fw1", "r1", "sw1", "ap1", "pc1"][..]),
    ] {
        relations = with_flow(relations, name, path);
    }
    let world = World::expand(&relations, &Style::default().rules);
    assert_eq!(world.broken(), Vec::new());
    for flow in world.flows() {
        for &node in &flow.nodes {
            if !world.shape(node).has_top_symbol() {
                continue;
            }
            let (named, placed, _) = world.node(node);
            for point in &flow.points {
                let over = [0, 2].iter().all(|&axis| {
                    (point[axis] - placed.centre[axis]).abs() < placed.size[axis] / 2.0 - 1e-9
                });
                let height = point[1] - placed.centre[1];
                assert!(
                    !over || height <= 1e-9 || height > placed.size[1] / 2.0 + 1e-9,
                    "{} {} {point:?}",
                    flow.id,
                    named.id
                );
            }
        }
    }
}

/// The pairs of flows of `world` with a common stretch outside the nodes with no child on both
/// paths, by name.
fn stretches(world: &World) -> Vec<(String, String)> {
    let flows = world.flows();
    let mut pairs = Vec::new();
    for (place, first) in flows.iter().enumerate() {
        for second in &flows[place + 1..] {
            let hidden: Vec<usize> = first
                .nodes
                .iter()
                .copied()
                .filter(|node| second.nodes.contains(node) && world.children(*node).is_empty())
                .collect();
            let seen = first.points.windows(2).any(|one| {
                second.points.windows(2).any(|other| {
                    common_stretch(one, other) && {
                        let middle = [0, 1, 2].map(|axis| {
                            let low = one[0][axis].min(one[1][axis]);
                            let high = one[0][axis].max(one[1][axis]);
                            let other_low = other[0][axis].min(other[1][axis]);
                            let other_high = other[0][axis].max(other[1][axis]);
                            (low.max(other_low) + high.min(other_high)) / 2.0
                        });
                        !hidden.iter().any(|&node| {
                            let (_, placed, _) = world.node(node);
                            (0..3).all(|axis| {
                                (middle[axis] - placed.centre[axis]).abs() < placed.size[axis] / 2.0
                            })
                        })
                    }
                })
            });
            if seen {
                pairs.push((first.id.to_string(), second.id.to_string()));
            }
        }
    }
    pairs
}

#[test]
fn flows_that_meet_at_an_end_of_a_segment_have_no_common_stretch() {
    let cases = [
        [["a", "b", "b"], ["c", "b", "d"]],
        [["b", "a", "a"], ["c", "a", "d"]],
        [["a", "d", "d"], ["b", "d", "c"]],
        [["c", "d", "d"], ["b", "d", "a"]],
    ];
    for [first, second] in cases {
        let mut relations = relations(&[
            ("room-1", "room", None),
            ("a", "endpoint", Some("room-1")),
            ("b", "endpoint", Some("room-1")),
            ("c", "endpoint", Some("room-1")),
            ("d", "endpoint", Some("room-1")),
        ]);
        relations = with_flow(relations, "f0", &first[..2]);
        relations = with_flow(relations, "f1", &second);
        let world = World::expand(&relations, &Style::default().rules);
        assert_eq!(world.broken(), Vec::new(), "{first:?} {second:?}");
        assert_eq!(stretches(&world), Vec::new(), "{first:?} {second:?}");
    }
}

/// The values of a linear congruential sequence below a bound.
struct Sequence(u64);

impl Sequence {
    fn below(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) % bound as u64) as usize
    }
}

/// A world of the default style from `seed`: 1 or 2 sites, each with 1 or 2 rooms of 2 to 5
/// nodes of the device kinds, 2 functions in a node where the sequence gives 0 below 4, and 1 to
/// 10 flows over 2 to 5 distinct nodes with no child, with the room arrangement layers for an odd
/// seed.
fn generated(seed: u64) -> World {
    let kinds = [
        "router",
        "switch",
        "firewall",
        "access-point",
        "workstation",
        "device",
        "network",
        "endpoint",
    ];
    let mut sequence = Sequence(seed.wrapping_mul(7_919).wrapping_add(13));
    let mut style = Style::default();
    if seed % 2 == 1 {
        style.rules.kinds.get_mut("room").unwrap().arrangement = Arrangement::Layers;
    }
    let mut relations = Relations::new();
    let mut leaves = Vec::new();
    for site in 0..1 + sequence.below(2) {
        let site = format!("site-{site}");
        relations.add(id(&site), "site", None).unwrap();
        for room in 0..1 + sequence.below(2) {
            let room = format!("{site}-room-{room}");
            relations.add(id(&room), "room", Some(id(&site))).unwrap();
            for node in 0..2 + sequence.below(4) {
                let name = format!("{room}-n{node}");
                let kind = kinds[sequence.below(kinds.len())];
                relations.add(id(&name), kind, Some(id(&room))).unwrap();
                if sequence.below(4) == 0 {
                    for function in 0..2 {
                        let function = format!("{name}-f{function}");
                        relations
                            .add(id(&function), "function", Some(id(&name)))
                            .unwrap();
                        leaves.push(function);
                    }
                } else {
                    leaves.push(name);
                }
            }
        }
    }
    for flow in 0..1 + sequence.below(10) {
        let length = 2 + sequence.below(4).min(leaves.len() - 2);
        let mut path: Vec<ObjectId> = Vec::new();
        while path.len() < length {
            let pick = id(&leaves[sequence.below(leaves.len())]);
            if !path.contains(&pick) {
                path.push(pick);
            }
        }
        relations.add_flow(id(&format!("f{flow}")), path).unwrap();
    }
    World::expand(&relations, &style.rules)
}

#[test]
fn flows_of_generated_worlds_break_no_rule_and_have_no_common_stretch() {
    let fixed = [2466, 2922, 3358, 6319, 6586, 7233, 7314, 7758, 7820, 9834];
    for seed in (0..400).chain(fixed) {
        let world = generated(seed);
        assert_eq!(world.broken(), Vec::new(), "seed {seed}");
        for flow in world.flows() {
            for pair in flow.points.windows(2) {
                let changed = (0..3)
                    .filter(|&axis| pair[0][axis] != pair[1][axis])
                    .count();
                assert_eq!(changed, 1, "seed {seed} {} {pair:?}", flow.id);
            }
        }
        assert_eq!(stretches(&world), Vec::new(), "seed {seed}");
    }
}

#[test]
fn rule_with_a_defined_shape_gives_the_node_its_shape_and_a_door_on_its_top() {
    let mut rules = rules();
    rules.kinds.insert(
        "tower".to_owned(),
        Rule {
            shape: Shape::Defined(0),
            ..rule(Arrangement::Row, [1.0, 2.0, 1.0])
        },
    );
    let relations = relations(&[("client", "endpoint", None), ("t1", "tower", None)]);
    let world = World::expand(&with_flow(relations, "f", &["t1", "client"]), &rules);
    let t1 = world.index(&id("t1")).unwrap();
    assert_eq!(world.shape(t1), Shape::Defined(0));
    assert!(!Shape::Defined(0).has_top_symbol());
    assert_eq!(points(&world, "f")[0], centre(&world, "t1"));
    assert_eq!(world.library(), &Library::default());
    assert_eq!(world.broken(), Vec::new());
}

#[test]
fn flow_between_nodes_of_2_adjacent_containers_of_a_row_goes_through_their_facing_sides() {
    let relations = with_flow(
        relations(&[
            ("left", "group", None),
            ("right", "group", None),
            ("tall", "group", Some("right")),
            ("a", "endpoint", Some("left")),
            ("b", "endpoint", Some("tall")),
        ]),
        "a-b",
        &["a", "b"],
    );
    let world = World::expand(&relations, &rules());
    assert_eq!(world.broken(), Vec::new());
    let line = points(&world, "a-b");
    let top = |name: &str| centre(&world, name)[1] + size(&world, name)[1] / 2.0;
    assert!(line.iter().all(|point| point[1] < top("right")), "{line:?}");
    let left = centre(&world, "left")[0] + size(&world, "left")[0] / 2.0;
    let right = centre(&world, "right")[0] - size(&world, "right")[0] / 2.0;
    let between: Vec<_> = line
        .iter()
        .filter(|point| point[0] > left && point[0] < right)
        .collect();
    assert!(
        between
            .iter()
            .all(|point| (point[0] - (left + right) / 2.0).abs() < 1e-9),
        "{line:?}"
    );
}

#[test]
fn flow_between_children_of_a_stack_stays_off_their_front_faces() {
    let relations = with_flow(
        relations(&[
            ("rack", "device", None),
            ("one", "function", Some("rack")),
            ("two", "function", Some("rack")),
            ("three", "function", Some("rack")),
        ]),
        "one-three",
        &["one", "three"],
    );
    let world = World::expand(&relations, &rules());
    let front = centre(&world, "two")[2] + size(&world, "two")[2] / 2.0;
    let face = centre(&world, "two")[0] + size(&world, "two")[0] / 2.0;
    for point in points(&world, "one-three") {
        assert!(point[2] < front, "{point:?}");
        let [_, y, _] = point;
        let low = centre(&world, "two")[1] - size(&world, "two")[1] / 2.0;
        let high = centre(&world, "two")[1] + size(&world, "two")[1] / 2.0;
        if y > low && y < high {
            assert!(point[0] > face, "{point:?}");
        }
    }
}

#[test]
fn flow_line_through_a_node_outside_its_parent_is_a_broken_rule() {
    let mut rules = rules();
    rules.kinds.get_mut("device").unwrap().margin = -1.5;
    rules.kinds.get_mut("site").unwrap().margin = -1.0;
    let mut relations = with_flow(site(), "pass", &["fw-01.in", "fw-01.out"]);
    relations = with_flow(relations, "across", &["core-01.in", "ips-01.out"]);
    relations = with_flow(relations, "out", &["client", "dns"]);
    let world = World::expand(&relations, &rules);
    let crosses = |start: [f64; 3], end: [f64; 3], index: usize| {
        let (_, placed, _) = world.node(index);
        let mut enter: f64 = 0.0;
        let mut leave: f64 = 1.0;
        for axis in 0..3 {
            let low = placed.centre[axis] - placed.size[axis] / 2.0 + 1e-9;
            let high = placed.centre[axis] + placed.size[axis] / 2.0 - 1e-9;
            let step = end[axis] - start[axis];
            if step.abs() < 1e-9 {
                if start[axis] <= low || start[axis] >= high {
                    return false;
                }
                continue;
            }
            let first = (low - start[axis]) / step;
            let second = (high - start[axis]) / step;
            enter = enter.max(first.min(second));
            leave = leave.min(first.max(second));
            if enter >= leave {
                return false;
            }
        }
        true
    };
    let mut expected = Vec::new();
    for flow in world.flows() {
        let mut allowed = vec![false; world.len()];
        for &node in &flow.nodes {
            let mut current = Some(node);
            while let Some(index) = current {
                allowed[index] = true;
                current = world.parent(index);
            }
        }
        for (index, &on_path) in allowed.iter().enumerate() {
            let crossed = flow
                .points
                .windows(2)
                .any(|pair| crosses(pair[0], pair[1], index));
            if !on_path && crossed {
                expected.push(Broken::Crosses {
                    flow: flow.id.clone(),
                    node: world.node(index).0.id.clone(),
                });
            }
        }
    }
    let broken = world.broken();
    assert!(broken.contains(&Broken::Outside {
        child: id("fw-01.zone"),
        parent: id("fw-01")
    }));
    let found: Vec<Broken> = broken
        .into_iter()
        .filter(|broken| matches!(broken, Broken::Crosses { .. }))
        .collect();
    assert!(!expected.is_empty());
    assert_eq!(found, expected);
}
