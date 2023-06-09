pub enum ChangeState {
    Changed,
    Unchanged,
}

pub trait Hashable {
    fn finalize(&mut self);
    fn compile(&mut self) -> ChangeState;
}

impl From<ChangeState> for bool {
    fn from(result: ChangeState) -> Self {
        match result {
            ChangeState::Changed => true,
            ChangeState::Unchanged => false,
        }
    }
}
