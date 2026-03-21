#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WorkflowSignal {
    IntentReceived,
    PolicyApproved,
    PolicyRejected,
    PlanCommitted,
    TaskScheduled,
    ExecutionStarted,
    ExecutionFailed,
    RuntimeBlocked,
    VerifyPassed,
    VerifyRejected,
    Closed,
}
