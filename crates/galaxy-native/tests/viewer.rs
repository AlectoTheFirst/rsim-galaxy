use galaxy_native::viewer::{Reply, Viewer};
use galaxy_render::{Renderer, Selection};
use galaxy_scene::{Json, ObjectId};

fn call(viewer: &mut Viewer, name: &str, arguments: &str) -> Reply {
    let reply = viewer.call(name, &Json::parse(arguments).unwrap(), None, None);
    assert!(!reply.refused, "{}", reply.text);
    reply
}

fn select(viewer: &mut Viewer, name: &str) -> Vec<(String, String)> {
    viewer.selection = Some(Selection::Node(ObjectId::new(name).unwrap()));
    viewer.details()
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
