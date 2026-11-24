use std::io::BufRead;
use std::sync::Arc;

use galaxy_render::{Camera, Renderer, Selection, View};
use galaxy_scene::{Change, Json, Load, Model, ObjectId, Style, World, wire};

use crate::load;
use crate::sources::Sources;

/// Size of the frame of the tool `frame` with no size in its arguments.
const FRAME: [u32; 2] = [1280, 800];

/// The tools of the viewer: undo, describe, frame and sources.
pub const TOOLS: &[wire::Tool] = &[
    wire::Tool {
        name: "undo",
        description: "Revert the last operation",
        schema: r#"{"type":"object","properties":{}}"#,
    },
    wire::Tool {
        name: "describe",
        description: "World text",
        schema: r#"{"type":"object","properties":{}}"#,
    },
    wire::Tool {
        name: "frame",
        description: "Image of the frame",
        schema: r#"{"type":"object","properties":{"width":{"type":"integer","minimum":16,"maximum":4096},"height":{"type":"integer","minimum":16,"maximum":4096}}}"#,
    },
    wire::Tool {
        name: "sources",
        description: "Sources",
        schema: r#"{"type":"object","properties":{}}"#,
    },
];

/// The answer to a tool call: its text, a PNG image, whether the call was refused and whether it
/// changed the world, and the lines for the standard error.
#[derive(Clone, Debug, Default)]
pub struct Reply {
    pub text: String,
    pub png: Option<Vec<u8>>,
    pub refused: bool,
    pub changed: bool,
    pub stderr: Vec<String>,
}

impl Reply {
    fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    fn refused(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            refused: true,
            ..Self::default()
        }
    }
}

/// A model with its world, the selection of the person and the sources of `load_view`.
pub struct Viewer {
    pub model: Model,
    pub world: World,
    pub selection: Option<Selection>,
    pub sources: Option<Arc<Sources>>,
    /// `true` when `world` is not the world of `model`.
    stale: bool,
    /// The renderer of `frame` with no renderer in its call, opened on its first use.
    renderer: Option<Renderer>,
}

impl Default for Viewer {
    fn default() -> Self {
        Self::new(None)
    }
}

impl Viewer {
    pub fn new(sources: Option<Arc<Sources>>) -> Self {
        let model = Model::new(Style::default());
        Self {
            world: model.world(),
            model,
            selection: None,
            sources,
            stale: false,
            renderer: None,
        }
    }

    /// Answers the tool `name` with `arguments`. `frame` draws with `renderer` from `camera`, or
    /// from the home view.
    pub fn call(
        &mut self,
        name: &str,
        arguments: &Json,
        renderer: Option<&Renderer>,
        camera: Option<Camera>,
    ) -> Reply {
        let reply = self.answer(name, arguments, renderer, camera, true);
        self.refresh();
        reply
    }

    /// Answers the tool `name` as `call` does, and leaves `world` stale after a change. With
    /// `lines` false, the text of an applied change is empty.
    fn answer(
        &mut self,
        name: &str,
        arguments: &Json,
        renderer: Option<&Renderer>,
        camera: Option<Camera>,
        lines: bool,
    ) -> Reply {
        match name {
            "undo" => match self.model.undo() {
                Some(change) => {
                    self.stale = true;
                    let (name, arguments) = wire::encode(&change);
                    Reply {
                        changed: true,
                        ..Reply::text(format!("undone {name} {arguments}"))
                    }
                }
                None => Reply::refused("log: no operation"),
            },
            "describe" => {
                self.refresh();
                Reply::text(self.describe())
            }
            "frame" => {
                self.refresh();
                self.frame(arguments, renderer, camera)
            }
            "sources" => Reply::text(match &self.sources {
                Some(sources) => sources.to_json().to_string(),
                None => r#"{"sources":[]}"#.to_owned(),
            }),
            "load_view" => self.load(arguments, load::Origin::Call),
            _ => {
                let change = match wire::decode(name, arguments) {
                    Ok(change) => change,
                    Err(refusal) => return Reply::refused(refusal.to_string()),
                };
                if let Err(refusal) = self.model.apply(change) {
                    return Reply::refused(refusal.to_string());
                }
                self.stale = true;
                if lines {
                    self.refresh();
                }
                let text = match (lines, name, arguments.get("name").and_then(Json::as_str)) {
                    (false, _, _) => String::new(),
                    (_, "define_shape", Some(shape)) => self.library_lines(shape, "shape"),
                    (_, "set_material", Some(material)) => self.library_lines(material, "material"),
                    (_, _, Some(name)) => self.line(name).unwrap_or_default(),
                    (_, _, None) => "applied".to_owned(),
                };
                Reply {
                    changed: true,
                    ..Reply::text(text)
                }
            }
        }
    }

    /// Applies the operations of a text of JSON lines, each `{"name":...,"arguments":...}`. A
    /// `load_view` with `sha256` replays the kept document of that hash. Gives the line number
    /// from 1 and the reply of each operation that is refused or has lines for the standard
    /// error. The world is expanded before each `describe` and `frame`, and after the last line.
    pub fn apply_ops(&mut self, input: impl BufRead) -> std::io::Result<Vec<(usize, Reply)>> {
        let replies = self.apply_lines(input);
        self.refresh();
        replies
    }

    fn apply_lines(&mut self, input: impl BufRead) -> std::io::Result<Vec<(usize, Reply)>> {
        let mut replies = Vec::new();
        for (index, line) in input.lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let reply = match Json::parse(&line) {
                Ok(call) => {
                    let name = call.get("name").and_then(Json::as_str).unwrap_or("");
                    let arguments = call.get("arguments").cloned().unwrap_or(Json::Null);
                    if name == "load_view" {
                        self.load(&arguments, load::Origin::Replay)
                    } else {
                        self.answer(name, &arguments, None, None, false)
                    }
                }
                Err(error) => Reply::refused(error.to_string()),
            };
            if reply.refused || !reply.stderr.is_empty() {
                replies.push((index + 1, reply));
            }
        }
        Ok(replies)
    }

    /// Reads and applies the load of the arguments of `load_view` from `origin`.
    fn load(&mut self, arguments: &Json, origin: load::Origin) -> Reply {
        match load::read(self.sources.as_deref(), arguments, origin) {
            Ok(load) => self.load_change(load),
            Err(refused) => Reply {
                stderr: refused.stderr,
                ..Reply::refused(refused.lines.join("\n"))
            },
        }
    }

    /// Applies a read load as 1 entry of the log, and answers with the answer of the load.
    pub fn apply_load(&mut self, load: Load) -> Reply {
        let reply = self.load_change(load);
        self.refresh();
        reply
    }

    /// Applies a read load as `apply_load` does, and leaves `world` stale.
    fn load_change(&mut self, load: Load) -> Reply {
        let answer = load.answer().to_json().to_string();
        if let Err(refusal) = self.model.apply(Change::Load(load)) {
            return Reply::refused(refusal.to_string());
        }
        self.stale = true;
        Reply {
            changed: true,
            ..Reply::text(answer)
        }
    }

    /// The operation log as JSON lines, each `{"name":...,"arguments":...}` of the wire form,
    /// a load with its `sha256`. An operation file of these lines replays the log.
    pub fn log(&self) -> String {
        self.log_from(0)
    }

    /// The lines of `log` from the entry `start` of the log.
    pub fn log_from(&self, start: usize) -> String {
        let mut text = String::new();
        for change in self.model.log().iter().skip(start) {
            let (name, arguments) = wire::encode(change);
            let line = Json::object([("name", Json::string(name)), ("arguments", arguments)]);
            text.push_str(&format!("{line}\n"));
        }
        text
    }

    /// The world text, then one line for each broken rule.
    pub fn describe(&self) -> String {
        let mut text = self.world.describe();
        for broken in self.world.broken() {
            text.push_str(&format!("broken {broken}\n"));
        }
        text
    }

    /// The line of the world text of the node or flow `name`, with no coordinates.
    pub fn line(&self, name: &str) -> Option<String> {
        let id = ObjectId::new(name)?;
        if let Some(index) = self.world.index(&id) {
            let (node, _, _) = self.world.node(index);
            let mut text = format!("{} {}", node.id, node.kind);
            if let Some(parent) = &node.parent {
                text.push_str(&format!(" in={parent}"));
            }
            for (field, value) in &node.fields {
                text.push_str(&format!(" {field}={value:?}"));
            }
            return Some(text);
        }
        let flow = self.world.flows().iter().find(|flow| flow.id == id)?;
        let path: Vec<&str> = flow.path.iter().map(ObjectId::as_str).collect();
        Some(format!(
            "{} flow path={}{}",
            flow.id,
            path.join(","),
            flow.data()
        ))
    }

    /// The lines of the world text of the defined shape or the material `name`, by `kind` shape
    /// or material.
    fn library_lines(&self, name: &str, kind: &str) -> String {
        let prefix = format!("{kind}:{name} ");
        self.world
            .library()
            .describe()
            .lines()
            .filter(|line| line.starts_with(&prefix))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The lines of the panel of the selection: the name, then a name and a value for the kind,
    /// the parent and each field of the source of a node, or for the outcome, its reason, each
    /// field of the source and each node of the path of a flow.
    pub fn details(&self) -> Vec<(String, String)> {
        let Some(selection) = &self.selection else {
            return Vec::new();
        };
        match selection {
            Selection::Node(id) => {
                let Some(index) = self.world.index(id) else {
                    return Vec::new();
                };
                let (node, _, _) = self.world.node(index);
                let mut lines = vec![
                    (String::new(), node.id.to_string()),
                    ("kind".to_owned(), node.kind.clone()),
                ];
                if let Some(parent) = &node.parent {
                    lines.push(("in".to_owned(), parent.to_string()));
                }
                lines.extend(node.fields.iter().cloned());
                lines
            }
            Selection::Flow(id) => {
                let Some(flow) = self.world.flows().iter().find(|flow| &flow.id == id) else {
                    return Vec::new();
                };
                let mut lines = vec![
                    (String::new(), flow.id.to_string()),
                    ("kind".to_owned(), "flow".to_owned()),
                ];
                if let Some(outcome) = &flow.outcome {
                    lines.push(("outcome".to_owned(), outcome.kind.name().to_owned()));
                    if let Some(reason) = &outcome.reason {
                        lines.push(("reason".to_owned(), reason.clone()));
                    }
                }
                lines.extend(flow.fields.iter().cloned());
                for (step, node) in flow.path.iter().enumerate() {
                    let name = if step == 0 { "path" } else { "" };
                    lines.push((format!("{name:<4}"), node.to_string()));
                }
                lines
            }
        }
    }

    /// Expands the world of the model when `world` is stale, and clears a selection that is not
    /// in it.
    fn refresh(&mut self) {
        if !self.stale {
            return;
        }
        self.stale = false;
        self.world = self.model.world();
        let present = match &self.selection {
            Some(Selection::Node(id)) => self.world.index(id).is_some(),
            Some(Selection::Flow(id)) => self.world.flows().iter().any(|flow| &flow.id == id),
            None => true,
        };
        if !present {
            self.selection = None;
        }
    }

    fn frame(
        &mut self,
        arguments: &Json,
        renderer: Option<&Renderer>,
        camera: Option<Camera>,
    ) -> Reply {
        let size = |field: &str, default: u32| match arguments.get(field) {
            None | Some(Json::Null) => Ok(default),
            Some(Json::Number(value))
                if value.fract() == 0.0 && (16.0..=4096.0).contains(value) =>
            {
                Ok(*value as u32)
            }
            Some(value) => Err(format!("{field}: {value}, valid: integer 16 to 4096")),
        };
        let (width, height) = match (size("width", FRAME[0]), size("height", FRAME[1])) {
            (Ok(width), Ok(height)) => (width, height),
            (Err(text), _) | (_, Err(text)) => return Reply::refused(text),
        };
        if let Some(renderer) = renderer {
            return self.draw(renderer, camera, width, height);
        }
        let own = match self.renderer.take() {
            Some(own) => own,
            None => match Renderer::new() {
                Ok(own) => own,
                Err(error) => return Reply::refused(error.to_string()),
            },
        };
        let reply = self.draw(&own, camera, width, height);
        self.renderer = Some(own);
        reply
    }

    /// The PNG image of the frame of `width` by `height` pixels from `camera`, or from the home
    /// view.
    fn draw(&self, renderer: &Renderer, camera: Option<Camera>, width: u32, height: u32) -> Reply {
        let look = self.model.style().look;
        let camera = camera
            .unwrap_or_else(|| Camera::home(&self.world, &look, width as f32 / height as f32));
        let details = self.details();
        let view = View {
            world: &self.world,
            look: &look,
            selection: self.selection.as_ref(),
            camera: &camera,
            details: &details,
        };
        let image = match renderer.render(&view, width, height) {
            Ok(image) => image,
            Err(error) => return Reply::refused(error.to_string()),
        };
        let mut png = Vec::new();
        if let Err(error) = image.write_png(&mut png) {
            return Reply::refused(error.to_string());
        }
        Reply {
            png: Some(png),
            ..Reply::text(format!("{width}x{height}"))
        }
    }
}
