use std::fmt;

use crate::id::ObjectId;

/// Shape of an object with the objects that it refers to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Primitive {
    /// A sphere.
    Globe,
    /// A box on the globe `on`.
    Building { on: ObjectId },
    /// A curve from the building `from` to the building `to`.
    Arc { from: ObjectId, to: ObjectId },
    /// The text of the object `of`.
    Label { of: ObjectId },
}

impl Primitive {
    pub fn kind(&self) -> Kind {
        match self {
            Primitive::Globe => Kind::Globe,
            Primitive::Building { .. } => Kind::Building,
            Primitive::Arc { .. } => Kind::Arc,
            Primitive::Label { .. } => Kind::Label,
        }
    }

    /// The referenced objects, in the order of `Kind::reference_fields`.
    pub fn targets(&self) -> Vec<&ObjectId> {
        match self {
            Primitive::Globe => Vec::new(),
            Primitive::Building { on } => vec![on],
            Primitive::Arc { from, to } => vec![from, to],
            Primitive::Label { of } => vec![of],
        }
    }
}

/// Kind of a primitive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Globe,
    Building,
    Arc,
    Label,
}

/// A field of a primitive that names another object, with the kinds that it accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReferenceField {
    pub name: &'static str,
    pub accepts: &'static [Kind],
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Globe, Kind::Building, Kind::Arc, Kind::Label];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Globe => "globe",
            Kind::Building => "building",
            Kind::Arc => "arc",
            Kind::Label => "label",
        }
    }

    pub fn reference_fields(self) -> &'static [ReferenceField] {
        match self {
            Kind::Globe => &[],
            Kind::Building => &[ReferenceField {
                name: "on",
                accepts: &[Kind::Globe],
            }],
            Kind::Arc => &[
                ReferenceField {
                    name: "from",
                    accepts: &[Kind::Building],
                },
                ReferenceField {
                    name: "to",
                    accepts: &[Kind::Building],
                },
            ],
            Kind::Label => &[ReferenceField {
                name: "of",
                accepts: &[Kind::Globe, Kind::Building, Kind::Arc],
            }],
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
