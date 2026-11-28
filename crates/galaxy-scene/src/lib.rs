//! Scene store of the galaxy engine: objects, operations, refusals and the operation log, the
//! relations with the rule set and their world, the style, the wire form of the operations, the
//! reader of the relations format and SHA-256.

pub mod document;
mod host;
mod id;
mod json;
mod library;
mod model;
mod primitive;
mod scene;
mod session;
pub mod sha256;
mod style;
pub mod wire;
mod world;

pub use document::{Answer, Code, Framing, Load, LoadRefusal};
pub use host::Host;
pub use id::{Handle, ObjectId};
pub use json::{Json, JsonError};
pub use library::{Axis, Colours, DefinedShape, Library, Material, Part, Solid};
pub use model::{Change, ChangeRefusal, Model, RuleTarget};
pub use primitive::{Kind, Primitive, ReferenceField};
pub use scene::{Binding, Object, Operation, Refusal, Scene};
pub use session::Session;
pub use style::{Light, Lines, Look, Projection, Scale, Scheme, Space, Style, Widths};
pub use world::{
    Arrangement, Broken, Flow, FlowLine, Laned, Magnitude, Node, Outcome, OutcomeKind, Placed,
    RelationRefusal, Relations, Rule, RuleSet, Shape, Unit, Vec3, World,
};
