use crate::id::Handle;

/// The data of the host program.
pub trait Host {
    /// The members of the set `set` in the order of the host program. `None` for a handle that is
    /// not a set.
    fn members(&self, set: &Handle) -> Option<Vec<Handle>>;
}
