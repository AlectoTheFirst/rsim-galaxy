use crate::host::Host;
use crate::id::{Handle, ObjectId};
use crate::primitive::Primitive;
use crate::scene::{Operation, Refusal, Scene};

/// A scene with the log of the operations that give it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Session {
    scene: Scene,
    log: Vec<Operation>,
    inverses: Vec<Operation>,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// The applied operations in order, with no undone operation.
    pub fn log(&self) -> &[Operation] {
        &self.log
    }

    /// Applies `operation` and adds it to the log. A refused operation changes nothing.
    pub fn apply(&mut self, operation: Operation) -> Result<(), Refusal> {
        let inverse = self.scene.apply(&operation)?;
        self.log.push(operation);
        self.inverses.push(inverse);
        Ok(())
    }

    /// Asks `host` for the members of `set`, applies the `AddEach` with them and adds it to the
    /// log.
    pub fn add_each(
        &mut self,
        id: ObjectId,
        primitive: Primitive,
        set: Handle,
        host: &impl Host,
    ) -> Result<(), Refusal> {
        let Some(members) = host.members(&set) else {
            return Err(Refusal::NoSet { set });
        };
        self.apply(Operation::AddEach {
            id,
            primitive,
            set,
            members,
        })
    }

    /// Reverts the last operation of the log, removes it from the log and gives it back.
    pub fn undo(&mut self) -> Option<Operation> {
        let operation = self.log.pop()?;
        let inverse = self.inverses.pop()?;
        self.scene
            .apply(&inverse)
            .expect("the inverse of the last operation applies");
        Some(operation)
    }

    /// The session of a log. The error holds the index of the refused operation.
    pub fn replay(log: &[Operation]) -> Result<Self, (usize, Refusal)> {
        let mut session = Self::new();
        for (index, operation) in log.iter().enumerate() {
            session
                .apply(operation.clone())
                .map_err(|refusal| (index, refusal))?;
        }
        Ok(session)
    }
}
