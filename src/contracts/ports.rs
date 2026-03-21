use async_trait::async_trait;

use super::{
    errors::ContractError,
    events::DomainEvent,
    ids::{SessionId, TaskId},
    types::{
        ConstraintSet, ExecutionPlan, Intent, LearningDelta, PolicyDecision, ReportArtifact,
        RunReceipt, TaskEnvelope, VerificationVerdict,
    },
};

#[async_trait]
pub trait OperatorControlPlane: Send + Sync {
    async fn approve_intent(&self, intent: &Intent) -> Result<bool, ContractError>;
    async fn veto_session(&self, session_id: &SessionId, reason: &str) -> Result<(), ContractError>;
}

#[async_trait]
pub trait PolicyRuleEngine: Send + Sync {
    async fn evaluate_intent(
        &self,
        intent: &Intent,
        constraints: &ConstraintSet,
    ) -> Result<PolicyDecision, ContractError>;
}

#[async_trait]
pub trait OrchestratorScheduler: Send + Sync {
    async fn build_plan(
        &self,
        intent: &Intent,
        decision: &PolicyDecision,
    ) -> Result<ExecutionPlan, ContractError>;
    async fn schedule_task(&self, plan: &ExecutionPlan) -> Result<Vec<TaskId>, ContractError>;
}

#[async_trait]
pub trait ExecutionPool: Send + Sync {
    async fn execute_task(&self, envelope: &TaskEnvelope) -> Result<RunReceipt, ContractError>;
}

#[async_trait]
pub trait RuntimeKernel: Send + Sync {
    async fn guard_and_execute(
        &self,
        envelope: &TaskEnvelope,
        pool: &dyn ExecutionPool,
    ) -> Result<RunReceipt, ContractError>;
}

#[async_trait]
pub trait VerifierAuditPipeline: Send + Sync {
    async fn verify(&self, receipt: &RunReceipt) -> Result<VerificationVerdict, ContractError>;
    async fn record_event(&self, event: &DomainEvent) -> Result<(), ContractError>;
}

#[async_trait]
pub trait LearningGraphEngine: Send + Sync {
    async fn apply_learning(
        &self,
        receipt: &RunReceipt,
        verdict: &VerificationVerdict,
    ) -> Result<LearningDelta, ContractError>;
}

#[async_trait]
pub trait ReportingObservability: Send + Sync {
    async fn emit_report(
        &self,
        verdict: &VerificationVerdict,
        delta: &LearningDelta,
    ) -> Result<ReportArtifact, ContractError>;
}
