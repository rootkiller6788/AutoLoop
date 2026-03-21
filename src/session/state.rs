#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum WorkflowState {
    Intake,
    PolicyReview,
    Planned,
    Scheduled,
    Executing,
    Verifying,
    Closed,
    Blocked,
}
