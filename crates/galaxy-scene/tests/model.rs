use galaxy_scene::wire::{self, TOOLS, WireRefusal};
use galaxy_scene::{
    Arrangement, Axis, Change, ChangeRefusal, Colours, Framing, Json, Library, Lines, Load, Look,
    Material, Model, ObjectId, Outcome, OutcomeKind, Part, RuleTarget, Scheme, Shape, Solid, Style,
};

fn id(name: &str) -> ObjectId {
    ObjectId::new(name).unwrap()
}

fn call(name: &str, arguments: &str) -> Result<Change, WireRefusal> {
    wire::decode(name, &Json::parse(arguments).unwrap())
}

fn site_log() -> Vec<Change> {
    [
        ("add_node", r#"{"name":"site-a","kind":"site"}"#),
        ("add_node", r#"{"name":"fw-01","kind":"device","in":"site-a"}"#),
        (
            "add_node",
            r#"{"name":"client","kind":"endpoint","in":"site-a","fields":{"ip":"192.0.2.10","port":53,"up":true}}"#,
        ),
        (
            "add_node",
            r#"{"name":"dns","kind":"endpoint","in":"site-a","fields":{"ip":"192.0.2.53"}}"#,
        ),
        ("add_flow", r#"{"name":"query","path":["client","fw-01","dns"]}"#),
    ]
    .iter()
    .map(|(name, arguments)| call(name, arguments).unwrap())
    .collect()
}

#[test]
fn json_text_gives_the_same_value_after_a_write_and_a_parse() {
    let text = r#"{"a":[1,2.5,-3e2,true,false,null],"b":"line\nquote\" tab\t é 😀","c":{}}"#;
    let value = Json::parse(text).unwrap();
    assert_eq!(Json::parse(&value.to_string()).unwrap(), value);
    assert_eq!(
        value.get("a").unwrap().as_array().unwrap()[2],
        Json::Number(-300.0)
    );
    assert_eq!(
        value.get("b").unwrap().as_str().unwrap(),
        "line\nquote\" tab\t \u{e9} \u{1f600}"
    );
    assert_eq!(Json::Number(3.0).to_string(), "3");
    assert_eq!(Json::Number(0.25).to_string(), "0.25");
}

#[test]
fn json_text_with_an_error_gives_its_byte_offset() {
    for (text, offset) in [
        ("{\"a\":}", 5),
        ("[1,2", 4),
        ("tru", 0),
        ("\"a", 2),
        ("1 2", 2),
    ] {
        let error = Json::parse(text).unwrap_err();
        assert_eq!(error.offset, offset, "{text}");
    }
    let deep = "[".repeat(200);
    assert!(Json::parse(&deep).is_err());
    let error = Json::parse(r#"{"ip":"192.0.2.1","ip":"192.0.2.2"}"#).unwrap_err();
    assert_eq!((error.offset, error.cause), (18, "duplicate member name"));
    for text in [r#""\u+041""#, r#""\u-041""#, r#""\u 041""#] {
        let error = Json::parse(text).unwrap_err();
        assert_eq!(
            (error.offset, error.cause),
            (3, "invalid \\u escape"),
            "{text}"
        );
    }
}

#[test]
fn json_object_with_60000_members_parses_and_refuses_a_duplicate_last_member() {
    let members: Vec<String> = (0..60_000)
        .map(|index| format!("\"m{index}\":{index}"))
        .collect();
    let text = format!("{{{}}}", members.join(","));
    let value = Json::parse(&text).unwrap();
    assert_eq!(value.get("m59999"), Some(&Json::Number(59_999.0)));
    let repeated = format!("{{{},\"m0\":0}}", members.join(","));
    assert_eq!(
        Json::parse(&repeated).unwrap_err().cause,
        "duplicate member name"
    );
}

#[test]
fn wire_refuses_more_than_64_fields_and_a_field_text_above_1024_characters() {
    let members: Vec<String> = (0..65).map(|index| format!("\"f{index}\":\"x\"")).collect();
    let arguments = format!(
        r#"{{"name":"a","kind":"site","fields":{{{}}}}}"#,
        members.join(",")
    );
    assert_eq!(
        call("add_node", &arguments).unwrap_err().to_string(),
        "fields: 65 members, valid: 64 members at most"
    );
    let long = "x".repeat(1025);
    let arguments = format!(r#"{{"name":"a","kind":"site","fields":{{"note":"{long}"}}}}"#);
    assert_eq!(
        call("add_node", &arguments).unwrap_err().to_string(),
        "fields.note: 1025 characters, valid: 1 to 1024 characters"
    );
    let at_most: Vec<String> = (0..64).map(|index| format!("\"f{index}\":\"x\"")).collect();
    let arguments = format!(
        r#"{{"name":"a","kind":"site","fields":{{{}}}}}"#,
        at_most.join(",")
    );
    assert!(call("add_node", &arguments).is_ok());
}

#[test]
fn each_tool_has_a_schema_of_an_object() {
    for tool in TOOLS {
        let schema = Json::parse(tool.schema).unwrap();
        assert_eq!(
            schema.get("type"),
            Some(&Json::string("object")),
            "{}",
            tool.name
        );
    }
}

#[test]
fn schema_of_set_rule_lists_each_shape_name() {
    let tool = TOOLS.iter().find(|tool| tool.name == "set_rule").unwrap();
    let schema = Json::parse(tool.schema).unwrap();
    let names = schema
        .get("properties")
        .and_then(|properties| properties.get("shape"))
        .and_then(|shape| shape.get("anyOf"))
        .and_then(Json::as_array)
        .and_then(|choices| choices[0].get("enum"))
        .and_then(Json::as_array)
        .unwrap()
        .iter()
        .map(|name| name.as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, Shape::NAMES);
}

#[test]
fn shape_name_gives_the_same_shape_after_a_parse() {
    for (index, shape) in Shape::ALL.into_iter().enumerate() {
        assert_eq!(shape.name(), Some(Shape::NAMES[index]));
        assert_eq!(Shape::parse(Shape::NAMES[index]), Some(shape));
    }
    assert_eq!(Shape::parse("Router"), None);
    assert_eq!(Shape::Defined(0).name(), None);
}

#[test]
fn encode_and_decode_of_each_change_give_the_same_change() {
    let mut log = site_log();
    log.push(
        call(
            "set_rule",
            r#"{"kind":"room","arrangement":"grid","size":[2,1,2],"gap":0.25,"margin":0}"#,
        )
        .unwrap(),
    );
    for shape in Shape::NAMES {
        log.push(
            call(
                "set_rule",
                &format!(r#"{{"kind":"{shape}","arrangement":"stack","shape":"{shape}"}}"#),
            )
            .unwrap(),
        );
    }
    for target in ["root", "default"] {
        log.push(
            call(
                "set_rule",
                &format!(r#"{{"kind":"{target}","arrangement":"row","size":[1,2,3]}}"#),
            )
            .unwrap(),
        );
    }
    log.push(call("set_look", r#"{"scheme":"dark","lines":"glow"}"#).unwrap());
    log.push(Change::Clear);
    let model = Model::replay(Style::default(), &log).unwrap();
    assert_eq!(model.log(), log);
    for change in log {
        let (name, arguments) = wire::encode(&change);
        let text = arguments.to_string();
        assert_eq!(call(name, &text).unwrap(), change, "{name} {text}");
    }
}

#[test]
fn flow_with_an_outcome_and_fields_gives_the_same_change_after_encode_and_decode() {
    let change = call(
        "add_flow",
        r#"{"name":"denied","path":["client","fw-01"],"outcome":{"kind":"drop","reason":"zone policy"},"fields":{"protocol":"tcp","port":443}}"#,
    )
    .unwrap();
    let Change::AddFlow {
        outcome, fields, ..
    } = &change
    else {
        panic!("add_flow");
    };
    assert_eq!(
        outcome,
        &Some(Outcome {
            kind: OutcomeKind::Drop,
            reason: Some("zone policy".to_owned()),
        })
    );
    assert_eq!(
        fields,
        &[
            ("protocol".to_owned(), "tcp".to_owned()),
            ("port".to_owned(), "443".to_owned()),
        ]
    );
    let mut log = vec![
        change,
        call(
            "add_flow",
            r#"{"name":"quiet","path":["client","dns"],"outcome":{"kind":"drop","reason":""}}"#,
        )
        .unwrap(),
    ];
    for kind in OutcomeKind::NAMES {
        log.push(
            call(
                "add_flow",
                &format!(
                    r#"{{"name":"{kind}","path":["client","dns"],"outcome":{{"kind":"{kind}"}}}}"#
                ),
            )
            .unwrap(),
        );
    }
    for change in &log {
        let (name, arguments) = wire::encode(change);
        let text = arguments.to_string();
        assert_eq!(&call(name, &text).unwrap(), change, "{name} {text}");
    }

    let mut changes = site_log();
    changes.extend(log);
    let model = Model::replay(Style::default(), &changes).unwrap();
    let world = model.world();
    let text = world.describe();
    let line = text
        .lines()
        .find(|line| line.starts_with("denied "))
        .unwrap();
    assert!(
        line.contains(r#" outcome=drop reason="zone policy" protocol="tcp" port="443" points="#),
        "{line}"
    );
    let quiet = text
        .lines()
        .find(|line| line.starts_with("quiet "))
        .unwrap();
    assert!(quiet.contains(" outcome=drop points="), "{quiet}");
    let flow = world.flows().iter().find(|flow| flow.id == id("exit"));
    assert_eq!(
        flow.unwrap().outcome.as_ref().unwrap().kind,
        OutcomeKind::Exit
    );
}

#[test]
fn schema_of_add_flow_lists_each_outcome_kind() {
    let tool = TOOLS.iter().find(|tool| tool.name == "add_flow").unwrap();
    let schema = Json::parse(tool.schema).unwrap();
    let names = schema
        .get("properties")
        .and_then(|properties| properties.get("outcome"))
        .and_then(|outcome| outcome.get("properties"))
        .and_then(|properties| properties.get("kind"))
        .and_then(|kind| kind.get("enum"))
        .and_then(Json::as_array)
        .unwrap()
        .iter()
        .map(|name| name.as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, OutcomeKind::NAMES);
}

#[test]
fn field_values_are_text() {
    let Change::AddNode { fields, .. } = &site_log()[2] else {
        panic!("add_node");
    };
    let expected = [("ip", "192.0.2.10"), ("port", "53"), ("up", "true")];
    let expected: Vec<(String, String)> = expected
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    assert_eq!(fields, &expected);
}

#[test]
fn wire_refusals_name_the_field_and_the_valid_values() {
    let cases = [
        (
            "paint",
            "{}",
            "no tool paint, valid: add_node, add_flow, set_rule, set_look, define_shape, set_material, clear, load_view",
        ),
        ("add_node", r#"{"kind":"site"}"#, "name: missing"),
        (
            "add_node",
            r#"{"name":"a b","kind":"site"}"#,
            "name: a b, valid: 1 to 64 ASCII letters, digits, -, _ and .",
        ),
        (
            "add_node",
            r#"{"name":"a","kind":3}"#,
            "kind: expected string",
        ),
        (
            "add_flow",
            r#"{"name":"f","path":"a"}"#,
            "path: expected array of strings",
        ),
        (
            "add_flow",
            r#"{"name":"f","path":["a","b"],"outcome":{"kind":"bounce"}}"#,
            "outcome.kind: bounce, valid: deliver, exit, drop, loop, unknown",
        ),
        (
            "add_flow",
            r#"{"name":"f","path":["a","b"],"outcome":{"reason":"zone policy"}}"#,
            "outcome.kind: missing",
        ),
        (
            "add_flow",
            r#"{"name":"f","path":["a","b"],"outcome":"drop"}"#,
            "outcome: expected object",
        ),
        (
            "add_flow",
            r#"{"name":"f","path":["a","b"],"outcome":{"kind":"drop","reason":3}}"#,
            "outcome.reason: expected string",
        ),
        (
            "add_flow",
            r#"{"name":"f","path":["a","b"],"fields":["tcp"]}"#,
            "fields: expected object",
        ),
        (
            "add_node",
            r#"{"name":"a","kind":"site","fields":{"source port":"53"}}"#,
            "fields: source port, valid: 1 to 64 ASCII letters, digits, -, _ and .",
        ),
        (
            "add_node",
            r#"{"name":"a","kind":"site","fields":{"ip":""}}"#,
            "fields.ip: 0 characters, valid: 1 to 1024 characters",
        ),
        (
            "set_rule",
            r#"{"kind":"room","arrangement":"ring"}"#,
            "arrangement: ring, valid: row, stack, grid, layers",
        ),
        (
            "set_rule",
            r#"{"kind":"room","arrangement":"row","size":[1,2]}"#,
            "size: expected array of 3 numbers",
        ),
        (
            "set_rule",
            r#"{"kind":"hub","arrangement":"stack","shape":"hub tower"}"#,
            "shape: hub tower, valid: 1 to 64 ASCII letters, digits, -, _ and .",
        ),
        ("define_shape", r#"{"name":"hub"}"#, "parts: missing"),
        (
            "define_shape",
            r#"{"name":"hub","parts":{}}"#,
            "parts: expected array of objects",
        ),
        (
            "define_shape",
            r#"{"name":"hub","parts":[{"solid":"cone","at":[0,0,0],"size":[1,1,1],"material":"body"}]}"#,
            "parts.0.solid: cone, valid: box, cylinder, prism",
        ),
        (
            "define_shape",
            r#"{"name":"hub","parts":[{"solid":"box","at":[0,0],"size":[1,1,1],"material":"body"}]}"#,
            "parts.0.at: expected array of 3 numbers",
        ),
        (
            "define_shape",
            r#"{"name":"hub","parts":[{"solid":"box","at":[0,0,0],"size":[1,1,1]}]}"#,
            "parts.0.material: missing",
        ),
        (
            "define_shape",
            r#"{"name":"hub","parts":[{"solid":"prism","at":[0,0,0],"size":[1,1,1],"material":"body"}]}"#,
            "parts.0.points: missing",
        ),
        (
            "define_shape",
            r#"{"name":"hub","parts":[{"solid":"box","at":[0,0,0],"size":[1,1,1],"material":"body"},{"solid":"box","at":[0,0,0],"size":[1,1,1],"material":"body","points":[[0,0],[1,0],[0,1]]}]}"#,
            "parts.1.solid: box, valid: prism with points",
        ),
        (
            "define_shape",
            r#"{"name":"hub","parts":[{"solid":"prism","at":[0,0,0],"size":[1,1,1],"material":"body","points":[[0,0],[1,0,2]]}]}"#,
            "parts.0.points: expected array of [u, v] pairs",
        ),
        (
            "define_shape",
            r#"{"name":"hub","parts":[{"solid":"cylinder","at":[0,0,0],"size":[1,1,1],"material":"body","axis":"w"}]}"#,
            "parts.0.axis: w, valid: x, y, z",
        ),
        (
            "define_shape",
            r#"{"name":"hub","parts":[{"solid":"box","at":[0,0,0],"size":[1,1,1],"material":"body","edges":"no"}]}"#,
            "parts.0.edges: expected boolean",
        ),
        (
            "define_shape",
            r#"{"name":"hub","parts":[{"solid":"box","at":[0,0,0],"size":[1,1,1],"material":"body"}],"symbol":[-1]}"#,
            "symbol: expected array of part indexes",
        ),
        (
            "set_material",
            r##"{"name":"chrome","light":"#C0C0C","dark":"#202020"}"##,
            "light: #C0C0C, valid: #RRGGBB",
        ),
        (
            "set_material",
            r##"{"name":"chrome","light":"#C0C0C0"}"##,
            "dark: missing",
        ),
        (
            "set_material",
            r##"{"name":"chrome","light":"#C0C0C0","dark":"#202020","lit":1}"##,
            "lit: expected boolean",
        ),
        (
            "set_look",
            r#"{"scheme":"neon"}"#,
            "scheme: neon, valid: light, dark",
        ),
        (
            "add_node",
            r#"{"name":"","kind":"site"}"#,
            r#"name: "", valid: 1 to 64 ASCII letters, digits, -, _ and ."#,
        ),
        (
            "add_node",
            r#"{"name":"a","kind":""}"#,
            r#"kind: "", valid: 1 or more characters"#,
        ),
        (
            "set_rule",
            r#"{"kind":"room","arrangement":""}"#,
            r#"arrangement: "", valid: row, stack, grid, layers"#,
        ),
    ];
    for (name, arguments, text) in cases {
        assert_eq!(
            call(name, arguments).unwrap_err().to_string(),
            text,
            "{name}"
        );
    }
}

#[test]
fn model_of_the_log_gives_a_world_with_the_source_data_that_breaks_no_rule() {
    let model = Model::replay(Style::default(), &site_log()).unwrap();
    let world = model.world();
    assert_eq!(world.len(), 4);
    assert_eq!(world.broken(), Vec::new());
    let client = world.index(&id("client")).unwrap();
    let (node, _, _) = world.node(client);
    assert_eq!(node.fields[0], ("ip".to_owned(), "192.0.2.10".to_owned()));
    assert!(world.describe().contains(" ip=\"192.0.2.53\"\n"));
}

#[test]
fn refused_change_changes_nothing_and_names_the_cause() {
    let mut model = Model::replay(Style::default(), &site_log()).unwrap();
    let before = model.clone();
    let cases = [
        (
            call("add_node", r#"{"name":"dns","kind":"endpoint"}"#).unwrap(),
            "node dns exists",
        ),
        (
            call("add_flow", r#"{"name":"g","path":["client","web"]}"#).unwrap(),
            "path: no node web",
        ),
        (
            call(
                "set_rule",
                r#"{"kind":"room","arrangement":"row","gap":-1}"#,
            )
            .unwrap(),
            "gap: -1, valid: 0 or more",
        ),
        (
            call(
                "set_rule",
                r#"{"kind":"room","arrangement":"row","size":[1,0,1]}"#,
            )
            .unwrap(),
            "size: 0, valid: above 0",
        ),
        (
            call("add_node", r#"{"name":"r1","kind":"root"}"#).unwrap(),
            "kind: root, valid: a name other than root and default",
        ),
        (
            Change::SetRule {
                target: RuleTarget::Kind("default".to_owned()),
                arrangement: Arrangement::Row,
                shape: None,
                size: [1.0; 3],
                gap: 0.5,
                margin: 0.5,
                label: None,
            },
            "kind: default, valid: a name other than root and default",
        ),
    ];
    for (change, text) in cases {
        let refusal = model.apply(change).unwrap_err();
        assert_eq!(refusal.to_string(), text);
        assert_eq!(model, before);
    }
    assert!(matches!(
        model.apply(Change::SetRule {
            target: RuleTarget::Root,
            arrangement: Arrangement::Row,
            shape: Some("box".to_owned()),
            size: [0.0; 3],
            gap: 1.0,
            margin: f64::NAN,
            label: None,
        }),
        Err(ChangeRefusal::Range {
            field: "margin",
            ..
        })
    ));
}

#[test]
fn add_node_with_a_label_gives_the_node_its_label_and_undo_removes_it() {
    let mut model = Model::replay(Style::default(), &site_log()).unwrap();
    let before = model.clone();
    let change = call(
        "add_node",
        r#"{"name":"core-01","kind":"router","in":"site-a","label":"Core 01 é"}"#,
    )
    .unwrap();
    let (name, arguments) = wire::encode(&change);
    assert_eq!(
        arguments.to_string(),
        r#"{"name":"core-01","kind":"router","in":"site-a","label":"Core 01 é"}"#
    );
    assert_eq!(call(name, &arguments.to_string()).unwrap(), change);
    model.apply(change.clone()).unwrap();
    let node = model.relations().nodes().last().unwrap();
    assert_eq!(node.label.as_deref(), Some("Core 01 é"));
    let world = model.world();
    let index = world.index(&id("core-01")).unwrap();
    assert_eq!(world.label(index), Some("Core 01 é"));
    assert_eq!(world.label(world.index(&id("dns")).unwrap()), Some("dns"));
    assert_eq!(model.undo(), Some(change));
    assert_eq!(model, before);
}

#[test]
fn refused_label_changes_nothing_and_names_the_valid_values() {
    let long = "é".repeat(257);
    let cases = [
        (
            r#"{"name":"r1","kind":"router","label":""}"#.to_owned(),
            "label: 0 characters, valid: 1 to 256 characters".to_owned(),
        ),
        (
            format!(r#"{{"name":"r1","kind":"router","label":"{long}"}}"#),
            "label: 257 characters, valid: 1 to 256 characters".to_owned(),
        ),
        (
            r#"{"name":"r1","kind":"router","label":5}"#.to_owned(),
            "label: 5, valid: string".to_owned(),
        ),
    ];
    for (arguments, text) in cases {
        assert_eq!(
            call("add_node", &arguments).unwrap_err().to_string(),
            text,
            "{arguments}"
        );
    }
    assert!(
        call(
            "add_node",
            &format!(
                r#"{{"name":"r1","kind":"router","label":"{}"}}"#,
                "é".repeat(256)
            )
        )
        .is_ok()
    );
    assert_eq!(
        call(
            "set_rule",
            r#"{"kind":"router","arrangement":"row","label":"no"}"#
        )
        .unwrap_err()
        .to_string(),
        r#"label: "no", valid: true, false"#
    );

    let mut model = Model::replay(Style::default(), &site_log()).unwrap();
    let before = model.clone();
    for (label, text) in [
        (
            String::new(),
            "label: 0 characters, valid: 1 to 256 characters".to_owned(),
        ),
        (
            long.clone(),
            "label: 257 characters, valid: 1 to 256 characters".to_owned(),
        ),
    ] {
        let refusal = model
            .apply(Change::AddNode {
                id: id("r1"),
                kind: "router".to_owned(),
                parent: None,
                label: Some(label),
                fields: Vec::new(),
            })
            .unwrap_err();
        assert_eq!(refusal.to_string(), text);
        assert_eq!(model, before);
    }
}

#[test]
fn set_rule_with_label_false_gives_no_label_and_a_rule_with_no_label_keeps_the_value() {
    let mut model = Model::replay(Style::default(), &site_log()).unwrap();
    assert!(model.style().rules.kinds.values().all(|rule| rule.label));
    assert!(model.style().rules.default.label);
    let client = model.world().index(&id("client")).unwrap();
    for (arguments, label, text) in [
        (
            r#"{"kind":"endpoint","arrangement":"row","label":false}"#,
            false,
            None,
        ),
        (
            r#"{"kind":"endpoint","arrangement":"row","size":[2,1,1]}"#,
            false,
            None,
        ),
        (
            r#"{"kind":"endpoint","arrangement":"row","label":true}"#,
            true,
            Some("client"),
        ),
        (
            r#"{"kind":"endpoint","arrangement":"stack"}"#,
            true,
            Some("client"),
        ),
    ] {
        let change = call("set_rule", arguments).unwrap();
        let (name, encoded) = wire::encode(&change);
        assert_eq!(call(name, &encoded.to_string()).unwrap(), change);
        assert_eq!(
            encoded.get("label").is_some(),
            arguments.contains("label"),
            "{arguments}"
        );
        model.apply(change).unwrap();
        assert_eq!(
            model.style().rules.kinds["endpoint"].label,
            label,
            "{arguments}"
        );
        assert_eq!(model.world().label(client), text, "{arguments}");
    }
    model
        .apply(
            call(
                "set_rule",
                r#"{"kind":"default","arrangement":"row","label":false}"#,
            )
            .unwrap(),
        )
        .unwrap();
    model
        .apply(call("set_rule", r#"{"kind":"hub","arrangement":"row"}"#).unwrap())
        .unwrap();
    assert!(!model.style().rules.kinds["hub"].label);
}

#[test]
fn undo_reverts_the_last_change() {
    let mut model = Model::replay(Style::default(), &site_log()).unwrap();
    let before = model.clone();
    let look = Look {
        scheme: Scheme::Dark,
        lines: Lines::Glow,
        ..Look::default()
    };
    model.apply(Change::SetLook { look }).unwrap();
    model.apply(Change::Clear).unwrap();
    assert!(model.relations().is_empty());
    assert_eq!(model.style().look, look);
    assert_eq!(model.undo(), Some(Change::Clear));
    assert_eq!(model.undo(), Some(Change::SetLook { look }));
    assert_eq!(model, before);
}

#[test]
fn set_rule_changes_the_world_of_the_same_relations() {
    let mut model = Model::replay(Style::default(), &site_log()).unwrap();
    let first = model.world();
    model
        .apply(
            call(
                "set_rule",
                r#"{"kind":"site","arrangement":"row","size":[4,1,4],"gap":2,"margin":1}"#,
            )
            .unwrap(),
        )
        .unwrap();
    let second = model.world();
    assert_ne!(first, second);
    assert_eq!(second.broken(), Vec::new());
}

#[test]
fn set_rule_with_no_shape_keeps_the_shape_of_the_kind() {
    let mut model = Model::new(Style::default());
    for (arguments, shape) in [
        (
            r#"{"kind":"switch","arrangement":"stack","size":[2,0.5,2]}"#,
            Shape::Switch,
        ),
        (
            r#"{"kind":"switch","arrangement":"stack","shape":"box"}"#,
            Shape::Box,
        ),
        (r#"{"kind":"switch","arrangement":"row"}"#, Shape::Box),
        (r#"{"kind":"hub","arrangement":"row"}"#, Shape::Box),
    ] {
        model.apply(call("set_rule", arguments).unwrap()).unwrap();
        let kind = Json::parse(arguments).unwrap();
        let kind = kind.get("kind").and_then(Json::as_str).unwrap();
        assert_eq!(model.style().rules.kinds[kind].shape, shape, "{arguments}");
    }
}

fn part(solid: Solid, at: [f64; 3], size: [f64; 3], material: &str) -> Part {
    Part {
        solid,
        at,
        size,
        axis: Axis::Y,
        material: material.to_owned(),
        edges: true,
    }
}

/// A tower of 3 parts: a body, a prism badge on its front of the material badge, and a
/// cylinder lamp along z.
fn tower(name: &str) -> Change {
    let mut lamp = part(Solid::Cylinder, [0.3, 0.4, 0.45], [0.1, 0.1, 0.1], "symbol");
    lamp.axis = Axis::Z;
    lamp.edges = false;
    let mut badge = part(
        Solid::Prism(vec![[-0.5, -0.5], [0.5, -0.5], [0.0, 0.5]]),
        [0.0, 0.2, 0.45],
        [0.4, 0.4, 0.1],
        "badge",
    );
    badge.axis = Axis::Z;
    Change::DefineShape {
        name: name.to_owned(),
        parts: vec![
            part(Solid::Box, [0.0, 0.0, -0.05], [1.0, 1.0, 0.9], "body"),
            badge,
            lamp,
        ],
        symbol: vec![1, 2],
    }
}

fn badge() -> Change {
    Change::SetMaterial {
        name: "badge".to_owned(),
        colours: Colours {
            light: [0x1F, 0x6F, 0xB2],
            dark: [0x5A, 0xA0, 0xE0],
            lit: false,
        },
    }
}

#[test]
fn define_shape_and_set_material_give_the_same_change_after_encode_and_decode() {
    let log = vec![
        badge(),
        call(
            "set_material",
            r##"{"name":"flame-core","light":"#ffd040","dark":"#FFE080"}"##,
        )
        .unwrap(),
        tower("tower"),
        call(
            "define_shape",
            r#"{"name":"rack-unit","parts":[{"solid":"box","at":[0,-0.25,0],"size":[1,0.5,1],"material":"body"},{"solid":"cylinder","at":[0,0.25,0],"size":[0.5,0.5,0.5],"axis":"x","material":"symbol","edges":false}],"symbol":[1]}"#,
        )
        .unwrap(),
    ];
    let model = Model::replay(Style::default(), &log).unwrap();
    assert_eq!(model.log(), log);
    for change in &log {
        let (name, arguments) = wire::encode(change);
        let text = arguments.to_string();
        assert_eq!(&call(name, &text).unwrap(), change, "{name} {text}");
    }
    let Change::SetMaterial { colours, .. } = &log[1] else {
        panic!("set_material");
    };
    assert_eq!(
        colours,
        &Colours {
            light: [0xFF, 0xD0, 0x40],
            dark: [0xFF, 0xE0, 0x80],
            lit: true,
        }
    );
    let (_, arguments) = wire::encode(&log[1]);
    assert_eq!(arguments.get("light"), Some(&Json::string("#FFD040")));
}

#[test]
fn rule_with_a_defined_shape_keeps_its_shape_after_a_redefinition() {
    let mut model = Model::replay(Style::default(), &site_log()).unwrap();
    model.apply(badge()).unwrap();
    model.apply(tower("tower")).unwrap();
    model
        .apply(
            call(
                "set_rule",
                r#"{"kind":"device","arrangement":"stack","shape":"tower"}"#,
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(model.style().rules.kinds["device"].shape, Shape::Defined(0));
    let redefined = call(
        "define_shape",
        r#"{"name":"tower","parts":[{"solid":"cylinder","at":[0,0,0],"size":[1,1,1],"material":"badge"}]}"#,
    )
    .unwrap();
    model.apply(redefined).unwrap();
    let world = model.world();
    let fw = world.index(&id("fw-01")).unwrap();
    assert_eq!(world.shape(fw), Shape::Defined(0));
    let library = world.library();
    assert_eq!(library.shapes.len(), 1);
    assert_eq!(library.shapes[0].parts[0].solid, Solid::Cylinder);
    assert_eq!(library.shapes[0].symbol, Vec::<usize>::new());
    let badge = library.material("badge").unwrap();
    assert_eq!(badge, Material::BUILT_IN.len());
    assert_eq!(
        library.materials[badge].colours.unwrap().light,
        [0x1F, 0x6F, 0xB2]
    );
    assert_eq!(world.broken(), Vec::new());
    assert_eq!(Model::replay(Style::default(), model.log()).unwrap(), model);
}

#[test]
fn world_text_has_the_parts_of_each_defined_shape_and_each_set_material() {
    let mut model = Model::new(Style::default());
    model.apply(badge()).unwrap();
    model.apply(tower("tower")).unwrap();
    let text = model.world().describe();
    assert_eq!(
        text,
        "shape:tower part=0 solid=box at=0,0,-0.05 size=1,1,0.9 axis=y material=body edges=true symbol=false\n\
         shape:tower part=1 solid=prism at=0,0.2,0.45 size=0.4,0.4,0.1 axis=z material=badge edges=true symbol=true points=-0.5,-0.5;0.5,-0.5;0,0.5\n\
         shape:tower part=2 solid=cylinder at=0.3,0.4,0.45 size=0.1,0.1,0.1 axis=z material=symbol edges=false symbol=true\n\
         material:badge light=#1F6FB2 dark=#5AA0E0 lit=false\n"
    );
}

#[test]
fn undo_of_define_shape_and_set_material_gives_the_model_before() {
    let mut model = Model::replay(Style::default(), &site_log()).unwrap();
    let before = model.clone();
    model.apply(badge()).unwrap();
    model.apply(tower("tower")).unwrap();
    let with_tower = model.clone();
    model
        .apply(
            call(
                "set_rule",
                r#"{"kind":"endpoint","arrangement":"row","shape":"tower"}"#,
            )
            .unwrap(),
        )
        .unwrap();
    model.undo();
    assert_eq!(model, with_tower);
    assert_eq!(model.undo(), Some(tower("tower")));
    assert_eq!(model.undo(), Some(badge()));
    assert_eq!(model, before);
    assert_eq!(model.style().library, Library::default());
}

#[test]
fn refused_shape_or_material_changes_nothing_and_names_the_cause_and_the_valid_values() {
    let mut model = Model::replay(Style::default(), &site_log()).unwrap();
    model.apply(badge()).unwrap();
    model.apply(tower("tower")).unwrap();
    let before = model.clone();
    let shape = |parts: &str, symbol: &str| {
        call(
            "define_shape",
            &format!(r#"{{"name":"hub","parts":[{parts}],"symbol":[{symbol}]}}"#),
        )
        .unwrap()
    };
    let cube = r#"{"solid":"box","at":[0,0,0],"size":[1,1,1],"material":"body"}"#;
    let prism = |points: &str| {
        format!(
            r#"{{"solid":"prism","at":[0,0,0],"size":[1,1,1],"material":"body","points":[{points}]}}"#
        )
    };
    let cases = [
        (
            call(
                "set_rule",
                r#"{"kind":"hub","arrangement":"stack","shape":"hub"}"#,
            )
            .unwrap(),
            "shape: hub, valid: box, cylinder, router, switch, firewall, device, access-point, workstation, cloud, layer3-switch, server, laptop, load-balancer, tower",
        ),
        (
            call(
                "define_shape",
                &format!(r#"{{"name":"router","parts":[{cube}]}}"#),
            )
            .unwrap(),
            "name: router, valid: a name other than box, cylinder, router, switch, firewall, device, access-point, workstation, cloud, layer3-switch, server, laptop, load-balancer",
        ),
        (
            call("define_shape", r#"{"name":"hub","parts":[]}"#).unwrap(),
            "parts: 0 parts, valid: 1 to 256 parts",
        ),
        (
            call(
                "define_shape",
                &format!(
                    r#"{{"name":"hub","parts":[{}]}}"#,
                    vec![cube; 257].join(",")
                ),
            )
            .unwrap(),
            "parts: 257 parts, valid: 1 to 256 parts",
        ),
        (
            shape(
                r#"{"solid":"box","at":[0,0,0],"size":[1,0,1],"material":"body"}"#,
                "",
            ),
            "parts.0.size: 0, valid: above 0",
        ),
        (
            shape(
                r#"{"solid":"box","at":[0.25,0,0],"size":[0.6,1,1],"material":"body"}"#,
                "",
            ),
            "parts.0: at=0.25,0,0 size=0.6,1,1, valid: at - size/2 and at + size/2 from -0.5 to 0.5 on each axis",
        ),
        (
            shape(
                r#"{"solid":"box","at":[0,0,0],"size":[1,1,1],"material":"chrome"}"#,
                "",
            ),
            "parts.0.material: chrome, valid: body, symbol, brick, flame, mortar, flame-core, badge",
        ),
        (
            shape(&prism("[0,0],[0.5,0]"), ""),
            "parts.0.points: 2 points, valid: 3 to 64 points",
        ),
        (
            shape(&prism("[0,0],[0.5,0],[0,0.7]"), ""),
            "parts.0.points.2: 0,0.7, valid: -0.5 to 0.5 on each axis",
        ),
        (
            shape(&prism("[-0.5,-0.5],[0.5,0.5],[0.5,-0.5],[-0.5,0.5]"), ""),
            "parts.0.points: edges 0 and 2 touch, valid: a simple polygon",
        ),
        (
            shape(&prism("[-0.5,-0.5],[0.5,-0.5],[0.5,-0.5],[0,0.5]"), ""),
            "parts.0.points: edge 1 of length 0, valid: a simple polygon",
        ),
        (
            shape(&prism("[-0.5,0],[0,0],[0.5,0]"), ""),
            "parts.0.points: edges 0 and 2 touch, valid: a simple polygon",
        ),
        (
            shape(
                &prism("[-0.5,-0.5],[0.5,-0.5],[0.5,0.3],[-0.3,-0.1],[0.4000000000000001,0.25]"),
                "",
            ),
            "parts.0.points: edges 2 and 3 touch, valid: a simple polygon",
        ),
        (
            shape(&format!("{cube},{cube}"), "0,2"),
            "symbol: 2, valid: part indexes 0 to 1",
        ),
        (
            shape(&format!("{cube},{cube}"), "1,0"),
            "symbol: all 2 parts, valid: part indexes that leave out 1 part or more",
        ),
    ];
    for (change, text) in cases {
        let refusal = model.apply(change).unwrap_err();
        assert_eq!(refusal.to_string(), text);
        assert_eq!(model, before);
    }
    let mut model = Model::new(Style::default());
    for index in Material::BUILT_IN.len()..Library::MATERIALS {
        let change = call(
            "set_material",
            &format!(r##"{{"name":"m{index}","light":"#101010","dark":"#202020"}}"##),
        )
        .unwrap();
        model.apply(change).unwrap();
    }
    let before = model.clone();
    let refusal = model
        .apply(
            call(
                "set_material",
                r##"{"name":"m64","light":"#101010","dark":"#202020"}"##,
            )
            .unwrap(),
        )
        .unwrap_err();
    assert_eq!(refusal.to_string(), "materials: 65, valid: 64 at most");
    assert_eq!(model, before);
    model
        .apply(
            call(
                "set_material",
                r##"{"name":"m6","light":"#303030","dark":"#404040"}"##,
            )
            .unwrap(),
        )
        .unwrap();
    assert!(matches!(
        model.apply(Change::SetMaterial {
            name: "a b".to_owned(),
            colours: Colours {
                light: [0; 3],
                dark: [0; 3],
                lit: true,
            },
        }),
        Err(ChangeRefusal::Value { .. })
    ));
}

#[test]
fn rule_of_a_node_is_the_rule_of_its_kind_that_the_agent_set() {
    let mut model = Model::replay(Style::default(), &site_log()).unwrap();
    model
        .apply(
            call(
                "set_rule",
                r#"{"kind":"endpoint","arrangement":"row","size":[0.8,0.6,1.2]}"#,
            )
            .unwrap(),
        )
        .unwrap();
    let world = model.world();
    let client = world.index(&id("client")).unwrap();
    assert_eq!(world.rule(client).size, [0.8, 0.6, 1.2]);
    assert_eq!(world.rule(client).shape, Shape::Workstation);
    let site = world.index(&id("site-a")).unwrap();
    assert_eq!(world.rule(site), &Style::default().rules.kinds["site"]);
}

/// The shape `name` of `count` prism parts of a regular polygon of 64 points.
fn polygons(name: &str, count: usize) -> Change {
    let points: Vec<[f64; 2]> = (0..64)
        .map(|index| {
            let angle = index as f64 * std::f64::consts::TAU / 64.0;
            [0.5 * angle.cos(), 0.5 * angle.sin()]
        })
        .collect();
    Change::DefineShape {
        name: name.to_owned(),
        parts: vec![part(Solid::Prism(points), [0.0; 3], [1.0; 3], "body"); count],
        symbol: Vec::new(),
    }
}

#[test]
fn define_shape_of_a_new_name_past_64_shapes_is_refused_and_a_redefinition_is_applied() {
    let mut model = Model::new(Style::default());
    model.apply(badge()).unwrap();
    for index in 0..Library::SHAPES {
        model.apply(tower(&format!("s{index}"))).unwrap();
    }
    let before = model.clone();
    let refusal = model.apply(tower("s64")).unwrap_err();
    assert_eq!(refusal.to_string(), "shapes: 65, valid: 64 at most");
    assert_eq!(model, before);
    model.apply(polygons("s0", 2)).unwrap();
    assert_eq!(model.style().library.shape("s0"), Some(0));
    assert_eq!(model.style().library.shapes.len(), Library::SHAPES);
}

#[test]
fn define_shape_past_the_vertex_budget_is_refused_and_a_redefinition_counts_its_new_size() {
    let Change::DefineShape { parts, .. } = polygons("one", 1) else {
        unreachable!()
    };
    assert_eq!(parts[0].vertices(), 2 * (12 * 64 - 12) + 36 * 3 * 64);
    let mut model = Model::new(Style::default());
    model.apply(polygons("a", 100)).unwrap();
    let before = model.clone();
    let refusal = model.apply(polygons("b", 20)).unwrap_err();
    assert_eq!(
        refusal.to_string(),
        "vertices: 1010880, valid: 1000000 at most"
    );
    assert_eq!(model, before);
    let refusal = model.apply(polygons("a", 119)).unwrap_err();
    assert_eq!(
        refusal.to_string(),
        "vertices: 1002456, valid: 1000000 at most"
    );
    assert_eq!(model, before);
    model.apply(polygons("a", 80)).unwrap();
    model.apply(polygons("b", 38)).unwrap();
    assert_eq!(model.style().library.vertices(), 118 * 8424);
}

const DOCUMENT: &str = r#"{
  "format": "galaxy-relations",
  "version": "0.1",
  "source": {"name": "example", "snapshot": "snap-1", "query": "q"},
  "nodes": [
    {"id": "site-a", "kind": "site"},
    {"id": "dns", "kind": "server", "in": "site-a"}
  ]
}
"#;

#[test]
fn add_node_after_a_load_keeps_the_load_and_undo_gives_the_model_before() {
    let load = Load::read("saved", "q", DOCUMENT.as_bytes(), Framing::Text).unwrap();
    let mut model = Model::replay(Style::default(), &site_log()).unwrap();
    model.apply(Change::Load(load.clone())).unwrap();
    let loaded = model.clone();
    assert_eq!(model.relations(), load.relations());
    let node = call(
        "add_node",
        r#"{"name":"web","kind":"server","in":"site-a"}"#,
    )
    .unwrap();
    model.apply(node.clone()).unwrap();
    assert_eq!(model.log().len(), site_log().len() + 2);
    assert!(model.world().index(&id("web")).is_some());
    assert_eq!(load.relations().nodes().len(), 2);
    assert_eq!(model.undo(), Some(node));
    assert_eq!(model, loaded);
    assert_eq!(model.undo(), Some(Change::Load(load)));
    assert_eq!(model, Model::replay(Style::default(), &site_log()).unwrap());
}
