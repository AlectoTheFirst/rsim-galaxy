use std::fmt;

use crate::id::ObjectId;
use crate::json::Json;
use crate::library::{Axis, Colours, Part, Solid, hex, parse_hex};
use crate::model::{Change, NAME_RULE, RuleTarget};
use crate::sha256;
use crate::style::{Light, Lines, Look, Projection, Scheme, Space};
use crate::world::{Arrangement, Node, Outcome, OutcomeKind};

/// An operation of the wire form: its name, its description and the JSON schema of its
/// arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: &'static str,
}

/// The operations that change a world.
pub const TOOLS: &[Tool] = &[
    Tool {
        name: "add_node",
        description: "Add a node",
        schema: r#"{"type":"object","properties":{"name":{"type":"string","description":"Node name","pattern":"^[A-Za-z0-9._-]{1,64}$"},"kind":{"type":"string","description":"Kind"},"in":{"type":"string","description":"Containing node"},"label":{"type":"string","description":"Label","minLength":1,"maxLength":256},"fields":{"type":"object","description":"Source data","maxProperties":64,"propertyNames":{"pattern":"^[A-Za-z0-9._-]{1,64}$"},"additionalProperties":{"type":["string","number","boolean"],"minLength":1,"maxLength":1024}}},"required":["name","kind"]}"#,
    },
    Tool {
        name: "add_flow",
        description: "Add a flow",
        schema: r#"{"type":"object","properties":{"name":{"type":"string","description":"Flow name","pattern":"^[A-Za-z0-9._-]{1,64}$"},"path":{"type":"array","description":"Path","items":{"type":"string"},"minItems":2},"outcome":{"type":"object","description":"Outcome","properties":{"kind":{"type":"string","enum":["deliver","exit","drop","loop","unknown"]},"reason":{"type":"string","description":"Reason"}},"required":["kind"]},"fields":{"type":"object","description":"Source data","maxProperties":64,"propertyNames":{"pattern":"^[A-Za-z0-9._-]{1,64}$"},"additionalProperties":{"type":["string","number","boolean"],"minLength":1,"maxLength":1024}}},"required":["name","path"]}"#,
    },
    Tool {
        name: "set_rule",
        description: "Set the rule of a kind",
        schema: r#"{"type":"object","properties":{"kind":{"type":"string","description":"Kind"},"arrangement":{"type":"string","enum":["row","stack","grid","layers"]},"shape":{"type":"string","description":"Shape name","anyOf":[{"enum":["box","cylinder","router","switch","firewall","device","access-point","workstation","cloud","layer3-switch","server","laptop","load-balancer"]},{"pattern":"^[A-Za-z0-9._-]{1,64}$"}]},"size":{"type":"array","description":"Size","items":{"type":"number","exclusiveMinimum":0},"minItems":3,"maxItems":3},"gap":{"type":"number","minimum":0},"margin":{"type":"number","minimum":0},"label":{"type":"boolean","description":"Label"}},"required":["kind","arrangement"]}"#,
    },
    Tool {
        name: "set_look",
        description: "Set the look",
        schema: r#"{"type":"object","properties":{"projection":{"type":"string","enum":["perspective","isometric"]},"space":{"type":"string","enum":["ground","floating"]},"light":{"type":"string","enum":["sun","flat"]},"lines":{"type":"string","enum":["crisp","glow"]},"scheme":{"type":"string","enum":["light","dark"]}}}"#,
    },
    Tool {
        name: "define_shape",
        description: "Define a shape",
        schema: r#"{"type":"object","properties":{"name":{"type":"string","description":"Shape name","pattern":"^[A-Za-z0-9._-]{1,64}$"},"parts":{"type":"array","minItems":1,"maxItems":256,"items":{"type":"object","properties":{"solid":{"type":"string","enum":["box","cylinder","prism"]},"at":{"type":"array","description":"Centre","items":{"type":"number","minimum":-0.5,"maximum":0.5},"minItems":3,"maxItems":3},"size":{"type":"array","description":"Size","items":{"type":"number","exclusiveMinimum":0,"maximum":1},"minItems":3,"maxItems":3},"axis":{"type":"string","description":"Axis","enum":["x","y","z"]},"points":{"type":"array","description":"Polygon","items":{"type":"array","items":{"type":"number","minimum":-0.5,"maximum":0.5},"minItems":2,"maxItems":2},"minItems":3,"maxItems":64},"material":{"type":"string","description":"Material name"},"edges":{"type":"boolean","description":"Outline edges"}},"required":["solid","at","size","material"]}},"symbol":{"type":"array","description":"Symbol parts","items":{"type":"integer","minimum":0}}},"required":["name","parts"]}"#,
    },
    Tool {
        name: "set_material",
        description: "Set the colours of a material",
        schema: r#"{"type":"object","properties":{"name":{"type":"string","description":"Material name","pattern":"^[A-Za-z0-9._-]{1,64}$"},"light":{"type":"string","description":"Light colour","pattern":"^#[0-9A-Fa-f]{6}$"},"dark":{"type":"string","description":"Dark colour","pattern":"^#[0-9A-Fa-f]{6}$"},"lit":{"type":"boolean","description":"Lit"}},"required":["name","light","dark"]}"#,
    },
    Tool {
        name: "clear",
        description: "Remove all nodes and flows",
        schema: r#"{"type":"object","properties":{}}"#,
    },
    Tool {
        name: "load_view",
        description: "Load the relations document of a source",
        schema: r#"{"type":"object","properties":{"source":{"type":"string","description":"Source name","pattern":"^[A-Za-z0-9._-]{1,64}$"},"query":{"type":"string","description":"Query","maxLength":8192}},"required":["source","query"]}"#,
    },
];

/// Cause of refused arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WireRefusal {
    /// The name is not the name of an operation.
    NoTool { name: String },
    /// A required argument is missing.
    Missing { field: &'static str },
    /// An argument has another JSON type.
    Type {
        field: &'static str,
        expected: &'static str,
    },
    /// An argument has a value out of its valid values.
    Value {
        field: &'static str,
        value: String,
        valid: String,
    },
    /// A text value of a field has a length out of `FIELD_TEXT`.
    Length { name: String, length: usize },
    /// An argument of the part `index` of a shape is refused.
    Part {
        index: usize,
        refusal: Box<WireRefusal>,
    },
}

impl fmt::Display for WireRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WireRefusal::NoTool { name } => {
                let names: Vec<&str> = TOOLS.iter().map(|tool| tool.name).collect();
                write!(f, "no tool {name}, valid: {}", names.join(", "))
            }
            WireRefusal::Missing { field } => write!(f, "{field}: missing"),
            WireRefusal::Type { field, expected } => write!(f, "{field}: expected {expected}"),
            WireRefusal::Value {
                field,
                value,
                valid,
            } if value.is_empty() => write!(f, "{field}: \"\", valid: {valid}"),
            WireRefusal::Value {
                field,
                value,
                valid,
            } => write!(f, "{field}: {value}, valid: {valid}"),
            WireRefusal::Length { name, length } => write!(
                f,
                "fields.{name}: {length} characters, valid: {} to {} characters",
                FIELD_TEXT[0], FIELD_TEXT[1]
            ),
            WireRefusal::Part { index, refusal } => write!(f, "parts.{index}.{refusal}"),
        }
    }
}

impl std::error::Error for WireRefusal {}

const TEXT_RULE: &str = "1 or more characters";
/// Largest number of characters of a query.
const QUERY: usize = 8192;
/// Largest number of fields of a record.
const FIELDS: usize = 64;
/// Smallest and largest number of characters of a text value of a field.
const FIELD_TEXT: [usize; 2] = [1, 1024];

/// The change of the operation `name` with `arguments`, a JSON object. `load_view` gives
/// `Change::LoadView`.
pub fn decode(name: &str, arguments: &Json) -> Result<Change, WireRefusal> {
    let empty = Json::Object(Vec::new());
    let arguments = match arguments {
        Json::Null => &empty,
        Json::Object(_) => arguments,
        _ => {
            return Err(WireRefusal::Type {
                field: "arguments",
                expected: "object",
            });
        }
    };
    match name {
        "add_node" => Ok(Change::AddNode {
            id: object_id(arguments, "name")?,
            kind: kind(arguments)?.to_owned(),
            parent: match arguments.get("in") {
                None | Some(Json::Null) => None,
                Some(_) => Some(object_id(arguments, "in")?),
            },
            label: label(arguments)?,
            fields: fields(arguments)?,
        }),
        "add_flow" => {
            let items = arguments
                .get("path")
                .ok_or(WireRefusal::Missing { field: "path" })?
                .as_array()
                .ok_or(WireRefusal::Type {
                    field: "path",
                    expected: "array of strings",
                })?;
            let path = items
                .iter()
                .map(|item| {
                    let text = item.as_str().ok_or(WireRefusal::Type {
                        field: "path",
                        expected: "array of strings",
                    })?;
                    ObjectId::new(text).ok_or_else(|| WireRefusal::Value {
                        field: "path",
                        value: text.to_owned(),
                        valid: NAME_RULE.to_owned(),
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Change::AddFlow {
                id: object_id(arguments, "name")?,
                path,
                outcome: outcome(arguments)?,
                fields: fields(arguments)?,
            })
        }
        "set_rule" => {
            let kind = kind(arguments)?;
            let target = match kind {
                "root" => RuleTarget::Root,
                "default" => RuleTarget::Default,
                _ => RuleTarget::Kind(kind.to_owned()),
            };
            let arrangement = match text(arguments, "arrangement")? {
                "row" => Arrangement::Row,
                "stack" => Arrangement::Stack,
                "grid" => Arrangement::Grid,
                "layers" => Arrangement::Layers,
                other => {
                    return Err(WireRefusal::Value {
                        field: "arrangement",
                        value: other.to_owned(),
                        valid: "row, stack, grid, layers".to_owned(),
                    });
                }
            };
            let size = match arguments.get("size") {
                None => [1.0; 3],
                Some(value) => {
                    let items = value.as_array().filter(|items| items.len() == 3).ok_or(
                        WireRefusal::Type {
                            field: "size",
                            expected: "array of 3 numbers",
                        },
                    )?;
                    let mut size = [0.0; 3];
                    for (slot, item) in size.iter_mut().zip(items) {
                        *slot = item.as_f64().ok_or(WireRefusal::Type {
                            field: "size",
                            expected: "array of 3 numbers",
                        })?;
                    }
                    size
                }
            };
            let shape = match arguments.get("shape") {
                None => None,
                Some(_) => Some(named(arguments, "shape")?),
            };
            Ok(Change::SetRule {
                target,
                arrangement,
                shape,
                size,
                gap: number(arguments, "gap", 0.5)?,
                margin: number(arguments, "margin", 0.5)?,
                label: match arguments.get("label") {
                    None => None,
                    Some(Json::Bool(value)) => Some(*value),
                    Some(other) => {
                        return Err(WireRefusal::Value {
                            field: "label",
                            value: other.to_string(),
                            valid: "true, false".to_owned(),
                        });
                    }
                },
            })
        }
        "set_look" => Ok(Change::SetLook {
            look: Look {
                projection: choice(
                    arguments,
                    "projection",
                    Projection::parse,
                    Projection::NAMES,
                )?,
                space: choice(arguments, "space", Space::parse, Space::NAMES)?,
                light: choice(arguments, "light", Light::parse, Light::NAMES)?,
                lines: choice(arguments, "lines", Lines::parse, Lines::NAMES)?,
                scheme: choice(arguments, "scheme", Scheme::parse, Scheme::NAMES)?,
            },
        }),
        "define_shape" => {
            let items = arguments
                .get("parts")
                .ok_or(WireRefusal::Missing { field: "parts" })?
                .as_array()
                .ok_or(WireRefusal::Type {
                    field: "parts",
                    expected: "array of objects",
                })?;
            let parts = items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    part(item).map_err(|refusal| WireRefusal::Part {
                        index,
                        refusal: Box::new(refusal),
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let symbol = match arguments.get("symbol") {
                None | Some(Json::Null) => Vec::new(),
                Some(value) => {
                    let expected = WireRefusal::Type {
                        field: "symbol",
                        expected: "array of part indexes",
                    };
                    let items = value.as_array().ok_or(expected.clone())?;
                    items
                        .iter()
                        .map(|item| {
                            item.as_f64()
                                .filter(|index| {
                                    index.fract() == 0.0 && (0.0..=u32::MAX as f64).contains(index)
                                })
                                .map(|index| index as usize)
                                .ok_or(expected.clone())
                        })
                        .collect::<Result<Vec<_>, _>>()?
                }
            };
            Ok(Change::DefineShape {
                name: named(arguments, "name")?,
                parts,
                symbol,
            })
        }
        "set_material" => Ok(Change::SetMaterial {
            name: named(arguments, "name")?,
            colours: Colours {
                light: colour(arguments, "light")?,
                dark: colour(arguments, "dark")?,
                lit: boolean(arguments, "lit", true)?,
            },
        }),
        "clear" => Ok(Change::Clear),
        "load_view" => {
            let source = named(arguments, "source")?;
            let query = text(arguments, "query")?;
            let length = query.chars().count();
            if length > QUERY {
                return Err(WireRefusal::Value {
                    field: "query",
                    value: format!("{length} characters"),
                    valid: format!("0 to {QUERY} characters"),
                });
            }
            let sha256 = match arguments.get("sha256") {
                None => None,
                Some(_) => {
                    let hash = text(arguments, "sha256")?;
                    if !sha256::is_hex(hash) {
                        return Err(WireRefusal::Value {
                            field: "sha256",
                            value: hash.to_owned(),
                            valid: "64 lowercase hex digits".to_owned(),
                        });
                    }
                    Some(hash.to_owned())
                }
            };
            Ok(Change::LoadView {
                source,
                query: query.to_owned(),
                sha256,
            })
        }
        _ => Err(WireRefusal::NoTool {
            name: name.to_owned(),
        }),
    }
}

/// The operation name and the JSON arguments of a change.
pub fn encode(change: &Change) -> (&'static str, Json) {
    match change {
        Change::AddNode {
            id,
            kind,
            parent,
            label,
            fields,
        } => {
            let mut members = vec![
                ("name".to_owned(), Json::string(id.as_str())),
                ("kind".to_owned(), Json::string(kind.as_str())),
            ];
            if let Some(parent) = parent {
                members.push(("in".to_owned(), Json::string(parent.as_str())));
            }
            if let Some(label) = label {
                members.push(("label".to_owned(), Json::string(label.as_str())));
            }
            if !fields.is_empty() {
                members.push(("fields".to_owned(), fields_json(fields)));
            }
            ("add_node", Json::Object(members))
        }
        Change::AddFlow {
            id,
            path,
            outcome,
            fields,
        } => {
            let mut members = vec![
                ("name".to_owned(), Json::string(id.as_str())),
                (
                    "path".to_owned(),
                    Json::Array(path.iter().map(|id| Json::string(id.as_str())).collect()),
                ),
            ];
            if let Some(outcome) = outcome {
                let mut parts = vec![("kind".to_owned(), Json::string(outcome.kind.name()))];
                if let Some(reason) = &outcome.reason {
                    parts.push(("reason".to_owned(), Json::string(reason.as_str())));
                }
                members.push(("outcome".to_owned(), Json::Object(parts)));
            }
            if !fields.is_empty() {
                members.push(("fields".to_owned(), fields_json(fields)));
            }
            ("add_flow", Json::Object(members))
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
            let kind = match target {
                RuleTarget::Root => "root",
                RuleTarget::Default => "default",
                RuleTarget::Kind(kind) => kind,
            };
            let arrangement = match arrangement {
                Arrangement::Row => "row",
                Arrangement::Stack => "stack",
                Arrangement::Grid => "grid",
                Arrangement::Layers => "layers",
            };
            let mut members = vec![
                ("kind".to_owned(), Json::string(kind)),
                ("arrangement".to_owned(), Json::string(arrangement)),
            ];
            if let Some(shape) = shape {
                members.push(("shape".to_owned(), Json::string(shape.as_str())));
            }
            members.extend([
                (
                    "size".to_owned(),
                    Json::Array(size.map(Json::Number).to_vec()),
                ),
                ("gap".to_owned(), Json::Number(*gap)),
                ("margin".to_owned(), Json::Number(*margin)),
            ]);
            if let Some(label) = label {
                members.push(("label".to_owned(), Json::Bool(*label)));
            }
            ("set_rule", Json::Object(members))
        }
        Change::SetLook { look } => (
            "set_look",
            Json::object([
                ("projection", Json::string(look.projection.name())),
                ("space", Json::string(look.space.name())),
                ("light", Json::string(look.light.name())),
                ("lines", Json::string(look.lines.name())),
                ("scheme", Json::string(look.scheme.name())),
            ]),
        ),
        Change::DefineShape {
            name,
            parts,
            symbol,
        } => {
            let numbers =
                |values: &[f64]| Json::Array(values.iter().copied().map(Json::Number).collect());
            let parts = parts
                .iter()
                .map(|part| {
                    let mut members = vec![
                        ("solid".to_owned(), Json::string(part.solid.name())),
                        ("at".to_owned(), numbers(&part.at)),
                        ("size".to_owned(), numbers(&part.size)),
                    ];
                    if part.axis != Axis::default() {
                        members.push(("axis".to_owned(), Json::string(part.axis.name())));
                    }
                    if let Solid::Prism(points) = &part.solid {
                        members.push((
                            "points".to_owned(),
                            Json::Array(points.iter().map(|point| numbers(point)).collect()),
                        ));
                    }
                    members.push(("material".to_owned(), Json::string(part.material.as_str())));
                    if !part.edges {
                        members.push(("edges".to_owned(), Json::Bool(false)));
                    }
                    Json::Object(members)
                })
                .collect();
            let mut members = vec![
                ("name".to_owned(), Json::string(name.as_str())),
                ("parts".to_owned(), Json::Array(parts)),
            ];
            if !symbol.is_empty() {
                members.push((
                    "symbol".to_owned(),
                    Json::Array(
                        symbol
                            .iter()
                            .map(|&index| Json::Number(index as f64))
                            .collect(),
                    ),
                ));
            }
            ("define_shape", Json::Object(members))
        }
        Change::SetMaterial { name, colours } => (
            "set_material",
            Json::object([
                ("name", Json::string(name.as_str())),
                ("light", Json::string(hex(colours.light))),
                ("dark", Json::string(hex(colours.dark))),
                ("lit", Json::Bool(colours.lit)),
            ]),
        ),
        Change::Clear => ("clear", Json::Object(Vec::new())),
        Change::Load(load) => load_view(load.source(), load.query(), Some(load.hash())),
        Change::LoadView {
            source,
            query,
            sha256,
        } => load_view(source, query, sha256.as_deref()),
    }
}

/// The arguments of `load_view`.
fn load_view(source: &str, query: &str, sha256: Option<&str>) -> (&'static str, Json) {
    let mut members = vec![
        ("source".to_owned(), Json::string(source)),
        ("query".to_owned(), Json::string(query)),
    ];
    if let Some(hash) = sha256 {
        members.push(("sha256".to_owned(), Json::string(hash)));
    }
    ("load_view", Json::Object(members))
}

/// The argument `field`, a name by the name rule.
fn named(arguments: &Json, field: &'static str) -> Result<String, WireRefusal> {
    Ok(object_id(arguments, field)?.as_str().to_owned())
}

/// The argument `field`, a colour `#RRGGBB`.
fn colour(arguments: &Json, field: &'static str) -> Result<[u8; 3], WireRefusal> {
    let text = text(arguments, field)?;
    parse_hex(text).ok_or_else(|| WireRefusal::Value {
        field,
        value: text.to_owned(),
        valid: "#RRGGBB".to_owned(),
    })
}

fn boolean(arguments: &Json, field: &'static str, default: bool) -> Result<bool, WireRefusal> {
    match arguments.get(field) {
        None => Ok(default),
        Some(Json::Bool(value)) => Ok(*value),
        Some(_) => Err(WireRefusal::Type {
            field,
            expected: "boolean",
        }),
    }
}

/// The argument `field`, an array of `N` numbers.
fn numbers<const N: usize>(
    value: Option<&Json>,
    field: &'static str,
    expected: &'static str,
) -> Result<[f64; N], WireRefusal> {
    let refusal = WireRefusal::Type { field, expected };
    let items = value
        .ok_or(WireRefusal::Missing { field })?
        .as_array()
        .filter(|items| items.len() == N)
        .ok_or(refusal.clone())?;
    let mut numbers = [0.0; N];
    for (slot, item) in numbers.iter_mut().zip(items) {
        *slot = item.as_f64().ok_or(refusal.clone())?;
    }
    Ok(numbers)
}

/// A part of the argument `parts` of `define_shape`.
fn part(item: &Json) -> Result<Part, WireRefusal> {
    if item.as_object().is_none() {
        return Err(WireRefusal::Type {
            field: "part",
            expected: "object",
        });
    }
    let solid = text(item, "solid")?;
    let points = item.get("points");
    let solid = match (solid, points) {
        ("box", None) => Solid::Box,
        ("cylinder", None) => Solid::Cylinder,
        ("prism", Some(value)) => {
            let expected = WireRefusal::Type {
                field: "points",
                expected: "array of [u, v] pairs",
            };
            let items = value.as_array().ok_or(expected.clone())?;
            Solid::Prism(
                items
                    .iter()
                    .map(|item| numbers::<2>(Some(item), "points", "array of [u, v] pairs"))
                    .collect::<Result<Vec<_>, _>>()?,
            )
        }
        ("prism", None) => return Err(WireRefusal::Missing { field: "points" }),
        ("box" | "cylinder", Some(_)) => {
            return Err(WireRefusal::Value {
                field: "solid",
                value: solid.to_owned(),
                valid: "prism with points".to_owned(),
            });
        }
        (other, _) => {
            return Err(WireRefusal::Value {
                field: "solid",
                value: other.to_owned(),
                valid: Solid::NAMES.join(", "),
            });
        }
    };
    let axis = match item.get("axis") {
        None => Axis::default(),
        Some(_) => {
            let text = text(item, "axis")?;
            Axis::parse(text).ok_or_else(|| WireRefusal::Value {
                field: "axis",
                value: text.to_owned(),
                valid: Axis::NAMES.join(", "),
            })?
        }
    };
    Ok(Part {
        solid,
        at: numbers(item.get("at"), "at", "array of 3 numbers")?,
        size: numbers(item.get("size"), "size", "array of 3 numbers")?,
        axis,
        material: named(item, "material")?,
        edges: boolean(item, "edges", true)?,
    })
}

fn text<'a>(arguments: &'a Json, field: &'static str) -> Result<&'a str, WireRefusal> {
    arguments
        .get(field)
        .ok_or(WireRefusal::Missing { field })?
        .as_str()
        .ok_or(WireRefusal::Type {
            field,
            expected: "string",
        })
}

/// The argument `label` of `add_node`, a string of 1 to 256 characters. A refusal of the length
/// gives the count of characters.
fn label(arguments: &Json) -> Result<Option<String>, WireRefusal> {
    let [fewest, most] = Node::LABEL;
    match arguments.get("label") {
        None => Ok(None),
        Some(Json::String(text)) if Node::valid_label(text) => Ok(Some(text.clone())),
        Some(Json::String(text)) => Err(WireRefusal::Value {
            field: "label",
            value: format!("{} characters", text.chars().count()),
            valid: format!("{fewest} to {most} characters"),
        }),
        Some(other) => Err(WireRefusal::Value {
            field: "label",
            value: other.to_string(),
            valid: "string".to_owned(),
        }),
    }
}

/// The argument `kind`, a string of 1 or more characters.
fn kind(arguments: &Json) -> Result<&str, WireRefusal> {
    let text = text(arguments, "kind")?;
    if text.is_empty() {
        return Err(WireRefusal::Value {
            field: "kind",
            value: String::new(),
            valid: TEXT_RULE.to_owned(),
        });
    }
    Ok(text)
}

fn object_id(arguments: &Json, field: &'static str) -> Result<ObjectId, WireRefusal> {
    let text = text(arguments, field)?;
    ObjectId::new(text).ok_or_else(|| WireRefusal::Value {
        field,
        value: text.to_owned(),
        valid: NAME_RULE.to_owned(),
    })
}

fn number(arguments: &Json, field: &'static str, default: f64) -> Result<f64, WireRefusal> {
    match arguments.get(field) {
        None => Ok(default),
        Some(value) => value.as_f64().ok_or(WireRefusal::Type {
            field,
            expected: "number",
        }),
    }
}

fn choice<T: Default>(
    arguments: &Json,
    field: &'static str,
    parse: fn(&str) -> Option<T>,
    names: &[&str],
) -> Result<T, WireRefusal> {
    match arguments.get(field) {
        None => Ok(T::default()),
        Some(value) => {
            let text = value.as_str().ok_or(WireRefusal::Type {
                field,
                expected: "string",
            })?;
            parse(text).ok_or_else(|| WireRefusal::Value {
                field,
                value: text.to_owned(),
                valid: names.join(", "),
            })
        }
    }
}

fn fields(arguments: &Json) -> Result<Vec<(String, String)>, WireRefusal> {
    let Some(value) = arguments.get("fields") else {
        return Ok(Vec::new());
    };
    let members = value.as_object().ok_or(WireRefusal::Type {
        field: "fields",
        expected: "object",
    })?;
    if members.len() > FIELDS {
        return Err(WireRefusal::Value {
            field: "fields",
            value: format!("{} members", members.len()),
            valid: format!("{FIELDS} members at most"),
        });
    }
    members
        .iter()
        .map(|(name, value)| {
            if ObjectId::new(name).is_none() {
                return Err(WireRefusal::Value {
                    field: "fields",
                    value: name.clone(),
                    valid: NAME_RULE.to_owned(),
                });
            }
            let text = match value {
                Json::String(text) => {
                    let length = text.chars().count();
                    if !(FIELD_TEXT[0]..=FIELD_TEXT[1]).contains(&length) {
                        return Err(WireRefusal::Length {
                            name: name.clone(),
                            length,
                        });
                    }
                    text.clone()
                }
                Json::Number(_) | Json::Bool(_) => value.to_string(),
                _ => {
                    return Err(WireRefusal::Type {
                        field: "fields",
                        expected: "string, number or boolean values",
                    });
                }
            };
            Ok((name.clone(), text))
        })
        .collect()
}

fn fields_json(fields: &[(String, String)]) -> Json {
    Json::Object(
        fields
            .iter()
            .map(|(name, value)| (name.clone(), Json::string(value.as_str())))
            .collect(),
    )
}

/// The argument `outcome`: an object with a kind and an optional reason. An empty reason is no
/// reason.
fn outcome(arguments: &Json) -> Result<Option<Outcome>, WireRefusal> {
    let value = match arguments.get("outcome") {
        None | Some(Json::Null) => return Ok(None),
        Some(value) => value,
    };
    if value.as_object().is_none() {
        return Err(WireRefusal::Type {
            field: "outcome",
            expected: "object",
        });
    }
    let kind = value
        .get("kind")
        .ok_or(WireRefusal::Missing {
            field: "outcome.kind",
        })?
        .as_str()
        .ok_or(WireRefusal::Type {
            field: "outcome.kind",
            expected: "string",
        })?;
    let kind = OutcomeKind::parse(kind).ok_or_else(|| WireRefusal::Value {
        field: "outcome.kind",
        value: kind.to_owned(),
        valid: OutcomeKind::NAMES.join(", "),
    })?;
    let reason = match value.get("reason") {
        None | Some(Json::Null) => None,
        Some(reason) => {
            let text = reason.as_str().ok_or(WireRefusal::Type {
                field: "outcome.reason",
                expected: "string",
            })?;
            Some(text.to_owned())
        }
    };
    Ok(Some(Outcome { kind, reason }))
}
