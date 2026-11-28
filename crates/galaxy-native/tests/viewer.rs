use galaxy_native::viewer::{Reply, Viewer};
use galaxy_render::{Camera, Renderer, Selection};
use galaxy_scene::{Framing, Json, Load, ObjectId};

fn call(viewer: &mut Viewer, name: &str, arguments: &str) -> Reply {
    let reply = viewer.call(name, &Json::parse(arguments).unwrap(), None, None);
    assert!(!reply.refused, "{}", reply.text);
    reply
}

fn select(viewer: &mut Viewer, name: &str) -> Vec<(String, String)> {
    viewer.selection = Some(Selection::Node(ObjectId::new(name).unwrap()));
    viewer.details()
}

fn select_flow(viewer: &mut Viewer, name: &str) -> Vec<(String, String)> {
    viewer.selection = Some(Selection::Flow(ObjectId::new(name).unwrap()));
    viewer.details()
}

/// A viewer with the site `site-a` and the devices `n00` to `n<count - 1>` in it.
fn devices(count: usize) -> Viewer {
    let mut viewer = Viewer::default();
    call(
        &mut viewer,
        "add_node",
        r#"{"name":"site-a","kind":"site"}"#,
    );
    for index in 0..count {
        call(
            &mut viewer,
            "add_node",
            &format!(r#"{{"name":"n{index:02}","kind":"device","in":"site-a"}}"#),
        );
    }
    viewer
}

/// A flow `f` along the devices `n00` to `n<count - 1>` with the field `step` on each hop.
fn long_flow(viewer: &mut Viewer, count: usize) {
    let path: Vec<String> = (0..count)
        .map(|index| format!(r#""n{index:02}""#))
        .collect();
    let hops: Vec<String> = (0..count)
        .map(|index| format!(r#"{{"fields":{{"step":"s{index}"}}}}"#))
        .collect();
    call(
        viewer,
        "add_flow",
        &format!(
            r#"{{"name":"f","path":[{}],"hops":[{}]}}"#,
            path.join(","),
            hops.join(",")
        ),
    );
}

fn line(name: &str, value: &str) -> (String, String) {
    (name.to_owned(), value.to_owned())
}

#[test]
fn panel_of_a_node_has_its_label_after_its_kind() {
    let mut viewer = Viewer::default();
    call(
        &mut viewer,
        "add_node",
        r#"{"name":"site-a","kind":"site"}"#,
    );
    call(
        &mut viewer,
        "add_node",
        r#"{"name":"core-01","kind":"router","in":"site-a","label":"Core 01"}"#,
    );
    assert_eq!(
        select(&mut viewer, "core-01"),
        vec![
            line("", "core-01"),
            line("kind", "router"),
            line("label", "Core 01"),
            line("in", "site-a"),
        ]
    );
    assert_eq!(
        select(&mut viewer, "site-a"),
        vec![
            line("", "site-a"),
            line("kind", "site"),
            line("label", "site-a")
        ]
    );
}

#[test]
fn panel_of_a_kind_with_no_label_shows_only_an_own_label() {
    let mut viewer = Viewer::default();
    call(
        &mut viewer,
        "add_node",
        r#"{"name":"site-a","kind":"site"}"#,
    );
    call(
        &mut viewer,
        "add_node",
        r#"{"name":"core-01","kind":"router","in":"site-a","label":"Core 01"}"#,
    );
    call(
        &mut viewer,
        "set_rule",
        r#"{"kind":"site","arrangement":"grid","label":false}"#,
    );
    call(
        &mut viewer,
        "set_rule",
        r#"{"kind":"router","arrangement":"row","label":false}"#,
    );
    assert_eq!(
        select(&mut viewer, "site-a"),
        vec![line("", "site-a"), line("kind", "site")]
    );
    assert_eq!(select(&mut viewer, "core-01")[2], line("label", "Core 01"));
}

#[test]
fn reply_of_add_node_holds_its_label() {
    let mut viewer = Viewer::default();
    call(
        &mut viewer,
        "add_node",
        r#"{"name":"site-a","kind":"site"}"#,
    );
    let reply = call(
        &mut viewer,
        "add_node",
        r#"{"name":"core-01","kind":"router","in":"site-a","label":"Core \"01\""}"#,
    );
    assert_eq!(
        reply.text,
        r#"core-01 router in=site-a label="Core \"01\"""#
    );
}

#[test]
fn text_of_frame_has_the_count_line_and_at_most_256_hidden_lines() {
    let renderer = match Renderer::new() {
        Ok(renderer) => renderer,
        Err(error) => {
            eprintln!("render test not run: {error}");
            return;
        }
    };
    let mut viewer = Viewer::default();
    call(
        &mut viewer,
        "add_node",
        r#"{"name":"site-a","kind":"site"}"#,
    );
    call(
        &mut viewer,
        "set_rule",
        r#"{"kind":"device","arrangement":"row","size":[4,4,4]}"#,
    );
    let label = "x".repeat(100);
    for index in 0..400 {
        call(
            &mut viewer,
            "add_node",
            &format!(
                r#"{{"name":"device-{index:03}","kind":"device","in":"site-a","label":"{label}"}}"#
            ),
        );
    }
    let arguments = Json::parse(r#"{"width":2048,"height":1280}"#).unwrap();
    let reply = viewer.call("frame", &arguments, Some(&renderer), None);
    assert!(!reply.refused, "{}", reply.text);
    assert!(reply.png.is_some());
    let lines: Vec<&str> = reply.text.lines().collect();
    assert_eq!(lines[0], "2048x1280");
    let counts = lines[1]
        .strip_prefix("labels shown=")
        .and_then(|rest| rest.split_once(" hidden="))
        .unwrap_or_else(|| panic!("{}", reply.text));
    let shown: usize = counts.0.parse().unwrap();
    let hidden: usize = counts.1.parse().unwrap();
    assert!(shown > 0, "{}", reply.text);
    assert!(hidden > 256, "{}", reply.text);
    let listed: Vec<&&str> = lines[2..]
        .iter()
        .filter(|line| line.starts_with("label "))
        .collect();
    assert_eq!(listed.len(), 256);
    assert!(
        listed
            .iter()
            .all(|line| line.contains(" hidden overlap=device-")),
        "{}",
        reply.text
    );
    assert_eq!(lines.len(), 2 + 256 + 1);
    assert_eq!(
        lines[lines.len() - 1],
        format!("labels unlisted={}", hidden - 256)
    );
}

#[test]
fn panel_of_a_flow_with_a_count_unit_has_its_magnitude_and_the_window() {
    let document = r#"{"format":"galaxy-relations","version":"0.1",
        "source":{"name":"example","snapshot":"snap-1","query":"q","window":{"from":"2026-11-23T12:00:00Z","to":"2026-11-23T12:05:00Z"}},
        "nodes":[{"id":"a","kind":"device"},{"id":"b","kind":"device"}],
        "flows":[
            {"id":"f","path":["a","b"],"outcome":{"kind":"deliver","reason":"resolved"},"magnitude":{"value":1200000000,"unit":"byte"},"fields":{"protocol":"udp"}},
            {"id":"g","path":["a","b"]}
        ]}"#;
    let mut viewer = Viewer::default();
    let load = Load::read("saved", "q", document.as_bytes(), Framing::Text).unwrap();
    assert!(!viewer.apply_load(load).refused);
    assert_eq!(
        select_flow(&mut viewer, "f"),
        vec![
            line("", "f"),
            line("kind", "flow"),
            line("outcome", "deliver"),
            line("reason", "resolved"),
            line("magnitude", "1200000000 byte"),
            line("window from", "2026-11-23T12:00:00Z"),
            line("window to", "2026-11-23T12:05:00Z"),
            line("protocol", "udp"),
            line("path", "a"),
            line("    ", "b"),
        ]
    );
    assert_eq!(
        select_flow(&mut viewer, "g"),
        vec![
            line("", "g"),
            line("kind", "flow"),
            line("magnitude", "not available"),
            line("path", "a"),
            line("    ", "b"),
        ]
    );
}

#[test]
fn panel_of_a_magnitude_has_the_shortest_text_of_the_double() {
    let mut viewer = devices(2);
    call(
        &mut viewer,
        "add_flow",
        r#"{"name":"f","path":["n00","n01"],"magnitude":{"value":0.30000000000000004,"unit":"bit/s"}}"#,
    );
    assert_eq!(
        select_flow(&mut viewer, "f")[2],
        line("magnitude", "0.30000000000000004 bit/s")
    );
}

#[test]
fn panel_of_a_reply_has_its_request_and_a_request_has_its_replies() {
    let mut viewer = devices(2);
    call(
        &mut viewer,
        "add_flow",
        r#"{"name":"request","path":["n00","n01"]}"#,
    );
    for name in ["reply-1", "reply-2"] {
        call(
            &mut viewer,
            "add_flow",
            &format!(
                r#"{{"name":"{name}","path":["n01","n00"],"outcome":{{"kind":"deliver"}},"reply_to":"request"}}"#
            ),
        );
    }
    assert_eq!(
        select_flow(&mut viewer, "reply-1"),
        vec![
            line("", "reply-1"),
            line("kind", "flow"),
            line("outcome", "deliver"),
            line("reply to", "request"),
            line("path", "n01"),
            line("    ", "n00"),
        ]
    );
    assert_eq!(
        select_flow(&mut viewer, "request"),
        vec![
            line("", "request"),
            line("kind", "flow"),
            line("replies", "reply-1"),
            line("       ", "reply-2"),
            line("path", "n00"),
            line("    ", "n01"),
        ]
    );
}

#[test]
fn panel_of_a_branch_has_its_group_and_the_count_of_the_branches() {
    let mut viewer = devices(3);
    call(
        &mut viewer,
        "add_flow",
        r#"{"name":"b0","path":["n00","n01"],"group":"q1","magnitude":{"value":2,"unit":"packet/s"}}"#,
    );
    call(
        &mut viewer,
        "add_flow",
        r#"{"name":"b1","path":["n00","n02"],"group":"q1","outcome":{"kind":"drop"},"magnitude":{"value":1,"unit":"packet/s"}}"#,
    );
    call(
        &mut viewer,
        "add_flow",
        r#"{"name":"other","path":["n01","n02"],"group":"q2"}"#,
    );
    assert_eq!(
        select_flow(&mut viewer, "b1"),
        vec![
            line("", "b1"),
            line("kind", "flow"),
            line("outcome", "drop"),
            line("magnitude", "1 packet/s"),
            line("group", "q1"),
            line("branches", "2"),
            line("path", "n00"),
            line("    ", "n02"),
        ]
    );
}

#[test]
fn panel_of_a_trunk_has_the_count_the_sum_and_the_flows_with_no_magnitude() {
    let mut viewer = devices(2);
    for (name, magnitude) in [
        ("t0", r#","magnitude":{"value":1500,"unit":"bit/s"}"#),
        ("t1", r#","magnitude":{"value":0.5,"unit":"bit/s"}"#),
        ("t2", ""),
    ] {
        call(
            &mut viewer,
            "add_flow",
            &format!(r#"{{"name":"{name}","path":["n00","n01"]{magnitude}}}"#),
        );
    }
    let ids = ["t0", "t1", "t2"].map(|name| ObjectId::new(name).unwrap());
    viewer.selection = Some(Selection::Trunk(ids.to_vec()));
    assert_eq!(
        viewer.details(),
        vec![
            line("", "trunk"),
            line("flows", "3"),
            line("sum", "1500.5 bit/s"),
            line("no magnitude", "1"),
        ]
    );
    viewer.selection = Some(Selection::Trunk(ids[..2].to_vec()));
    assert_eq!(
        viewer.details(),
        vec![
            line("", "trunk"),
            line("flows", "2"),
            line("sum", "1500.5 bit/s"),
        ]
    );
}

#[test]
fn panel_of_a_path_of_64_nodes_has_each_node_with_the_fields_of_its_hop() {
    let mut viewer = devices(64);
    long_flow(&mut viewer, 64);
    let details = select_flow(&mut viewer, "f");
    let mut expected = vec![line("", "f"), line("kind", "flow")];
    for index in 0..64 {
        let name = if index == 0 { "path" } else { "    " };
        expected.push(line(name, &format!("n{index:02}")));
        expected.push(line("  step", &format!("s{index}")));
    }
    assert_eq!(details, expected);
}

#[test]
fn panel_of_a_path_of_65_nodes_has_the_count_and_the_hop_nearest_to_the_pick() {
    let mut viewer = devices(65);
    long_flow(&mut viewer, 65);
    viewer.selection = Some(Selection::Flow(ObjectId::new("f").unwrap()));
    viewer.hop = 40;
    assert_eq!(
        viewer.details(),
        vec![
            line("", "f"),
            line("kind", "flow"),
            line("hops", "65"),
            line("node", "n40"),
            line("  step", "s40"),
        ]
    );
}

#[test]
fn nearest_hop_is_the_node_of_the_path_at_the_pick() {
    let mut viewer = devices(65);
    long_flow(&mut viewer, 65);
    viewer.selection = Some(Selection::Flow(ObjectId::new("f").unwrap()));
    let look = viewer.model.style().look;
    let camera = Camera::home(&viewer.world, &look, 1600.0 / 1000.0);
    let index = viewer.world.index(&ObjectId::new("n40").unwrap()).unwrap();
    let (_, placed, _) = viewer.world.node(index);
    let pick = camera.project(&look, placed.centre, 1600, 1000).unwrap();
    assert_eq!(viewer.nearest_hop(&camera, 1600, 1000, pick), 40);
}

#[test]
fn time_option_refuses_a_value_that_is_not_a_finite_number_of_0_or_more() {
    for text in ["-1", "NaN", "inf", "soon"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_galaxy-native"))
            .args(["--time", text, "--png", "unused.png"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(&format!("--time: {text}, valid: a number of 0 or more")),
            "{stderr}"
        );
    }
}
