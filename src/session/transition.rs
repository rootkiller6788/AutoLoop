use std::{error::Error, fmt};

use super::{signal::WorkflowSignal, state::WorkflowState};

#[derive(Debug, Clone)]
pub struct TransitionError {
    pub from: WorkflowState,
    pub signal: WorkflowSignal,
}

impl fmt::Display for TransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid workflow transition: {:?} --({:?})-> ?",
            self.from, self.signal
        )
    }
}

impl Error for TransitionError {}

pub fn next_state(
    from: WorkflowState,
    signal: WorkflowSignal,
) -> Result<WorkflowState, TransitionError> {
    use WorkflowSignal as Sig;
    use WorkflowState as St;

    let target = match (from, signal) {
        (St::Intake, Sig::IntentReceived) => St::PolicyReview,
        (St::PolicyReview, Sig::PolicyApproved) => St::Planned,
        (St::PolicyReview, Sig::PolicyRejected) => St::PolicyReview,
        (St::Planned, Sig::PlanCommitted) => St::Scheduled,
        (St::Scheduled, Sig::TaskScheduled) => St::Executing,
        (St::Executing, Sig::ExecutionStarted) => St::Executing,
        (St::Executing, Sig::ExecutionFailed) => St::Planned,
        (St::Executing, Sig::RuntimeBlocked) => St::Blocked,
        (St::Executing, Sig::VerifyPassed) => St::Verifying,
        (St::Executing, Sig::VerifyRejected) => St::Verifying,
        (St::Verifying, Sig::VerifyRejected) => St::Planned,
        (St::Verifying, Sig::VerifyPassed) => St::Closed,
        (St::Blocked, Sig::PolicyApproved) => St::Planned,
        (St::Closed, Sig::Closed) => St::Closed,
        (_, Sig::Closed) => St::Closed,
        (St::Executing, Sig::TaskScheduled) => St::Executing,
        (St::Executing, Sig::PlanCommitted) => St::Executing,
        (St::Executing, Sig::PolicyRejected) => St::Planned,
        (St::Executing, Sig::PolicyApproved) => St::Executing,
        (St::Scheduled, Sig::ExecutionStarted) => St::Executing,
        (St::Planned, Sig::TaskScheduled) => St::Scheduled,
        (St::Planned, Sig::VerifyRejected) => St::Planned,
        (St::Planned, Sig::PolicyRejected) => St::PolicyReview,
        (St::Closed, Sig::VerifyRejected) => St::Planned,
        (St::Closed, Sig::VerifyPassed) => St::Closed,
        _ => {
            return Err(TransitionError { from, signal });
        }
    };

    Ok(target)
}
