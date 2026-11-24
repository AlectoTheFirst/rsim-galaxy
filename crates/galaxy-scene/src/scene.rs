use std::collections::BTreeMap;
use std::fmt;

use crate::id::{Handle, ObjectId};
use crate::primitive::{Kind, Primitive};

/// The data of an object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Binding {
    /// No data object.
    None,
    /// 1 data object.
    One(Handle),
    /// The members of the set `set` in the order of the host program. An object with this binding
    /// is a set object: it has 1 instance for each member.
    Each { set: Handle, members: Vec<Handle> },
}

/// An object of a scene: its primitive and its data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Object {
    pub primitive: Primitive,
    pub binding: Binding,
}

/// A change of a scene.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operation {
    Add {
        id: ObjectId,
        primitive: Primitive,
        handle: Option<Handle>,
    },
    /// Adds the set object `id` with 1 instance for each handle of `members`, the members of
    /// `set`.
    AddEach {
        id: ObjectId,
        primitive: Primitive,
        set: Handle,
        members: Vec<Handle>,
    },
    Remove {
        id: ObjectId,
    },
}

/// Cause of a refused operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The name of the new object is in use.
    Exists { id: ObjectId },
    /// The scene has no object with the name.
    NoObject { id: ObjectId },
    /// A reference field names an object that the scene does not have.
    NoTarget {
        field: &'static str,
        target: ObjectId,
    },
    /// A reference field names an object of a kind that the field does not accept.
    WrongKind {
        field: &'static str,
        target: ObjectId,
        kind: Kind,
        accepts: &'static [Kind],
    },
    /// A reference field names a set object.
    SetTarget {
        field: &'static str,
        target: ObjectId,
    },
    /// The 2 ends of an arc are the same object.
    SameEnds { object: ObjectId },
    /// The objects `by` refer to the object to remove.
    InUse { id: ObjectId, by: Vec<ObjectId> },
    /// The host program has no set with the handle.
    NoSet { set: Handle },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Exists { id } => write!(f, "object {id} exists"),
            Refusal::NoObject { id } => write!(f, "no object {id}"),
            Refusal::NoTarget { field, target } => write!(f, "{field}: no object {target}"),
            Refusal::WrongKind {
                field,
                target,
                kind,
                accepts,
            } => {
                let names: Vec<&str> = accepts.iter().map(|kind| kind.name()).collect();
                write!(
                    f,
                    "{field}: {target} is a {kind}, valid kinds: {}",
                    names.join(", ")
                )
            }
            Refusal::SetTarget { field, target } => {
                write!(f, "{field}: {target} is a set object")
            }
            Refusal::SameEnds { object } => {
                write!(f, "from and to are the same object {object}")
            }
            Refusal::InUse { id, by } => {
                let names: Vec<&str> = by.iter().map(ObjectId::as_str).collect();
                write!(f, "{id} is in use by {}", names.join(", "))
            }
            Refusal::NoSet { set } => write!(f, "no set {:?}", set.as_str()),
        }
    }
}

impl std::error::Error for Refusal {}

/// The objects of a scene by name. Each reference of an object names an object of the scene that
/// is not a set object.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scene {
    objects: BTreeMap<ObjectId, Object>,
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: &ObjectId) -> Option<&Object> {
        self.objects.get(id)
    }

    /// The objects in name order.
    pub fn objects(&self) -> impl Iterator<Item = (&ObjectId, &Object)> {
        self.objects.iter()
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// Applies `operation` and gives the operation that reverts it. A refused operation changes
    /// nothing.
    pub fn apply(&mut self, operation: &Operation) -> Result<Operation, Refusal> {
        match operation {
            Operation::Add {
                id,
                primitive,
                handle,
            } => {
                let binding = handle.clone().map_or(Binding::None, Binding::One);
                self.add(id, primitive, binding)
            }
            Operation::AddEach {
                id,
                primitive,
                set,
                members,
            } => {
                let binding = Binding::Each {
                    set: set.clone(),
                    members: members.clone(),
                };
                self.add(id, primitive, binding)
            }
            Operation::Remove { id } => {
                let by = self.users(id);
                if !by.is_empty() {
                    return Err(Refusal::InUse { id: id.clone(), by });
                }
                let Some(object) = self.objects.remove(id) else {
                    return Err(Refusal::NoObject { id: id.clone() });
                };
                let id = id.clone();
                let primitive = object.primitive;
                Ok(match object.binding {
                    Binding::None => Operation::Add {
                        id,
                        primitive,
                        handle: None,
                    },
                    Binding::One(handle) => Operation::Add {
                        id,
                        primitive,
                        handle: Some(handle),
                    },
                    Binding::Each { set, members } => Operation::AddEach {
                        id,
                        primitive,
                        set,
                        members,
                    },
                })
            }
        }
    }

    /// Adds the object `id` and gives its remove.
    fn add(
        &mut self,
        id: &ObjectId,
        primitive: &Primitive,
        binding: Binding,
    ) -> Result<Operation, Refusal> {
        if self.objects.contains_key(id) {
            return Err(Refusal::Exists { id: id.clone() });
        }
        let fields = primitive.kind().reference_fields();
        for (field, target) in fields.iter().zip(primitive.targets()) {
            let Some(object) = self.objects.get(target) else {
                return Err(Refusal::NoTarget {
                    field: field.name,
                    target: target.clone(),
                });
            };
            let kind = object.primitive.kind();
            if !field.accepts.contains(&kind) {
                return Err(Refusal::WrongKind {
                    field: field.name,
                    target: target.clone(),
                    kind,
                    accepts: field.accepts,
                });
            }
            if matches!(object.binding, Binding::Each { .. }) {
                return Err(Refusal::SetTarget {
                    field: field.name,
                    target: target.clone(),
                });
            }
        }
        if let Primitive::Arc { from, to } = primitive
            && from == to
        {
            return Err(Refusal::SameEnds {
                object: from.clone(),
            });
        }
        self.objects.insert(
            id.clone(),
            Object {
                primitive: primitive.clone(),
                binding,
            },
        );
        Ok(Operation::Remove { id: id.clone() })
    }

    /// The objects that refer to `id`, in name order.
    fn users(&self, id: &ObjectId) -> Vec<ObjectId> {
        self.objects
            .iter()
            .filter(|(_, object)| object.primitive.targets().contains(&id))
            .map(|(user, _)| user.clone())
            .collect()
    }

    /// One line for each object in name order: the name, the kind, each reference field, and the
    /// handle or the set with its member count.
    pub fn describe(&self) -> String {
        let mut text = String::new();
        for (id, object) in &self.objects {
            let kind = object.primitive.kind();
            text.push_str(id.as_str());
            text.push(' ');
            text.push_str(kind.name());
            let fields = kind.reference_fields();
            for (field, target) in fields.iter().zip(object.primitive.targets()) {
                text.push_str(&format!(" {}={target}", field.name));
            }
            match &object.binding {
                Binding::None => {}
                Binding::One(handle) => text.push_str(&format!(" handle={:?}", handle.as_str())),
                Binding::Each { set, members } => text.push_str(&format!(
                    " set={:?} members={}",
                    set.as_str(),
                    members.len()
                )),
            }
            text.push('\n');
        }
        text
    }
}
