use std::path::{Path, PathBuf};
use std::sync::Arc;

use galaxy_native::sources::Sources;
use galaxy_native::viewer::{Reply, Viewer};
use galaxy_scene::{Json, sha256, wire};

const FIXTURE: &str = env!("CARGO_BIN_EXE_galaxy-fixture");
const TOKEN: &str = "example-token-5d1e7a0c42";
const OTHER: &str = "example-token-other-93b8";

fn documents() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/documents")
}

fn load(viewer: &mut Viewer, source: &str, query: &str) -> Reply {
    let arguments = Json::object([
        ("source", Json::string(source)),
        ("query", Json::string(query)),
    ]);
    viewer.call("load_view", &arguments, None, None)
}

/// The texts that leave the viewer: the reply, its lines for the standard error, the world text
/// and the wire form of each entry of the log.
fn texts(viewer: &Viewer, reply: &Reply) -> String {
    let mut text = format!(
        "{}\n{}\n{}",
        reply.text,
        reply.stderr.join("\n"),
        viewer.describe()
    );
    for change in viewer.model.log() {
        let (name, arguments) = wire::encode(change);
        text.push_str(&format!("\n{name} {arguments}"));
    }
    text
}

#[test]
fn the_credential_reaches_the_source_through_env_and_leaves_in_no_text() {
    let dir = std::env::temp_dir()
        .join("galaxy-native-tests")
        .join("credential");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("token"), format!("{TOKEN}\n")).unwrap();
    // SAFETY: this test binary has 1 test, and no other thread reads the environment.
    unsafe {
        std::env::set_var("GALAXY_TEST_TOKEN", TOKEN);
        std::env::set_var("GALAXY_TEST_OTHER", OTHER);
    }
    let fixture = |variable: &str, extra: &[&str]| {
        let mut command = vec![
            FIXTURE.to_owned(),
            documents().to_str().unwrap().to_owned(),
            "--credential".to_owned(),
            variable.to_owned(),
            sha256::hex(TOKEN.as_bytes()),
            "--absent".to_owned(),
            "GALAXY_TEST_OTHER".to_owned(),
        ];
        command.extend(extra.iter().map(|word| (*word).to_owned()));
        Json::Array(command.into_iter().map(Json::string).collect())
    };
    let source = |command: Json, env: Json| {
        Json::object([
            ("transport", Json::string("mcp")),
            ("description", Json::string("inventory adapter")),
            ("command", command),
            ("env", env),
        ])
    };
    let escaped = dir.join("escaped");
    std::fs::create_dir_all(&escaped).unwrap();
    let document = std::fs::read_to_string(documents().join("site-a.json"))
        .unwrap()
        .replace("\"192.0.2.1\"", &format!("\"\\u0065{}\"", &TOKEN[1..]));
    assert!(!document.contains(TOKEN));
    std::fs::write(escaped.join("leak.json"), document).unwrap();
    let escaped_command = Json::Array(
        [
            FIXTURE,
            escaped.to_str().unwrap(),
            "--credential",
            "FIXTURE_TOKEN",
            &sha256::hex(TOKEN.as_bytes()),
        ]
        .map(Json::string)
        .to_vec(),
    );
    let configuration = Json::object([(
        "sources",
        Json::object([
            (
                "escaped",
                source(
                    escaped_command,
                    Json::object([("FIXTURE_TOKEN", Json::string("env:GALAXY_TEST_TOKEN"))]),
                ),
            ),
            (
                "padded",
                source(
                    fixture(
                        "FIXTURE_TOKEN",
                        &["--echo", "FIXTURE_TOKEN", "--pad", "1001"],
                    ),
                    Json::object([("FIXTURE_TOKEN", Json::string("env:GALAXY_TEST_TOKEN"))]),
                ),
            ),
            (
                "by-file",
                source(
                    fixture("FIXTURE_TOKEN", &[]),
                    Json::object([("FIXTURE_TOKEN", Json::string("file:token"))]),
                ),
            ),
            (
                "by-env",
                source(
                    fixture("FIXTURE_TOKEN", &["--echo", "FIXTURE_TOKEN"]),
                    Json::object([("FIXTURE_TOKEN", Json::string("env:GALAXY_TEST_TOKEN"))]),
                ),
            ),
            (
                "unset",
                source(
                    fixture("FIXTURE_TOKEN", &[]),
                    Json::object([("FIXTURE_TOKEN", Json::string("env:GALAXY_TEST_UNSET"))]),
                ),
            ),
            (
                "other",
                source(
                    fixture("OTHER_TOKEN", &[]),
                    Json::object([("OTHER_TOKEN", Json::string("env:GALAXY_TEST_OTHER"))]),
                ),
            ),
        ]),
    )]);
    let path = dir.join("sources.json");
    std::fs::write(&path, configuration.to_string()).unwrap();
    let mut viewer = Viewer::new(Some(Arc::new(Sources::read(&path).unwrap())));

    for source in ["by-file", "by-env"] {
        let reply = load(&mut viewer, source, "site-a");
        assert!(!reply.refused, "{source}: {}", reply.text);
        let text = texts(&viewer, &reply);
        assert!(!text.contains(TOKEN) && !text.contains(OTHER));
    }

    let reply = load(&mut viewer, "by-env", "site-b");
    assert_eq!(
        reply.text,
        "load galaxy_relations: source, valid: a text of the source with no credential"
    );
    let reply = load(&mut viewer, "by-file", "refused");
    assert!(reply.refused);
    let text = texts(&viewer, &reply);
    assert!(!text.contains(TOKEN) && !text.contains(OTHER));

    let reply = load(&mut viewer, "escaped", "leak");
    assert_eq!(
        reply.text,
        "load document: source, valid: a document with no credential"
    );
    let text = texts(&viewer, &reply);
    assert!(!text.contains(TOKEN) && !text.contains(&TOKEN[1..]));
    let reply = load(&mut viewer, "padded", "site-b");
    assert_eq!(
        reply.text,
        "load galaxy_relations: source, valid: a text of the source with no credential"
    );

    let reply = load(&mut viewer, "unset", "site-a");
    assert_eq!(
        reply.text,
        "load env.FIXTURE_TOKEN: credential, valid: env:<name> or file:<path>"
    );
    let reply = load(&mut viewer, "other", "site-a");
    assert_eq!(
        reply.text,
        "load galaxy_relations: source, valid: credential: not valid"
    );
    assert_eq!(viewer.model.log().len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}
