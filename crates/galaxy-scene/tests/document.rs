use std::collections::BTreeMap;

use galaxy_scene::document::{self, RECORD_BYTES, REFUSALS};
use galaxy_scene::sha256::{self, Sha256};
use galaxy_scene::wire;
use galaxy_scene::{Change, ChangeRefusal, Code, Framing, Json, Load, LoadRefusal, Model, Style};

const DOCUMENT: &str = r#"{
  "format": "galaxy-relations",
  "version": "0.1",
  "source": {"name": "example", "version": "1.0", "snapshot": "snap-1", "time": "2026-11-23T12:00:00Z", "query": "path client to dns"},
  "nodes": [
    {"id": "site-a", "kind": "site"},
    {"id": "client", "kind": "endpoint", "in": "site-a", "fields": {"ip": "198.51.100.20", "port": 53, "up": true}},
    {"id": "core-01", "kind": "router", "in": "site-a"},
    {"id": "core-01.f1", "kind": "function", "in": "core-01", "fields": {"step": "route_lookup"}},
    {"id": "dns", "kind": "server", "in": "site-a", "fields": {"ip": "192.0.2.53"}}
  ],
  "flows": [
    {"id": "q1.b0", "path": ["client", "core-01.f1", "dns"], "outcome": {"kind": "deliver", "reason": "resolved"}, "fields": {"protocol": "udp"}},
    {"id": "q1.b1", "path": ["client", "dns"], "outcome": {"kind": "drop"}}
  ]
}
"#;

const STREAM: &str = r#"{"header":{"format":"galaxy-relations","version":"0.1","source":{"name":"example","version":"1.0","snapshot":"snap-1","time":"2026-11-23T12:00:00Z","query":"path client to dns"}}}
{"node":{"id":"site-a","kind":"site"}}
{"node":{"id":"client","kind":"endpoint","in":"site-a","fields":{"ip":"198.51.100.20","port":53,"up":true}}}
{"node":{"id":"core-01","kind":"router","in":"site-a"}}
{"node":{"id":"core-01.f1","kind":"function","in":"core-01","fields":{"step":"route_lookup"}}}
{"node":{"id":"dns","kind":"server","in":"site-a","fields":{"ip":"192.0.2.53"}}}
{"flow":{"id":"q1.b0","path":["client","core-01.f1","dns"],"outcome":{"kind":"deliver","reason":"resolved"},"fields":{"protocol":"udp"}}}
{"flow":{"id":"q1.b1","path":["client","dns"],"outcome":{"kind":"drop"}}}
{"end":{"nodes":5,"links":0,"flows":2}}
"#;

const HEADER: &str = r#"{"header":{"format":"galaxy-relations","version":"0.1","source":{"name":"example","snapshot":"snap-1","query":"q"}}}"#;

fn load(bytes: &str) -> Load {
    Load::read("saved", "q", bytes.as_bytes(), Framing::Text).unwrap()
}

fn refusals(bytes: &str) -> Vec<String> {
    refused(bytes).iter().map(ToString::to_string).collect()
}

fn refused(bytes: &str) -> Vec<LoadRefusal> {
    Load::read("saved", "q", bytes.as_bytes(), Framing::Text).unwrap_err()
}

/// A document with `nodes`, `links` and `flows` as the JSON text of their arrays.
fn document(nodes: &str, links: &str, flows: &str) -> String {
    format!(
        r#"{{"format":"galaxy-relations","version":"0.1","source":{{"name":"example","snapshot":"snap-1","query":"q"}},"nodes":[{nodes}],"links":[{links}],"flows":[{flows}]}}"#
    )
}

/// A stream of the header and `lines`.
fn stream(lines: &[&str]) -> String {
    let mut text = format!("{HEADER}\n");
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    text
}

#[test]
fn sha256_gives_the_digests_of_the_fips_180_4_examples() {
    for (message, digest) in [
        (
            "",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            "abc",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
        (
            "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        ),
        (
            "abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu",
            "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1",
        ),
    ] {
        assert_eq!(sha256::hex(message.as_bytes()), digest, "{message}");
    }
    let mut hash = Sha256::new();
    for _ in 0..1000 {
        hash.update(&[b'a'; 1000]);
    }
    let digest: String = hash
        .finish()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(
        digest,
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
}

#[test]
fn document_and_stream_of_the_same_records_give_the_same_world() {
    let from_document = load(DOCUMENT);
    let from_stream = load(STREAM);
    assert_eq!(from_document.relations(), from_stream.relations());
    assert_eq!(from_document.answer(), from_stream.answer());
    assert_ne!(from_document.hash(), from_stream.hash());
    let world = |load: &Load| {
        let mut model = Model::new(Style::default());
        model.apply(Change::Load(load.clone())).unwrap();
        model.world().describe()
    };
    let text = world(&from_document);
    assert_eq!(text, world(&from_stream));
    assert!(text.contains(r#"client endpoint in=site-a"#), "{text}");
    assert!(
        text.contains(r#"ip="198.51.100.20" port="53" up="true""#),
        "{text}"
    );
    assert!(
        text.contains("q1.b0 flow path=client,core-01.f1,dns outcome=deliver reason=\"resolved\" protocol=\"udp\""),
        "{text}"
    );
}

#[test]
fn answer_holds_the_source_the_counts_the_kinds_and_no_record_value() {
    let answer = load(DOCUMENT).answer().to_json().to_string();
    assert_eq!(
        answer,
        r#"{"source":{"name":"example","version":"1.0","snapshot":"snap-1","time":"2026-11-23T12:00:00Z"},"nodes":5,"links":0,"flows":2,"kinds":[{"kind":"endpoint","nodes":1,"fields":["ip","port","up"]},{"kind":"function","nodes":1,"fields":["step"]},{"kind":"router","nodes":1,"fields":[]},{"kind":"server","nodes":1,"fields":["ip"]},{"kind":"site","nodes":1,"fields":[]}],"partial":[],"ignored":[],"not_shown":[]}"#
    );
}

#[test]
fn answer_names_the_ignored_members_and_lines_and_the_members_not_shown() {
    let text = stream(&[
        r#"{"node":{"id":"a","kind":"laptop","label":"A","handle":"198.51.100.7","priority":1,"fields":{"serial":{"missing":"not_available","reason":"not_read","note":"x"}}}}"#,
        r#"{"node":{"id":"b","kind":"l3-switch"}}"#,
        r#"{"rule":{"kind":"router"}}"#,
        r#"{"link":{"id":"cable-1","ends":["a","b"],"kind":"cable"}}"#,
        r#"{"flow":{"id":"f","path":["a","b"],"hops":[{},{"fields":{"result":"forward"}}],"magnitude":{"value":2000,"unit":"bit/s"},"group":"g","handle":"q1/branch/0","outcome":{"kind":"exit","score":3}}}"#,
        r#"{"flow":{"id":"r","path":["b","a"],"reply_to":"f","fields":{"port":{"missing":"not_known"}}}}"#,
        r#"{"end":{"nodes":2,"links":1,"flows":2,"partial":[{"part":"replies","reason":"limit"}]}}"#,
    ]);
    let loaded = load(&text);
    let answer = loaded.answer();
    assert_eq!(
        answer.ignored,
        [
            "line rule",
            "node.fields.missing.note",
            "node.priority",
            "flow.outcome.score"
        ]
        .map(str::to_owned)
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
    );
    assert_eq!(
        answer.not_shown,
        [
            "node.fields.missing",
            "node.handle",
            "link",
            "flow.fields.missing",
            "flow.hops",
            "flow.magnitude",
            "flow.group",
            "flow.reply_to",
            "flow.handle",
        ]
    );
    assert_eq!(answer.unit.as_deref(), Some("bit/s"));
    assert_eq!(answer.partial, [["replies".to_owned(), "limit".to_owned()]]);
    assert_eq!((answer.nodes, answer.links, answer.flows), (2, 1, 2));
    let kinds: Vec<(&str, usize, &[String])> = answer
        .kinds
        .iter()
        .map(|kind| (kind.kind.as_str(), kind.nodes, kind.fields.as_slice()))
        .collect();
    assert_eq!(
        kinds,
        [
            ("l3-switch", 1, &[][..]),
            ("laptop", 1, &["serial".to_owned()][..])
        ]
    );
    assert_eq!(loaded.relations().nodes()[0].fields, Vec::new());
    let world = {
        let mut model = Model::new(Style::default());
        model.apply(Change::Load(loaded.clone())).unwrap();
        model.world()
    };
    let shape = |name: &str| {
        let index = world
            .index(&galaxy_scene::ObjectId::new(name).unwrap())
            .unwrap();
        world.shape(index).name().unwrap()
    };
    assert_eq!((shape("a"), shape("b")), ("laptop", "layer3-switch"));
}

#[test]
fn a_node_label_of_a_document_is_the_label_of_the_node_and_not_in_not_shown() {
    let text = document(
        r#"{"id":"site-a","kind":"site","label":"Site A"},{"id":"r1","kind":"router","in":"site-a"}"#,
        "",
        "",
    );
    let loaded = load(&text);
    let labels: Vec<(&str, Option<&str>)> = loaded
        .relations()
        .nodes()
        .iter()
        .map(|node| (node.id.as_str(), node.label.as_deref()))
        .collect();
    assert_eq!(labels, [("site-a", Some("Site A")), ("r1", None)]);
    assert!(loaded.answer().not_shown.is_empty());
    let mut model = Model::new(Style::default());
    model.apply(Change::Load(loaded)).unwrap();
    let world = model.world();
    assert_eq!(world.label(0), Some("Site A"));
    assert_eq!(world.label(1), Some("r1"));
}

#[test]
fn each_check_of_the_reader_gives_its_refusal_text() {
    let node = |text: &str| document(text, "", "");
    let cases: Vec<(String, &str)> = vec![
        (
            "{\"format\":".to_owned(),
            "document byte 10: json, valid: JSON of RFC 8259 in UTF-8",
        ),
        (
            "{\"a\":1,\"a\":2}".to_owned(),
            "document byte 7: json, valid: JSON of RFC 8259 in UTF-8",
        ),
        (
            DOCUMENT.replace("galaxy-relations", "other"),
            "document format: format, valid: galaxy-relations",
        ),
        (
            DOCUMENT.replace("\"0.1\"", "\"1.0\""),
            "document version: version, valid: 0.1",
        ),
        (node(r#"{"id":"a"}"#), "nodes[0] kind: missing, valid: id, kind"),
        (node(r#"{"id":"a","kind":7}"#), "nodes[0] kind: type, valid: string"),
        (
            node(r#"{"id":"a","kind":null}"#),
            "nodes[0] kind: type, valid: string",
        ),
        (
            node(r#"{"id":"a","kind":"root"}"#),
            "nodes[0] kind: value, valid: 1 to 64 ASCII letters, digits, -, _ and ., not root, not default",
        ),
        (
            node(&format!(r#"{{"id":"a","kind":"site","label":"{}"}}"#, "x".repeat(257))),
            "nodes[0] label: size, valid: 1 to 256 characters",
        ),
        (
            node(r#"{"id":"a","kind":"site"},{"id":"a","kind":"site"}"#),
            "nodes[1] id: exists, valid: an id of no record before",
        ),
        (
            node(r#"{"id":"a","kind":"site","in":"b"},{"id":"b","kind":"site"}"#),
            "nodes[0] in: no_node, valid: the id of a node before the record",
        ),
        (
            document(
                r#"{"id":"a","kind":"site"},{"id":"b","kind":"site"}"#,
                "",
                r#"{"id":"r","path":["a","b"],"reply_to":"x"}"#,
            ),
            "flows[0] reply_to: no_flow, valid: the id of a flow before the record with no reply_to",
        ),
        (
            document(r#"{"id":"a","kind":"site"}"#, "", r#"{"id":"f","path":["a"]}"#),
            "flows[0] path: short_path, valid: 2 or more",
        ),
        (
            document(
                r#"{"id":"a","kind":"site"},{"id":"b","kind":"site"}"#,
                "",
                r#"{"id":"f","path":["a","b","b"]}"#,
            ),
            "flows[0] path[2]: repeat, valid: another node",
        ),
        (
            document(
                r#"{"id":"a","kind":"site"},{"id":"b","kind":"site"}"#,
                "",
                r#"{"id":"f","path":["a","b"],"magnitude":{"value":1,"unit":"bit/s"}},{"id":"g","path":["a","b"],"magnitude":{"value":1,"unit":"packet/s"}}"#,
            ),
            "flows[1] magnitude.unit: unit, valid: bit/s",
        ),
        (
            document(
                r#"{"id":"a","kind":"site"},{"id":"b","kind":"site"}"#,
                "",
                r#"{"id":"f","path":["a","b"],"magnitude":{"value":1,"unit":"packet"}}"#,
            ),
            "document source.window: window, valid: from and to, from not after to",
        ),
        (
            DOCUMENT.replace(
                r#""query": "path client to dns""#,
                r#""query": "path client to dns", "window": {"from": "2026-11-23T12:00:01Z", "to": "2026-11-23T12:00:00.5Z"}"#,
            ),
            "document source.window: window, valid: from and to, from not after to",
        ),
        (
            "{\"node\":{\"id\":\"a\",\"kind\":\"site\"}}\n{\"end\":{\"nodes\":1,\"links\":0,\"flows\":0}}\n".to_owned(),
            "line 1 node: header, valid: header",
        ),
        (
            stream(&[r#"{"node":{"id":"a","kind":"site"},"link":{}}"#]),
            "line 2 text: line, valid: header, node, link, flow, next, abort, end",
        ),
        (
            stream(&[r#"{"next":{"cursor":"page-2"}}"#, r#"{"end":{"nodes":0,"links":0,"flows":0}}"#]),
            "line 2 next: line, valid: next as the last line of a page",
        ),
        (
            format!(
                "\u{feff}{}",
                stream(&[r#"{"end":{"nodes":0,"links":0,"flows":0}}"#])
            ),
            "line 1 byte 0: json, valid: JSON of RFC 8259 in UTF-8",
        ),
        (
            "{\"node\":{\"id\":\"a\",\"kind\":\"site\"}}".to_owned(),
            "line 1 text: line, valid: LF at the end of the line",
        ),
        (
            "{\"node\":{\"id\":\"a\",\"kind\":\"site\"}}\n".to_owned(),
            "line 1 node: header, valid: header",
        ),
        (
            stream(&[r#"{"node":{"id":"a","kind":"site"}}"#]),
            "line 2 node: no_end, valid: end as the last line",
        ),
        (
            stream(&[r#"{"abort":{"reason":"source timeout"}}"#]),
            "line 2 abort: abort, valid: source timeout",
        ),
        (
            stream(&[r#"{"node":{"id":"a","kind":"site"}}"#, r#"{"end":{"nodes":2,"links":0,"flows":0}}"#]),
            "line 3 end: count, valid: nodes 1, links 0, flows 0",
        ),
        (
            stream(&["{\"node\":{"]),
            "line 2 byte 9: json, valid: JSON of RFC 8259 in UTF-8",
        ),
    ];
    for (text, expected) in cases {
        let refusals = refusals(&text);
        assert_eq!(
            refusals.first().map(String::as_str),
            Some(expected),
            "{text}"
        );
    }
    let codes: Vec<&str> = Code::ALL.iter().map(|code| code.name()).collect();
    assert_eq!(&codes[..3], ["json", "format", "version"]);
}

#[test]
fn checks_cover_the_limits_and_the_members_of_each_class() {
    let a_b = r#"{"id":"a","kind":"site"},{"id":"b","kind":"site"}"#;
    for (text, expected) in [
        (
            document(a_b, r#"{"id":"l","ends":["a","a"]}"#, ""),
            "links[0] ends[1]: repeat, valid: another node",
        ),
        (
            document(a_b, r#"{"id":"l","ends":["a","b","a"]}"#, ""),
            "links[0] ends: size, valid: 2 items",
        ),
        (
            document(a_b, r#"{"id":"l","ends":["a","c"]}"#, ""),
            "links[0] ends[1]: no_node, valid: the id of a node before the record",
        ),
        (
            document(a_b, r#"{"id":"l","ends":["a","b"]}"#, r#"{"id":"f","path":["a","l"]}"#),
            "flows[0] path[1]: no_node, valid: the id of a node before the record",
        ),
        (
            document(a_b, "", r#"{"id":"f","path":["a","b"],"hops":[{}]}"#),
            "flows[0] hops: size, valid: 1 item for each node of path",
        ),
        (
            document(a_b, "", r#"{"id":"f","path":["a","b"],"outcome":{"kind":"lost"}}"#),
            "flows[0] outcome.kind: value, valid: deliver, exit, drop, loop, unknown",
        ),
        (
            document(a_b, "", r#"{"id":"f","path":["a","b"],"magnitude":{"value":-1,"unit":"bit/s"}}"#),
            "flows[0] magnitude.value: value, valid: 0 or more",
        ),
        (
            document(a_b, "", r#"{"id":"f","path":["a","b"],"magnitude":{"value":1,"unit":"kbit/s"}}"#),
            "flows[0] magnitude.unit: value, valid: bit/s, packet/s, byte, packet, flow",
        ),
        (
            document(a_b, "", r#"{"id":"f","path":["a","b"]},{"id":"r","path":["b","a"],"reply_to":"f"},{"id":"s","path":["a","b"],"reply_to":"r"}"#),
            "flows[2] reply_to: no_flow, valid: the id of a flow before the record with no reply_to",
        ),
        (
            document(r#"{"id":"a","kind":"site","fields":{"count":9007199254740993}}"#, "", ""),
            "nodes[0] fields.count: value, valid: an integer from -9007199254740991 to 9007199254740991, or a string",
        ),
        (
            document(r#"{"id":"a","kind":"site","fields":{"serial":{"missing":"lost"}}}"#, "", ""),
            "nodes[0] fields.serial.missing: value, valid: not_available, not_known",
        ),
        (
            document(r#"{"id":"a","kind":"site","fields":{"serial":""}}"#, "", ""),
            "nodes[0] fields.serial: size, valid: 1 to 1024 characters",
        ),
        (
            document(r#"{"id":"a","kind":"site","fields":{"bad name":1}}"#, "", ""),
            "nodes[0] fields: value, valid: member names of 1 to 64 ASCII letters, digits, -, _ and .",
        ),
        (
            document(&format!(r#"{{"id":"a","kind":"site","fields":{{{}}}}}"#, (0..65).map(|index| format!("\"f{index}\":1")).collect::<Vec<_>>().join(",")), "", ""),
            "nodes[0] fields: size, valid: 64 members",
        ),
        (
            DOCUMENT.replace("2026-11-23T12:00:00Z", "2026-02-30T12:00:00Z"),
            "document source.time: value, valid: RFC 3339 time in UTC with Z",
        ),
        (
            DOCUMENT.replace(r#""snapshot": "snap-1", "#, ""),
            "document source.snapshot: missing, valid: name, snapshot, query",
        ),
        (
            r#"{"format":"galaxy-relations","version":"0.1","source":{"name":"example","snapshot":"s","query":"q"}}"#.to_owned(),
            "document nodes: missing, valid: format, version, source, nodes",
        ),
    ] {
        assert_eq!(refusals(&text).first().map(String::as_str), Some(expected), "{text}");
    }
    let kinds: Vec<String> = (0..257)
        .map(|index| format!(r#"{{"id":"n{index}","kind":"k{index}"}}"#))
        .collect();
    assert_eq!(
        refusals(&document(&kinds.join(","), "", "")),
        ["nodes[256] kind: size, valid: 256 kinds"]
    );
    let long = format!(
        r#"{{"node":{{"id":"a","kind":"site","label":"{}"}}}}"#,
        "x".repeat(document::RECORD_BYTES)
    );
    assert_eq!(
        refusals(&stream(&[&long]))[0],
        "line 2 text: size, valid: 1048576 bytes"
    );
}

#[test]
fn a_refused_load_gives_at_most_20_refusals_in_the_order_of_the_document() {
    let nodes: Vec<String> = (0..30)
        .map(|index| format!(r#"{{"id":"n{index}","kind":"root"}}"#))
        .collect();
    let refused = refusals(&document(&nodes.join(","), "", ""));
    assert_eq!(refused.len(), REFUSALS);
    for (index, text) in refused.iter().enumerate() {
        assert!(
            text.starts_with(&format!("nodes[{index}] kind: value")),
            "{text}"
        );
    }
}

#[test]
fn a_refusal_holds_no_value_of_a_record() {
    let secret = "203.0.113.99 serial EX-9999";
    let text = document(
        &format!(
            r#"{{"id":"a","kind":"{secret}"}},{{"id":"b","kind":"site","fields":{{"note":{{"missing":"{secret}"}}}}}},{{"id":"c","kind":"site","in":"{secret}"}}"#
        ),
        "",
        &format!(r#"{{"id":"f","path":["b","{secret}"]}}"#),
    );
    let refused = refused(&text);
    assert_eq!(refused.len(), 4);
    for refusal in &refused {
        assert!(!refusal.to_string().contains("203.0.113.99"), "{refusal}");
        assert!(
            refusal
                .value
                .as_deref()
                .is_some_and(|value| value.contains(secret)),
            "{refusal}"
        );
    }
}

#[test]
fn a_stream_of_mcp_pages_reads_its_next_lines() {
    let text = stream(&[
        r#"{"node":{"id":"a","kind":"site"}}"#,
        r#"{"next":{"cursor":"page-2"}}"#,
        r#"{"node":{"id":"b","kind":"site"}}"#,
        r#"{"end":{"nodes":2,"links":0,"flows":0}}"#,
    ]);
    let paged = Load::read("inventory", "q", text.as_bytes(), Framing::Kept).unwrap();
    assert_eq!(paged.relations().nodes().len(), 2);
    let ended = stream(&[
        r#"{"node":{"id":"a","kind":"site"}}"#,
        r#"{"next":{"cursor":"page-2"}}"#,
    ]);
    assert_eq!(
        Load::read("inventory", "q", ended.as_bytes(), Framing::Kept)
            .unwrap_err()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["line 3 next: no_end, valid: end as the last line"]
    );
    let after_end = format!(
        "{}{}\n",
        stream(&[r#"{"end":{"nodes":0,"links":0,"flows":0}}"#]),
        r#"{"node":{"id":"a","kind":"site"}}"#
    );
    assert_eq!(
        refusals(&after_end),
        ["line 3 node: no_end, valid: end as the last line"]
    );
}

#[test]
fn a_stream_of_pages_checks_each_page_end_in_any_order() {
    let lines = [
        HEADER,
        r#"{"node":{"id":"a","kind":"site"}}"#,
        r#"{"next":{"cursor":"page-2"}}"#,
        r#"{"node":{"id":"b","kind":"site"}}"#,
        r#"{"next":{"cursor":"page-3"}}"#,
        r#"{"end":{"nodes":2,"links":0,"flows":0}}"#,
    ];
    let text = lines.join("\n") + "\n";
    let after = |count: usize| {
        lines[..count]
            .iter()
            .map(|line| line.len() + 1)
            .sum::<usize>()
    };
    let read = |ends: &[usize]| {
        Load::read("inventory", "q", text.as_bytes(), Framing::Pages(ends))
            .map(|load| load.relations().nodes().len())
            .map_err(|refusals| refusals.iter().map(ToString::to_string).collect::<Vec<_>>())
    };
    assert_eq!(read(&[after(3), after(5), text.len()]), Ok(2));
    let inside = after(3) + 4;
    let refused = Err(vec![
        "line 4 text: line, valid: a page that ends in a line break".to_owned(),
        "line 5 next: line, valid: next as the last line of a page".to_owned(),
    ]);
    assert_eq!(read(&[after(3), inside, text.len()]), refused);
    assert_eq!(read(&[text.len(), inside, after(3), after(3)]), refused);
}

#[test]
fn a_line_above_the_record_limit_after_the_check_stops_gives_no_unit() {
    let bad: Vec<String> = (0..REFUSALS)
        .map(|index| format!(r#"{{"node":{{"id":"n{index}","kind":"root"}}}}"#))
        .collect();
    let flow = |padding: usize| {
        format!(
            r#"{{"flow":{{"id":"f","path":["a","b"],"magnitude":{{"value":1,"unit":"byte"}}}}}}{}"#,
            " ".repeat(padding)
        )
    };
    let refused = |last: &str| {
        let mut lines: Vec<&str> = bad.iter().map(String::as_str).collect();
        lines.push(last);
        refusals(&stream(&lines))
    };
    let window = "line 1 source.window: window, valid: from and to, from not after to";
    assert_eq!(refused(&flow(0))[0], window);
    let long = flow(0);
    let long = flow(RECORD_BYTES + 1 - long.len());
    assert!(!refused(&long).iter().any(|text| text == window));
}

#[test]
fn a_refused_load_changes_nothing() {
    let mut model = Model::new(Style::default());
    model.apply(Change::Load(load(DOCUMENT))).unwrap();
    let before = model.clone();
    assert!(Load::read("saved", "q", b"{", Framing::Text).is_err());
    assert_eq!(
        model.apply(Change::LoadView {
            source: "saved".to_owned(),
            query: "q".to_owned(),
            sha256: None,
        }),
        Err(ChangeRefusal::Unread)
    );
    assert_eq!(model, before);
    assert_eq!(
        ChangeRefusal::Unread.to_string(),
        "load_view: not read, valid: a document read by the viewport"
    );
}

#[test]
fn undo_of_a_load_restores_the_relations_before_it_and_keeps_the_style() {
    let mut model = Model::new(Style::default());
    for (name, arguments) in [
        ("add_node", r#"{"name":"old","kind":"site"}"#),
        (
            "set_rule",
            r#"{"kind":"server","arrangement":"row","shape":"cylinder"}"#,
        ),
    ] {
        model
            .apply(wire::decode(name, &Json::parse(arguments).unwrap()).unwrap())
            .unwrap();
    }
    let before = model.clone();
    model.apply(Change::Load(load(DOCUMENT))).unwrap();
    assert_eq!(model.relations().nodes().len(), 5);
    assert_eq!(model.style(), before.style());
    assert_eq!(model.log().len(), before.log().len() + 1);
    assert!(matches!(model.undo(), Some(Change::Load(_))));
    assert_eq!(model, before);
}

#[test]
fn replay_of_a_log_with_a_load_and_the_kept_bytes_gives_the_same_model() {
    let mut model = Model::new(Style::default());
    let mut kept = BTreeMap::new();
    model
        .apply(wire::decode("set_look", &Json::parse(r#"{"scheme":"dark"}"#).unwrap()).unwrap())
        .unwrap();
    for text in [DOCUMENT, STREAM] {
        let loaded = Load::read(
            "saved",
            "path client to dns",
            text.as_bytes(),
            Framing::Text,
        )
        .unwrap();
        kept.insert(loaded.hash().to_owned(), text.as_bytes().to_vec());
        model.apply(Change::Load(loaded)).unwrap();
    }
    let lines: Vec<String> = model
        .log()
        .iter()
        .map(|change| {
            let (name, arguments) = wire::encode(change);
            format!("{name} {arguments}")
        })
        .collect();
    assert_eq!(
        lines[1],
        format!(
            r#"load_view {{"source":"saved","query":"path client to dns","sha256":"{}"}}"#,
            sha256::hex(DOCUMENT.as_bytes())
        )
    );
    let log: Vec<Change> = lines
        .iter()
        .map(|line| {
            let (name, arguments) = line.split_once(' ').unwrap();
            match wire::decode(name, &Json::parse(arguments).unwrap()).unwrap() {
                Change::LoadView {
                    source,
                    query,
                    sha256: Some(hash),
                } => {
                    let bytes = &kept[&hash];
                    assert_eq!(sha256::hex(bytes), hash);
                    Change::Load(Load::read(&source, &query, bytes, Framing::Kept).unwrap())
                }
                change => change,
            }
        })
        .collect();
    assert_eq!(Model::replay(Style::default(), &log).unwrap(), model);
}

#[test]
fn wire_of_load_view_takes_a_source_and_a_query() {
    let call = |arguments: &str| wire::decode("load_view", &Json::parse(arguments).unwrap());
    assert_eq!(
        call(r#"{"source":"saved","query":"path"}"#).unwrap(),
        Change::LoadView {
            source: "saved".to_owned(),
            query: "path".to_owned(),
            sha256: None,
        }
    );
    for (arguments, refusal) in [
        (r#"{"query":"path"}"#, "source: missing"),
        (r#"{"source":"saved"}"#, "query: missing"),
        (
            r#"{"source":"a b","query":""}"#,
            "source: a b, valid: 1 to 64 ASCII letters, digits, -, _ and .",
        ),
        (
            r#"{"source":"saved","query":"","sha256":"ABC"}"#,
            "sha256: ABC, valid: 64 lowercase hex digits",
        ),
    ] {
        assert_eq!(
            call(arguments).unwrap_err().to_string(),
            refusal,
            "{arguments}"
        );
    }
    let long = format!(r#"{{"source":"saved","query":"{}"}}"#, "q".repeat(8193));
    assert_eq!(
        call(&long).unwrap_err().to_string(),
        "query: 8193 characters, valid: 0 to 8192 characters"
    );
    let change = Change::LoadView {
        source: "saved".to_owned(),
        query: "path".to_owned(),
        sha256: Some(sha256::hex(b"")),
    };
    let (name, arguments) = wire::encode(&change);
    assert_eq!(wire::decode(name, &arguments).unwrap(), change);
}

#[test]
fn a_document_on_1_line_with_text_after_it_is_refused_as_json() {
    let text = format!(
        "{}\ngarbage\n",
        document(r#"{"id":"a","kind":"site"}"#, "", "")
    );
    let refused = refused(&text);
    assert_eq!(refused[0].code, Code::Json, "{refused:?}");
}

#[test]
fn a_stream_with_no_lf_after_its_last_line_is_refused() {
    let mut text = stream(&[
        r#"{"node":{"id":"a","kind":"site"}}"#,
        r#"{"end":{"nodes":1,"links":0,"flows":0}}"#,
    ]);
    assert!(Load::read("saved", "q", text.as_bytes(), Framing::Text).is_ok());
    text.pop();
    assert_eq!(
        refusals(&text),
        ["line 3 text: line, valid: LF at the end of the line"]
    );
}

#[test]
fn a_refusal_in_partial_before_the_records_comes_first() {
    let nodes: Vec<String> = (0..25)
        .map(|index| format!(r#"{{"id":"n{index}","kind":"root"}}"#))
        .collect();
    let text = format!(
        r#"{{"format":"galaxy-relations","version":"0.1","source":{{"name":"example","snapshot":"snap-1","query":"q"}},"partial":[{{"part":""}}],"nodes":[{}]}}"#,
        nodes.join(",")
    );
    let refused = refusals(&text);
    assert_eq!(refused.len(), REFUSALS);
    assert!(refused[0].starts_with("document partial"), "{refused:?}");
    assert!(refused[REFUSALS - 1].starts_with("nodes["), "{refused:?}");
}

#[test]
fn the_window_refusal_comes_at_the_place_of_source_before_the_refusals_of_later_lines() {
    let header = r#"{"header":{"format":"galaxy-relations","version":"0.1","source":{"name":"example","snapshot":"snap-1","query":"q"}}}"#;
    let text = format!(
        "{header}\n{}\n{}\n{}\n{}\n",
        r#"{"node":{"id":"a","kind":"root"}}"#,
        r#"{"node":{"id":"b","kind":"site"}}"#,
        r#"{"flow":{"id":"f","path":["b","b2"],"magnitude":{"value":3,"unit":"packet"}}}"#,
        r#"{"end":{"nodes":2,"links":0,"flows":1}}"#
    );
    let refused = refusals(&text);
    assert_eq!(
        refused[..2],
        [
            "line 1 source.window: window, valid: from and to, from not after to",
            "line 2 kind: value, valid: 1 to 64 ASCII letters, digits, -, _ and ., not root, not default",
        ]
    );
}

#[test]
fn the_window_refusal_stays_in_the_first_20_refusals_after_the_check_stops() {
    let nodes: Vec<String> = (0..25)
        .map(|index| format!(r#"{{"id":"n{index}","kind":"root"}}"#))
        .collect();
    let text = format!(
        r#"{{"format":"galaxy-relations","version":"0.1","source":{{"name":"example","snapshot":"snap-1","query":"q"}},"nodes":[{}],"flows":[{{"id":"f","path":["n0","n1"],"magnitude":{{"value":1,"unit":"byte"}}}}]}}"#,
        nodes.join(",")
    );
    let refused = refusals(&text);
    assert_eq!(refused.len(), REFUSALS);
    assert_eq!(
        refused[0],
        "document source.window: window, valid: from and to, from not after to"
    );
    assert!(
        refused[REFUSALS - 1].starts_with("nodes[18]"),
        "{refused:?}"
    );
}

#[test]
fn a_document_reads_its_arrays_in_the_order_nodes_links_flows() {
    let text = r#"{"format":"galaxy-relations","version":"0.1","source":{"name":"example","snapshot":"snap-1","query":"q"},"flows":[{"id":"f","path":["a","b"]}],"nodes":[{"id":"a","kind":"site"},{"id":"b","kind":"site"}]}"#;
    let loaded = load(text);
    assert_eq!(loaded.relations().flows().len(), 1);
}

#[test]
fn a_document_reads_source_and_partial_in_the_order_of_the_text() {
    let text = r#"{"partial":[{"part":"","reason":"r"}],"format":"galaxy-relations","version":"0.1","source":{"name":"bad name","snapshot":"snap-1","query":"q"},"nodes":[]}"#;
    let refused = refusals(text);
    assert_eq!(
        refused,
        [
            "document partial[0].part: size, valid: 1 to 1024 characters",
            "document source.name: value, valid: 1 to 64 ASCII letters, digits, -, _ and .",
        ]
    );
}

#[test]
fn the_window_refusal_follows_a_source_after_the_records() {
    let text = r#"{"format":"galaxy-relations","version":"0.1","nodes":[{"id":"a","kind":"root"},{"id":"b","kind":"site"}],"flows":[{"id":"f","path":["b","a"],"magnitude":{"value":1,"unit":"byte"}}],"source":{"name":"bad name","snapshot":"snap-1","query":"q"}}"#;
    let refused = refusals(text);
    assert_eq!(
        refused,
        [
            "nodes[0] kind: value, valid: 1 to 64 ASCII letters, digits, -, _ and ., not root, not default",
            "document source.name: value, valid: 1 to 64 ASCII letters, digits, -, _ and .",
            "document source.window: window, valid: from and to, from not after to",
        ]
    );
    let windowed = r#"{"format":"galaxy-relations","version":"0.1","nodes":[{"id":"a","kind":"site"},{"id":"b","kind":"site"}],"flows":[{"id":"f","path":["b","a"],"magnitude":{"value":1,"unit":"byte"}}],"source":{"name":"example","snapshot":"snap-1","query":"q","window":{"from":"2026-11-23T12:00:00Z","to":"2026-11-23T12:01:00Z"}}}"#;
    assert_eq!(load(windowed).answer().unit.as_deref(), Some("byte"));
}

#[test]
fn an_abort_line_with_no_valid_reason_is_refused_with_the_code_abort() {
    let refused = refusals(&stream(&[r#"{"abort":{}}"#]));
    assert_eq!(
        refused,
        [
            "line 2 abort.reason: missing, valid: reason",
            "line 2 abort: abort, valid: no abort line",
        ]
    );
}

#[test]
fn ignored_holds_the_names_by_the_name_rule_in_both_framings() {
    let document = r#"{"format":"galaxy-relations","version":"0.1","x-extra":1,"source":{"name":"example","snapshot":"snap-1","query":"q"},"nodes":[{"id":"n","kind":"router","label with spaces and data 192.0.2.1":1}]}"#;
    assert_eq!(load(document).answer().ignored, ["x-extra"]);
    let header = r#"{"header":{"format":"galaxy-relations","version":"0.1","x-extra":1,"source":{"name":"example","snapshot":"snap-1","query":"q"}}}"#;
    let text = format!(
        "{header}\n{}\n{}\n{}\n",
        r#"{"node":{"id":"n","kind":"router","label with spaces and data 192.0.2.1":1}}"#,
        r#"{"line kind 192.0.2.7":{}}"#,
        r#"{"end":{"nodes":1,"links":0,"flows":0}}"#
    );
    assert_eq!(load(&text).answer().ignored, ["x-extra"]);
}

#[test]
fn ignored_holds_at_most_64_names_in_name_order() {
    let members: Vec<String> = (0..100)
        .map(|index| format!(r#""x{index:03}":1"#))
        .collect();
    let text = format!(
        r#"{{"format":"galaxy-relations","version":"0.1",{},"source":{{"name":"example","snapshot":"snap-1","query":"q"}},"nodes":[]}}"#,
        members.join(",")
    );
    let ignored = load(&text).answer().ignored.clone();
    let expected: Vec<String> = (0..document::IGNORED)
        .map(|index| format!("x{index:03}"))
        .collect();
    assert_eq!(ignored, expected);
}

#[test]
fn a_line_of_an_unknown_kind_is_refused_with_the_member_text() {
    let text = "{\"node 192.0.2.7 secret-ish text\":{}}\n";
    let refused = refused(text);
    assert_eq!(
        refused.iter().map(ToString::to_string).collect::<Vec<_>>(),
        ["line 1 text: header, valid: header"]
    );
    assert_eq!(
        refused[0].value.as_deref(),
        Some("\"node 192.0.2.7 secret-ish text\"")
    );
    let after = refusals(&stream(&[
        r#"{"end":{"nodes":0,"links":0,"flows":0}}"#,
        r#"{"x y":{}}"#,
    ]));
    assert_eq!(after, ["line 3 text: no_end, valid: end as the last line"]);
}

#[test]
fn the_refusals_of_format_and_version_come_before_the_other_members_of_a_document() {
    assert_eq!(
        refusals(r#"{"format":"other","version":"0.1"}"#),
        [
            "document format: format, valid: galaxy-relations",
            "document source: missing, valid: format, version, source, nodes",
            "document nodes: missing, valid: format, version, source, nodes",
        ]
    );
    assert_eq!(
        refusals(r#"{"version":"9.9","nodes":[]}"#),
        [
            "document format: missing, valid: format, version, source, nodes",
            "document version: version, valid: 0.1",
            "document source: missing, valid: format, version, source, nodes",
        ]
    );
}

#[test]
fn parse_limited_refuses_a_text_with_more_values_than_the_limit() {
    let text = r#"{"nodes":[1,2,3]}"#;
    assert!(Json::parse_limited(text, 5).is_ok());
    let error = Json::parse_limited(text, 4).unwrap_err();
    assert_eq!(error.cause, "more values than the limit");
    assert_eq!(Json::parse_limited(text, 5), Json::parse(text));
}
