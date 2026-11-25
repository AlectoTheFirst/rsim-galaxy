use std::collections::BTreeMap;
use std::fmt;
use std::sync::Mutex;

use crate::id::ObjectId;
use crate::library::Library;

/// A length for each of the axes x, y and z. The y axis points up.
pub type Vec3 = [f64; 3];

/// A node of the relations: its name, its kind, the node that contains it, its label and the
/// data of the source by field name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub id: ObjectId,
    pub kind: String,
    pub parent: Option<ObjectId>,
    /// The label text, 1 to `Node::LABEL` characters.
    pub label: Option<String>,
    pub fields: Vec<(String, String)>,
}

impl Node {
    /// The smallest and the largest number of characters of a label, counted as Unicode scalar
    /// values.
    pub const LABEL: [usize; 2] = [1, 256];

    /// `true` for a label text of 1 to 256 characters.
    pub fn valid_label(label: &str) -> bool {
        (Self::LABEL[0]..=Self::LABEL[1]).contains(&label.chars().count())
    }
}

/// The end of a flow at the last node of its path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OutcomeKind {
    /// The destination receives the flow.
    Deliver,
    /// The flow leaves the network that the source knows.
    Exit,
    /// The last node discards the flow.
    Drop,
    /// The flow returns to a node of its path.
    Loop,
    /// The end of the flow is not known.
    Unknown,
}

impl OutcomeKind {
    /// The kinds in the order of their index.
    pub const ALL: [OutcomeKind; 5] = [
        OutcomeKind::Deliver,
        OutcomeKind::Exit,
        OutcomeKind::Drop,
        OutcomeKind::Loop,
        OutcomeKind::Unknown,
    ];

    /// The names of the kinds in the order of their index.
    pub const NAMES: &'static [&'static str] = &["deliver", "exit", "drop", "loop", "unknown"];

    pub fn name(self) -> &'static str {
        Self::NAMES[self as usize]
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == text)
    }
}

/// The outcome of a flow: its kind and the reason of the source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub kind: OutcomeKind,
    pub reason: Option<String>,
}

/// A flow: the nodes of its path in order, from the source to the destination, its outcome and
/// the data of the source by field name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Flow {
    pub id: ObjectId,
    pub path: Vec<ObjectId>,
    pub outcome: Option<Outcome>,
    pub fields: Vec<(String, String)>,
}

/// Cause of a refused node or flow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelationRefusal {
    /// The name of the new node or flow is in use by a node, or by a flow when `flow` is `true`.
    Exists { id: ObjectId, flow: bool },
    /// The relations have no node with the name of the parent.
    NoParent { parent: ObjectId },
    /// The path of a flow names a node that the relations do not have.
    NoNode { node: ObjectId },
    /// The path of a flow has fewer than 2 nodes.
    ShortPath { count: usize },
    /// The path of a flow has the same node 2 times in sequence.
    Repeat { node: ObjectId },
}

impl fmt::Display for RelationRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RelationRefusal::Exists { id, flow } => {
                let holder = if *flow { "flow" } else { "node" };
                write!(f, "{holder} {id} exists")
            }
            RelationRefusal::NoParent { parent } => write!(f, "in: no node {parent}"),
            RelationRefusal::NoNode { node } => write!(f, "path: no node {node}"),
            RelationRefusal::ShortPath { count } => {
                write!(f, "path: {count} nodes, valid: 2 or more")
            }
            RelationRefusal::Repeat { node } => write!(f, "path: {node} follows {node}"),
        }
    }
}

impl std::error::Error for RelationRefusal {}

/// The nodes with their containment and the flows, in the order of their addition. A parent is
/// before its children.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Relations {
    nodes: Vec<Node>,
    index: BTreeMap<ObjectId, usize>,
    flows: Vec<Flow>,
    flow_index: BTreeMap<ObjectId, usize>,
}

impl Relations {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a node with no data of the source. A refused node changes nothing.
    pub fn add(
        &mut self,
        id: ObjectId,
        kind: &str,
        parent: Option<ObjectId>,
    ) -> Result<(), RelationRefusal> {
        self.add_with_fields(id, kind, parent, Vec::new())
    }

    /// Adds a node with the data of the source and no label. A refused node changes nothing.
    pub fn add_with_fields(
        &mut self,
        id: ObjectId,
        kind: &str,
        parent: Option<ObjectId>,
        fields: Vec<(String, String)>,
    ) -> Result<(), RelationRefusal> {
        self.add_node(Node {
            id,
            kind: kind.to_owned(),
            parent,
            label: None,
            fields,
        })
    }

    /// Adds `node`. The caller checks the label. A refused node changes nothing.
    pub fn add_node(&mut self, node: Node) -> Result<(), RelationRefusal> {
        if let Some(refusal) = self.in_use(&node.id) {
            return Err(refusal);
        }
        if let Some(parent) = &node.parent
            && !self.index.contains_key(parent)
        {
            return Err(RelationRefusal::NoParent {
                parent: parent.clone(),
            });
        }
        self.index.insert(node.id.clone(), self.nodes.len());
        self.nodes.push(node);
        Ok(())
    }

    /// Adds a flow along `path` with no outcome and no data of the source. A refused flow
    /// changes nothing.
    pub fn add_flow(&mut self, id: ObjectId, path: Vec<ObjectId>) -> Result<(), RelationRefusal> {
        self.add_flow_with_data(id, path, None, Vec::new())
    }

    /// Adds a flow along `path` with its outcome and the data of the source. An empty reason of
    /// the outcome is no reason. A refused flow changes nothing.
    pub fn add_flow_with_data(
        &mut self,
        id: ObjectId,
        path: Vec<ObjectId>,
        outcome: Option<Outcome>,
        fields: Vec<(String, String)>,
    ) -> Result<(), RelationRefusal> {
        if let Some(refusal) = self.in_use(&id) {
            return Err(refusal);
        }
        if path.len() < 2 {
            return Err(RelationRefusal::ShortPath { count: path.len() });
        }
        if let Some(node) = path.iter().find(|node| !self.index.contains_key(*node)) {
            return Err(RelationRefusal::NoNode { node: node.clone() });
        }
        if let Some(pair) = path.windows(2).find(|pair| pair[0] == pair[1]) {
            return Err(RelationRefusal::Repeat {
                node: pair[0].clone(),
            });
        }
        let outcome = outcome.map(|outcome| Outcome {
            reason: outcome.reason.filter(|reason| !reason.is_empty()),
            ..outcome
        });
        self.flow_index.insert(id.clone(), self.flows.len());
        self.flows.push(Flow {
            id,
            path,
            outcome,
            fields,
        });
        Ok(())
    }

    /// The nodes in the order of their addition.
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// The flows in the order of their addition.
    pub fn flows(&self) -> &[Flow] {
        &self.flows
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.flows.is_empty()
    }

    /// The refusal of a new node or flow with the name `id` when a node or a flow holds it.
    fn in_use(&self, id: &ObjectId) -> Option<RelationRefusal> {
        let flow = if self.index.contains_key(id) {
            false
        } else if self.flow_index.contains_key(id) {
            true
        } else {
            return None;
        };
        Some(RelationRefusal::Exists {
            id: id.clone(),
            flow,
        })
    }
}

/// Places of the children of a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrangement {
    /// Side by side along x.
    Row,
    /// One on the other along y.
    Stack,
    /// Rows along x, then along z, in the order of the children. Each child keeps its own size.
    /// A row ends before the child that makes it wider than a square of the children with their
    /// gaps. With children of 1 size, a row has the smallest count whose square holds them.
    Grid,
    /// Columns along x by the step of the child on the flows, then along z in the order of the
    /// children. The steps of the flows count in the order of the flows, each but a step that
    /// closes a cycle with the steps before it. A child that no counted step enters is in the
    /// first column, and a counted step from a child in column n goes to column n + 1 or further.
    Layers,
}

/// The form of a node in its box: a built-in shape of the mesh registry by name, or a defined
/// shape by its index in `Library::shapes`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Shape {
    /// The box.
    #[default]
    Box,
    /// The upright cylinder in the box, with an elliptic base of the width and depth of the box.
    Cylinder,
    /// The puck with a cross of 4 arrows on its top, 2 inward and 2 outward.
    Router,
    /// The slab with 4 parallel arrows along x on its top, alternating right and left.
    Switch,
    /// The brick wall with a flame above it.
    Firewall,
    /// The box of a generic device.
    Device,
    /// The low rounded slab with a dot and the arcs of a radio wave on each side of it on its
    /// top.
    AccessPoint,
    /// The monitor on a neck and a foot, with a tower beside it.
    Workstation,
    /// The upright cloud of 4 lobes on a flat base.
    Cloud,
    /// The square slab with a cross of 4 arrows on its top, each pointing outward on x or z.
    LayerThreeSwitch,
    /// The upright tower with the lines of drive bays on its front.
    Server,
    /// The open laptop: a thin base with a keyboard, and a screen tilted back with its face toward
    /// +z.
    Laptop,
    /// The slab with 1 arrow on its top that splits into 3 arrows.
    LoadBalancer,
    /// The defined shape at this index of `Library::shapes`.
    Defined(usize),
}

impl Shape {
    /// The built-in shapes in the order of `NAMES`.
    pub const ALL: [Shape; 13] = [
        Shape::Box,
        Shape::Cylinder,
        Shape::Router,
        Shape::Switch,
        Shape::Firewall,
        Shape::Device,
        Shape::AccessPoint,
        Shape::Workstation,
        Shape::Cloud,
        Shape::LayerThreeSwitch,
        Shape::Server,
        Shape::Laptop,
        Shape::LoadBalancer,
    ];

    /// The names of the built-in shapes in the order of `ALL`.
    pub const NAMES: &'static [&'static str] = &[
        "box",
        "cylinder",
        "router",
        "switch",
        "firewall",
        "device",
        "access-point",
        "workstation",
        "cloud",
        "layer3-switch",
        "server",
        "laptop",
        "load-balancer",
    ];

    /// The name of a built-in shape, `None` for a defined shape.
    pub fn name(self) -> Option<&'static str> {
        Self::ALL
            .iter()
            .position(|shape| *shape == self)
            .map(|index| Self::NAMES[index])
    }

    /// The built-in shape `text`.
    pub fn parse(text: &str) -> Option<Self> {
        Self::NAMES
            .iter()
            .position(|name| *name == text)
            .map(|index| Self::ALL[index])
    }

    /// `true` for a built-in shape with a symbol on its top face.
    pub fn has_top_symbol(self) -> bool {
        matches!(
            self,
            Shape::Router
                | Shape::Switch
                | Shape::AccessPoint
                | Shape::LayerThreeSwitch
                | Shape::LoadBalancer
        )
    }
}

/// The rule of a kind.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rule {
    /// Places of the children.
    pub arrangement: Arrangement,
    /// Form of the node.
    pub shape: Shape,
    /// Size of a node with no child.
    pub size: Vec3,
    /// Distance between 2 children.
    pub gap: f64,
    /// Distance between the children and the surface of the node.
    pub margin: f64,
    /// `true` for a label on each node of the kind.
    pub label: bool,
}

/// The rule of each kind, the rule of a kind with no rule, and the rule that places the nodes
/// with no parent.
#[derive(Clone, Debug, PartialEq)]
pub struct RuleSet {
    pub root: Rule,
    pub default: Rule,
    pub kinds: BTreeMap<String, Rule>,
}

impl RuleSet {
    fn rule(&self, kind: &str) -> &Rule {
        self.kinds.get(kind).unwrap_or(&self.default)
    }
}

/// The box of a node: its centre and its size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub centre: Vec3,
    pub size: Vec3,
}

/// A rule of the visual contract that a world breaks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Broken {
    /// A child is not inside its parent.
    Outside { child: ObjectId, parent: ObjectId },
    /// 2 nodes with the same parent intersect.
    Intersect { first: ObjectId, second: ObjectId },
    /// The line of a flow goes through a node that is not on its path and does not contain a
    /// node of its path.
    Crosses { flow: ObjectId, node: ObjectId },
}

impl fmt::Display for Broken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Broken::Outside { child, parent } => write!(f, "{child} is outside {parent}"),
            Broken::Intersect { first, second } => write!(f, "{first} and {second} intersect"),
            Broken::Crosses { flow, node } => write!(f, "flow {flow} crosses {node}"),
        }
    }
}

/// The line of a flow: the points of its polyline in its lanes, from the source to the
/// destination.
#[derive(Clone, Debug, PartialEq)]
pub struct FlowLine {
    pub id: ObjectId,
    pub path: Vec<ObjectId>,
    /// The indexes of the nodes of the path.
    pub nodes: Vec<usize>,
    pub points: Vec<Vec3>,
    /// The smallest distance from the line to the line of an adjacent lane.
    pub spacing: f64,
    /// The distance from each segment of the line to the line of an adjacent lane.
    pub gaps: Vec<f64>,
    pub outcome: Option<Outcome>,
    pub fields: Vec<(String, String)>,
}

/// A polyline in its lanes, the smallest distance from it to the line of an adjacent lane, and
/// that distance for each of its segments: `LANE` for a segment with no adjacent lane.
#[derive(Clone, Debug, PartialEq)]
pub struct Laned {
    pub points: Vec<Vec3>,
    pub spacing: f64,
    pub gaps: Vec<f64>,
}

impl FlowLine {
    /// The outcome, its reason and the data of the source of the flow as text: a space before
    /// each name and value.
    pub fn data(&self) -> String {
        let mut text = String::new();
        if let Some(outcome) = &self.outcome {
            text.push_str(&format!(" outcome={}", outcome.kind.name()));
            if let Some(reason) = &outcome.reason {
                text.push_str(&format!(" reason={reason:?}"));
            }
        }
        for (name, value) in &self.fields {
            text.push_str(&format!(" {name}={value:?}"));
        }
        text
    }
}

/// The boxes of the nodes of relations by a rule set.
#[derive(Clone, Debug, PartialEq)]
pub struct World {
    nodes: Vec<Node>,
    placed: Vec<Placed>,
    index: BTreeMap<ObjectId, usize>,
    parents: Vec<Option<usize>>,
    rules: Vec<Rule>,
    has_children: Vec<bool>,
    children: Vec<Vec<usize>>,
    top: Vec<usize>,
    root: Rule,
    roots: Placed,
    /// The column of each node in a parent with the arrangement layers.
    columns: Vec<usize>,
    /// The columns of each parent with the arrangement layers, `None` for the nodes with no
    /// parent.
    layered: BTreeMap<Option<usize>, Layered>,
    flows: Vec<FlowLine>,
    library: Library,
    kept: Kept,
}

/// The columns of a parent with the arrangement layers.
#[derive(Clone, Debug, PartialEq)]
struct Layered {
    /// The members of each column in the order of the children.
    columns: Vec<Vec<usize>>,
    /// The lowest centre on y less a quarter of the height of the children.
    height: f64,
}

/// The paths of a call of `World::lanes` with its lines.
type Lanes = (Vec<Vec<usize>>, Vec<Laned>);

/// The paths of the last call of `World::lanes` with its lines. Any 2 values compare equal. A
/// world compares equal with no regard to its kept lines.
#[derive(Default)]
struct Kept(Mutex<Option<Lanes>>);

impl Clone for Kept {
    fn clone(&self) -> Self {
        Self(Mutex::new(self.0.lock().ok().and_then(|kept| kept.clone())))
    }
}

impl fmt::Debug for Kept {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Kept")
    }
}

impl PartialEq for Kept {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl World {
    /// Sizes each node from its children, places each node in its parent and routes each flow. The
    /// nodes with no parent stand on the plane y = 0 around the origin.
    ///
    /// The children of a node with the arrangement row, grid or layers leave it upward, and the
    /// children of a node with the arrangement stack leave it toward +x. The margin of a node on
    /// that side is its corridor: a line goes from a child to the corridor through a door on the
    /// surface of the child, and along the corridor to the next child.
    pub fn expand(relations: &Relations, rules: &RuleSet) -> Self {
        let nodes = &relations.nodes;
        let mut roots = Vec::new();
        let mut children = vec![Vec::new(); nodes.len()];
        for (index, node) in nodes.iter().enumerate() {
            match &node.parent {
                Some(parent) => children[relations.index[parent]].push(index),
                None => roots.push(index),
            }
        }

        let parent_of: Vec<Option<usize>> = nodes
            .iter()
            .map(|node| node.parent.as_ref().map(|parent| relations.index[parent]))
            .collect();
        let mut links = vec![Vec::new(); nodes.len()];
        let mut root_links = Vec::new();
        for flow in &relations.flows {
            for pair in flow.path.windows(2) {
                let chain = |index: usize| {
                    let mut chain = vec![index];
                    while let Some(parent) = parent_of[*chain.last().unwrap_or(&index)] {
                        chain.push(parent);
                    }
                    chain.reverse();
                    chain
                };
                let from = chain(relations.index[&pair[0]]);
                let to = chain(relations.index[&pair[1]]);
                let shared = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
                if shared == from.len() || shared == to.len() {
                    continue;
                }
                let link = (from[shared], to[shared]);
                match shared {
                    0 => root_links.push(link),
                    _ => links[from[shared - 1]].push(link),
                }
            }
        }

        let mut sizes = vec![[0.0; 3]; nodes.len()];
        let mut offsets = vec![[0.0; 3]; nodes.len()];
        let mut columns = vec![0; nodes.len()];
        for index in (0..nodes.len()).rev() {
            let rule = rules.rule(&nodes[index].kind);
            if children[index].is_empty() {
                sizes[index] = rule.size;
                continue;
            }
            let extent = arrange(
                rule,
                &children[index],
                &links[index],
                &sizes,
                &mut offsets,
                &mut columns,
            );
            sizes[index] = extent.map(|length| length + 2.0 * rule.margin);
        }
        let extent = arrange(
            &rules.root,
            &roots,
            &root_links,
            &sizes,
            &mut offsets,
            &mut columns,
        );

        let mut placed = vec![
            Placed {
                centre: [0.0; 3],
                size: [0.0; 3],
            };
            nodes.len()
        ];
        let ground = [0.0, extent[1] / 2.0, 0.0];
        for (index, node) in nodes.iter().enumerate() {
            let origin = match &node.parent {
                Some(parent) => placed[relations.index[parent]].centre,
                None => ground,
            };
            placed[index] = Placed {
                centre: [0, 1, 2].map(|axis| origin[axis] + offsets[index][axis]),
                size: sizes[index],
            };
        }
        let mut layered = BTreeMap::new();
        let groups = std::iter::once((None, &rules.root, &roots)).chain(
            children
                .iter()
                .enumerate()
                .filter(|(_, list)| !list.is_empty())
                .map(|(index, list)| (Some(index), rules.rule(&nodes[index].kind), list)),
        );
        for (parent, rule, siblings) in groups {
            if rule.arrangement != Arrangement::Layers {
                continue;
            }
            let count = siblings.iter().map(|&node| columns[node] + 1).max();
            let mut members = vec![Vec::new(); count.unwrap_or(0)];
            for &node in siblings {
                members[columns[node]].push(node);
            }
            let height = siblings
                .iter()
                .map(|&node| placed[node].centre[1] - placed[node].size[1] / 4.0)
                .fold(f64::MAX, f64::min);
            layered.insert(
                parent,
                Layered {
                    columns: members,
                    height,
                },
            );
        }
        let mut world = Self {
            nodes: nodes.clone(),
            placed,
            index: relations.index.clone(),
            parents: parent_of,
            rules: nodes.iter().map(|node| *rules.rule(&node.kind)).collect(),
            has_children: children.iter().map(|list| !list.is_empty()).collect(),
            children,
            top: roots,
            root: rules.root,
            roots: Placed {
                centre: ground,
                size: extent,
            },
            columns,
            layered,
            flows: Vec::new(),
            library: Library::default(),
            kept: Kept::default(),
        };
        let paths: Vec<Vec<usize>> = relations
            .flows
            .iter()
            .map(|flow| flow.path.iter().map(|id| relations.index[id]).collect())
            .collect();
        let lines = world.lanes(&paths);
        world.flows = relations
            .flows
            .iter()
            .zip(paths)
            .zip(lines)
            .map(|((flow, nodes), line)| FlowLine {
                id: flow.id.clone(),
                path: flow.path.clone(),
                nodes,
                points: line.points,
                spacing: line.spacing,
                gaps: line.gaps,
                outcome: flow.outcome.clone(),
                fields: flow.fields.clone(),
            })
            .collect();
        world
    }

    /// The nodes in the order of the relations, each with its box and with `true` for a node
    /// with children.
    pub fn nodes(&self) -> impl Iterator<Item = (&Node, &Placed, bool)> {
        self.nodes
            .iter()
            .zip(&self.placed)
            .zip(&self.has_children)
            .map(|((node, placed), &has_children)| (node, placed, has_children))
    }

    /// The lines of the flows in the order of the relations.
    pub fn flows(&self) -> &[FlowLine] {
        &self.flows
    }

    /// The count of the nodes. The index of a node is its place in the order of the relations.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The shape of the node `index`, from the rule of its kind.
    pub fn shape(&self, index: usize) -> Shape {
        self.rules[index].shape
    }

    /// The rule of the kind of the node `index` in the rule set of the world.
    pub fn rule(&self, index: usize) -> &Rule {
        &self.rules[index]
    }

    /// The label text of the node at `index`: its own label, else its id. `None` when the rule
    /// of its kind has `label` false.
    pub fn label(&self, index: usize) -> Option<&str> {
        if !self.rules[index].label {
            return None;
        }
        let node = &self.nodes[index];
        Some(node.label.as_deref().unwrap_or(node.id.as_str()))
    }

    /// The defined shapes and the materials of the style of the world.
    pub fn library(&self) -> &Library {
        &self.library
    }

    /// The world with the defined shapes and the materials of `library`.
    pub fn with_library(mut self, library: Library) -> Self {
        self.library = library;
        self
    }

    /// The node at `index` with its box and with `true` for a node with children.
    pub fn node(&self, index: usize) -> (&Node, &Placed, bool) {
        (
            &self.nodes[index],
            &self.placed[index],
            self.has_children[index],
        )
    }

    /// The index of the node `id`.
    pub fn index(&self, id: &ObjectId) -> Option<usize> {
        self.index.get(id).copied()
    }

    /// The index of the parent of the node at `index`.
    pub fn parent(&self, index: usize) -> Option<usize> {
        self.parents[index]
    }

    /// The indexes of the children of the node at `index`, in the order of the relations.
    pub fn children(&self, index: usize) -> &[usize] {
        &self.children[index]
    }

    /// The indexes of the nodes with no parent, in the order of the relations.
    pub fn top(&self) -> &[usize] {
        &self.top
    }

    /// The polyline through the nodes at the indexes of `path`, with no point 2 times in
    /// sequence.
    pub fn line(&self, path: &[usize]) -> Vec<Vec3> {
        self.base(path).into_iter().map(|step| step.point).collect()
    }

    /// The lines along `paths` in lanes. The segments of the lines from `World::line` that lie
    /// on 1 line along 1 axis, each in 1 step of its path, with a common stretch of their extent
    /// in their lanes, are 1 bundle. The bundles grow in rounds until no 2 bundles have a common
    /// stretch. Each segment of a bundle has its own lane. The order of the lanes follows the
    /// side from which a line comes at the low end of the bundle, then the lane of the run
    /// before the low end, then the side to which the line goes at the high end, then the lane
    /// of the run after the high end. The lanes of a bundle lie side by side around the line from
    /// `World::line`: along z for a segment along x, along x for a segment along z, and along x
    /// and -z for a segment along y. 2 adjacent lanes of a bundle are `LANE` apart, or less so
    /// that the outermost lane stays in the room of each segment of the bundle and nearer than
    /// half the distance to a parallel segment beside it. A line changes lane at the corner of a
    /// run along y and a run along x or z, or in the segment along y or along x or z next to that
    /// corner, at 1/2, 1/4 or 3/4 of the segment from the corner, where that removes a common
    /// stretch with another line. The world keeps the lines of the last `paths`.
    pub fn lanes(&self, paths: &[Vec<usize>]) -> Vec<Laned> {
        if let Ok(kept) = self.kept.0.lock()
            && let Some((kept_paths, lines)) = kept.as_ref()
            && kept_paths.as_slice() == paths
        {
            return lines.clone();
        }
        let lines = self.lay(paths);
        if let Ok(mut kept) = self.kept.0.lock() {
            *kept = Some((paths.to_vec(), lines.clone()));
        }
        lines
    }

    /// The lines along `paths` in lanes, by the rules of `World::lanes`.
    fn lay(&self, paths: &[Vec<usize>]) -> Vec<Laned> {
        let bases: Vec<Vec<Step>> = paths.iter().map(|path| self.base(path)).collect();
        let mut runs: Vec<Run> = Vec::new();
        let mut owners: Vec<Vec<usize>> = Vec::with_capacity(bases.len());
        for (flow, steps) in bases.iter().enumerate() {
            let first_run = runs.len();
            let points: Vec<Vec3> = steps.iter().map(|step| step.point).collect();
            let mut owner: Vec<usize> = Vec::with_capacity(steps.len().saturating_sub(1));
            for index in 1..steps.len() {
                let (start, end) = (points[index - 1], points[index]);
                let axis = along(start, end);
                let sign = (end[axis] - start[axis]).signum();
                let step = &steps[index];
                let toward = step.lanes.map_or(lateral(axis), unit);
                let current = owner.last().copied().filter(|&run| {
                    let run = &runs[run];
                    run.axis == axis
                        && run.toward == toward
                        && run.sign == sign
                        && run.hop == step.hop
                        && (0..3).all(|q| q == axis || (run.line[q] - start[q]).abs() <= TOLERANCE)
                });
                match current {
                    Some(run) => {
                        let run = &mut runs[run];
                        run.low = run.low.min(end[axis]);
                        run.high = run.high.max(end[axis]);
                        run.room = run.room.min(step.room);
                        run.last = index;
                        owner.push(owner[owner.len() - 1]);
                    }
                    None => {
                        owner.push(runs.len());
                        runs.push(Run {
                            flow,
                            axis,
                            toward,
                            sign,
                            hop: step.hop,
                            line: start,
                            low: start[axis].min(end[axis]),
                            high: start[axis].max(end[axis]),
                            room: step.room,
                            first: index - 1,
                            last: index,
                            sides: [0.0; 2],
                            offset: 0.0,
                            gap: LANE,
                            before: None,
                            after: None,
                        });
                    }
                }
            }
            for run in &mut runs[first_run..] {
                let toward = run.toward;
                let back = side(&points, run.first, (0..run.first).rev(), toward);
                let ahead = side(&points, run.last, run.last + 1..points.len(), toward);
                run.sides = if run.sign > 0.0 {
                    [back, ahead]
                } else {
                    [ahead, back]
                };
            }
            owners.push(owner);
        }

        for steps in &owners {
            for (segment, &run) in steps.iter().enumerate() {
                if runs[run].first == segment && segment > 0 {
                    runs[run].before = Some(steps[segment - 1]);
                }
                if runs[run].last == segment + 1 && segment + 1 < steps.len() {
                    runs[run].after = Some(steps[segment + 1]);
                }
            }
        }
        let mut tracks: BTreeMap<(usize, [i64; 2], [i64; 3]), Vec<usize>> = BTreeMap::new();
        for (index, run) in runs.iter().enumerate() {
            let grain = |value: f64| (value / GRAIN).round() as i64;
            let others = [
                grain(run.line[(run.axis + 1) % 3]),
                grain(run.line[(run.axis + 2) % 3]),
            ];
            tracks
                .entry((run.axis, others, run.toward.map(grain)))
                .or_default()
                .push(index);
        }
        let mut planes: BTreeMap<(usize, [i64; 3], i64), Vec<usize>> = BTreeMap::new();
        for (index, run) in runs.iter().enumerate() {
            let grain = |value: f64| (value / GRAIN).round() as i64;
            planes
                .entry((run.axis, run.toward.map(grain), grain(plane(run))))
                .or_default()
                .push(index);
        }
        for members in planes.values_mut() {
            members.sort_by(|&first, &second| {
                across(&runs[first])
                    .total_cmp(&across(&runs[second]))
                    .then(first.cmp(&second))
            });
        }
        let mut bundle_of: Vec<usize> = (0..runs.len()).collect();
        let mut spacing = vec![LANE; bases.len()];
        for _ in 0..ROUNDS {
            let reaches: Vec<[f64; 2]> = (0..runs.len()).map(|run| reach(&runs, run)).collect();
            let mut changed = false;
            for members in tracks.values_mut() {
                members.sort_by(|&first, &second| {
                    reaches[first][0]
                        .total_cmp(&reaches[second][0])
                        .then(runs[first].flow.cmp(&runs[second].flow))
                        .then(runs[first].first.cmp(&runs[second].first))
                });
                let mut held: Option<(usize, f64)> = None;
                for &member in members.iter() {
                    let [low, high] = reaches[member];
                    match held {
                        Some((bundle, reach)) if low < reach - TOLERANCE => {
                            changed |= join(&mut bundle_of, bundle, member);
                            held = Some((bundle, reach.max(high)));
                        }
                        _ => held = Some((member, high)),
                    }
                }
            }
            let mut rooms: Vec<f64> = runs.iter().map(|run| run.room).collect();
            for members in planes.values() {
                for (place, &member) in members.iter().enumerate() {
                    let here = across(&runs[member]);
                    let [low, high] = reaches[member];
                    let mut room = rooms[member];
                    let sides: [&mut dyn Iterator<Item = &usize>; 2] = [
                        &mut members[..place].iter().rev(),
                        &mut members[place + 1..].iter(),
                    ];
                    for others in sides {
                        for &other in others {
                            let distance = (across(&runs[other]) - here).abs();
                            if distance <= GRAIN {
                                continue;
                            }
                            if distance * ROOM / 2.0 >= room {
                                break;
                            }
                            let [other_low, other_high] = reaches[other];
                            if other_low <= high + TOLERANCE && low <= other_high + TOLERANCE {
                                room = distance * ROOM / 2.0;
                            }
                        }
                    }
                    rooms[member] = room;
                }
            }
            let mut bundles: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
            for run in 0..runs.len() {
                let bundle = find(&mut bundle_of, run);
                bundles.entry(bundle).or_default().push(run);
            }
            spacing = vec![LANE; bases.len()];
            for mut bundle in bundles.into_values() {
                if bundle.len() < 2 {
                    continue;
                }
                let room = bundle
                    .iter()
                    .map(|&member| rooms[member])
                    .fold(f64::MAX, f64::min);
                let half = (bundle.len() - 1) as f64 / 2.0;
                let gap = LANE.min(room / half);
                let kept: BTreeMap<usize, [f64; 2]> = bundle
                    .iter()
                    .map(|&member| {
                        let run = &runs[member];
                        let lane = |neighbour: Option<usize>| {
                            neighbour.map_or(0.0, |neighbour| {
                                let other = &runs[neighbour];
                                let [a, b] = [other.toward, run.toward];
                                other.offset * (0..3).map(|q| a[q] * b[q]).sum::<f64>()
                            })
                        };
                        let lanes = if run.sign > 0.0 {
                            [lane(run.before), lane(run.after)]
                        } else {
                            [lane(run.after), lane(run.before)]
                        };
                        (member, lanes)
                    })
                    .collect();
                let lanes = |run: usize| kept[&run];
                bundle.sort_by(|&first, &second| {
                    runs[first].order(&runs[second], lanes(first), lanes(second))
                });
                for (rank, &member) in bundle.iter().enumerate() {
                    let offset = (rank as f64 - half) * gap;
                    changed |= (runs[member].offset - offset).abs() > TOLERANCE;
                    runs[member].offset = offset;
                    runs[member].gap = gap;
                    let flow = runs[member].flow;
                    spacing[flow] = spacing[flow].min(gap);
                }
            }
            if !changed {
                break;
            }
        }

        let mut lines: Vec<(Vec<Vec3>, Vec<f64>, Vec<usize>)> = bases
            .iter()
            .zip(&owners)
            .map(|(steps, owner)| trace(steps, owner, &runs, &BTreeMap::new()))
            .collect();
        let mut segments: BTreeMap<Track, Vec<(usize, [f64; 2])>> = BTreeMap::new();
        for (flow, (points, _, _)) in lines.iter().enumerate() {
            for (track, extent) in pieces(points) {
                segments.entry(track).or_default().push((flow, extent));
            }
        }
        let mut moved: Vec<BTreeMap<usize, usize>> = vec![BTreeMap::new(); lines.len()];
        for _ in 0..ROUNDS {
            let mut changed = false;
            for flow in 0..lines.len() {
                let shared =
                    |points: &[Vec3], segments: &BTreeMap<Track, Vec<(usize, [f64; 2])>>| {
                        pieces(points)
                            .filter(|&(track, [low, high])| {
                                segments.get(&track).is_some_and(|others| {
                                    others.iter().any(|&(other, [other_low, other_high])| {
                                        other != flow
                                            && other_low < high - TOLERANCE
                                            && low < other_high - TOLERANCE
                                    })
                                })
                            })
                            .count()
                    };
                let mut least = shared(&lines[flow].0, &segments);
                if least == 0 || lines[flow].2.is_empty() {
                    continue;
                }
                let corners = lines[flow].2.clone();
                let trials = corners
                    .iter()
                    .flat_map(|&corner| {
                        (0..=2 * PARTS.len()).map(move |place| Some((corner, place)))
                    })
                    .chain((corners.len() > 1).then_some(None));
                for choice in trials {
                    let trial = match choice {
                        Some((corner, place)) => {
                            let mut trial = moved[flow].clone();
                            if place == 2 * PARTS.len() {
                                trial.remove(&corner);
                            } else {
                                trial.insert(corner, place);
                            }
                            trial
                        }
                        None => corners.iter().map(|&corner| (corner, 0)).collect(),
                    };
                    if trial == moved[flow] {
                        continue;
                    }
                    let line = trace(&bases[flow], &owners[flow], &runs, &trial);
                    let count = shared(&line.0, &segments);
                    if count < least {
                        for (track, extent) in pieces(&lines[flow].0) {
                            if let Some(entries) = segments.get_mut(&track)
                                && let Some(place) =
                                    entries.iter().position(|&entry| entry == (flow, extent))
                            {
                                entries.remove(place);
                            }
                        }
                        for (track, extent) in pieces(&line.0) {
                            segments.entry(track).or_default().push((flow, extent));
                        }
                        least = count;
                        moved[flow] = trial;
                        lines[flow] = line;
                        changed = true;
                        if least == 0 {
                            break;
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
        lines
            .into_iter()
            .enumerate()
            .map(|(flow, (points, gaps, _))| Laned {
                points,
                spacing: spacing[flow],
                gaps,
            })
            .collect()
    }

    /// The steps of the polyline through the nodes at the indexes of `path`, with no point 2
    /// times in sequence. A segment from the end of a step of the path to the start of the next
    /// step lies in their common node and has the room of that node.
    fn base(&self, path: &[usize]) -> Vec<Step> {
        let mut steps: Vec<Step> = Vec::new();
        for (hop, pair) in path.windows(2).enumerate() {
            for (place, step) in self.route(pair[0], pair[1]).into_iter().enumerate() {
                let mut step = Step { hop, ..step };
                match steps.last() {
                    Some(last)
                        if (0..3).all(|q| (last.point[q] - step.point[q]).abs() <= TOLERANCE) => {}
                    Some(last) => {
                        if place == 0 {
                            let axis = along(last.point, step.point);
                            step.room = self.rise_room(pair[0], last.point, lateral(axis));
                        }
                        steps.push(step);
                    }
                    None => steps.push(step),
                }
            }
        }
        steps
    }

    /// The steps from node `from` to node `to`. For 2 nodes of a parent with the arrangement
    /// layers with `to` in a later column: at a quarter of the height of the lowest node of the
    /// parent above its bottom, along x from `from` to the gap before each later column up to
    /// the column of `to`, in each gap along z to the free stretch of the next column nearest to
    /// the line, and along x into `to`. Else: out of `from` and each of its ancestors to the
    /// corridor of the lowest common ancestor, along the corridor 1 axis after the other, then
    /// into `to` in reverse order. Each segment is parallel to 1 axis.
    fn route(&self, from: usize, to: usize) -> Vec<Step> {
        if let Some(steps) = self.wire(from, to) {
            return steps;
        }
        let up = self.chain(from);
        let down = self.chain(to);
        let common = up.iter().position(|node| down.contains(node));
        let up_end = common.unwrap_or(up.len());
        let down_end = match common {
            Some(position) => down
                .iter()
                .position(|&node| node == up[position])
                .unwrap_or(0),
            None => down.len(),
        };
        if let Some(steps) = self.side(from, to, &up[..up_end], &down[..down_end]) {
            return steps;
        }
        let mut steps = self.climb(from, to, &up[..up_end]);
        let mut back = self.descend(to, from, &down[..down_end]);
        let common = common.map(|position| up[position]);
        let room = self.corridor_room(common);
        if let (Some(last), Some(next)) = (steps.last(), back.first()) {
            let mut corner = last.point;
            let next = next.point;
            for axis in 0..3 {
                if corner[axis] != next[axis] {
                    corner[axis] = next[axis];
                    steps.push(Step::new(corner, room).along(self.corridor_lanes(common, axis)));
                }
            }
        }
        if let Some(first) = back.first_mut()
            && steps.last().is_some_and(|last| last.point == first.point)
        {
            back.remove(0);
        }
        steps.extend(back);
        steps
    }

    /// The steps of `climb` from `node` toward `other` through `chain` in reverse order, each
    /// after the first with the room and the lanes of the segment that ends at it.
    fn descend(&self, node: usize, other: usize, chain: &[usize]) -> Vec<Step> {
        let mut back = self.climb(node, other, chain);
        back.reverse();
        let segments: Vec<(f64, Option<usize>)> =
            back.iter().map(|step| (step.room, step.lanes)).collect();
        for (step, (room, lanes)) in back.iter_mut().skip(1).zip(segments) {
            step.room = room;
            step.lanes = lanes;
        }
        if let Some(first) = back.first_mut() {
            first.lanes = None;
        }
        back
    }

    /// The steps from `from` to `to` through the facing sides of the 2 last nodes of `up` and
    /// `down`: 2 adjacent children of a parent with the arrangement row, 1 or both with
    /// children, or 2 children in 2 columns of a parent with the arrangement layers. The line
    /// leaves the first along x by `leave`. It goes in the gap after the column of the first along
    /// z to the free stretch of each column between them and along x through that column. In the
    /// gap before the second it goes along y and z, and it enters the second along x by `leave`.
    fn side(&self, from: usize, to: usize, up: &[usize], down: &[usize]) -> Option<Vec<Step>> {
        let (&first, &second) = (up.last()?, down.last()?);
        let parent = self.parents[first];
        if parent != self.parents[second] || first == second {
            return None;
        }
        let (rule, siblings) = match parent {
            Some(parent) => (&self.rules[parent], &self.children[parent]),
            None => (&self.root, &self.top),
        };
        let place = |node: usize| siblings.iter().position(|&sibling| sibling == node);
        let column = |node: usize| match rule.arrangement {
            Arrangement::Layers => Some(self.columns[node]),
            _ => place(node),
        };
        let fits = match rule.arrangement {
            Arrangement::Row => {
                (self.has_children[first] || self.has_children[second])
                    && place(first)?.abs_diff(place(second)?) == 1
            }
            _ => false,
        };
        if !fits {
            return None;
        }
        let side = if column(second)? > column(first)? {
            1.0
        } else {
            -1.0
        };
        let mut steps = self.leave(from, to, up, side)?;
        let mut back = self.leave(to, from, down, -side)?;
        back.reverse();
        let segments: Vec<(f64, Option<usize>)> =
            back.iter().map(|step| (step.room, step.lanes)).collect();
        for (step, (room, lanes)) in back.iter_mut().skip(1).zip(segments) {
            step.room = room;
            step.lanes = lanes;
        }
        let start = steps.last()?.point;
        let end = back.first()?.point;
        let near = |members: &mut dyn Iterator<Item = usize>| {
            members
                .map(|node| {
                    let placed = &self.placed[node];
                    placed.centre[0] - side * placed.size[0] / 2.0
                })
                .fold(side * f64::MAX, |least, x| {
                    if side * x < side * least { x } else { least }
                })
        };
        let between: Vec<usize> = match rule.arrangement {
            Arrangement::Layers => {
                let [low, high] = [self.columns[first], self.columns[second]];
                let mut list: Vec<usize> = (low.min(high) + 1..low.max(high)).collect();
                if side < 0.0 {
                    list.reverse();
                }
                list
            }
            _ => Vec::new(),
        };
        let mut point = start;
        let mut room = self.rise_room(first, start, lateral(0));
        for &inner in &between {
            let members = self.members(parent, inner);
            let turn = near(&mut members.iter().copied()) - side * rule.gap / 2.0;
            point[0] = turn;
            steps.push(Step::new(point, room));
            let (z, next) = self.free(members, point[2], end[2], rule.gap);
            if z != point[2] {
                point[2] = z;
                steps.push(Step::new(point, rule.gap / 2.0 * ROOM));
            }
            room = next;
        }
        let last = match rule.arrangement {
            Arrangement::Layers => self.members(parent, self.columns[second]),
            _ => std::slice::from_ref(&second),
        };
        let turn = near(&mut last.iter().copied()) - side * rule.gap / 2.0;
        point[0] = turn;
        steps.push(Step::new(point, room));
        for axis in [1, 2] {
            if point[axis] != end[axis] {
                point[axis] = end[axis];
                let lanes = None::<usize>;
                steps.push(Step::new(point, rule.gap / 2.0 * ROOM).along(lanes));
            }
        }
        back[0].room = self.rise_room(second, end, lateral(0));
        steps.extend(back);
        Some(steps)
    }

    /// The steps from the start point of `node` toward `other` out of each node of `chain` along
    /// x through its face on the side `side`, at the height and the depth of the line. A node of
    /// `chain` with a sibling in the way of the line leaves in the middle of the gap after it
    /// along y to the corridor of its parent above the children, and along x at the height of
    /// that corridor. `None` for a node with a corridor beside its children in its start, and for
    /// a sibling in the way in a parent with a corridor beside its children.
    fn leave(&self, node: usize, other: usize, chain: &[usize], side: f64) -> Option<Vec<Step>> {
        if self.has_children[node] && exit_axis(self.rules[node].arrangement) != 1 {
            return None;
        }
        let mut steps = self.start(node, other);
        let mut point = steps.last()?.point;
        let face =
            |node: usize| self.placed[node].centre[0] + side * self.placed[node].size[0] / 2.0;
        for (place, &inner) in chain.iter().enumerate() {
            let outer = chain.get(place + 1).copied();
            let lanes = outer
                .filter(|&outer| exit_axis(self.rules[outer].arrangement) == 0)
                .map(|_| 1);
            let toward = lanes.map_or(lateral(0), unit);
            if side * (face(inner) - point[0]) > TOLERANCE {
                let room = self.rise_room(inner, point, toward);
                point[0] = face(inner);
                steps.push(Step::new(point, room).along(lanes));
            }
            let Some(outer) = outer else {
                break;
            };
            let rule = &self.rules[outer];
            let out = [face(outer), point[1], point[2]];
            let clear = self.clearance(outer, inner, point, out);
            if clear >= LANE {
                let mut room = self.rise_room(inner, point, toward);
                if lanes.is_none() {
                    room = room.min(clear * ROOM);
                }
                point = out;
                steps.push(Step::new(point, room).along(lanes));
                continue;
            }
            let (axis, corridor) = self.corridor(Some(outer));
            if axis != 1 {
                return None;
            }
            let room = self.rise_room(inner, point, lateral(0));
            let middle = self.placed[inner].centre[0]
                + side * (self.placed[inner].size[0] / 2.0 + rule.gap / 2.0);
            if side * (middle - point[0]) > TOLERANCE {
                point[0] = middle;
                steps.push(Step::new(point, room));
            }
            point[1] = corridor;
            steps.push(Step::new(point, rule.gap / 2.0 * ROOM));
            point[0] = face(outer);
            steps.push(Step::new(point, self.corridor_room(Some(outer))));
        }
        Some(steps)
    }

    /// The members of `column` of `parent` with the arrangement layers in the order of the
    /// children, `None` for the nodes with no parent.
    fn members(&self, parent: Option<usize>, column: usize) -> &[usize] {
        self.layered
            .get(&parent)
            .and_then(|layered| layered.columns.get(column))
            .map_or(&[], Vec::as_slice)
    }

    /// The steps of the wire from `from` to `to` in a later column of a parent with the
    /// arrangement layers.
    fn wire(&self, from: usize, to: usize) -> Option<Vec<Step>> {
        let parent = self.parents[from];
        if parent != self.parents[to] {
            return None;
        }
        let rule = match parent {
            Some(parent) => &self.rules[parent],
            None => &self.root,
        };
        if rule.arrangement != Arrangement::Layers || self.columns[to] <= self.columns[from] {
            return None;
        }
        let height = self.layered.get(&parent)?.height;
        let level = |node: usize| {
            let mut point = self.placed[node].centre;
            point[1] = height;
            point
        };
        let depth = |node: usize| self.placed[node].size[2] / 2.0 * ROOM;
        let mut steps = vec![Step::new(level(from), 0.0)];
        let mut z = level(from)[2];
        let mut room = depth(from);
        for column in self.columns[from] + 1..=self.columns[to] {
            let members = self.members(parent, column);
            let left = members
                .iter()
                .map(|&node| self.placed[node].centre[0] - self.placed[node].size[0] / 2.0)
                .fold(f64::MAX, f64::min);
            let turn = left - rule.gap / 2.0;
            steps.push(Step::new([turn, height, z], room));
            let (target, next) = if column == self.columns[to] {
                (self.placed[to].centre[2], depth(to))
            } else {
                self.free(members, z, self.placed[to].centre[2], rule.gap)
            };
            if target != z {
                steps.push(Step::new([turn, height, target], rule.gap / 2.0 * ROOM));
                z = target;
            }
            room = next;
        }
        steps.push(Step::new(level(to), room));
        Some(steps)
    }

    /// The coordinate on z of the free stretch of the column of `members` nearest to `z`, then
    /// nearest to `goal`, then the largest, and its room: `z` itself when a quarter of `gap` or
    /// more lies between it and each node of the column, the middle of each stretch between 2
    /// nodes of the column, and half of `gap` before the first and after the last node.
    fn free(&self, members: &[usize], z: f64, goal: f64, gap: f64) -> (f64, f64) {
        let mut spans: Vec<[f64; 2]> = members
            .iter()
            .map(|&node| {
                let placed = &self.placed[node];
                [-0.5, 0.5].map(|side| placed.centre[2] + side * placed.size[2])
            })
            .collect();
        spans.sort_by(|first, second| first[0].total_cmp(&second[0]));
        let Some(first) = spans.first() else {
            return (z, gap / 2.0 * ROOM);
        };
        let clear = spans
            .iter()
            .map(|span| (span[0] - z).max(z - span[1]))
            .fold(f64::MAX, f64::min);
        if clear >= gap / 4.0 - TOLERANCE {
            return (z, clear * ROOM);
        }
        let mut candidates = vec![(first[0] - gap / 2.0, gap / 2.0)];
        let mut reach = first[1];
        for span in &spans[1..] {
            if span[0] - reach > TOLERANCE {
                candidates.push(((span[0] + reach) / 2.0, (span[0] - reach) / 2.0));
            }
            reach = reach.max(span[1]);
        }
        candidates.push((reach + gap / 2.0, gap / 2.0));
        candidates
            .into_iter()
            .min_by(|first, second| {
                (first.0 - z)
                    .abs()
                    .total_cmp(&(second.0 - z).abs())
                    .then((first.0 - goal).abs().total_cmp(&(second.0 - goal).abs()))
                    .then(second.0.total_cmp(&first.0))
            })
            .map_or((z, gap / 2.0 * ROOM), |(at, room)| (at, room * ROOM))
    }

    /// The steps from the start point of `node` toward `other` out of each node of `chain` into
    /// the corridor of its parent. The segments out of a child of a parent with the arrangement
    /// stack have their lanes along y, and the segment along y from the corridor of a node with
    /// the arrangement stack to its face has its lanes along x.
    fn climb(&self, node: usize, other: usize, chain: &[usize]) -> Vec<Step> {
        let mut steps = self.start(node, other);
        let mut point = steps[steps.len() - 1].point;
        for &inner in chain {
            let (axis, corridor) = self.corridor(self.parents[inner]);
            let placed = &self.placed[inner];
            let lanes = if axis == 0 {
                Some(1)
            } else if self.has_children[inner] {
                self.corridor_lanes(Some(inner), axis)
            } else {
                None
            };
            let outside = if axis == 0 { lanes } else { None };
            let room = |lanes: Option<usize>| {
                self.rise_room(inner, point, lanes.map_or(lateral(axis), unit))
            };
            let (inner_room, outer_room) = (room(lanes), room(outside));
            point[axis] = placed.centre[axis] + placed.size[axis] / 2.0;
            steps.push(Step::new(point, inner_room).along(lanes));
            point[axis] = corridor;
            steps.push(Step::new(point, outer_room).along(outside));
        }
        steps
    }

    /// The distance on z from the segment along x from `start` to `end` in `outer` to the faces
    /// of `outer` and to each child of `outer` other than `inner` beside it on x and y, 0 for a
    /// child that the segment crosses.
    fn clearance(&self, outer: usize, inner: usize, start: Vec3, end: Vec3) -> f64 {
        let placed = &self.placed[outer];
        let walls = placed.size[2] / 2.0 - (start[2] - placed.centre[2]).abs();
        let [low, high] = [start[0].min(end[0]), start[0].max(end[0])];
        self.children[outer]
            .iter()
            .filter(|&&child| child != inner)
            .map(|&child| &self.placed[child])
            .filter(|child| {
                (child.centre[0] - (low + high) / 2.0).abs() < (child.size[0] + high - low) / 2.0
                    && (child.centre[1] - start[1]).abs() < child.size[1] / 2.0
            })
            .map(|child| ((child.centre[2] - start[2]).abs() - child.size[2] / 2.0).max(0.0))
            .fold(walls, f64::min)
    }

    /// The room of a segment from `point` in `inner` out of its surface, or beside it, with its
    /// lanes along `toward`: on each axis of its lane, the distance from `point` to the faces of
    /// `inner` for a point inside it, the smaller of the distance to its face and to the middle of
    /// the gap of the parent for a point outside it, and the smaller of half of that gap and half
    /// of its size for a point on a face.
    fn rise_room(&self, inner: usize, point: Vec3, toward: Vec3) -> f64 {
        let placed = &self.placed[inner];
        let gap = match self.parents[inner] {
            Some(parent) => self.rules[parent].gap,
            None => self.root.gap,
        };
        (0..3)
            .filter(|&q| toward[q] != 0.0)
            .map(|q| {
                let inside = placed.size[q] / 2.0 - (point[q] - placed.centre[q]).abs();
                let room = if inside > TOLERANCE {
                    inside
                } else if inside < -TOLERANCE {
                    (-inside).min(gap / 2.0 + inside)
                } else {
                    (gap / 2.0).min(placed.size[q] / 2.0)
                };
                room * ROOM / toward[q].abs()
            })
            .fold(f64::MAX, f64::min)
    }

    /// The axis of the lanes of a segment along `axis` in the corridor of `node`, or of the plane
    /// of the nodes with no parent: x for a segment along y in the corridor of the arrangement
    /// stack, else `None` for the lanes of `lateral`.
    fn corridor_lanes(&self, node: Option<usize>, axis: usize) -> Option<usize> {
        let rule = match node {
            Some(node) => &self.rules[node],
            None => &self.root,
        };
        (axis == 1 && exit_axis(rule.arrangement) == 0).then_some(0)
    }

    /// The room of a segment in the corridor of `node`, or of the plane of the nodes with no
    /// parent: the margin for a corridor above the children, and the part `ROOM` of half the
    /// margin for a corridor beside them.
    fn corridor_room(&self, node: Option<usize>) -> f64 {
        let rule = match node {
            Some(node) => &self.rules[node],
            None => &self.root,
        };
        match exit_axis(rule.arrangement) {
            1 => rule.margin,
            _ => rule.margin / 2.0 * ROOM,
        }
    }

    /// The first steps of a line out of `node`. For a node with children: the point in its
    /// corridor in line with its centre. For a node with no child whose shape has a symbol on
    /// its top face, in a parent whose children leave it upward: its centre, then the door a
    /// quarter of the gap of the parent outside the middle of its side toward `other` on x, at
    /// the height of its centre. Else the centre of the node.
    fn start(&self, node: usize, other: usize) -> Vec<Step> {
        let placed = &self.placed[node];
        let mut point = placed.centre;
        if self.has_children[node] {
            let (axis, corridor) = self.corridor(Some(node));
            point[axis] = corridor;
            return vec![Step::new(point, 0.0)];
        }
        if !self.rules[node].shape.has_top_symbol() || self.corridor(self.parents[node]).0 != 1 {
            return vec![Step::new(point, 0.0)];
        }
        let side = if self.placed[other].centre[0] < placed.centre[0] {
            -1.0
        } else {
            1.0
        };
        let gap = match self.parents[node] {
            Some(parent) => self.rules[parent].gap,
            None => self.root.gap,
        };
        point[0] += side * (placed.size[0] / 2.0 + gap / 4.0);
        vec![
            Step::new(placed.centre, 0.0),
            Step::new(point, placed.size[2] / 2.0 * ROOM),
        ]
    }

    /// The axis of the corridor of a node, or of the plane of the nodes with no parent, and the
    /// coordinate of the middle of the corridor on that axis.
    fn corridor(&self, node: Option<usize>) -> (usize, f64) {
        match node {
            Some(node) => {
                let rule = &self.rules[node];
                let axis = exit_axis(rule.arrangement);
                let placed = &self.placed[node];
                let outside = placed.centre[axis] + placed.size[axis] / 2.0;
                (axis, outside - rule.margin / 2.0)
            }
            None => {
                let axis = exit_axis(self.root.arrangement);
                let outside = self.roots.centre[axis] + self.roots.size[axis] / 2.0;
                (axis, outside + self.root.margin / 2.0)
            }
        }
    }

    /// The node and its ancestors, from the node to the node with no parent.
    fn chain(&self, node: usize) -> Vec<usize> {
        let mut chain = vec![node];
        let mut current = node;
        while let Some(parent) = self.parents[current] {
            chain.push(parent);
            current = parent;
        }
        chain
    }

    pub fn get(&self, id: &ObjectId) -> Option<&Placed> {
        self.index.get(id).map(|&index| &self.placed[index])
    }

    /// The broken rules, in the order of the nodes.
    pub fn broken(&self) -> Vec<Broken> {
        let mut broken = Vec::new();
        for (index, node) in self.nodes.iter().enumerate() {
            let Some(parent) = &node.parent else {
                continue;
            };
            if !inside(&self.placed[index], &self.placed[self.index[parent]]) {
                broken.push(Broken::Outside {
                    child: node.id.clone(),
                    parent: parent.clone(),
                });
            }
        }
        let mut pairs = Vec::new();
        let mut spans = Vec::new();
        let mut unbounded = Vec::new();
        for group in std::iter::once(&self.top).chain(&self.children) {
            spans.clear();
            unbounded.clear();
            for &node in group {
                let placed = &self.placed[node];
                let (centre, half) = (placed.centre[0], placed.size[0] / 2.0);
                if centre.is_nan() || placed.size[0].is_nan() {
                    continue;
                }
                // The pad exceeds the rounding error of `intersect` on x. The spans of 2 nodes
                // that intersect overlap.
                let pad = (centre.abs() + half.abs()) * 1e-12 + TOLERANCE;
                let (low, high) = (centre - half - pad, centre + half + pad);
                if !centre.is_finite() || !half.is_finite() || low.is_nan() || high.is_nan() {
                    unbounded.push(node);
                    continue;
                }
                spans.push((low, high, node));
            }
            spans.sort_unstable_by(|first, second| first.0.total_cmp(&second.0));
            for (place, &(_, high, first)) in spans.iter().enumerate() {
                for &(low, _, second) in &spans[place + 1..] {
                    if low >= high {
                        break;
                    }
                    if intersect(&self.placed[first], &self.placed[second]) {
                        pairs.push((first.min(second), first.max(second)));
                    }
                }
            }
            for (place, &first) in unbounded.iter().enumerate() {
                let others = spans
                    .iter()
                    .map(|&(_, _, node)| node)
                    .chain(unbounded[place + 1..].iter().copied());
                for second in others {
                    if intersect(&self.placed[first], &self.placed[second]) {
                        pairs.push((first.min(second), first.max(second)));
                    }
                }
            }
        }
        pairs.sort_unstable();
        for (first, second) in pairs {
            broken.push(Broken::Intersect {
                first: self.nodes[first].id.clone(),
                second: self.nodes[second].id.clone(),
            });
        }
        let bounds = self.bounds();
        let mut allowed = vec![false; self.nodes.len()];
        let mut hit = vec![false; self.nodes.len()];
        let mut set = Vec::new();
        let mut hits = Vec::new();
        let mut open: Vec<usize> = Vec::new();
        for flow in &self.flows {
            for &node in &flow.nodes {
                let mut current = Some(node);
                while let Some(index) = current {
                    if !allowed[index] {
                        allowed[index] = true;
                        set.push(index);
                    }
                    current = self.parents[index];
                }
            }
            for pair in flow.points.windows(2) {
                let (start, end) = (pair[0], pair[1]);
                let low = [0, 1, 2].map(|axis| start[axis].min(end[axis]));
                let high = [0, 1, 2].map(|axis| start[axis].max(end[axis]));
                let finite = start.iter().chain(&end).all(|value| value.is_finite());
                open.clear();
                open.extend(self.top.iter().rev());
                while let Some(index) = open.pop() {
                    let [lowest, highest] = bounds[index];
                    if finite && (0..3).any(|q| highest[q] <= low[q] || lowest[q] >= high[q]) {
                        continue;
                    }
                    if !allowed[index] && !hit[index] && crosses(start, end, &self.placed[index]) {
                        hit[index] = true;
                        hits.push(index);
                    }
                    open.extend(self.children[index].iter().rev());
                }
            }
            hits.sort_unstable();
            for &index in &hits {
                hit[index] = false;
                broken.push(Broken::Crosses {
                    flow: flow.id.clone(),
                    node: self.nodes[index].id.clone(),
                });
            }
            hits.clear();
            for &index in &set {
                allowed[index] = false;
            }
            set.clear();
        }
        broken
    }

    /// The box of each node and its descendants that holds each box of `crosses` of them, as the
    /// lowest and the highest coordinate on each axis. A box with a coordinate that is not a
    /// number reaches from -infinity to +infinity.
    fn bounds(&self) -> Vec<[Vec3; 2]> {
        let mut bounds: Vec<[Vec3; 2]> = self
            .placed
            .iter()
            .map(|placed| {
                let mut lowest = [0.0; 3];
                let mut highest = [0.0; 3];
                for axis in 0..3 {
                    let low = placed.centre[axis] - placed.size[axis] / 2.0 + TOLERANCE;
                    let high = placed.centre[axis] + placed.size[axis] / 2.0 - TOLERANCE;
                    if low.is_nan() || high.is_nan() {
                        lowest[axis] = f64::NEG_INFINITY;
                        highest[axis] = f64::INFINITY;
                    } else {
                        lowest[axis] = low.min(high);
                        highest[axis] = low.max(high);
                    }
                }
                [lowest, highest]
            })
            .collect();
        for index in (0..bounds.len()).rev() {
            if let Some(parent) = self.parents[index] {
                let [lowest, highest] = bounds[index];
                let outer = &mut bounds[parent];
                for axis in 0..3 {
                    outer[0][axis] = outer[0][axis].min(lowest[axis]);
                    outer[1][axis] = outer[1][axis].max(highest[axis]);
                }
            }
        }
        bounds
    }

    /// One line for each node in name order: the name, the kind, the parent, the own label, the
    /// centre and the size. Then one line for each flow in name order: the name, the path, the
    /// outcome with its reason, the data of the source and the points. Then the lines of
    /// `Library::describe`.
    pub fn describe(&self) -> String {
        let mut text = String::new();
        for (id, &index) in &self.index {
            let node = &self.nodes[index];
            let placed = &self.placed[index];
            text.push_str(&format!("{id} {}", node.kind));
            if let Some(parent) = &node.parent {
                text.push_str(&format!(" in={parent}"));
            }
            if let Some(label) = &node.label {
                text.push_str(&format!(" label={label:?}"));
            }
            let [x, y, z] = placed.centre;
            let [width, height, depth] = placed.size;
            text.push_str(&format!(" at={x},{y},{z} size={width},{height},{depth}"));
            for (name, value) in &node.fields {
                text.push_str(&format!(" {name}={value:?}"));
            }
            text.push('\n');
        }
        let mut flows: Vec<&FlowLine> = self.flows.iter().collect();
        flows.sort_by(|first, second| first.id.cmp(&second.id));
        for flow in flows {
            let path: Vec<&str> = flow.path.iter().map(ObjectId::as_str).collect();
            let points: Vec<String> = flow
                .points
                .iter()
                .map(|[x, y, z]| format!("{x},{y},{z}"))
                .collect();
            text.push_str(&format!("{} flow path={}", flow.id, path.join(",")));
            text.push_str(&flow.data());
            text.push_str(&format!(" points={}\n", points.join(";")));
        }
        text.push_str(&self.library.describe());
        text
    }
}

/// Writes the offset of each child from the centre of its parent and gives the size of the box
/// around the children. `links` holds the steps of the flows from a child to a child. The
/// arrangement layers writes the column of each child to `columns`.
fn arrange(
    rule: &Rule,
    children: &[usize],
    links: &[(usize, usize)],
    sizes: &[Vec3],
    offsets: &mut [Vec3],
    columns_of: &mut [usize],
) -> Vec3 {
    let largest = |axis: usize| {
        children
            .iter()
            .map(|&child| sizes[child][axis])
            .fold(0.0, f64::max)
    };
    let gaps = |count: usize| rule.gap * count.saturating_sub(1) as f64;
    match rule.arrangement {
        Arrangement::Row | Arrangement::Stack => {
            let along = if rule.arrangement == Arrangement::Row {
                0
            } else {
                1
            };
            let total: f64 = children.iter().map(|&child| sizes[child][along]).sum();
            let mut extent = [largest(0), largest(1), largest(2)];
            extent[along] = total + gaps(children.len());
            let mut cursor = -extent[along] / 2.0;
            for &child in children {
                let size = sizes[child];
                offsets[child] = [0.0, (size[1] - extent[1]) / 2.0, 0.0];
                offsets[child][along] = cursor + size[along] / 2.0;
                cursor += size[along] + rule.gap;
            }
            extent
        }
        Arrangement::Grid => {
            let mut columns = 1;
            while columns * columns < children.len() {
                columns += 1;
            }
            let area: f64 = children
                .iter()
                .map(|&child| (sizes[child][0] + rule.gap) * (sizes[child][2] + rule.gap))
                .sum();
            let square = (children.len() as f64).sqrt();
            let limit = (area.sqrt() * columns as f64 / square).max(largest(0)) + TOLERANCE;

            let mut rows: Vec<(Vec<usize>, f64, f64)> = Vec::new();
            for &child in children {
                let [width, _, depth] = sizes[child];
                match rows.last_mut() {
                    Some((members, used, deep))
                        if *used + rule.gap + width <= limit && !members.is_empty() =>
                    {
                        members.push(child);
                        *used += rule.gap + width;
                        *deep = deep.max(depth);
                    }
                    _ => rows.push((vec![child], width, depth)),
                }
            }
            let extent = [
                rows.iter().map(|(_, used, _)| *used).fold(0.0, f64::max),
                largest(1),
                rows.iter().map(|(_, _, deep)| *deep).sum::<f64>() + gaps(rows.len()),
            ];
            let mut front = -extent[2] / 2.0;
            for (members, used, deep) in &rows {
                let mut left = -used / 2.0;
                for &child in members {
                    let [width, height, _] = sizes[child];
                    offsets[child] = [
                        left + width / 2.0,
                        (height - extent[1]) / 2.0,
                        front + deep / 2.0,
                    ];
                    left += width + rule.gap;
                }
                front += deep + rule.gap;
            }
            extent
        }
        Arrangement::Layers => {
            let places: BTreeMap<usize, usize> = children
                .iter()
                .enumerate()
                .rev()
                .map(|(place, &child)| (child, place))
                .collect();
            let mut next = vec![Vec::new(); children.len()];
            let mut accepted = std::collections::BTreeSet::new();
            let mut search = Search {
                seen: vec![0; children.len()],
                generation: 0,
                open: Vec::new(),
            };
            let mut entering = vec![0usize; children.len()];
            for &(from, to) in links {
                if let (Some(&from), Some(&to)) = (places.get(&from), places.get(&to))
                    && from != to
                    && !accepted.contains(&(from, to))
                    && !search.reaches(&next, to, from)
                {
                    accepted.insert((from, to));
                    next[from].push(to);
                    entering[to] += 1;
                }
            }
            // The column of a child is the length of the longest path to it in `next`, which
            // has no cycle.
            let mut column = vec![0usize; children.len()];
            let mut queue: std::collections::VecDeque<usize> = (0..children.len())
                .filter(|&place| entering[place] == 0)
                .collect();
            while let Some(from) = queue.pop_front() {
                for &to in &next[from] {
                    column[to] = column[to].max(column[from] + 1);
                    entering[to] -= 1;
                    if entering[to] == 0 {
                        queue.push_back(to);
                    }
                }
            }
            let count = column.iter().max().map_or(0, |last| last + 1);
            let mut columns = vec![Vec::new(); count];
            for (place, &child) in children.iter().enumerate() {
                columns[column[place]].push(child);
            }
            columns.retain(|members| !members.is_empty());
            for (place, members) in columns.iter().enumerate() {
                for &child in members {
                    columns_of[child] = place;
                }
            }
            order(&mut columns, links, columns_of);
            let width = |members: &Vec<usize>| {
                members
                    .iter()
                    .map(|&child| sizes[child][0])
                    .fold(0.0, f64::max)
            };
            let depth = |members: &Vec<usize>| {
                members.iter().map(|&child| sizes[child][2]).sum::<f64>() + gaps(members.len())
            };
            let extent = [
                columns.iter().map(width).sum::<f64>() + gaps(columns.len()),
                largest(1),
                columns.iter().map(depth).fold(0.0, f64::max),
            ];
            let mut left = -extent[0] / 2.0;
            for members in &columns {
                let wide = width(members);
                let mut front = -depth(members) / 2.0;
                for &child in members {
                    let [_, height, deep] = sizes[child];
                    offsets[child] = [
                        left + wide / 2.0,
                        (height - extent[1]) / 2.0,
                        front + deep / 2.0,
                    ];
                    front += deep + rule.gap;
                }
                left += wide + rule.gap;
            }
            extent
        }
    }
}

/// The buffers of a search of the places of a parent with the arrangement layers. A place is
/// seen in the current search when its entry in `seen` equals `generation`.
struct Search {
    seen: Vec<u64>,
    generation: u64,
    open: Vec<usize>,
}

impl Search {
    /// Whether `next` leads from the place `from` to the place `to`.
    fn reaches(&mut self, next: &[Vec<usize>], from: usize, to: usize) -> bool {
        self.generation += 1;
        self.open.clear();
        self.open.push(from);
        while let Some(place) = self.open.pop() {
            if place == to {
                return true;
            }
            if self.seen[place] != self.generation {
                self.seen[place] = self.generation;
                self.open.extend(&next[place]);
            }
        }
        false
    }
}

/// Orders the members of each column of `columns` to reduce the crossings of `links` between 2
/// adjacent columns. A link from a column to a later column has a waypoint in each column
/// between them, and a link to the same or an earlier column has none and does not count. The
/// rounds alternate a sweep from the first to the last column and a sweep back: each member gets
/// the mean of the places of its neighbours in the column before it in the sweep, or its own
/// place with no neighbour there, and the column takes the order of these values. After each
/// sweep, 2 adjacent members change places while that removes crossings. The order with the
/// fewest crossings stays. With more than `ORDER_LINKS` links between adjacent columns, each
/// column keeps the order of the children.
fn order(columns: &mut [Vec<usize>], links: &[(usize, usize)], column_of: &[usize]) {
    let mut slots: Vec<Vec<usize>> = Vec::with_capacity(columns.len());
    let mut members: Vec<Option<usize>> = Vec::new();
    let mut item: BTreeMap<usize, usize> = BTreeMap::new();
    for column in columns.iter() {
        let mut slot = Vec::with_capacity(column.len());
        for &node in column {
            item.insert(node, members.len());
            slot.push(members.len());
            members.push(Some(node));
        }
        slots.push(slot);
    }
    let pairs: std::collections::BTreeSet<(usize, usize)> = links
        .iter()
        .filter(|(from, to)| {
            item.contains_key(from) && item.contains_key(to) && column_of[*to] > column_of[*from]
        })
        .copied()
        .collect();
    let mut span = 0usize;
    for &(from, to) in &pairs {
        span += column_of[to] - column_of[from];
        if span > ORDER_LINKS {
            return;
        }
    }
    if span == 0 {
        return;
    }
    members.reserve(span);
    let mut edges: Vec<(usize, usize)> = Vec::with_capacity(span);
    for (from, to) in pairs {
        let mut previous = item[&from];
        for slot in &mut slots[column_of[from] + 1..column_of[to]] {
            slot.push(members.len());
            edges.push((previous, members.len()));
            previous = members.len();
            members.push(None);
        }
        edges.push((previous, item[&to]));
    }
    let mut before = vec![Vec::new(); members.len()];
    let mut after = vec![Vec::new(); members.len()];
    for &(from, to) in &edges {
        after[from].push(to);
        before[to].push(from);
    }
    let mut place = vec![0usize; members.len()];
    let mut column = vec![0usize; members.len()];
    for (index, slot) in slots.iter().enumerate() {
        for (rank, &member) in slot.iter().enumerate() {
            place[member] = rank;
            column[member] = index;
        }
    }
    let total = |place: &[usize]| {
        let mut count = 0;
        for (index, first) in edges.iter().enumerate() {
            for second in &edges[index + 1..] {
                if column[first.0] == column[second.0]
                    && (place[first.0] < place[second.0]) != (place[first.1] < place[second.1])
                    && place[first.0] != place[second.0]
                    && place[first.1] != place[second.1]
                {
                    count += 1;
                }
            }
        }
        count
    };
    let between = |first: usize, second: usize, place: &[usize]| {
        [&before, &after]
            .iter()
            .map(|neighbours| {
                neighbours[first]
                    .iter()
                    .flat_map(|&a| neighbours[second].iter().map(move |&b| (a, b)))
                    .filter(|&(a, b)| place[a] > place[b])
                    .count()
            })
            .sum::<usize>()
    };
    let mut best = (total(&place), slots.clone());
    let count = slots.len();
    for round in 0..ORDER_ROUNDS {
        if best.0 == 0 {
            break;
        }
        let (sweep, neighbours): (Vec<usize>, _) = if round % 2 == 0 {
            ((1..count).collect(), &before)
        } else {
            ((0..count.saturating_sub(1)).rev().collect(), &after)
        };
        for index in sweep {
            let mut keyed: Vec<(f64, usize)> = slots[index]
                .iter()
                .map(|&member| {
                    let near = &neighbours[member];
                    let key = if near.is_empty() {
                        place[member] as f64
                    } else {
                        near.iter().map(|&other| place[other] as f64).sum::<f64>()
                            / near.len() as f64
                    };
                    (key, member)
                })
                .collect();
            keyed.sort_by(|first, second| {
                first
                    .0
                    .total_cmp(&second.0)
                    .then(place[first.1].cmp(&place[second.1]))
            });
            slots[index] = keyed.into_iter().map(|(_, member)| member).collect();
            for (rank, &member) in slots[index].iter().enumerate() {
                place[member] = rank;
            }
        }
        for _ in 0..members.len() {
            let mut changed = false;
            for slot in slots.iter_mut() {
                for rank in 0..slot.len().saturating_sub(1) {
                    let (first, second) = (slot[rank], slot[rank + 1]);
                    if between(second, first, &place) < between(first, second, &place) {
                        slot.swap(rank, rank + 1);
                        place[first] = rank + 1;
                        place[second] = rank;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        let crossings = total(&place);
        if crossings < best.0 {
            best = (crossings, slots.clone());
        }
    }
    for (column, slot) in columns.iter_mut().zip(best.1) {
        *column = slot
            .into_iter()
            .filter_map(|member| members[member])
            .collect();
    }
}

/// The axis on which the children of a node with `arrangement` leave it.
fn exit_axis(arrangement: Arrangement) -> usize {
    match arrangement {
        Arrangement::Row | Arrangement::Grid | Arrangement::Layers => 1,
        Arrangement::Stack => 0,
    }
}

/// The largest distance of 2 coordinates that are equal.
const TOLERANCE: f64 = 1e-9;

/// The largest distance between 2 adjacent lanes.
const LANE: f64 = 0.45;

/// The largest count of links between adjacent columns of the arrangement layers that `order`
/// orders.
const ORDER_LINKS: usize = 1000;
/// The count of the rounds of `order`.
const ORDER_ROUNDS: usize = 12;

/// The part of the room around a segment that its lanes take.
const ROOM: f64 = 0.8;

/// The largest count of rounds that join bundles and set their lanes.
const ROUNDS: usize = 16;

/// The step of the coordinates that a segment keeps when 2 segments are compared.
const GRAIN: f64 = 1e-6;

/// The places of a jog in a segment next to a corner of a run along y, as parts of the segment
/// from the corner.
const PARTS: [f64; 3] = [0.5, 0.25, 0.75];

/// The axis of a segment and its other 2 coordinates in steps of `GRAIN`.
type Track = (usize, [i64; 2]);

/// A point of the line through the centres of the nodes of a path, with the index of the step
/// of the path, the room of the segment that ends at the point: the largest distance of a lane
/// from the segment, and the axis of the lanes of that segment, `None` for the lanes of
/// `lateral`.
#[derive(Clone, Copy, Debug)]
struct Step {
    point: Vec3,
    hop: usize,
    room: f64,
    lanes: Option<usize>,
}

impl Step {
    fn new(point: Vec3, room: f64) -> Self {
        Self {
            point,
            hop: 0,
            room,
            lanes: None,
        }
    }

    /// The step with its segment in lanes along `lanes`.
    fn along(self, lanes: Option<usize>) -> Self {
        Self { lanes, ..self }
    }
}

/// The segments of a line through the centres that follow each other on 1 line along 1 axis in 1
/// direction in 1 step of the path: the index of the line, the axis, the direction of its lanes,
/// the sign of the direction, the step, the point of the start, the lowest and highest coordinate
/// on the axis, the room, the indexes of the first and last point, the side of the line before the
/// low end and after the high end, the offset of the lane, the distance to an adjacent lane, and
/// the run before and after it in its line.
#[derive(Clone, Debug)]
struct Run {
    flow: usize,
    axis: usize,
    toward: Vec3,
    sign: f64,
    hop: usize,
    line: Vec3,
    low: f64,
    high: f64,
    room: f64,
    first: usize,
    last: usize,
    sides: [f64; 2],
    offset: f64,
    gap: f64,
    before: Option<usize>,
    after: Option<usize>,
}

impl Run {
    /// The order of 2 runs of a bundle across it. `lanes` and `other_lanes` hold the lane across
    /// the bundle of the run before the low end and of the run after the high end of each run.
    /// The first key is the side from which the line of a run comes at the low end of the
    /// bundle. Of 2 runs from 1 side, the run that joins the bundle at a higher coordinate lies
    /// farther toward that side. The next key is the lane of the run before the low end, then
    /// the side to which the line goes at the high end. Of 2 runs to 1 side, the run that leaves
    /// the bundle at a lower coordinate lies farther toward that side. The last keys are the
    /// lane of the run after the high end, the line and the place in the line.
    fn order(&self, other: &Run, lanes: [f64; 2], other_lanes: [f64; 2]) -> std::cmp::Ordering {
        let key = |run: &Run, lanes: [f64; 2]| {
            [
                run.sides[0],
                run.sides[0] * run.low,
                lanes[0],
                run.sides[1],
                -run.sides[1] * run.high,
                lanes[1],
            ]
        };
        let (first, second) = (key(self, lanes), key(other, other_lanes));
        first
            .iter()
            .zip(&second)
            .map(|(a, b)| a.total_cmp(b))
            .find(|order| order.is_ne())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(self.flow.cmp(&other.flow))
            .then(self.first.cmp(&other.first))
    }
}

/// The polyline of the steps `steps` of a line with the run of each segment in `owner`, each
/// run in its lane, with the gap of each segment and the indexes of the points of `steps` where
/// a run along y meets a run along x or z with a jog. Between 2 runs in 2 lanes, the line goes
/// across on the axis of neither run: for a run along y and a run along x or z, at the corner,
/// or, for an index in `moved`, at the part of `PARTS` at its place modulo the length of `PARTS`
/// from the corner: in the segment of the run along y next to the corner for a place below the
/// length of `PARTS`, and in the segment of the run along x or z next to the corner for a higher
/// place.
/// A run along y of 1 segment with a jog on 1 axis at each end goes across 1 time, from the lane
/// of the run before it to the lane of the run after it. Between 2 runs on 1 axis, the line goes
/// across first on an axis on which the lanes of the run before do not lie, and last on an axis
/// on which the lanes of the run after do not lie.
fn trace(
    steps: &[Step],
    owner: &[usize],
    runs: &[Run],
    moved: &BTreeMap<usize, usize>,
) -> (Vec<Vec3>, Vec<f64>, Vec<usize>) {
    let mut points: Vec<Vec3> = Vec::with_capacity(steps.len());
    let mut corners = Vec::new();
    let line = |index: usize| {
        let run = &runs[owner[index]];
        let toward = run.toward;
        (
            run.axis,
            [0, 1, 2].map(|q| run.line[q] + run.offset * toward[q]),
        )
    };
    let Some(&first) = steps.first() else {
        return (points, Vec::new(), corners);
    };
    if owner.is_empty() {
        points.push(first.point);
        return (points, Vec::new(), corners);
    }
    let gap = |index: usize| runs[owner[index]].gap;
    let mut gaps: Vec<f64> = Vec::with_capacity(steps.len());
    let (axis, mut start) = line(0);
    start[axis] = first.point[axis];
    points.push(start);
    let mut previous = line(0);
    for index in 1..owner.len() {
        let (a, held) = previous;
        let (b, mut next) = if owner[index] == owner[index - 1] {
            previous
        } else {
            line(index)
        };
        for q in 0..3 {
            if q != a && q != b && (next[q] - held[q]).abs() <= TOLERANCE {
                next[q] = held[q];
            }
        }
        let (before, after) = (gap(index - 1), gap(index));
        let across = before.min(after);
        let mut corner = held;
        if a != b && (a == 1 || b == 1) {
            let third = 3 - a - b;
            if held[third] != next[third] {
                corners.push(index);
            }
            if held[third] != next[third]
                && let Some(&place) = moved.get(&index)
                && place >= PARTS.len()
            {
                let part = PARTS[place - PARTS.len()];
                if a == 1 {
                    corner[1] = next[1];
                    points.push(corner);
                    gaps.push(before);
                    corner[b] = held[b] + (steps[index + 1].point[b] - held[b]) * part;
                    points.push(corner);
                    gaps.push(across);
                    corner[third] = next[third];
                } else {
                    corner[a] = next[a] + (steps[index - 1].point[a] - next[a]) * part;
                    points.push(corner);
                    gaps.push(before);
                    corner[third] = next[third];
                    points.push(corner);
                    gaps.push(across);
                    corner[a] = next[a];
                }
                points.push(corner);
                gaps.push(across);
                previous = (b, next);
                continue;
            }
            if held[third] != next[third]
                && let Some(&place) = moved.get(&index)
            {
                let level = |point: usize| steps[point].point[1];
                let height = |far: usize| level(index) + (level(far) - level(index)) * PARTS[place];
                if a == 1 {
                    corner[1] = height(index - 1);
                    let back = points.len() >= 2
                        && points[points.len() - 1] == corner
                        && (0..3).all(|q| q == third || points[points.len() - 2][q] == corner[q]);
                    if back {
                        points.pop();
                        gaps.pop();
                        corner = points[points.len() - 1];
                    } else {
                        points.push(corner);
                        gaps.push(before);
                    }
                    corner[third] = next[third];
                    points.push(corner);
                    gaps.push(across);
                    corner[1] = next[1];
                } else {
                    corner[a] = next[a];
                    points.push(corner);
                    gaps.push(before);
                    corner[1] = height(index + 1);
                    points.push(corner);
                    gaps.push(across);
                    corner[third] = next[third];
                }
                points.push(corner);
                gaps.push(across);
                previous = (b, next);
                continue;
            }
        }
        corner[a] = if a == b {
            steps[index].point[a]
        } else {
            next[a]
        };
        points.push(corner);
        gaps.push(before);
        let towards = [runs[owner[index - 1]].toward, runs[owner[index]].toward];
        let mut order = [0, 1, 2];
        order.sort_by_key(|&q| match (towards[0][q] == 0.0, towards[1][q] == 0.0) {
            (true, _) => 0,
            (false, false) => 1,
            (false, true) => 2,
        });
        for q in order {
            if q != a && q != b && corner[q] != next[q] {
                corner[q] = next[q];
                points.push(corner);
                gaps.push(across);
            }
        }
        previous = (b, next);
    }
    let (axis, mut end) = previous;
    end[axis] = steps[steps.len() - 1].point[axis];
    points.push(end);
    gaps.push(gap(owner.len() - 1));
    let (points, gaps) = snap(&points, &gaps);
    (points, gaps, corners)
}

/// The track of each segment of the polyline `points` and its lowest and highest coordinate on
/// its axis.
fn pieces(points: &[Vec3]) -> impl Iterator<Item = (Track, [f64; 2])> + '_ {
    points.windows(2).map(|pair| {
        let axis = along(pair[0], pair[1]);
        let grain = |value: f64| (value / GRAIN).round() as i64;
        let others = [
            grain(pair[0][(axis + 1) % 3]),
            grain(pair[0][(axis + 2) % 3]),
        ];
        (
            (axis, others),
            [
                pair[0][axis].min(pair[1][axis]),
                pair[0][axis].max(pair[1][axis]),
            ],
        )
    })
}

/// The coordinate of the line of `run` across its axis and the direction of its lanes.
fn plane(run: &Run) -> f64 {
    let axis = unit(run.axis);
    (0..3)
        .map(|q| {
            let (a, b) = ((q + 1) % 3, (q + 2) % 3);
            run.line[q] * (axis[a] * run.toward[b] - axis[b] * run.toward[a]).abs()
        })
        .sum()
}

/// The coordinate of the line of `run` in the direction of its lanes.
fn across(run: &Run) -> f64 {
    (0..3).map(|q| run.line[q] * run.toward[q]).sum()
}

/// The coordinate of the lane of `run` on `axis`.
fn lane(run: &Run, axis: usize) -> f64 {
    run.line[axis] + run.offset * run.toward[axis]
}

/// The lowest and highest coordinate of the line of `runs[run]` in its lane on its axis: an end
/// next to a run on another axis is on the lane of that run.
fn reach(runs: &[Run], run: usize) -> [f64; 2] {
    let current = &runs[run];
    let end = |next: Option<usize>, base: f64| match next {
        Some(next) if runs[next].axis != current.axis => lane(&runs[next], current.axis),
        _ => base,
    };
    let (start, finish) = if current.sign > 0.0 {
        (current.low, current.high)
    } else {
        (current.high, current.low)
    };
    let (start, finish) = (end(current.before, start), end(current.after, finish));
    [start.min(finish), start.max(finish)]
}

/// The bundle of `run` in the forest `bundle_of`: its lowest run.
fn find(bundle_of: &mut [usize], run: usize) -> usize {
    let mut root = run;
    while bundle_of[root] != root {
        root = bundle_of[root];
    }
    let mut current = run;
    while bundle_of[current] != root {
        let next = bundle_of[current];
        bundle_of[current] = root;
        current = next;
    }
    root
}

/// Joins the bundles of `first` and `second` under the lower run, and gives `true` when they
/// were 2 bundles.
fn join(bundle_of: &mut [usize], first: usize, second: usize) -> bool {
    let (first, second) = (find(bundle_of, first), find(bundle_of, second));
    if first == second {
        return false;
    }
    let (low, high) = (first.min(second), first.max(second));
    bundle_of[high] = low;
    true
}

/// The polyline `points` with the gap of each segment in `gaps`, with each coordinate that
/// differs from the one of the point before by `TOLERANCE` or less set to that one, and with no
/// point 2 times in sequence.
fn snap(points: &[Vec3], gaps: &[f64]) -> (Vec<Vec3>, Vec<f64>) {
    let Some(&first) = points.first() else {
        return (Vec::new(), Vec::new());
    };
    let mut kept = vec![first];
    let mut kept_gaps = Vec::with_capacity(gaps.len());
    for (&point, &gap) in points[1..].iter().zip(gaps) {
        let last = kept[kept.len() - 1];
        let point = [0, 1, 2].map(|q| {
            if (point[q] - last[q]).abs() <= TOLERANCE {
                last[q]
            } else {
                point[q]
            }
        });
        if point != last {
            kept.push(point);
            kept_gaps.push(gap);
        }
    }
    (kept, kept_gaps)
}

/// The axis on which `start` and `end` differ most.
fn along(start: Vec3, end: Vec3) -> usize {
    (0..3)
        .max_by(|&a, &b| {
            (end[a] - start[a])
                .abs()
                .total_cmp(&(end[b] - start[b]).abs())
                .then(b.cmp(&a))
        })
        .unwrap_or(0)
}

/// The unit vector along `axis`.
fn unit(axis: usize) -> Vec3 {
    let mut vector = [0.0; 3];
    vector[axis] = 1.0;
    vector
}

/// The direction of the lanes of a segment along `axis`, of length 1, outside a corridor of the
/// arrangement stack.
fn lateral(axis: usize) -> Vec3 {
    match axis {
        0 => [0.0, 0.0, 1.0],
        2 => [1.0, 0.0, 0.0],
        _ => [
            std::f64::consts::FRAC_1_SQRT_2,
            0.0,
            -std::f64::consts::FRAC_1_SQRT_2,
        ],
    }
}

/// The sign of the first distance along `toward` from the point `from` of `points` to the
/// points at the indexes of `others` that is not 0, or 0.
fn side(points: &[Vec3], from: usize, others: impl Iterator<Item = usize>, toward: Vec3) -> f64 {
    for other in others {
        let distance: f64 = (0..3)
            .map(|q| (points[other][q] - points[from][q]) * toward[q])
            .sum();
        if distance.abs() > TOLERANCE {
            return distance.signum();
        }
    }
    0.0
}

/// Whether the segment from `start` to `end` goes through the inside of `placed`. A segment on
/// a face does not.
fn crosses(start: Vec3, end: Vec3, placed: &Placed) -> bool {
    let mut enter: f64 = 0.0;
    let mut leave: f64 = 1.0;
    for axis in 0..3 {
        let low = placed.centre[axis] - placed.size[axis] / 2.0 + TOLERANCE;
        let high = placed.centre[axis] + placed.size[axis] / 2.0 - TOLERANCE;
        let step = end[axis] - start[axis];
        if step.abs() < TOLERANCE {
            if start[axis] <= low || start[axis] >= high {
                return false;
            }
            continue;
        }
        let first = (low - start[axis]) / step;
        let second = (high - start[axis]) / step;
        enter = enter.max(first.min(second));
        leave = leave.min(first.max(second));
        if enter >= leave {
            return false;
        }
    }
    true
}

fn inside(child: &Placed, parent: &Placed) -> bool {
    (0..3).all(|axis| {
        let reach = (child.centre[axis] - parent.centre[axis]).abs() + child.size[axis] / 2.0;
        reach <= parent.size[axis] / 2.0 + TOLERANCE
    })
}

fn intersect(first: &Placed, second: &Placed) -> bool {
    (0..3).all(|axis| {
        let distance = (first.centre[axis] - second.centre[axis]).abs();
        distance + TOLERANCE < (first.size[axis] + second.size[axis]) / 2.0
    })
}
