pub trait Checkpoint {
    /// add a checkpoint to this data at current state
    fn checkpoint(&mut self);

    /// rollback to previous state, panics if no prior checkpoint
    fn rollback(&mut self);

    /// cull checkpoints to depth, that is depth of zero means
    ///  no checkpoints, only current state
    fn purge(&mut self, depth: usize);

    /// returns the number of checkpoints
    fn depth(&self) -> usize;
}
