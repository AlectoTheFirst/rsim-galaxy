use std::fmt;
use std::sync::Arc;

use crate::document::Load;
use crate::id::ObjectId;
use crate::library::{Colours, DefinedShape, Library, Material, Part, Solid};
use crate::style::{Look, Style};
use crate::world::{
    Arrangement, Flow, Magnitude, Node, Outcome, RelationRefusal, Relations, Rule, Shape, Vec3,
    World,
};

/// The rule that a `SetRule` sets.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuleTarget {
    /// The rule that places the nodes with no parent.
    Root,
    /// The rule of a kind with no rule.
    Default,
    /// The rule of a kind.
    Kind(String),
}

/// A change of a world.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    AddNode {
        id: ObjectId,
        kind: String,
        parent: Option<ObjectId>,
        label: Option<String>,
        fields: Vec<(String, String)>,
    },
    /// Adds a flow. `hops` holds the fields of each node of `path`, or no item.
    AddFlow {
        id: ObjectId,
        path: Vec<ObjectId>,
        outcome: Option<Outcome>,
        magnitude: Option<Magnitude>,
        group: Option<String>,
        reply_to: Option<ObjectId>,
        hops: Vec<Vec<(String, String)>>,
        fields: Vec<(String, String)>,
    },
    /// Sets the rule of `target`. `shape` is the name of a built-in or a defined shape. A `shape`
    /// or a `label` of `None` keeps the value of the rule that `target` has.
    SetRule {
        target: RuleTarget,
        arrangement: Arrangement,
        shape: Option<String>,
        size: Vec3,
        gap: f64,
        margin: f64,
        label: Option<bool>,
    },
    SetLook {
        look: Look,
    },
    /// Defines the shape `name` from its parts. A defined name gets the new parts and the new
    /// symbol.
    DefineShape {
        name: String,
        parts: Vec<Part>,
        symbol: Vec<usize>,
    },
    /// Sets the colours of the material `name`, and adds a material of a new name.
    SetMaterial {
        name: String,
        colours: Colours,
    },
    /// Removes all nodes and flows.
    Clear,
    /// Replaces the nodes and the flows with the records of a read document.
    Load(Load),
    /// The arguments of `load_view`: the name of the source, the query, and the SHA-256 hash of
    /// the kept bytes of the document of a log entry. The viewport reads the document and gives
    /// the `Load`.
    LoadView {
        source: String,
        query: String,
        sha256: Option<String>,
    },
}

/// Cause of a refused change.
#[derive(Clone, Debug, PartialEq)]
pub enum ChangeRefusal {
    Relation(RelationRefusal),
    /// A kind with the name of a rule target: root or default.
    Kind {
        kind: String,
    },
    /// A value of a rule out of its range.
    Range {
        field: &'static str,
        value: f64,
        valid: &'static str,
    },
    /// A value of a shape or a material out of its valid values.
    Value {
        field: String,
        value: String,
        valid: String,
    },
    /// A `LoadView` with no read document.
    Unread,
}

impl fmt::Display for ChangeRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChangeRefusal::Relation(refusal) => write!(f, "{refusal}"),
            ChangeRefusal::Kind { kind } => {
                write!(f, "kind: {kind}, valid: a name other than root and default")
            }
            ChangeRefusal::Range {
                field,
                value,
                valid,
            } => write!(f, "{field}: {value}, valid: {valid}"),
            ChangeRefusal::Value {
                field,
                value,
                valid,
            } => write!(f, "{field}: {value}, valid: {valid}"),
            ChangeRefusal::Unread => {
                write!(
                    f,
                    "load_view: not read, valid: a document read by the viewport"
                )
            }
        }
    }
}

impl std::error::Error for ChangeRefusal {}

/// The relations and the style of a world with the log of the changes that give them from the
/// base style.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    base: Style,
    style: Style,
    relations: Arc<Relations>,
    log: Vec<Change>,
}

impl Model {
    /// The model with no node and the style `base`.
    pub fn new(base: Style) -> Self {
        Self {
            style: base.clone(),
            base,
            relations: Arc::new(Relations::new()),
            log: Vec::new(),
        }
    }

    pub fn style(&self) -> &Style {
        &self.style
    }

    pub fn relations(&self) -> &Relations {
        &self.relations
    }

    /// The applied changes in order, with no undone change.
    pub fn log(&self) -> &[Change] {
        &self.log
    }

    /// The world of the relations by the rule set of the style, with the defined shapes and the
    /// materials of the style.
    pub fn world(&self) -> World {
        World::expand(&self.relations, &self.style.rules).with_library(self.style.library.clone())
    }

    /// Applies `change` and adds it to the log. A refused change changes nothing.
    pub fn apply(&mut self, change: Change) -> Result<(), ChangeRefusal> {
        match &change {
            Change::AddNode {
                id,
                kind,
                parent,
                label,
                fields,
            } => {
                check_kind(kind)?;
                if let Some(label) = label {
                    check_label(label)?;
                }
                Arc::make_mut(&mut self.relations)
                    .add_node(Node {
                        id: id.clone(),
                        kind: kind.clone(),
                        parent: parent.clone(),
                        label: label.clone(),
                        fields: fields.clone(),
                    })
                    .map_err(ChangeRefusal::Relation)?
            }
            Change::AddFlow {
                id,
                path,
                outcome,
                magnitude,
                group,
                reply_to,
                hops,
                fields,
            } => {
                if let Some(magnitude) = magnitude
                    && !(magnitude.value.is_finite() && magnitude.value >= 0.0)
                {
                    return Err(ChangeRefusal::Value {
                        field: "magnitude.value".to_owned(),
                        value: magnitude.value.to_string(),
                        valid: "0 or more".to_owned(),
                    });
                }
                if let Some(group) = group {
                    check_name("group", group)?;
                }
                Arc::make_mut(&mut self.relations)
                    .add_flow_with_data(Flow {
                        id: id.clone(),
                        path: path.clone(),
                        outcome: outcome.clone(),
                        magnitude: *magnitude,
                        group: group.clone(),
                        reply_to: reply_to.clone(),
                        hops: hops.clone(),
                        fields: fields.clone(),
                    })
                    .map_err(ChangeRefusal::Relation)?
            }
            Change::SetRule {
                target,
                arrangement,
                shape,
                size,
                gap,
                margin,
                label,
            } => {
                let rules = &mut self.style.rules;
                let current = match target {
                    RuleTarget::Root => &rules.root,
                    RuleTarget::Default => &rules.default,
                    RuleTarget::Kind(kind) => {
                        check_kind(kind)?;
                        rules.kinds.get(kind).unwrap_or(&rules.default)
                    }
                };
                let shape = match shape {
                    None => current.shape,
                    Some(name) => resolve(&self.style.library, name)?,
                };
                let rule = Rule {
                    arrangement: *arrangement,
                    shape,
                    size: *size,
                    gap: *gap,
                    margin: *margin,
                    label: label.unwrap_or(current.label),
                };
                check(&rule, target == &RuleTarget::Root)?;
                match target {
                    RuleTarget::Root => rules.root = rule,
                    RuleTarget::Default => rules.default = rule,
                    RuleTarget::Kind(kind) => {
                        rules.kinds.insert(kind.clone(), rule);
                    }
                }
            }
            Change::SetLook { look } => self.style.look = *look,
            Change::DefineShape {
                name,
                parts,
                symbol,
            } => {
                check_shape(&self.style.library, name, parts, symbol)?;
                let shape = DefinedShape {
                    name: name.clone(),
                    parts: parts.clone(),
                    symbol: symbol.clone(),
                };
                let library = &mut self.style.library;
                let index = library.shape(name);
                if index.is_none() && library.shapes.len() >= Library::SHAPES {
                    return Err(ChangeRefusal::Value {
                        field: "shapes".to_owned(),
                        value: (Library::SHAPES + 1).to_string(),
                        valid: format!("{} at most", Library::SHAPES),
                    });
                }
                let replaced = index.map_or(0, |index| library.shapes[index].vertices());
                let vertices = library.vertices() - replaced + shape.vertices();
                if vertices > Library::VERTICES {
                    return Err(ChangeRefusal::Value {
                        field: "vertices".to_owned(),
                        value: vertices.to_string(),
                        valid: format!("{} at most", Library::VERTICES),
                    });
                }
                match index {
                    Some(index) => library.shapes[index] = shape,
                    None => library.shapes.push(shape),
                }
            }
            Change::SetMaterial { name, colours } => {
                check_name("name", name)?;
                let library = &mut self.style.library;
                match library.material(name) {
                    Some(index) => library.materials[index].colours = Some(*colours),
                    None if library.materials.len() >= Library::MATERIALS => {
                        return Err(ChangeRefusal::Value {
                            field: "materials".to_owned(),
                            value: (Library::MATERIALS + 1).to_string(),
                            valid: format!("{} at most", Library::MATERIALS),
                        });
                    }
                    None => library.materials.push(Material {
                        name: name.clone(),
                        colours: Some(*colours),
                    }),
                }
            }
            Change::Clear => self.relations = Arc::new(Relations::new()),
            Change::Load(load) => self.relations = load.shared(),
            Change::LoadView { .. } => return Err(ChangeRefusal::Unread),
        }
        self.log.push(change);
        Ok(())
    }

    /// Reverts the last change of the log, removes it from the log and gives it back. The model
    /// applies the log again with no add node and no add flow before the last load or clear.
    pub fn undo(&mut self) -> Option<Change> {
        let change = self.log.pop()?;
        let log = std::mem::take(&mut self.log);
        let reset = log
            .iter()
            .rposition(|change| matches!(change, Change::Load(_) | Change::Clear));
        let mut model = Self::new(self.base.clone());
        for (index, change) in log.into_iter().enumerate() {
            let replaced = reset.is_some_and(|reset| index < reset)
                && matches!(change, Change::AddNode { .. } | Change::AddFlow { .. });
            if replaced {
                model.log.push(change);
            } else {
                model.apply(change).expect("each change of the log applies");
            }
        }
        *self = model;
        Some(change)
    }

    /// The model of a log from the base style. The error holds the index of the refused change.
    pub fn replay(base: Style, log: &[Change]) -> Result<Self, (usize, ChangeRefusal)> {
        let mut model = Self::new(base);
        for (index, change) in log.iter().enumerate() {
            model
                .apply(change.clone())
                .map_err(|refusal| (index, refusal))?;
        }
        Ok(model)
    }
}

/// Refuses the kinds root and default, the names of the rule targets of the wire form.
fn check_kind(kind: &str) -> Result<(), ChangeRefusal> {
    if kind == "root" || kind == "default" {
        return Err(ChangeRefusal::Kind {
            kind: kind.to_owned(),
        });
    }
    Ok(())
}

/// Refuses a label of fewer than 1 or more than 256 characters with its count of characters.
fn check_label(label: &str) -> Result<(), ChangeRefusal> {
    if !Node::valid_label(label) {
        let [fewest, most] = Node::LABEL;
        return Err(ChangeRefusal::Value {
            field: "label".to_owned(),
            value: format!("{} characters", label.chars().count()),
            valid: format!("{fewest} to {most} characters"),
        });
    }
    Ok(())
}

/// The built-in or defined shape `name`.
fn resolve(library: &Library, name: &str) -> Result<Shape, ChangeRefusal> {
    if let Some(shape) = Shape::parse(name) {
        return Ok(shape);
    }
    library
        .shape(name)
        .map(Shape::Defined)
        .ok_or_else(|| ChangeRefusal::Value {
            field: "shape".to_owned(),
            value: name.to_owned(),
            valid: Shape::NAMES
                .iter()
                .copied()
                .chain(library.shapes.iter().map(|shape| shape.name.as_str()))
                .collect::<Vec<_>>()
                .join(", "),
        })
}

/// The rule of the names of shapes and materials.
pub(crate) const NAME_RULE: &str = "1 to 64 ASCII letters, digits, -, _ and .";

/// Largest distance of a coordinate of a part outside the unit box.
const TOLERANCE: f64 = 1e-9;

fn check_name(field: &str, name: &str) -> Result<(), ChangeRefusal> {
    if ObjectId::new(name).is_none() {
        return Err(ChangeRefusal::Value {
            field: field.to_owned(),
            value: if name.is_empty() {
                "\"\"".to_owned()
            } else {
                name.to_owned()
            },
            valid: NAME_RULE.to_owned(),
        });
    }
    Ok(())
}

fn material_names(library: &Library) -> String {
    library
        .materials
        .iter()
        .map(|material| material.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Refuses a shape with a name out of the name rule or of a built-in shape, a count of parts
/// out of `Library::PARTS`, a part with a size of 0 or less or outside the unit box, a part of
/// an unknown material, a prism with a count of points out of `Library::POINTS`, a point
/// outside the unit square or a polygon that is not simple, a symbol index with no part, and a
/// symbol that holds each part.
fn check_shape(
    library: &Library,
    name: &str,
    parts: &[Part],
    symbol: &[usize],
) -> Result<(), ChangeRefusal> {
    check_name("name", name)?;
    if Shape::parse(name).is_some() {
        return Err(ChangeRefusal::Value {
            field: "name".to_owned(),
            value: name.to_owned(),
            valid: format!("a name other than {}", Shape::NAMES.join(", ")),
        });
    }
    let [fewest, most] = Library::PARTS;
    if !(fewest..=most).contains(&parts.len()) {
        return Err(ChangeRefusal::Value {
            field: "parts".to_owned(),
            value: format!("{} parts", parts.len()),
            valid: format!("{fewest} to {most} parts"),
        });
    }
    for (index, part) in parts.iter().enumerate() {
        let field = |name: &str| format!("parts.{index}.{name}");
        for value in part.size {
            if !(value.is_finite() && value > 0.0) {
                return Err(ChangeRefusal::Value {
                    field: field("size"),
                    value: value.to_string(),
                    valid: "above 0".to_owned(),
                });
            }
        }
        let inside = (0..3).all(|axis| {
            let (at, half) = (part.at[axis], part.size[axis] / 2.0);
            at - half >= -0.5 - TOLERANCE && at + half <= 0.5 + TOLERANCE
        });
        if !inside {
            let [x, y, z] = part.at;
            let [width, height, depth] = part.size;
            return Err(ChangeRefusal::Value {
                field: format!("parts.{index}"),
                value: format!("at={x},{y},{z} size={width},{height},{depth}"),
                valid: "at - size/2 and at + size/2 from -0.5 to 0.5 on each axis".to_owned(),
            });
        }
        if library.material(&part.material).is_none() {
            return Err(ChangeRefusal::Value {
                field: field("material"),
                value: part.material.clone(),
                valid: material_names(library),
            });
        }
        if let Solid::Prism(points) = &part.solid {
            check_polygon(&field("points"), points)?;
        }
    }
    if let Some(&index) = symbol.iter().find(|&&index| index >= parts.len()) {
        return Err(ChangeRefusal::Value {
            field: "symbol".to_owned(),
            value: index.to_string(),
            valid: format!("part indexes 0 to {}", parts.len() - 1),
        });
    }
    if (0..parts.len()).all(|index| symbol.contains(&index)) {
        return Err(ChangeRefusal::Value {
            field: "symbol".to_owned(),
            value: format!("all {} parts", parts.len()),
            valid: "part indexes that leave out 1 part or more".to_owned(),
        });
    }
    Ok(())
}

/// Refuses a count of points out of `Library::POINTS`, a point outside the unit square, and a
/// polygon with an edge of length 0, 2 edges that touch other than at their common point, or
/// an area of 0.
fn check_polygon(field: &str, points: &[[f64; 2]]) -> Result<(), ChangeRefusal> {
    let [fewest, most] = Library::POINTS;
    if !(fewest..=most).contains(&points.len()) {
        return Err(ChangeRefusal::Value {
            field: field.to_owned(),
            value: format!("{} points", points.len()),
            valid: format!("{fewest} to {most} points"),
        });
    }
    for (index, &[u, v]) in points.iter().enumerate() {
        let inside = |value: f64| (-0.5 - TOLERANCE..=0.5 + TOLERANCE).contains(&value);
        if !(inside(u) && inside(v)) {
            return Err(ChangeRefusal::Value {
                field: format!("{field}.{index}"),
                value: format!("{u},{v}"),
                valid: "-0.5 to 0.5 on each axis".to_owned(),
            });
        }
    }
    let count = points.len();
    let edge = |index: usize| (points[index], points[(index + 1) % count]);
    let not_simple = |cause: String| ChangeRefusal::Value {
        field: field.to_owned(),
        value: cause,
        valid: "a simple polygon".to_owned(),
    };
    if let Some(index) = (0..count).find(|&index| edge(index).0 == edge(index).1) {
        return Err(not_simple(format!("edge {index} of length 0")));
    }
    for first in 0..count {
        let (a, b) = edge(first);
        for second in first + 1..count {
            let (c, d) = edge(second);
            let adjacent = second == first + 1 || (first == 0 && second == count - 1);
            let touch = if adjacent {
                let (shared, own, other) = if second == first + 1 {
                    (b, a, d)
                } else {
                    (a, b, c)
                };
                overlap(shared, own, other)
            } else {
                intersect(a, b, c, d)
            };
            if touch {
                return Err(not_simple(format!("edges {first} and {second} touch")));
            }
        }
    }
    let area: f64 = (0..count)
        .map(|index| {
            let ([u0, v0], [u1, v1]) = edge(index);
            u0 * v1 - u1 * v0
        })
        .sum();
    if area.abs() / 2.0 <= TOLERANCE {
        return Err(not_simple("area 0".to_owned()));
    }
    Ok(())
}

/// The cross product of `b - a` and `c - a`.
fn cross(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// `true` for `c` on the line through `a` and `b`: the cross product is at most `TOLERANCE` times
/// the lengths of `b - a` and `c - a`.
fn collinear(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> bool {
    let length = |p: [f64; 2]| ((p[0] - a[0]).powi(2) + (p[1] - a[1]).powi(2)).sqrt();
    cross(a, b, c).abs() <= TOLERANCE * length(b) * length(c)
}

/// `true` for a point `c` on the line through `a` and `b` within their bounds.
fn on_segment(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> bool {
    collinear(a, b, c)
        && c[0] >= a[0].min(b[0])
        && c[0] <= a[0].max(b[0])
        && c[1] >= a[1].min(b[1])
        && c[1] <= a[1].max(b[1])
}

/// `true` for the segments `a b` and `c d` with a common point.
fn intersect(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let (d1, d2) = (cross(c, d, a), cross(c, d, b));
    let (d3, d4) = (cross(a, b, c), cross(a, b, d));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true;
    }
    on_segment(c, d, a) || on_segment(c, d, b) || on_segment(a, b, c) || on_segment(a, b, d)
}

/// `true` for 2 edges from the common point `shared` to `own` and to `other` with more than
/// their common point in common: the edges lie on 1 line on the same side of `shared`.
fn overlap(shared: [f64; 2], own: [f64; 2], other: [f64; 2]) -> bool {
    let along = |p: [f64; 2]| [p[0] - shared[0], p[1] - shared[1]];
    let (first, second) = (along(own), along(other));
    collinear([0.0, 0.0], first, second) && first[0] * second[0] + first[1] * second[1] > 0.0
}

/// Refuses a rule with a size of 0 or less, a gap or a margin below 0, or a value that is not
/// finite. The rule of the nodes with no parent has no size of its own.
fn check(rule: &Rule, root: bool) -> Result<(), ChangeRefusal> {
    if !root {
        for value in rule.size {
            if !(value.is_finite() && value > 0.0) {
                return Err(ChangeRefusal::Range {
                    field: "size",
                    value,
                    valid: "above 0",
                });
            }
        }
    }
    for (field, value) in [("gap", rule.gap), ("margin", rule.margin)] {
        if !(value.is_finite() && value >= 0.0) {
            return Err(ChangeRefusal::Range {
                field,
                value,
                valid: "0 or more",
            });
        }
    }
    Ok(())
}
