use std::io::BufRead;
use std::sync::Arc;

use galaxy_render::{Camera, Renderer, Selection, View};
use galaxy_scene::{Change, FlowLine, Json, Load, Model, ObjectId, Style, World, wire};

use crate::load;
use crate::sources::Sources;

/// Size of the frame of the tool `frame` with no size in its arguments.
const FRAME: [u32; 2] = [1280, 800];
/// The largest count of the nodes of a path whose panel shows each hop.
pub const HOPS: usize = 64;
/// The value of a panel line with no information.
const NOT_AVAILABLE: &str = "not available";

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
    /// The virtual time in seconds of each frame.
    pub time: f64,
    /// The index in the path of the selected flow of the hop in its panel, for a path of more
    /// than `HOPS` nodes.
    pub hop: usize,
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
            time: 0.0,
            hop: 0,
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
            if let Some(label) = &node.label {
                text.push_str(&format!(" label={label:?}"));
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
    /// the label, the parent and each field of the source of a node, the lines of `flow_details`
    /// for a flow, and the count of the flows, the sum of their magnitudes and the count of the
    /// flows with no magnitude for a trunk.
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
                if let Some(label) = node.label.as_deref().or(self.world.label(index)) {
                    lines.push(("label".to_owned(), label.to_owned()));
                }
                if let Some(parent) = &node.parent {
                    lines.push(("in".to_owned(), parent.to_string()));
                }
                lines.extend(node.fields.iter().cloned());
                lines
            }
            Selection::Flow(id) => match self.world.flows().iter().find(|flow| &flow.id == id) {
                Some(flow) => self.flow_details(flow),
                None => Vec::new(),
            },
            Selection::Trunk(ids) => self.trunk_details(ids),
        }
    }

    /// The lines of the panel of `flow`: its id, its kind, its outcome, its reason, its
    /// magnitude, the window of a count unit, its group with the count of its branches, its
    /// request, its replies, the data of the source, then its path with its hops. A path of up
    /// to `HOPS` nodes has 1 line for each node with the fields of its hop after it. A longer
    /// path has the count of its hops and the node and the fields of the hop `self.hop`.
    fn flow_details(&self, flow: &FlowLine) -> Vec<(String, String)> {
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
        match &flow.magnitude {
            Some(magnitude) => {
                lines.push((
                    "magnitude".to_owned(),
                    format!("{} {}", magnitude.value, magnitude.unit.name()),
                ));
                if magnitude.unit.is_count()
                    && let Some([from, to]) = self.world.window()
                {
                    lines.push(("window from".to_owned(), from.clone()));
                    lines.push(("window to".to_owned(), to.clone()));
                }
            }
            None if self.world.unit().is_some() => {
                lines.push(("magnitude".to_owned(), NOT_AVAILABLE.to_owned()));
            }
            None => {}
        }
        if let Some(group) = &flow.group {
            let branches = self
                .world
                .flows()
                .iter()
                .filter(|other| other.group.as_ref() == Some(group))
                .count();
            lines.push(("group".to_owned(), group.clone()));
            lines.push(("branches".to_owned(), branches.to_string()));
        }
        if let Some(request) = &flow.reply_to {
            lines.push(("reply to".to_owned(), request.to_string()));
        }
        let replies = self
            .world
            .flows()
            .iter()
            .filter(|other| other.reply_to.as_ref() == Some(&flow.id));
        for (index, reply) in replies.enumerate() {
            let name = if index == 0 { "replies" } else { "" };
            lines.push((format!("{name:<7}"), reply.id.to_string()));
        }
        lines.extend(flow.fields.iter().cloned());
        let hop_lines = |lines: &mut Vec<(String, String)>, step: usize| {
            if let Some(fields) = flow.hops.get(step) {
                for (name, value) in fields {
                    lines.push((format!("  {name}"), value.clone()));
                }
            }
        };
        if flow.path.len() <= HOPS {
            for (step, node) in flow.path.iter().enumerate() {
                let name = if step == 0 { "path" } else { "" };
                lines.push((format!("{name:<4}"), node.to_string()));
                hop_lines(&mut lines, step);
            }
        } else {
            let step = self.hop.min(flow.path.len() - 1);
            lines.push(("hops".to_owned(), flow.path.len().to_string()));
            lines.push(("node".to_owned(), flow.path[step].to_string()));
            hop_lines(&mut lines, step);
        }
        lines
    }

    /// The lines of the panel of the trunk of the flows `ids`: the count of its flows, the sum
    /// of their magnitudes with the unit of the world, and the count of the flows with no
    /// magnitude when it is above 0.
    fn trunk_details(&self, ids: &[ObjectId]) -> Vec<(String, String)> {
        let flows: Vec<&FlowLine> = self
            .world
            .flows()
            .iter()
            .filter(|flow| ids.contains(&flow.id))
            .collect();
        if flows.is_empty() {
            return Vec::new();
        }
        let mut lines = vec![
            (String::new(), "trunk".to_owned()),
            ("flows".to_owned(), flows.len().to_string()),
        ];
        let sum: f64 = flows
            .iter()
            .filter_map(|flow| flow.magnitude.map(|magnitude| magnitude.value))
            .sum();
        lines.push((
            "sum".to_owned(),
            match self.world.unit() {
                Some(unit) => format!("{sum} {}", unit.name()),
                None => NOT_AVAILABLE.to_owned(),
            },
        ));
        let none = flows.iter().filter(|flow| flow.magnitude.is_none()).count();
        if none > 0 {
            lines.push(("no magnitude".to_owned(), none.to_string()));
        }
        lines
    }

    /// The index in the path of the selected flow of the node whose centre is nearest to the
    /// pixel `pick` in a frame of `width` by `height` pixels from `camera`, the first on equal
    /// distances. 0 with no selected flow or no node in front of the camera.
    pub fn nearest_hop(&self, camera: &Camera, width: u32, height: u32, pick: [f32; 2]) -> usize {
        let Some(Selection::Flow(id)) = &self.selection else {
            return 0;
        };
        let Some(flow) = self.world.flows().iter().find(|flow| &flow.id == id) else {
            return 0;
        };
        let look = self.model.style().look;
        let mut nearest: Option<(usize, f32)> = None;
        for (step, &node) in flow.nodes.iter().enumerate() {
            let (_, placed, _) = self.world.node(node);
            let centre = placed.centre;
            let Some([x, y]) = camera.project(&look, centre, width, height) else {
                continue;
            };
            let distance = (x - pick[0]).hypot(y - pick[1]);
            if nearest.is_none_or(|(_, least)| distance < least) {
                nearest = Some((step, distance));
            }
        }
        nearest.map_or(0, |(step, _)| step)
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
            Some(Selection::Trunk(ids)) => ids
                .iter()
                .all(|id| self.world.flows().iter().any(|flow| &flow.id == id)),
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
    /// view, at the scale factor 1 and the time `self.time`. The text is the line
    /// `<width>x<height>`, then the lines of the labels of the frame.
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
            scale: 1.0,
            time: self.time,
        };
        let image = match renderer.render(&view, width, height) {
            Ok(image) => image,
            Err(error) => return Reply::refused(error.to_string()),
        };
        let mut png = Vec::new();
        if let Err(error) = image.write_png(&mut png) {
            return Reply::refused(error.to_string());
        }
        let mut lines = vec![format!("{width}x{height}")];
        lines.extend(renderer.labels(&view, width, height).lines());
        Reply {
            png: Some(png),
            ..Reply::text(lines.join("\n"))
        }
    }
}
