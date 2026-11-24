# rsim-galaxy

A 3D engine for network visualisations, in Rust. An AI agent sends the operations, and the engine keeps the scene.

`STATUS.md` gives the state of each crate. `.mcp.json` registers the viewport as the MCP server `galaxy`. Its tools are the operations of the wire form and `undo`, `describe`, `frame` and `sources`.

`--sources <file.json>` names the sources of `load_view`:

```json
{
  "sources": {
    "saved": {"transport": "file", "description": "saved documents", "path": "documents"},
    "inventory": {
      "transport": "mcp",
      "description": "inventory adapter",
      "command": ["example-adapter", "--mcp"],
      "env": {"INVENTORY_TOKEN": "env:INVENTORY_TOKEN"}
    }
  }
}
```

A credential reference is `env:<name>` or `file:<path>`. A relative path and the command resolve in the directory of the file. The bytes of each loaded document stay in `galaxy-documents/<sha256>` next to the file. A `load_view` line with `sha256` in an operation file reads them and sends no request. `--log <file.jsonl>` writes the operation log as an operation file with the `sha256` of each load.

## Commands

| Task | Command |
|---|---|
| All tests | `cargo test --workspace` |
| Desktop window | `cargo run -p galaxy-native -- [--sources <file.json>] [--ops <file.jsonl>] [--log <file.jsonl>] [--mcp]` |
| Frame of an operation file | `cargo run -p galaxy-native -- [--sources <file.json>] --ops <file.jsonl> --png <file.png>` |
| Lint | `cargo clippy --workspace --all-targets` |
| Format check | `cargo fmt --all --check` |

## Licence

Apache-2.0. The text is in `LICENSE`.

The fonts Inter and JetBrains Mono in `crates/galaxy-render/fonts/` have the SIL Open Font License 1.1. The texts are in `OFL-Inter.txt` and `OFL-JetBrainsMono.txt` in that folder.
