use std::io::{BufRead, Write};

use galaxy_scene::{Json, wire};

use crate::viewer::{self, Reply};

const PROTOCOL: &str = "2025-06-18";

/// Serves the tools on MCP over JSON-RPC lines: reads requests from `input` until its end and
/// writes the answers to `output`. `call` answers a tool call.
pub fn serve(
    input: impl BufRead,
    mut output: impl Write,
    mut call: impl FnMut(&str, &Json) -> Reply,
) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let answer = match Json::parse(&line) {
            Ok(message) => answer(&message, &mut call),
            Err(error) => Some(failure(Json::Null, -32700, &error.to_string())),
        };
        if let Some(answer) = answer {
            writeln!(output, "{answer}")?;
            output.flush()?;
        }
    }
    Ok(())
}

/// The answer to a JSON-RPC message, or `None` for a notification.
fn answer(message: &Json, call: &mut impl FnMut(&str, &Json) -> Reply) -> Option<Json> {
    let id = message.get("id")?.clone();
    let method = message.get("method").and_then(Json::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or(Json::Null);
    let result = match method {
        "initialize" => {
            let version = params
                .get("protocolVersion")
                .and_then(Json::as_str)
                .unwrap_or(PROTOCOL);
            Json::object([
                ("protocolVersion", Json::string(version)),
                (
                    "capabilities",
                    Json::object([("tools", Json::Object(Vec::new()))]),
                ),
                (
                    "serverInfo",
                    Json::object([
                        ("name", Json::string("galaxy")),
                        ("version", Json::string(env!("CARGO_PKG_VERSION"))),
                    ]),
                ),
            ])
        }
        "ping" => Json::Object(Vec::new()),
        "tools/list" => {
            let tools = wire::TOOLS
                .iter()
                .chain(viewer::TOOLS)
                .map(|tool| {
                    Json::object([
                        ("name", Json::string(tool.name)),
                        ("description", Json::string(tool.description)),
                        (
                            "inputSchema",
                            Json::parse(tool.schema).unwrap_or(Json::Null),
                        ),
                    ])
                })
                .collect();
            Json::object([("tools", Json::Array(tools))])
        }
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Json::as_str) else {
                return Some(failure(id, -32602, "params.name: missing"));
            };
            let arguments = params.get("arguments").cloned().unwrap_or(Json::Null);
            let reply = call(name, &arguments);
            let mut content = vec![Json::object([
                ("type", Json::string("text")),
                ("text", Json::string(reply.text)),
            ])];
            if let Some(png) = reply.png {
                content.push(Json::object([
                    ("type", Json::string("image")),
                    ("data", Json::string(base64(&png))),
                    ("mimeType", Json::string("image/png")),
                ]));
            }
            Json::object([
                ("content", Json::Array(content)),
                ("isError", Json::Bool(reply.refused)),
            ])
        }
        _ => return Some(failure(id, -32601, &format!("no method {method}"))),
    };
    Some(Json::object([
        ("jsonrpc", Json::string("2.0")),
        ("id", id),
        ("result", result),
    ]))
}

fn failure(id: Json, code: i32, message: &str) -> Json {
    Json::object([
        ("jsonrpc", Json::string("2.0")),
        ("id", id),
        (
            "error",
            Json::object([
                ("code", Json::Number(f64::from(code))),
                ("message", Json::string(message)),
            ]),
        ),
    ])
}

/// The standard base64 text of `bytes`, with padding.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = chunk
            .iter()
            .enumerate()
            .fold(0u32, |value, (place, &byte)| {
                value | u32::from(byte) << (16 - 8 * place)
            });
        for place in 0..4 {
            if place <= chunk.len() {
                text.push(char::from(
                    ALPHABET[(value >> (18 - 6 * place) & 63) as usize],
                ));
            } else {
                text.push('=');
            }
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(lines: &[&str]) -> Vec<Json> {
        let input = lines.join("\n");
        let mut output = Vec::new();
        let mut viewer = crate::viewer::Viewer::default();
        serve(input.as_bytes(), &mut output, |name, arguments| {
            viewer.call(name, arguments, None, None)
        })
        .unwrap();
        String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| Json::parse(line).unwrap())
            .collect()
    }

    #[test]
    fn base64_of_0_to_3_bytes() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn initialize_and_tools_list_give_the_server_and_12_tools() {
        let answers = session(&[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        ]);
        assert_eq!(answers.len(), 2);
        let result = answers[0].get("result").unwrap();
        assert_eq!(
            result.get("protocolVersion"),
            Some(&Json::string("2025-03-26"))
        );
        let tools = answers[1].get("result").unwrap().get("tools").unwrap();
        let names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool.get("name").unwrap().as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "add_node",
                "add_flow",
                "set_rule",
                "set_look",
                "define_shape",
                "set_material",
                "clear",
                "load_view",
                "undo",
                "describe",
                "frame",
                "sources"
            ]
        );
    }

    #[test]
    fn tool_calls_change_the_world_and_a_refusal_is_an_error_result() {
        let answers = session(&[
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"add_node","arguments":{"name":"site-a","kind":"site"}}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"add_node","arguments":{"name":"dns","kind":"endpoint","in":"site-a","fields":{"ip":"192.0.2.53"}}}}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"add_node","arguments":{"name":"dns","kind":"endpoint"}}}"#,
            r#"{"jsonrpc":"2.0","id":"x","method":"tools/call","params":{"name":"undo"}}"#,
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"describe"}}"#,
            r#"{"jsonrpc":"2.0","id":6,"method":"resources/list"}"#,
        ]);
        let text = |answer: &Json| {
            let result = answer.get("result").unwrap();
            let content = result.get("content").unwrap().as_array().unwrap();
            let text = content[0].get("text").unwrap().as_str().unwrap().to_owned();
            (text, result.get("isError") == Some(&Json::Bool(true)))
        };
        assert_eq!(
            text(&answers[1]),
            ("dns endpoint in=site-a ip=\"192.0.2.53\"".to_owned(), false)
        );
        assert_eq!(text(&answers[2]), ("node dns exists".to_owned(), true));
        assert_eq!(answers[3].get("id"), Some(&Json::string("x")));
        assert!(text(&answers[3]).0.starts_with("undone add_node "));
        assert!(text(&answers[4]).0.starts_with("site-a site at="));
        assert!(!text(&answers[4]).0.contains("dns"));
        let error = answers[5].get("error").unwrap();
        assert_eq!(error.get("code"), Some(&Json::Number(-32601.0)));
    }

    #[test]
    fn frame_with_a_size_that_is_not_a_whole_number_from_16_to_4096_is_refused() {
        let mut viewer = crate::viewer::Viewer::default();
        for (arguments, text) in [
            (
                r#"{"width":"big"}"#,
                r#"width: "big", valid: integer 16 to 4096"#,
            ),
            (
                r#"{"width":100.5}"#,
                "width: 100.5, valid: integer 16 to 4096",
            ),
            (r#"{"height":8}"#, "height: 8, valid: integer 16 to 4096"),
        ] {
            let reply = viewer.call("frame", &Json::parse(arguments).unwrap(), None, None);
            assert!(reply.refused, "{arguments}");
            assert_eq!(reply.text, text);
        }
    }
}
