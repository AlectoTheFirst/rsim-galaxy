use std::path::{Path, PathBuf};
use std::sync::Arc;

use galaxy_native::mcp;
use galaxy_native::sources::{DOCUMENTS, Sources};
use galaxy_native::viewer::{Reply, Viewer};
use galaxy_scene::{Json, sha256, wire};

const FIXTURE: &str = env!("CARGO_BIN_EXE_galaxy-fixture");

fn documents() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/documents")
}

/// An empty directory for the test `name`.
fn directory(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("galaxy-native-tests").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes `sources.json` in `dir` with the sources `files`, `inventory` (the fixture with pages of
/// `page` bytes, or the program `command`) and `store`, and reads it.
fn sources(dir: &Path, command: Option<&str>, page: usize) -> Arc<Sources> {
    let command = match command {
        Some(program) => Json::Array(vec![Json::string(program)]),
        None => Json::Array(
            [
                FIXTURE,
                documents().to_str().unwrap(),
                "--page",
                &page.to_string(),
            ]
            .map(Json::string)
            .to_vec(),
        ),
    };
    let configuration = Json::object([(
        "sources",
        Json::object([
            (
                "files",
                Json::object([
                    ("transport", Json::string("file")),
                    ("description", Json::string("saved documents")),
                    ("path", Json::string(documents().to_str().unwrap())),
                ]),
            ),
            (
                "inventory",
                Json::object([
                    ("transport", Json::string("mcp")),
                    ("description", Json::string("inventory adapter")),
                    ("command", command),
                ]),
            ),
            (
                "store",
                Json::object([
                    ("transport", Json::string("http")),
                    ("description", Json::string("relations store")),
                    ("url", Json::string("https://store.example.net/relations")),
                ]),
            ),
        ]),
    )]);
    let path = dir.join("sources.json");
    std::fs::write(&path, configuration.to_string()).unwrap();
    Arc::new(Sources::read(&path).unwrap())
}

fn call(viewer: &mut Viewer, name: &str, arguments: &str) -> Reply {
    viewer.call(name, &Json::parse(arguments).unwrap(), None, None)
}

fn load(viewer: &mut Viewer, source: &str, query: &str) -> Reply {
    let arguments = Json::object([
        ("source", Json::string(source)),
        ("query", Json::string(query)),
    ]);
    viewer.call("load_view", &arguments, None, None)
}

/// The kept files of `dir` by name.
fn kept(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir.join(DOCUMENTS))
        .map(|entries| {
            entries
                .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn load_over_mcp_gives_the_answer_and_keeps_the_bytes() {
    let dir = directory("mcp-document");
    let mut viewer = Viewer::new(Some(sources(&dir, None, 4_194_304)));
    let reply = load(&mut viewer, "inventory", "site-a");
    assert!(!reply.refused, "{}", reply.text);
    assert!(reply.changed);
    let answer = Json::parse(&reply.text).unwrap();
    assert_eq!(answer.get("nodes"), Some(&Json::Number(11.0)));
    assert_eq!(answer.get("links"), Some(&Json::Number(0.0)));
    assert_eq!(answer.get("flows"), Some(&Json::Number(3.0)));
    assert_eq!(
        answer.get("source").unwrap().get("snapshot"),
        Some(&Json::string("snapshot-2026-11-23-120000"))
    );
    assert_eq!(
        answer.get("not_shown"),
        Some(&Json::Array(vec![Json::string("node.label")]))
    );
    assert!(!reply.text.contains("192.0.2."));
    assert_eq!(viewer.world.flows().len(), 3);
    assert_eq!(viewer.model.log().len(), 1);
    let bytes = std::fs::read(documents().join("site-a.json")).unwrap();
    assert_eq!(kept(&dir), [sha256::hex(&bytes)]);
    let (name, arguments) = wire::encode(&viewer.model.log()[0]);
    assert_eq!(name, "load_view");
    assert_eq!(
        arguments.to_string(),
        format!(
            r#"{{"source":"inventory","query":"site-a","sha256":"{}"}}"#,
            sha256::hex(&bytes)
        )
    );
}

#[test]
fn a_paged_stream_over_mcp_gives_the_world_of_the_document() {
    let dir = directory("mcp-stream");
    let mut viewer = Viewer::new(Some(sources(&dir, None, 400)));
    let reply = load(&mut viewer, "inventory", "site-a");
    assert!(!reply.refused, "{}", reply.text);
    let document = viewer.describe();
    let reply = load(&mut viewer, "inventory", "site-a-stream");
    assert!(!reply.refused, "{}", reply.text);
    assert_eq!(viewer.describe(), document);
    assert_eq!(viewer.model.log().len(), 2);
    let names = kept(&dir);
    assert_eq!(names.len(), 2);
    let stream = names
        .iter()
        .map(|name| std::fs::read_to_string(dir.join(DOCUMENTS).join(name)).unwrap())
        .find(|text| text.starts_with(r#"{"header""#))
        .unwrap();
    assert!(stream.matches(r#"{"next":{"cursor":"#).count() >= 3);
    assert!(stream.ends_with("}}\n"));
}

#[test]
fn load_from_file_reads_a_document_and_a_stream() {
    let dir = directory("file");
    let mut viewer = Viewer::new(Some(sources(&dir, None, 4_194_304)));
    let reply = load(&mut viewer, "files", "site-a");
    assert!(!reply.refused, "{}", reply.text);
    let document = viewer.describe();
    let reply = load(&mut viewer, "files", "site-a-stream");
    assert!(!reply.refused, "{}", reply.text);
    assert_eq!(viewer.describe(), document);
    assert_eq!(kept(&dir).len(), 2);
    let reply = load(&mut viewer, "files", "site-b");
    assert!(reply.refused);
    assert_eq!(
        reply.text,
        "load query: source, valid: refused, site-a, site-a-stream"
    );
    let reply = load(&mut viewer, "files", "../site-a");
    assert_eq!(
        reply.text,
        "load query: value, valid: 1 to 64 ASCII letters, digits, -, _ and ."
    );
}

#[test]
fn a_json_file_is_read_as_a_document_and_a_jsonl_file_as_a_stream() {
    let dir = directory("file-forms");
    let files = dir.join("documents");
    std::fs::create_dir_all(&files).unwrap();
    std::fs::copy(
        documents().join("site-a-stream.jsonl"),
        files.join("streamy.json"),
    )
    .unwrap();
    std::fs::copy(documents().join("site-a.json"), files.join("docy.jsonl")).unwrap();
    let configuration = Json::object([(
        "sources",
        Json::object([(
            "files",
            Json::object([
                ("transport", Json::string("file")),
                ("description", Json::string("saved documents")),
                ("path", Json::string("documents")),
            ]),
        )]),
    )]);
    let path = dir.join("sources.json");
    std::fs::write(&path, configuration.to_string()).unwrap();
    let mut viewer = Viewer::new(Some(Arc::new(Sources::read(&path).unwrap())));
    let reply = load(&mut viewer, "files", "streamy");
    assert!(reply.refused);
    assert!(reply.text.starts_with("document byte "), "{}", reply.text);
    let reply = load(&mut viewer, "files", "docy");
    assert!(reply.refused);
    assert_eq!(
        reply.text.lines().next(),
        Some("line 1 byte 1: json, valid: JSON of RFC 8259 in UTF-8")
    );
    assert_eq!(viewer.model.log().len(), 0);
}

#[test]
fn a_refused_load_changes_nothing_and_writes_the_values_to_stderr() {
    let dir = directory("refused");
    let mut viewer = Viewer::new(Some(sources(&dir, None, 4_194_304)));
    call(
        &mut viewer,
        "set_rule",
        r#"{"kind":"server","arrangement":"row","shape":"cylinder"}"#,
    );
    let reply = load(&mut viewer, "files", "site-a");
    assert!(!reply.refused, "{}", reply.text);
    let model = viewer.model.clone();
    let text = viewer.describe();
    let kept_before = kept(&dir);
    for (source, query) in [("files", "refused"), ("inventory", "refused")] {
        let reply = load(&mut viewer, source, query);
        assert!(reply.refused);
        assert!(!reply.changed);
        assert_eq!(
            reply.text,
            "nodes[0] kind: value, valid: 1 to 64 ASCII letters, digits, -, _ and ., not root, not default\n\
             nodes[1] in: no_node, valid: the id of a node before the record"
        );
        assert!(!reply.text.contains("rack-9"));
        assert_eq!(
            reply.stderr,
            [r#"nodes[0] kind: "root""#, r#"nodes[1] in: "rack-9""#]
        );
        assert_eq!(viewer.model, model);
        assert_eq!(viewer.describe(), text);
        assert_eq!(kept(&dir), kept_before);
    }
}

#[test]
fn undo_of_a_load_removes_the_whole_load_and_keeps_the_style() {
    let dir = directory("undo");
    let mut viewer = Viewer::new(Some(sources(&dir, None, 4_194_304)));
    call(
        &mut viewer,
        "set_rule",
        r#"{"kind":"server","arrangement":"row","shape":"cylinder"}"#,
    );
    call(&mut viewer, "add_node", r#"{"name":"lab","kind":"site"}"#);
    let before = viewer.model.clone();
    let reply = load(&mut viewer, "inventory", "site-a");
    assert!(!reply.refused, "{}", reply.text);
    call(&mut viewer, "set_look", r#"{"scheme":"dark"}"#);
    let style = viewer.model.style().clone();
    call(&mut viewer, "undo", "{}");
    let reply = call(&mut viewer, "undo", "{}");
    assert!(
        reply
            .text
            .starts_with(r#"undone load_view {"source":"inventory","query":"site-a","sha256":""#)
    );
    assert_eq!(viewer.model, before);
    assert_eq!(viewer.world.flows().len(), 0);
    assert!(viewer.describe().starts_with("lab site"));
    assert_eq!(viewer.model.style().rules, style.rules);
}

#[test]
fn replay_of_an_ops_file_reads_the_kept_bytes_with_no_source_running() {
    let dir = directory("replay");
    let mut viewer = Viewer::new(Some(sources(&dir, None, 400)));
    call(
        &mut viewer,
        "set_rule",
        r#"{"kind":"server","arrangement":"row","shape":"cylinder"}"#,
    );
    let reply = load(&mut viewer, "inventory", "site-a-stream");
    assert!(!reply.refused, "{}", reply.text);
    let ops = viewer.log();
    assert_eq!(ops.lines().count(), 2);
    assert!(ops.contains(r#""sha256":""#), "{ops}");
    let stopped = sources(&dir, Some("/nonexistent/galaxy-fixture"), 400);
    let mut replay = Viewer::new(Some(stopped.clone()));
    assert_eq!(replay.apply_ops(ops.as_bytes()).unwrap().len(), 0);
    assert_eq!(replay.model, viewer.model);
    assert_eq!(replay.describe(), viewer.describe());
    let mut live = Viewer::new(Some(stopped));
    let reply = load(&mut live, "inventory", "site-a-stream");
    assert!(reply.refused);
    assert!(
        reply.text.starts_with("load command: source, valid: "),
        "{}",
        reply.text
    );
}

#[test]
fn a_changed_or_missing_kept_file_refuses_the_replay() {
    let dir = directory("changed");
    let mut viewer = Viewer::new(Some(sources(&dir, None, 4_194_304)));
    let reply = load(&mut viewer, "files", "site-a");
    assert!(!reply.refused, "{}", reply.text);
    let (_, arguments) = wire::encode(&viewer.model.log()[0]);
    let hash = arguments
        .get("sha256")
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();
    let path = dir.join(DOCUMENTS).join(&hash);
    let text = std::fs::read_to_string(&path)
        .unwrap()
        .replace("192.0.2.80", "192.0.2.81");
    std::fs::write(&path, text).unwrap();
    let mut replay = Viewer::new(viewer.sources.clone());
    let refused = replay
        .apply_ops(format!(r#"{{"name":"load_view","arguments":{arguments}}}"#).as_bytes())
        .unwrap();
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].0, 1);
    assert_eq!(
        refused[0].1.text,
        format!("load sha256: source, valid: the kept file {DOCUMENTS}/{hash} with this SHA-256")
    );
    std::fs::remove_file(&path).unwrap();
    let refused = replay
        .apply_ops(format!(r#"{{"name":"load_view","arguments":{arguments}}}"#).as_bytes())
        .unwrap();
    assert_eq!(
        refused[0].1.text,
        format!("load sha256: source, valid: the kept file {DOCUMENTS}/{hash}")
    );
    assert_eq!(replay.model.log().len(), 0);
}

#[test]
fn load_view_of_the_agent_takes_no_sha256_and_a_replay_needs_a_source_of_the_configuration() {
    let dir = directory("agent-sha256");
    let mut viewer = Viewer::new(Some(sources(&dir, None, 4_194_304)));
    let reply = load(&mut viewer, "files", "site-a");
    assert!(!reply.refused, "{}", reply.text);
    let (_, arguments) = wire::encode(&viewer.model.log()[0]);
    let hash = arguments
        .get("sha256")
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();
    let mut agent = Viewer::new(viewer.sources.clone());
    let reply = agent.call("load_view", &arguments, None, None);
    assert_eq!(reply.text, "load sha256: value, valid: source and query");
    let other = format!(
        r#"{{"name":"load_view","arguments":{{"source":"no-such-source","query":"anything","sha256":"{hash}"}}}}"#
    );
    let refused = agent.apply_ops(other.as_bytes()).unwrap();
    assert_eq!(
        refused[0].1.text,
        "load source: no_source, valid: files, inventory, store"
    );
    assert_eq!(agent.model.log().len(), 0);
    let schema = galaxy_scene::wire::TOOLS
        .iter()
        .find(|tool| tool.name == "load_view")
        .unwrap()
        .schema;
    assert!(!schema.contains("sha256"));
}

#[test]
fn the_kept_file_holds_the_bytes_of_the_source_as_sent() {
    let dir = directory("as-sent");
    let documents = dir.join("documents");
    std::fs::create_dir_all(&documents).unwrap();
    let text = std::fs::read_to_string(self::documents().join("site-a.json")).unwrap();
    let bytes = text.trim_end().as_bytes().to_vec();
    std::fs::write(documents.join("open.json"), &bytes).unwrap();
    let command = Json::Array(
        [FIXTURE, documents.to_str().unwrap()]
            .map(Json::string)
            .to_vec(),
    );
    let configuration = Json::object([(
        "sources",
        Json::object([(
            "inventory",
            Json::object([
                ("transport", Json::string("mcp")),
                ("description", Json::string("inventory adapter")),
                ("command", command),
            ]),
        )]),
    )]);
    let path = dir.join("sources.json");
    std::fs::write(&path, configuration.to_string()).unwrap();
    let mut viewer = Viewer::new(Some(Arc::new(Sources::read(&path).unwrap())));
    let reply = load(&mut viewer, "inventory", "open");
    assert!(!reply.refused, "{}", reply.text);
    let kept = dir.join(DOCUMENTS).join(sha256::hex(&bytes));
    assert_eq!(std::fs::read(kept).unwrap(), bytes);
}

#[test]
fn a_bare_command_name_is_the_program_of_that_name_in_the_directory_of_the_configuration() {
    let dir = directory("bare-command");
    std::fs::copy(FIXTURE, dir.join("galaxy-probe-adapter")).unwrap();
    let command = Json::Array(
        ["galaxy-probe-adapter", documents().to_str().unwrap()]
            .map(Json::string)
            .to_vec(),
    );
    let configuration = Json::object([(
        "sources",
        Json::object([(
            "inventory",
            Json::object([
                ("transport", Json::string("mcp")),
                ("description", Json::string("inventory adapter")),
                ("command", command),
            ]),
        )]),
    )]);
    let path = dir.join("sources.json");
    std::fs::write(&path, configuration.to_string()).unwrap();
    let mut viewer = Viewer::new(Some(Arc::new(Sources::read(&path).unwrap())));
    let reply = load(&mut viewer, "inventory", "site-a");
    assert!(!reply.refused, "{}", reply.text);
}

#[test]
fn a_page_with_no_line_break_before_the_next_page_or_with_an_early_next_line_is_refused_at_its_line()
 {
    for (frame, text) in [
        (
            "open",
            "line 7 text: line, valid: a page that ends in a line break",
        ),
        (
            "early",
            "line 6 next: line, valid: next as the last line of a page",
        ),
    ] {
        let dir = directory(&format!("frame-{frame}"));
        let command = Json::Array(
            [
                FIXTURE,
                documents().to_str().unwrap(),
                "--page",
                "600",
                "--frame",
                frame,
            ]
            .map(Json::string)
            .to_vec(),
        );
        let configuration = Json::object([(
            "sources",
            Json::object([(
                "inventory",
                Json::object([
                    ("transport", Json::string("mcp")),
                    ("description", Json::string("inventory adapter")),
                    ("command", command),
                ]),
            )]),
        )]);
        let path = dir.join("sources.json");
        std::fs::write(&path, configuration.to_string()).unwrap();
        let mut viewer = Viewer::new(Some(Arc::new(Sources::read(&path).unwrap())));
        let reply = load(&mut viewer, "inventory", "site-a-stream");
        assert!(reply.refused, "{frame}");
        assert_eq!(reply.text.lines().next(), Some(text), "{frame}");
        assert!(reply.text.lines().count() > 1, "{frame}");
        assert_eq!(viewer.model.log().len(), 0);
    }
}

#[test]
fn an_http_source_refuses_with_code_source() {
    let dir = directory("http");
    let mut viewer = Viewer::new(Some(sources(&dir, None, 4_194_304)));
    let reply = load(&mut viewer, "store", "site-a");
    assert!(reply.refused);
    assert_eq!(reply.text, "load transport: source, valid: mcp, file");
    assert_eq!(viewer.model.log().len(), 0);
    let configuration = Json::object([(
        "sources",
        Json::object([(
            "store",
            Json::object([
                ("transport", Json::string("http")),
                ("description", Json::string("relations store")),
                ("url", Json::string("https://store.example.net/relations")),
                ("credential", Json::string("env:GALAXY_TEST_HTTP_UNSET")),
            ]),
        )]),
    )]);
    let path = dir.join("credential.json");
    std::fs::write(&path, configuration.to_string()).unwrap();
    let mut viewer = Viewer::new(Some(Arc::new(Sources::read(&path).unwrap())));
    let reply = load(&mut viewer, "store", "site-a");
    assert_eq!(reply.text, "load transport: source, valid: mcp, file");
}

#[test]
fn a_source_outside_the_configuration_and_a_refusal_of_the_source_refuse_the_load() {
    let dir = directory("no-source");
    let mut viewer = Viewer::new(Some(sources(&dir, None, 4_194_304)));
    let reply = load(&mut viewer, "rsim", "site-a");
    assert_eq!(
        reply.text,
        "load source: no_source, valid: files, inventory, store"
    );
    let reply = load(&mut viewer, "inventory", "site-b");
    assert_eq!(
        reply.text,
        "load galaxy_relations: source, valid: query: no document site-b"
    );
    let mut bare = Viewer::default();
    let reply = load(&mut bare, "inventory", "site-a");
    assert_eq!(
        reply.text,
        "load source: no_source, valid: a source of --sources <file>"
    );
    let reply = load(&mut viewer, "bad name", "site-a");
    assert_eq!(
        reply.text,
        "load source: no_source, valid: files, inventory, store"
    );
    let reply = call(&mut viewer, "load_view", r#"{"source":"inventory"}"#);
    assert!(reply.refused);
    assert_eq!(reply.text, "load query: missing, valid: source, query");
    let reply = call(
        &mut viewer,
        "load_view",
        r#"{"source":"inventory","query":7}"#,
    );
    assert_eq!(reply.text, "load query: type, valid: string");
    let long = "q".repeat(8193);
    let reply = load(&mut viewer, "inventory", &long);
    assert_eq!(reply.text, "load query: value, valid: 0 to 8192 characters");
    assert_eq!(viewer.model.log().len(), 0);
}

#[test]
fn sources_gives_the_name_and_the_description_of_each_source() {
    let dir = directory("sources");
    let mut viewer = Viewer::new(Some(sources(&dir, None, 4_194_304)));
    assert_eq!(
        call(&mut viewer, "sources", "{}").text,
        r#"{"sources":[{"name":"files","description":"saved documents"},{"name":"inventory","description":"inventory adapter"},{"name":"store","description":"relations store"}]}"#
    );
    assert_eq!(
        call(&mut Viewer::default(), "sources", "{}").text,
        r#"{"sources":[]}"#
    );
}

#[test]
fn the_configuration_checks_give_each_refusal() {
    let text = r#"{
        "sources": {
            "a b": {"transport": "file", "description": "x", "path": "."},
            "web": {"transport": "http", "description": "web", "url": "http://store.example.net/relations", "credential": "vault:token"},
            "local": {"transport": "http", "description": "local", "url": "http://127.0.0.1:8080/relations", "credential": "env:LOCAL_TOKEN"},
            "adapter": {"transport": "mcp", "description": "adapter", "command": [], "env": {"1TOKEN": "env:TOKEN", "TOKEN": "file:"}},
            "saved": {"transport": "ftp", "description": "saved"},
            "plain": {"transport": "file", "path": ".", "url": "https://store.example.net"}
        },
        "colour": "blue"
    }"#;
    let refusals: Vec<String> = Sources::from_json(&Json::parse(text).unwrap(), PathBuf::from("."))
        .unwrap_err()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        refusals,
        [
            "configuration colour: value, valid: sources",
            "configuration sources: value, valid: 1 to 64 ASCII letters, digits, -, _ and .",
            "sources.web url: value, valid: https://<host>/<path>, http:// for a loopback address",
            "sources.web credential: value, valid: env:<name> or file:<path>",
            "sources.adapter command: size, valid: 1 or more strings",
            "sources.adapter env.1TOKEN: value, valid: ASCII letters, digits and _, not a digit first",
            "sources.adapter env.TOKEN: value, valid: env:<name> or file:<path>",
            "sources.saved transport: value, valid: mcp, http, file",
            "sources.plain description: missing, valid: description",
            "sources.plain url: value, valid: transport, description, path",
        ]
    );
    let refusals = Sources::from_json(
        &Json::parse(r#"{"sources":[]}"#).unwrap(),
        PathBuf::from("."),
    )
    .unwrap_err();
    assert_eq!(
        refusals[0].to_string(),
        "configuration sources: type, valid: object"
    );
}

#[test]
fn the_mcp_server_lists_and_calls_sources_and_load_view() {
    let dir = directory("server");
    let mut viewer = Viewer::new(Some(sources(&dir, None, 4_194_304)));
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"sources"}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"load_view","arguments":{"source":"inventory","query":"site-a"}}}"#,
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"load_view","arguments":{"source":"store","query":"site-a"}}}"#,
    ]
    .join("\n");
    let mut output = Vec::new();
    mcp::serve(input.as_bytes(), &mut output, |name, arguments| {
        viewer.call(name, arguments, None, None)
    })
    .unwrap();
    let answers: Vec<Json> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| Json::parse(line).unwrap())
        .collect();
    let names: Vec<&str> = answers[0]
        .get("result")
        .unwrap()
        .get("tools")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool.get("name").unwrap().as_str().unwrap())
        .collect();
    assert!(names.contains(&"load_view") && names.contains(&"sources"));
    let result = |index: usize| {
        let result = answers[index].get("result").unwrap();
        let text = result.get("content").unwrap().as_array().unwrap()[0]
            .get("text")
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned();
        (text, result.get("isError") == Some(&Json::Bool(true)))
    };
    assert!(result(1).0.contains(r#""name":"inventory""#));
    let (text, error) = result(2);
    assert!(!error);
    assert_eq!(
        Json::parse(&text).unwrap().get("flows"),
        Some(&Json::Number(3.0))
    );
    assert_eq!(
        result(3),
        ("load transport: source, valid: mcp, file".to_owned(), true)
    );
    assert_eq!(viewer.world.flows().len(), 3);
}

/// A viewer with the source `inventory`, the fixture with `--frame frame`.
fn framed(name: &str, frame: &str) -> Viewer {
    let dir = directory(name);
    let command = Json::Array(
        [FIXTURE, documents().to_str().unwrap(), "--frame", frame]
            .map(Json::string)
            .to_vec(),
    );
    let configuration = Json::object([(
        "sources",
        Json::object([(
            "inventory",
            Json::object([
                ("transport", Json::string("mcp")),
                ("description", Json::string("inventory adapter")),
                ("command", command),
            ]),
        )]),
    )]);
    let path = dir.join("sources.json");
    std::fs::write(&path, configuration.to_string()).unwrap();
    Viewer::new(Some(Arc::new(Sources::read(&path).unwrap())))
}

#[test]
fn the_client_answers_ping_and_refuses_each_other_request_of_the_source() {
    let mut viewer = framed("ask", "ask");
    let reply = load(&mut viewer, "inventory", "site-a-stream");
    assert!(!reply.refused, "{}", reply.text);
}

#[test]
fn a_request_with_65_requests_of_the_source_is_refused() {
    let mut viewer = framed("flood", "flood");
    let reply = load(&mut viewer, "inventory", "site-a-stream");
    assert_eq!(
        reply.text,
        "load tools/call: source, valid: 64 requests of the source or fewer"
    );
    assert_eq!(viewer.model.log().len(), 0);
}

#[test]
fn pages_stop_at_the_limit_of_the_nodes() {
    let mut viewer = framed("endless", "endless");
    let reply = load(&mut viewer, "inventory", "site-a");
    assert_eq!(reply.text, "load page 11: size, valid: 1000000 nodes");
    assert_eq!(viewer.model.log().len(), 0);
}

#[test]
fn a_cursor_of_more_than_1024_characters_stops_the_pages_at_its_line() {
    let mut viewer = framed("cursor", "cursor");
    let reply = load(&mut viewer, "inventory", "site-a");
    assert_eq!(
        reply.text,
        "load line 2: size, valid: next.cursor of 1 to 1024 characters"
    );
}

#[test]
fn a_json_file_above_the_document_limit_is_refused_before_it_is_read() {
    let dir = directory("large-file");
    let files = dir.join("files");
    std::fs::create_dir_all(&files).unwrap();
    let file = std::fs::File::create(files.join("large.json")).unwrap();
    file.set_len(galaxy_scene::document::DOCUMENT_BYTES as u64 + 1)
        .unwrap();
    let configuration = Json::object([(
        "sources",
        Json::object([(
            "files",
            Json::object([
                ("transport", Json::string("file")),
                ("description", Json::string("saved documents")),
                ("path", Json::string("files")),
            ]),
        )]),
    )]);
    let path = dir.join("sources.json");
    std::fs::write(&path, configuration.to_string()).unwrap();
    let mut viewer = Viewer::new(Some(Arc::new(Sources::read(&path).unwrap())));
    let reply = load(&mut viewer, "files", "large");
    assert_eq!(reply.text, "document text: size, valid: 67108864 bytes");
}

#[test]
fn a_jsonl_file_above_the_stream_limit_is_refused_before_it_is_read() {
    let dir = directory("large-stream");
    let files = dir.join("files");
    std::fs::create_dir_all(&files).unwrap();
    let file = std::fs::File::create(files.join("large.jsonl")).unwrap();
    file.set_len(galaxy_native::fetch::STREAM as u64 + 1)
        .unwrap();
    let configuration = Json::object([(
        "sources",
        Json::object([(
            "files",
            Json::object([
                ("transport", Json::string("file")),
                ("description", Json::string("saved documents")),
                ("path", Json::string("files")),
            ]),
        )]),
    )]);
    let path = dir.join("sources.json");
    std::fs::write(&path, configuration.to_string()).unwrap();
    let mut viewer = Viewer::new(Some(Arc::new(Sources::read(&path).unwrap())));
    let reply = load(&mut viewer, "files", "large");
    assert_eq!(reply.text, "document text: size, valid: 268435456 bytes");
}

#[test]
fn the_log_option_writes_the_operation_log_as_an_operation_file() {
    let dir = directory("log-option");
    sources(&dir, None, 4_194_304);
    let ops = dir.join("ops.jsonl");
    std::fs::write(
        &ops,
        "{\"name\":\"set_look\",\"arguments\":{\"scheme\":\"dark\"}}\n\
         {\"name\":\"load_view\",\"arguments\":{\"source\":\"files\",\"query\":\"site-a\"}}\n",
    )
    .unwrap();
    let log = dir.join("log.jsonl");
    std::process::Command::new(env!("CARGO_BIN_EXE_galaxy-native"))
        .args(["--sources", dir.join("sources.json").to_str().unwrap()])
        .args(["--ops", ops.to_str().unwrap()])
        .args(["--log", log.to_str().unwrap()])
        .args(["--png", dir.join("frame.png").to_str().unwrap()])
        .args(["--width", "64", "--height", "48"])
        .output()
        .unwrap();
    let written = std::fs::read_to_string(&log).unwrap();
    let hash = &kept(&dir)[0];
    assert_eq!(
        written,
        format!(
            "{{\"name\":\"set_look\",\"arguments\":{{\"projection\":\"perspective\",\"space\":\"ground\",\"light\":\"sun\",\"lines\":\"crisp\",\"scheme\":\"dark\"}}}}\n\
             {{\"name\":\"load_view\",\"arguments\":{{\"source\":\"files\",\"query\":\"site-a\",\"sha256\":\"{hash}\"}}}}\n"
        )
    );
}
