use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Result, bail};
use autoloop_spacetimedb_adapter::{
    BudgetAccount, CostAttribution, PermissionAction, QuotaWindow, ScheduleEvent, SpacetimeDb,
    SpendLedger, SpendLedgerKind,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio::{process::Command, time::{Duration, timeout}};

use crate::{
    config::{RuntimeConfig, RuntimeGateMode},
    contracts::types::TaskEnvelope,
    hooks::LearningTask,
    observability::event_stream::{
        ArtifactDigest, DeterminismBoundary, ReplayAnalysisReport, ReplayDeviation, ReplaySnapshot,
        SeedRecord, append_event, digest_text, digest_value, get_replay_snapshot,
        persist_replay_analysis, persist_replay_snapshot,
    },
    orchestration::{ExecutionReport, RequirementBrief, RoutingContext},
    providers::{ChatMessage, LlmResponse, ProviderRegistry},
    session::signal::WorkflowSignal,
    tools::{CapabilityRisk, CapabilityStatus, ExecutionStep, ExecutionStepResult, ForgedMcpToolManifest, RenderedCommandSpec, ToolRegistry, TrustStatus, build_command_spec},
};

#[derive(Debug, Clone)]
pub struct ResourceLimits {
    pub max_parallel_agents: usize,
    pub max_memory_mb: u32,
}

#[derive(Debug, Clone)]
pub struct McpExecutionProfile {
    pub enabled: bool,
    pub allow_network_tools: bool,
    pub tool_breaker_failure_threshold: u32,
    pub tool_breaker_cooldown_ms: u64,
    pub mcp_breaker_failure_threshold: u32,
    pub mcp_breaker_cooldown_ms: u64,
}

#[derive(Debug, Clone)]
pub struct RuntimeKernel {
    pub limits: ResourceLimits,
    pub mcp: McpExecutionProfile,
    pub gate_mode: RuntimeGateMode,
    pub gate_enforce_ratio: f32,
    pub rollback_contract_version: Option<String>,
    pub budget_enforced: bool,
    pub default_budget_micros: u64,
    pub quota_window_ms: u64,
    pub quota_window_budget_micros: u64,
    budget_lock: Arc<Mutex<()>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxPolicy {
    pub filesystem_allow: Vec<String>,
    pub filesystem_deny: Vec<String>,
    pub cpu_budget_ms: u64,
    pub memory_budget_mb: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GuardDecision {
    Allow,
    RequiresApproval,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeGuardReport {
    pub decision: GuardDecision,
    pub attempts_allowed: u8,
    pub timeout_secs: u64,
    pub reason: String,
    pub breaker_key: String,
    pub sandbox_policy: Option<SandboxPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeExecuteResult {
    pub content: String,
    pub guard_report: RuntimeGuardReport,
    pub provider_response: Option<LlmResponse>,
    pub estimated_prompt_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DegradeProfileKind {
    Normal,
    ProviderFallback,
    McpConservative,
    ReadOnly,
    QueueThrottle,
    ManualTakeover,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DegradeProfile {
    pub profile_id: String,
    pub kind: DegradeProfileKind,
    pub reason: String,
    pub activated_at_ms: u64,
    pub expires_at_ms: Option<u64>,
    pub max_parallel_agents_override: Option<usize>,
    pub allow_provider_calls: bool,
    pub allow_mcp_calls: bool,
    pub read_only_mode: bool,
    pub requires_manual_takeover: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryPlan {
    pub plan_id: String,
    pub session_id: String,
    pub trace_id: String,
    pub profile: DegradeProfileKind,
    pub trigger: String,
    pub steps: Vec<String>,
    pub cooldown_ms: u64,
    pub auto_recover_enabled: bool,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailoverRecord {
    pub record_id: String,
    pub session_id: String,
    pub trace_id: String,
    pub task_id: String,
    pub capability_id: String,
    pub trigger: String,
    pub profile: DegradeProfileKind,
    pub outcome: String,
    pub recovered: bool,
    pub started_at_ms: u64,
    pub recovered_at_ms: Option<u64>,
    pub mttr_ms: Option<u64>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosCase {
    pub case_id: String,
    pub name: String,
    pub fault: String,
    pub expected_profile: DegradeProfileKind,
    pub target: String,
    pub injected_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayRunRequest {
    pub snapshot_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayRunReport {
    pub snapshot_id: String,
    pub session_id: String,
    pub trace_id: String,
    pub task_id: String,
    pub capability_id: String,
    pub matched: bool,
    pub deterministic_boundary_respected: bool,
    pub original_output_digest: String,
    pub replay_output_digest: String,
    pub route_model_changed: bool,
    pub deviations: Vec<ReplayDeviation>,
    pub notes: Vec<String>,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostBreakdown {
    pub token_cost_micros: u64,
    pub tool_cost_micros: u64,
    pub duration_cost_micros: u64,
    pub total_cost_micros: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetReconciliationReport {
    pub tenant_id: String,
    pub account_id: String,
    pub ledger_settled_micros: u64,
    pub ledger_reserved_open_micros: i64,
    pub account_spent_micros: u64,
    pub account_reserved_micros: u64,
    pub consistent: bool,
}

#[derive(Debug, Clone)]
struct BudgetReservation {
    reservation_id: String,
    account_id: String,
    tenant_id: String,
    principal_id: String,
    policy_id: String,
    reserved_micros: u64,
    started_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxedExecutionResult {
    pub executable: String,
    pub args: Vec<String>,
    pub working_directory: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CircuitState {
    pub scope_key: String,
    pub failure_count: u32,
    pub success_count: u32,
    pub phase: CircuitPhase,
    pub opened_at_ms: Option<u64>,
    pub last_failure_ms: Option<u64>,
    pub last_success_ms: Option<u64>,
    pub cooldown_ms: u64,
    pub threshold: u32,
    pub last_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CircuitPhase {
    #[default]
    Closed,
    Open,
    HalfOpen,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpDispatchRequest {
    pub session_id: String,
    pub tool_name: String,
    pub payload: String,
    pub actor_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationProtocol {
    pub protocol_name: String,
    pub metric_name: String,
    pub time_budget_secs: u64,
    pub mutable_by_agent: bool,
    pub acceptance_checks: Vec<String>,
    pub required_verifiers: Vec<String>,
    pub immutable_artifacts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationResult {
    pub metric_name: String,
    pub score: f32,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationRecord {
    pub actions: Vec<ExecutionStepResult>,
    pub evaluation: EvaluationResult,
    pub keep: bool,
    pub rollback_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerifierVerdict {
    Pass,
    NeedsIteration,
    Reject,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskLevelJudgement {
    pub task_role: String,
    pub satisfied: bool,
    pub score: f32,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteCorrectnessReport {
    pub task_role: String,
    pub tool_name: Option<String>,
    pub route_variant: String,
    pub aligned_with_catalog: bool,
    pub aligned_with_graph: bool,
    pub guard_ok: bool,
    pub score: f32,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityRegressionCase {
    pub tool_name: String,
    pub capability_id: String,
    pub version: u32,
    pub status: String,
    pub approval_status: String,
    pub health_score: f32,
    pub passed: bool,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityRegressionSuite {
    pub suite_name: String,
    pub all_passed: bool,
    pub score: f32,
    pub failing_tools: Vec<String>,
    pub cases: Vec<CapabilityRegressionCase>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifierReport {
    pub verifier_name: String,
    pub verdict: VerifierVerdict,
    pub overall_score: f32,
    pub summary: String,
    pub task_judgements: Vec<TaskLevelJudgement>,
    pub route_reports: Vec<RouteCorrectnessReport>,
    pub capability_regression: CapabilityRegressionSuite,
    pub recommended_actions: Vec<String>,
}

impl RuntimeKernel {
    pub fn from_config(config: &RuntimeConfig) -> Self {
        Self {
            limits: ResourceLimits {
                max_parallel_agents: config.max_parallel_agents,
                max_memory_mb: config.max_memory_mb,
            },
            mcp: McpExecutionProfile {
                enabled: config.mcp_enabled,
                allow_network_tools: config.allow_network_tools,
                tool_breaker_failure_threshold: config.tool_breaker_failure_threshold,
                tool_breaker_cooldown_ms: config.tool_breaker_cooldown_ms,
                mcp_breaker_failure_threshold: config.mcp_breaker_failure_threshold,
                mcp_breaker_cooldown_ms: config.mcp_breaker_cooldown_ms,
            },
            gate_mode: config.gate_mode.clone(),
            gate_enforce_ratio: config.gate_enforce_ratio.clamp(0.0, 1.0),
            rollback_contract_version: config.rollback_contract_version.clone(),
            budget_enforced: config.budget_enforced,
            default_budget_micros: config.default_budget_micros,
            quota_window_ms: config.quota_window_ms,
            quota_window_budget_micros: config.quota_window_budget_micros,
            budget_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.limits.max_parallel_agents == 0 {
            bail!("runtime.max_parallel_agents must be greater than 0");
        }
        if self.limits.max_memory_mb == 0 {
            bail!("runtime.max_memory_mb must be greater than 0");
        }
        if self.mcp.tool_breaker_failure_threshold == 0 {
            bail!("runtime.tool_breaker_failure_threshold must be greater than 0");
        }
        if self.mcp.mcp_breaker_failure_threshold == 0 {
            bail!("runtime.mcp_breaker_failure_threshold must be greater than 0");
        }
        if !(0.0..=1.0).contains(&self.gate_enforce_ratio) {
            bail!("runtime.gate_enforce_ratio must be within [0.0, 1.0]");
        }
        if self.default_budget_micros == 0 {
            bail!("runtime.default_budget_micros must be greater than 0");
        }
        if self.quota_window_ms == 0 {
            bail!("runtime.quota_window_ms must be greater than 0");
        }
        if self.quota_window_budget_micros == 0 {
            bail!("runtime.quota_window_budget_micros must be greater than 0");
        }
        Ok(())
    }

    pub async fn dispatch_mcp_event(
        &self,
        db: &SpacetimeDb,
        request: McpDispatchRequest,
    ) -> Result<ScheduleEvent> {
        db.enforce_permission(&request.actor_id, PermissionAction::Dispatch)
            .await?;

        db.create_schedule_event(
            request.session_id,
            "mcp.dispatch".into(),
            request.tool_name,
            request.payload,
            request.actor_id,
        )
        .await
    }

    pub async fn guard_tool_execution_with_state(
        &self,
        db: &SpacetimeDb,
        actor_id: &str,
        tool_name: &str,
        manifest: Option<&ForgedMcpToolManifest>,
    ) -> Result<RuntimeGuardReport> {
        let mut report = self.guard_tool_execution(actor_id, tool_name, manifest);
        if report.decision != GuardDecision::Allow {
            return Ok(report);
        }

        let now_ms = current_time_ms();
        let tool_key = self.tool_circuit_key(tool_name);
        if let Some(tool_state) = self.load_circuit_state(db, &tool_key).await? {
            if let Some(block_reason) = self.circuit_block_reason(&tool_state, now_ms) {
                report.decision = GuardDecision::Blocked;
                report.attempts_allowed = 0;
                report.reason = format!("tool circuit open: {block_reason}");
                report.breaker_key = tool_key;
                return Ok(report);
            }
            if tool_state.phase == CircuitPhase::HalfOpen {
                report.attempts_allowed = 1;
                report.reason = format!("tool circuit is half-open: {}", report.reason);
                report.breaker_key = tool_key;
            }
        }

        if let Some(server_name) = server_name_for(tool_name, manifest) {
            let server_key = self.server_circuit_key(&server_name);
            if let Some(server_state) = self.load_circuit_state(db, &server_key).await? {
                if let Some(block_reason) = self.circuit_block_reason(&server_state, now_ms) {
                    report.decision = GuardDecision::Blocked;
                    report.attempts_allowed = 0;
                    report.reason = format!("mcp circuit open: {block_reason}");
                    report.breaker_key = server_key;
                    return Ok(report);
                }
                if server_state.phase == CircuitPhase::HalfOpen {
                    report.attempts_allowed = report.attempts_allowed.min(1);
                    report.reason = format!("mcp circuit is half-open: {}", report.reason);
                    report.breaker_key = server_key;
                }
            }
        }

        Ok(report)
    }

    pub fn guard_tool_execution(
        &self,
        actor_id: &str,
        tool_name: &str,
        manifest: Option<&ForgedMcpToolManifest>,
    ) -> RuntimeGuardReport {
        let timeout_secs = manifest
            .and_then(|manifest| manifest.success_signal.as_ref().map(|_| 120))
            .unwrap_or(90);
        let breaker_key = format!("{actor_id}:{tool_name}");

        if let Some(manifest) = manifest {
            if manifest.status != CapabilityStatus::Active {
                return RuntimeGuardReport {
                    decision: GuardDecision::Blocked,
                    attempts_allowed: 0,
                    timeout_secs,
                    reason: format!("capability status {:?} is not runnable", manifest.status),
                    breaker_key,
                    sandbox_policy: Some(self.sandbox_policy_for(tool_name, manifest)),
                };
            }
            if manifest.approval_status != crate::tools::ApprovalStatus::Verified {
                return RuntimeGuardReport {
                    decision: GuardDecision::RequiresApproval,
                    attempts_allowed: 0,
                    timeout_secs,
                    reason: "capability is not verified yet".into(),
                    breaker_key,
                    sandbox_policy: Some(self.sandbox_policy_for(tool_name, manifest)),
                };
            }
            if manifest.trust_status != TrustStatus::Trusted {
                return RuntimeGuardReport {
                    decision: GuardDecision::Blocked,
                    attempts_allowed: 0,
                    timeout_secs,
                    reason: format!(
                        "capability trust status {:?} is not trusted ({})",
                        manifest.trust_status,
                        manifest.trust_findings.join("; ")
                    ),
                    breaker_key,
                    sandbox_policy: Some(self.sandbox_policy_for(tool_name, manifest)),
                };
            }
            if manifest.health_score < 0.4 {
                return RuntimeGuardReport {
                    decision: GuardDecision::Blocked,
                    attempts_allowed: 0,
                    timeout_secs,
                    reason: format!("capability health {:.2} is below runtime minimum", manifest.health_score),
                    breaker_key,
                    sandbox_policy: Some(self.sandbox_policy_for(tool_name, manifest)),
                };
            }
            if manifest.requires_gate() || (self.mcp.allow_network_tools && manifest.risk == CapabilityRisk::High) {
                return RuntimeGuardReport {
                    decision: GuardDecision::RequiresApproval,
                    attempts_allowed: 1,
                    timeout_secs,
                    reason: "capability risk requires approval gate".into(),
                    breaker_key,
                    sandbox_policy: Some(self.sandbox_policy_for(tool_name, manifest)),
                };
            }
        }

        RuntimeGuardReport {
            decision: GuardDecision::Allow,
            attempts_allowed: 2,
            timeout_secs,
            reason: "runtime guard allows bounded execution".into(),
            breaker_key,
            sandbox_policy: manifest.map(|manifest| self.sandbox_policy_for(tool_name, manifest)),
        }
    }

    pub async fn record_execution_outcome(
        &self,
        db: &SpacetimeDb,
        report: &ExecutionReport,
    ) -> Result<Vec<CircuitState>> {
        let Some(tool_name) = report.tool_used.as_deref() else {
            return Ok(Vec::new());
        };
        if !report.guard_decision.eq_ignore_ascii_case("allow") {
            return Ok(Vec::new());
        }

        let now_ms = current_time_ms();
        let succeeded = report.outcome_score > 0
            && !report.output.to_ascii_lowercase().contains("failed")
            && !report.output.to_ascii_lowercase().contains("blocked");

        let mut updates = Vec::new();

        let tool_key = self.tool_circuit_key(tool_name);
        let tool_state = self
            .load_circuit_state(db, &tool_key)
            .await?
            .unwrap_or_else(|| self.default_circuit_state(tool_key.clone(), false));
        let updated_tool_state =
            self.transition_circuit_state(tool_state, succeeded, now_ms, report.output.clone());
        self.persist_circuit_state(db, &updated_tool_state).await?;
        updates.push(updated_tool_state);

        if let Some(server_name) = report
            .mcp_server
            .clone()
            .or_else(|| server_name_for(tool_name, None))
        {
            let server_key = self.server_circuit_key(&server_name);
            let server_state = self
                .load_circuit_state(db, &server_key)
                .await?
                .unwrap_or_else(|| self.default_circuit_state(server_key.clone(), true));
            let updated_server_state = self.transition_circuit_state(
                server_state,
                succeeded,
                now_ms,
                report.output.clone(),
            );
            self.persist_circuit_state(db, &updated_server_state).await?;
            updates.push(updated_server_state);
        }

        Ok(updates)
    }

    pub async fn execute_sandboxed_manifest(
        &self,
        manifest: &ForgedMcpToolManifest,
        arguments: &str,
        policy: &SandboxPolicy,
    ) -> Result<SandboxedExecutionResult> {
        let spec = build_command_spec(manifest, arguments)?;
        validate_command_spec(&spec, policy)?;

        let working_directory = resolve_working_directory(spec.working_directory.as_deref())?;
        enforce_working_directory_policy(&working_directory, policy)?;

        let mut command = Command::new(&spec.executable);
        command
            .args(&spec.args)
            .current_dir(&working_directory)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let child = command.spawn()?;
        let timeout_budget = Duration::from_secs(policy.cpu_budget_ms.max(1000) / 1000);
        match timeout(timeout_budget, child.wait_with_output()).await {
            Ok(output) => {
                let output = output?;
                Ok(SandboxedExecutionResult {
                    executable: spec.executable,
                    args: spec.args,
                    working_directory: working_directory.to_string_lossy().to_string(),
                    exit_code: output.status.code(),
                    stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
                    timed_out: false,
                })
            }
            Err(_) => Ok(SandboxedExecutionResult {
                executable: spec.executable,
                args: spec.args,
                working_directory: working_directory.to_string_lossy().to_string(),
                exit_code: None,
                stdout: String::new(),
                stderr: "sandbox timeout exceeded".into(),
                timed_out: true,
            }),
        }
    }

    pub async fn circuit_snapshot(
        &self,
        db: &SpacetimeDb,
    ) -> Result<HashMap<String, CircuitState>> {
        let mut snapshot = HashMap::new();
        for record in db.list_knowledge_by_prefix("metrics:circuit:").await? {
            if let Ok(state) = serde_json::from_str::<CircuitState>(&record.value) {
                snapshot.insert(record.key, state);
            }
        }
        Ok(snapshot)
    }

    pub async fn reconcile_budget_account(
        &self,
        db: &SpacetimeDb,
        tenant_id: &str,
        account_id: &str,
    ) -> Result<BudgetReconciliationReport> {
        let account = db
            .get_budget_account(tenant_id, account_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("budget account not found"))?;
        let ledger = db.list_spend_ledger(tenant_id, account_id).await?;
        let ledger_settled_micros = ledger
            .iter()
            .filter(|entry| entry.kind == SpendLedgerKind::Settle)
            .map(|entry| entry.amount_micros.max(0) as u64)
            .sum::<u64>();
        let ledger_reserved_open_micros = ledger.iter().fold(0i64, |acc, entry| match entry.kind {
            SpendLedgerKind::Reserve => acc.saturating_add(entry.amount_micros.max(0)),
            SpendLedgerKind::Settle => acc.saturating_sub(entry.amount_micros.max(0)),
            SpendLedgerKind::Refund => acc.saturating_sub(entry.amount_micros.abs()),
            SpendLedgerKind::Blocked => acc,
        });
        let consistent = ledger_settled_micros == account.spent_micros
            && ledger_reserved_open_micros == account.reserved_micros as i64;
        Ok(BudgetReconciliationReport {
            tenant_id: tenant_id.to_string(),
            account_id: account_id.to_string(),
            ledger_settled_micros,
            ledger_reserved_open_micros,
            account_spent_micros: account.spent_micros,
            account_reserved_micros: account.reserved_micros,
            consistent,
        })
    }

    pub fn workflow_signal_from_execution_report(
        &self,
        report: &ExecutionReport,
    ) -> WorkflowSignal {
        if report.guard_decision.eq_ignore_ascii_case("blocked") {
            return WorkflowSignal::RuntimeBlocked;
        }
        if report.outcome_score <= 0
            || report.output.to_ascii_lowercase().contains("failed")
            || report.output.to_ascii_lowercase().contains("blocked")
        {
            return WorkflowSignal::ExecutionFailed;
        }
        WorkflowSignal::ExecutionStarted
    }

    pub async fn execute(
        &self,
        db: &SpacetimeDb,
        tools: &ToolRegistry,
        providers: &ProviderRegistry,
        actor_id: &str,
        envelope: &TaskEnvelope,
        manifest: Option<&ForgedMcpToolManifest>,
        preferred_model: Option<&str>,
    ) -> Result<RuntimeExecuteResult> {
        self.validate_execution_identity(db, envelope).await?;
        let started_at_ms = current_time_ms();
        let capability_id = envelope.capability_id.as_ref();
        let active_degrade = self
            .active_degrade_profile(db, &envelope.session_id.to_string())
            .await?;
        if let Some(profile) = active_degrade.as_ref() {
            if profile.read_only_mode && !capability_id.starts_with("provider:") {
                return Ok(RuntimeExecuteResult {
                    content: format!(
                        "runtime is in read-only degrade profile `{}`; write/mcp execution blocked",
                        profile.profile_id
                    ),
                    guard_report: RuntimeGuardReport {
                        decision: GuardDecision::Blocked,
                        attempts_allowed: 0,
                        timeout_secs: envelope.constraints.timeout_ms / 1000,
                        reason: format!("degrade read-only mode active: {}", profile.reason),
                        breaker_key: format!("degrade:{}", profile.profile_id),
                        sandbox_policy: None,
                    },
                    provider_response: None,
                    estimated_prompt_tokens: None,
                });
            }
            if capability_id.starts_with("provider:") && !profile.allow_provider_calls {
                return Ok(RuntimeExecuteResult {
                    content: format!(
                        "provider path disabled by degrade profile `{}`",
                        profile.profile_id
                    ),
                    guard_report: RuntimeGuardReport {
                        decision: GuardDecision::Blocked,
                        attempts_allowed: 0,
                        timeout_secs: envelope.constraints.timeout_ms / 1000,
                        reason: format!("degrade profile disabled provider calls: {}", profile.reason),
                        breaker_key: format!("degrade:{}", profile.profile_id),
                        sandbox_policy: None,
                    },
                    provider_response: None,
                    estimated_prompt_tokens: None,
                });
            }
            if capability_id.starts_with("mcp::") && !profile.allow_mcp_calls {
                return Ok(RuntimeExecuteResult {
                    content: format!(
                        "mcp path disabled by degrade profile `{}`; waiting for recovery",
                        profile.profile_id
                    ),
                    guard_report: RuntimeGuardReport {
                        decision: GuardDecision::RequiresApproval,
                        attempts_allowed: 0,
                        timeout_secs: envelope.constraints.timeout_ms / 1000,
                        reason: format!("degrade profile disabled mcp calls: {}", profile.reason),
                        breaker_key: format!("degrade:{}", profile.profile_id),
                        sandbox_policy: None,
                    },
                    provider_response: None,
                    estimated_prompt_tokens: None,
                });
            }
        }
        let provider_messages = if capability_id.starts_with("provider:") {
            Some(
                if let Ok(messages) = serde_json::from_value::<Vec<ChatMessage>>(envelope.payload.clone()) {
                    messages
                } else if let Some(text) = envelope.payload.as_str() {
                    vec![ChatMessage {
                        role: "user".into(),
                        content: text.to_string(),
                    }]
                } else {
                    vec![ChatMessage {
                        role: "user".into(),
                        content: serde_json::to_string(&envelope.payload)?,
                    }]
                },
            )
        } else {
            None
        };
        let estimated_tokens_for_precharge = provider_messages
            .as_ref()
            .map(|messages| estimate_tokens(messages))
            .unwrap_or_else(|| {
                let payload = envelope
                    .payload
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| envelope.payload.to_string());
                (payload.len() as u32).saturating_div(4).max(1)
            });
        let reservation = match self
            .precharge_budget(db, envelope, estimated_tokens_for_precharge)
            .await
        {
            Ok(reservation) => reservation,
            Err(error) => {
                let _ = append_event(
                    db,
                    "runtime_blocks",
                    envelope.trace_id.to_string(),
                    envelope.session_id.to_string(),
                    Some(envelope.task_id.to_string()),
                    Some(envelope.capability_id.to_string()),
                    self.effective_contract_version(),
                    serde_json::json!({
                        "reason": "budget_precharge_exceeded",
                        "error": error.to_string(),
                        "tenant_id": &envelope.identity.tenant_id,
                        "principal_id": &envelope.identity.principal_id,
                        "policy_id": &envelope.identity.policy_id,
                    }),
                )
                .await;
                return Err(error);
            }
        };
        if capability_id.starts_with("provider:") {
            let messages = provider_messages.unwrap_or_default();
            let estimated_prompt_tokens = estimate_tokens(&messages);
            if estimated_prompt_tokens > envelope.constraints.max_tokens {
                if let Some(reservation) = reservation.as_ref() {
                    self
                        .rollback_budget(db, envelope, reservation, "provider_token_budget_exceeded")
                        .await?;
                }
                let _ = append_event(
                    db,
                    "runtime_blocks",
                    envelope.trace_id.to_string(),
                    envelope.session_id.to_string(),
                    Some(envelope.task_id.to_string()),
                    Some(envelope.capability_id.to_string()),
                    self.effective_contract_version(),
                    serde_json::json!({
                        "reason": "provider token budget exceeded",
                        "estimated_prompt_tokens": estimated_prompt_tokens,
                        "max_tokens": envelope.constraints.max_tokens,
                        "tenant_id": &envelope.identity.tenant_id,
                        "principal_id": &envelope.identity.principal_id,
                        "policy_id": &envelope.identity.policy_id,
                        "lease_token": &envelope.identity.lease_token,
                    }),
                )
                .await;
                bail!(
                    "provider token budget exceeded: estimated={} max={}",
                    estimated_prompt_tokens,
                    envelope.constraints.max_tokens
                );
            }
            let traced_response = match providers.chat_with_trace(&messages, preferred_model).await {
                Ok(response) => response,
                Err(error) => {
                    let reason = format!("provider unavailable: {error}");
                    let _ = self
                        .apply_degrade_profile(
                            db,
                            &envelope.session_id.to_string(),
                            &envelope.trace_id.to_string(),
                            DegradeProfileKind::ProviderFallback,
                            &reason,
                        )
                        .await;
                    let _ = self
                        .build_recovery_plan(
                            db,
                            &envelope.session_id.to_string(),
                            &envelope.trace_id.to_string(),
                            DegradeProfileKind::ProviderFallback,
                        )
                        .await;
                    if let Some(reservation) = reservation.as_ref() {
                        self
                            .rollback_budget(db, envelope, reservation, "provider_execution_error")
                            .await?;
                    }
                    let fallback_response = fallback_provider_response(&messages, &error.to_string());
                    return Ok(RuntimeExecuteResult {
                        content: fallback_response.content.clone().unwrap_or_else(|| {
                            "provider degraded fallback response".to_string()
                        }),
                        guard_report: RuntimeGuardReport {
                            decision: GuardDecision::Allow,
                            attempts_allowed: 1,
                            timeout_secs: envelope.constraints.timeout_ms / 1000,
                            reason: "provider degraded to fallback mode".into(),
                            breaker_key: format!("degrade:provider:{}", envelope.session_id),
                            sandbox_policy: None,
                        },
                        provider_response: Some(fallback_response),
                        estimated_prompt_tokens: Some(estimated_prompt_tokens),
                    });
                }
            };
            let response = traced_response.response.clone();
            let duration_ms = current_time_ms().saturating_sub(started_at_ms);
            let cost_breakdown = if let Some(reservation) = reservation.as_ref() {
                self.settle_budget(
                    db,
                    envelope,
                    reservation,
                    estimated_prompt_tokens,
                    0,
                    duration_ms,
                )
                .await?
            } else {
                self.cost_breakdown(estimated_prompt_tokens, 0, duration_ms)
            };
            let _ = append_event(
                db,
                "task_runs",
                envelope.trace_id.to_string(),
                envelope.session_id.to_string(),
                Some(envelope.task_id.to_string()),
                Some(envelope.capability_id.to_string()),
                self.effective_contract_version(),
                serde_json::json!({
                    "estimated_prompt_tokens": estimated_prompt_tokens,
                    "preferred_model": preferred_model,
                    "route_model": traced_response.route.model,
                    "route_stage": format!("{:?}", traced_response.route.stage),
                    "route_cache_hit": traced_response.route.cache_hit,
                    "tenant_id": &envelope.identity.tenant_id,
                    "principal_id": &envelope.identity.principal_id,
                    "policy_id": &envelope.identity.policy_id,
                    "lease_token": &envelope.identity.lease_token,
                    "cost_breakdown": cost_breakdown,
                }),
            )
            .await;
            let _ = self
                .record_replay_snapshot(
                    db,
                    actor_id,
                    envelope,
                    preferred_model,
                    Some(&traced_response.route.model),
                    "provider",
                    &envelope.payload,
                    &response.content.clone().unwrap_or_default(),
                    vec![
                        ArtifactDigest {
                            name: "provider_input".into(),
                            algorithm: "siphash64".into(),
                            digest: digest_text(&serde_json::to_string(&messages).unwrap_or_default()),
                        },
                        ArtifactDigest {
                            name: "provider_output".into(),
                            algorithm: "siphash64".into(),
                            digest: digest_text(&response.content.clone().unwrap_or_default()),
                        },
                    ],
                    DeterminismBoundary {
                        mode: "best_effort".into(),
                        locked_fields: vec![
                            "session_id".into(),
                            "trace_id".into(),
                            "task_id".into(),
                            "capability_id".into(),
                            "payload".into(),
                            "constraints".into(),
                            "preferred_model".into(),
                        ],
                        non_deterministic_steps: vec!["provider_response_generation".into()],
                        external_dependencies: vec![providers.default_provider().to_string()],
                    },
                    Some(SeedRecord {
                        seed_key: "provider_route".into(),
                        seed_value: traced_response.route.model.clone(),
                        source: "providers.route_for_messages".into(),
                    }),
                )
                .await;
            return Ok(RuntimeExecuteResult {
                content: response.content.clone().unwrap_or_default(),
                guard_report: RuntimeGuardReport {
                    decision: GuardDecision::Allow,
                    attempts_allowed: 1,
                    timeout_secs: envelope.constraints.timeout_ms / 1000,
                    reason: "provider execution allowed".into(),
                    breaker_key: format!("{actor_id}:{capability_id}"),
                    sandbox_policy: None,
                },
                provider_response: Some(response),
                estimated_prompt_tokens: Some(estimated_prompt_tokens),
            });
        }

        let tool_name = capability_id;
        let mut guard = self
            .guard_tool_execution_with_state(db, actor_id, tool_name, manifest)
            .await?;

        if envelope.constraints.requires_human_approval
            && guard.decision == GuardDecision::Allow
        {
            guard.decision = GuardDecision::RequiresApproval;
            guard.reason = "TaskEnvelope requires human approval".into();
        }

        let original_decision = guard.decision.clone();
        let enforced = self.should_enforce_gate(&envelope.session_id, &envelope.task_id);
        if !enforced && guard.decision != GuardDecision::Allow {
            guard.decision = GuardDecision::Allow;
            guard.reason = format!(
                "shadow-observe-only (original decision {:?})",
                original_decision
            );
        }

        let content_result: Result<String> = match guard.decision {
            GuardDecision::Blocked => {
                Ok(format!("Execution blocked by runtime guard: {}", guard.reason))
            }
            GuardDecision::RequiresApproval => {
                Ok(format!(
                    "Execution requires approval before running {}: {}",
                    tool_name, guard.reason
                ))
            }
            GuardDecision::Allow => {
                let arguments_result: Result<String> = if let Some(text) = envelope.payload.as_str() {
                    Ok(text.to_string())
                } else {
                    serde_json::to_string(&envelope.payload).map_err(anyhow::Error::from)
                };
                match arguments_result {
                    Ok(arguments) => {
                        if let Some(manifest) = manifest {
                            if tools.allow_shell {
                                let mut policy = self.sandbox_policy_for(tool_name, manifest);
                                policy.cpu_budget_ms = envelope.constraints.timeout_ms;
                                policy.memory_budget_mb = envelope.constraints.max_memory_mb;
                                if !envelope.constraints.io_allow_paths.is_empty() {
                                    policy.filesystem_allow = envelope.constraints.io_allow_paths.clone();
                                }
                                if !envelope.constraints.io_deny_paths.is_empty() {
                                    policy.filesystem_deny = envelope.constraints.io_deny_paths.clone();
                                }
                                guard.sandbox_policy = Some(policy.clone());
                                match self
                                    .execute_sandboxed_manifest(manifest, &arguments, &policy)
                                    .await
                                {
                                    Ok(executed) => serde_json::to_string(&executed)
                                        .map_err(anyhow::Error::from),
                                    Err(error) => Err(error),
                                }
                            } else {
                                tools
                                    .execute(tool_name, &arguments)
                                    .await
                                    .map(|result| result.content)
                            }
                        } else {
                            tools
                                .execute(tool_name, &arguments)
                                .await
                                .map(|result| result.content)
                        }
                    }
                    Err(error) => Err(error),
                }
            }
        };
        let content = match content_result {
            Ok(content) => content,
            Err(error) => {
                if capability_id.starts_with("mcp::") {
                    let reason = format!("mcp execution failed: {error}");
                    let _ = self
                        .apply_degrade_profile(
                            db,
                            &envelope.session_id.to_string(),
                            &envelope.trace_id.to_string(),
                            DegradeProfileKind::McpConservative,
                            &reason,
                        )
                        .await;
                    let _ = self
                        .build_recovery_plan(
                            db,
                            &envelope.session_id.to_string(),
                            &envelope.trace_id.to_string(),
                            DegradeProfileKind::McpConservative,
                        )
                        .await;
                    if let Some(reservation) = reservation.as_ref() {
                        self
                            .rollback_budget(db, envelope, reservation, "mcp_execution_error")
                            .await?;
                    }
                    return Ok(RuntimeExecuteResult {
                        content: format!(
                            "mcp degraded mode activated due to failure; execution paused: {}",
                            error
                        ),
                        guard_report: RuntimeGuardReport {
                            decision: GuardDecision::RequiresApproval,
                            attempts_allowed: 0,
                            timeout_secs: envelope.constraints.timeout_ms / 1000,
                            reason: "mcp conservative degrade profile active".into(),
                            breaker_key: format!("degrade:mcp:{}", envelope.session_id),
                            sandbox_policy: None,
                        },
                        provider_response: None,
                        estimated_prompt_tokens: None,
                    });
                }
                if let Some(reservation) = reservation.as_ref() {
                    self
                        .rollback_budget(db, envelope, reservation, "execution_error")
                        .await?;
                }
                return Err(error);
            }
        };
        let duration_ms = current_time_ms().saturating_sub(started_at_ms);
        let cost_breakdown = if let Some(reservation) = reservation.as_ref() {
            match guard.decision {
                GuardDecision::Allow => {
                    self.settle_budget(db, envelope, reservation, 0, 1, duration_ms)
                        .await?
                }
                GuardDecision::Blocked => {
                    self.rollback_budget(db, envelope, reservation, "runtime_guard_blocked")
                        .await?;
                    self.cost_breakdown(0, 0, 0)
                }
                GuardDecision::RequiresApproval => {
                    self.rollback_budget(db, envelope, reservation, "requires_human_approval")
                        .await?;
                    self.cost_breakdown(0, 0, 0)
                }
            }
        } else {
            self.cost_breakdown(0, 0, duration_ms)
        };

        let event_kind = if guard.decision == GuardDecision::Blocked {
            "runtime_blocks"
        } else {
            "task_runs"
        };
        let _ = append_event(
            db,
            event_kind,
            envelope.trace_id.to_string(),
            envelope.session_id.to_string(),
            Some(envelope.task_id.to_string()),
            Some(envelope.capability_id.to_string()),
            self.effective_contract_version(),
            serde_json::json!({
                "decision": format!("{:?}", guard.decision),
                "original_decision": format!("{:?}", original_decision),
                "enforced": enforced,
                "reason": guard.reason.clone(),
                "tenant_id": &envelope.identity.tenant_id,
                "principal_id": &envelope.identity.principal_id,
                "policy_id": &envelope.identity.policy_id,
                "lease_token": &envelope.identity.lease_token,
                "cost_breakdown": cost_breakdown,
            }),
        )
        .await;
        let tool_artifacts = vec![
            ArtifactDigest {
                name: "tool_input".into(),
                algorithm: "siphash64".into(),
                digest: digest_value(&envelope.payload),
            },
            ArtifactDigest {
                name: "tool_output".into(),
                algorithm: "siphash64".into(),
                digest: digest_text(&content),
            },
        ];
        let determinism_boundary = if tools.allow_shell {
            DeterminismBoundary {
                mode: "best_effort".into(),
                locked_fields: vec![
                    "session_id".into(),
                    "trace_id".into(),
                    "task_id".into(),
                    "capability_id".into(),
                    "payload".into(),
                    "constraints".into(),
                ],
                non_deterministic_steps: vec!["sandboxed_shell_execution".into()],
                external_dependencies: vec!["local_shell".into()],
            }
        } else {
            DeterminismBoundary {
                mode: "strict".into(),
                locked_fields: vec![
                    "session_id".into(),
                    "trace_id".into(),
                    "task_id".into(),
                    "capability_id".into(),
                    "payload".into(),
                    "constraints".into(),
                ],
                non_deterministic_steps: Vec::new(),
                external_dependencies: Vec::new(),
            }
        };
        let _ = self
            .record_replay_snapshot(
                db,
                actor_id,
                envelope,
                preferred_model,
                None,
                "tool",
                &envelope.payload,
                &content,
                tool_artifacts,
                determinism_boundary,
                None,
            )
            .await;

        Ok(RuntimeExecuteResult {
            content,
            guard_report: guard,
            provider_response: None,
            estimated_prompt_tokens: None,
        })
    }

    #[allow(dead_code)]
    pub async fn execute_provider(
        &self,
        db: &SpacetimeDb,
        tools: &ToolRegistry,
        providers: &ProviderRegistry,
        envelope: &TaskEnvelope,
        messages: &[ChatMessage],
        preferred_model: Option<&str>,
    ) -> Result<RuntimeExecuteResult> {
        let mut provider_envelope = envelope.clone();
        provider_envelope.payload = serde_json::to_value(messages)?;
        self.execute(
            db,
            tools,
            providers,
            "provider-exec",
            &provider_envelope,
            None,
            preferred_model,
        )
        .await
    }

    pub async fn active_degrade_profile(&self, db: &SpacetimeDb, session_id: &str) -> Result<Option<DegradeProfile>> {
        let now = current_time_ms();
        let profile = db
            .get_knowledge(&format!("runtime:degrade:{session_id}:active"))
            .await?
            .and_then(|record| serde_json::from_str::<DegradeProfile>(&record.value).ok())
            .filter(|profile| profile.expires_at_ms.map(|expires| expires > now).unwrap_or(true));
        Ok(profile)
    }

    pub async fn apply_degrade_profile(
        &self,
        db: &SpacetimeDb,
        session_id: &str,
        trigger: &str,
        profile_kind: DegradeProfileKind,
        reason: &str,
    ) -> Result<FailoverRecord> {
        let now = current_time_ms();
        let profile = self.degrade_profile_from_kind(profile_kind.clone(), reason, now);
        let failover = FailoverRecord {
            record_id: format!("failover:{session_id}:{now}"),
            session_id: session_id.to_string(),
            trace_id: trigger.to_string(),
            task_id: "system".into(),
            capability_id: "runtime:degrade".into(),
            trigger: trigger.to_string(),
            profile: profile_kind.clone(),
            outcome: "degrade_applied".into(),
            recovered: false,
            started_at_ms: now,
            recovered_at_ms: None,
            mttr_ms: None,
            notes: vec![reason.to_string()],
        };
        db.upsert_json_knowledge(
            format!("runtime:degrade:{session_id}:active"),
            &profile,
            "runtime-degrade",
        )
        .await?;
        db.upsert_json_knowledge(
            format!("runtime:failover:{session_id}:{now}"),
            &failover,
            "runtime-failover",
        )
        .await?;
        let _ = append_event(
            db,
            "runtime_failover",
            trigger.to_string(),
            session_id.to_string(),
            None,
            Some("runtime:degrade".into()),
            self.effective_contract_version(),
            serde_json::json!({
                "profile": profile_kind,
                "reason": reason,
                "failover_record_id": failover.record_id,
            }),
        )
        .await;
        Ok(failover)
    }

    pub async fn recover_from_degrade(
        &self,
        db: &SpacetimeDb,
        session_id: &str,
        reason: &str,
    ) -> Result<Option<FailoverRecord>> {
        let active = self.active_degrade_profile(db, session_id).await?;
        if active.is_none() {
            return Ok(None);
        }
        let now = current_time_ms();
        let mut records = db
            .list_knowledge_by_prefix(&format!("runtime:failover:{session_id}:"))
            .await?
            .into_iter()
            .filter_map(|record| serde_json::from_str::<FailoverRecord>(&record.value).ok())
            .collect::<Vec<_>>();
        records.sort_by_key(|record| record.started_at_ms);
        let mut latest = records.into_iter().rev().find(|record| !record.recovered);
        if let Some(ref mut record) = latest {
            record.recovered = true;
            record.recovered_at_ms = Some(now);
            record.mttr_ms = Some(now.saturating_sub(record.started_at_ms));
            record.outcome = "recovered".into();
            record.notes.push(reason.to_string());
            db.upsert_json_knowledge(
                format!("runtime:failover:{}:{}", record.session_id, now),
                record,
                "runtime-failover",
            )
            .await?;
        }
        db.upsert_json_knowledge(
            format!("runtime:degrade:{session_id}:active"),
            &serde_json::json!({
                "status": "cleared",
                "cleared_at_ms": now,
                "reason": reason,
            }),
            "runtime-degrade",
        )
        .await?;
        let _ = append_event(
            db,
            "runtime_recovery",
            format!("recovery:{session_id}:{now}"),
            session_id.to_string(),
            None,
            Some("runtime:degrade".into()),
            self.effective_contract_version(),
            serde_json::json!({
                "reason": reason,
                "recovered": latest.is_some(),
            }),
        )
        .await;
        Ok(latest)
    }

    pub async fn build_recovery_plan(
        &self,
        db: &SpacetimeDb,
        session_id: &str,
        trigger: &str,
        profile: DegradeProfileKind,
    ) -> Result<RecoveryPlan> {
        let now = current_time_ms();
        let steps = match profile {
            DegradeProfileKind::ProviderFallback => vec![
                "switch provider route to fallback or cached responses".into(),
                "keep verifier enabled, but lower exploration".into(),
                "probe provider health before returning to normal".into(),
            ],
            DegradeProfileKind::McpConservative => vec![
                "pause non-critical MCP tasks".into(),
                "allow stable pool only for critical tasks".into(),
                "re-open MCP circuit after cooldown and one probe".into(),
            ],
            DegradeProfileKind::ReadOnly => vec![
                "freeze mutating operations".into(),
                "serve read-only responses from memory/graph".into(),
                "require operator approval before write path recovery".into(),
            ],
            DegradeProfileKind::QueueThrottle => vec![
                "reduce parallelism".into(),
                "drop low-priority tasks".into(),
                "resume adaptive pool when queue drains".into(),
            ],
            DegradeProfileKind::ManualTakeover => vec![
                "handoff to operator control plane".into(),
                "lock risky capabilities".into(),
                "resume only after manual approval".into(),
            ],
            DegradeProfileKind::Normal => vec!["no recovery action required".into()],
        };
        let plan = RecoveryPlan {
            plan_id: format!("recovery-plan:{session_id}:{now}"),
            session_id: session_id.to_string(),
            trace_id: trigger.to_string(),
            profile,
            trigger: trigger.to_string(),
            steps,
            cooldown_ms: self.mcp.tool_breaker_cooldown_ms.max(self.mcp.mcp_breaker_cooldown_ms),
            auto_recover_enabled: true,
            created_at_ms: now,
        };
        db.upsert_json_knowledge(
            format!("runtime:recovery-plan:{session_id}:{now}"),
            &plan,
            "runtime-recovery",
        )
        .await?;
        Ok(plan)
    }

    pub async fn run_chaos_case(&self, db: &SpacetimeDb, session_id: &str, case: ChaosCase) -> Result<FailoverRecord> {
        let trigger = format!("chaos:{}:{}", case.case_id, case.injected_at_ms);
        db.upsert_json_knowledge(
            format!("runtime:chaos:{session_id}:{}", case.case_id),
            &case,
            "runtime-chaos",
        )
        .await?;
        let failover = self
            .apply_degrade_profile(
                db,
                session_id,
                &trigger,
                case.expected_profile.clone(),
                &format!("chaos injected: {} ({})", case.name, case.fault),
            )
            .await?;
        let _ = self
            .build_recovery_plan(db, session_id, &trigger, case.expected_profile)
            .await;
        Ok(failover)
    }

    fn degrade_profile_from_kind(
        &self,
        kind: DegradeProfileKind,
        reason: &str,
        now: u64,
    ) -> DegradeProfile {
        match kind {
            DegradeProfileKind::ProviderFallback => DegradeProfile {
                profile_id: format!("degrade:provider-fallback:{now}"),
                kind,
                reason: reason.to_string(),
                activated_at_ms: now,
                expires_at_ms: Some(now.saturating_add(self.mcp.tool_breaker_cooldown_ms.max(30_000))),
                max_parallel_agents_override: Some(self.limits.max_parallel_agents.min(2)),
                allow_provider_calls: true,
                allow_mcp_calls: true,
                read_only_mode: false,
                requires_manual_takeover: false,
            },
            DegradeProfileKind::McpConservative => DegradeProfile {
                profile_id: format!("degrade:mcp-conservative:{now}"),
                kind,
                reason: reason.to_string(),
                activated_at_ms: now,
                expires_at_ms: Some(now.saturating_add(self.mcp.mcp_breaker_cooldown_ms.max(30_000))),
                max_parallel_agents_override: Some(self.limits.max_parallel_agents.min(2)),
                allow_provider_calls: true,
                allow_mcp_calls: false,
                read_only_mode: false,
                requires_manual_takeover: false,
            },
            DegradeProfileKind::ReadOnly => DegradeProfile {
                profile_id: format!("degrade:read-only:{now}"),
                kind,
                reason: reason.to_string(),
                activated_at_ms: now,
                expires_at_ms: Some(now.saturating_add(300_000)),
                max_parallel_agents_override: Some(1),
                allow_provider_calls: true,
                allow_mcp_calls: false,
                read_only_mode: true,
                requires_manual_takeover: true,
            },
            DegradeProfileKind::QueueThrottle => DegradeProfile {
                profile_id: format!("degrade:queue-throttle:{now}"),
                kind,
                reason: reason.to_string(),
                activated_at_ms: now,
                expires_at_ms: Some(now.saturating_add(120_000)),
                max_parallel_agents_override: Some(self.limits.max_parallel_agents.saturating_div(2).max(1)),
                allow_provider_calls: true,
                allow_mcp_calls: true,
                read_only_mode: false,
                requires_manual_takeover: false,
            },
            DegradeProfileKind::ManualTakeover => DegradeProfile {
                profile_id: format!("degrade:manual:{now}"),
                kind,
                reason: reason.to_string(),
                activated_at_ms: now,
                expires_at_ms: None,
                max_parallel_agents_override: Some(1),
                allow_provider_calls: true,
                allow_mcp_calls: false,
                read_only_mode: true,
                requires_manual_takeover: true,
            },
            DegradeProfileKind::Normal => DegradeProfile {
                profile_id: format!("degrade:normal:{now}"),
                kind,
                reason: reason.to_string(),
                activated_at_ms: now,
                expires_at_ms: Some(now.saturating_add(60_000)),
                max_parallel_agents_override: None,
                allow_provider_calls: true,
                allow_mcp_calls: true,
                read_only_mode: false,
                requires_manual_takeover: false,
            },
        }
    }

    async fn record_replay_snapshot(
        &self,
        db: &SpacetimeDb,
        actor_id: &str,
        envelope: &TaskEnvelope,
        preferred_model: Option<&str>,
        route_model: Option<&str>,
        execution_surface: &str,
        parameters: &serde_json::Value,
        output_content: &str,
        artifacts: Vec<ArtifactDigest>,
        boundary: DeterminismBoundary,
        seed: Option<SeedRecord>,
    ) -> Result<()> {
        let replay_input = serde_json::json!({
            "actor_id": actor_id,
            "preferred_model": preferred_model,
            "execution_surface": execution_surface,
            "envelope": envelope,
        });
        let snapshot = ReplaySnapshot {
            snapshot_id: String::new(),
            session_id: envelope.session_id.to_string(),
            trace_id: envelope.trace_id.to_string(),
            task_id: envelope.task_id.to_string(),
            capability_id: envelope.capability_id.to_string(),
            actor_id: actor_id.to_string(),
            preferred_model: preferred_model.map(str::to_string),
            route_model: route_model.map(str::to_string),
            input_digest: digest_value(&envelope.payload),
            parameters_digest: digest_value(parameters),
            output_digest: digest_text(output_content),
            artifacts,
            boundary,
            seed,
            replay_input,
            created_at_ms: 0,
        };
        let persisted = persist_replay_snapshot(db, snapshot).await?;
        let _ = append_event(
            db,
            "replay_snapshots",
            envelope.trace_id.to_string(),
            envelope.session_id.to_string(),
            Some(envelope.task_id.to_string()),
            Some(envelope.capability_id.to_string()),
            self.effective_contract_version(),
            serde_json::json!({
                "snapshot_id": persisted.snapshot_id,
                "input_digest": persisted.input_digest,
                "output_digest": persisted.output_digest,
                "boundary_mode": persisted.boundary.mode,
            }),
        )
        .await;
        Ok(())
    }

    pub async fn replay_from_snapshot(
        &self,
        db: &SpacetimeDb,
        tools: &ToolRegistry,
        providers: &ProviderRegistry,
        request: &ReplayRunRequest,
    ) -> Result<ReplayRunReport> {
        let snapshot = get_replay_snapshot(db, &request.snapshot_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("replay snapshot not found: {}", request.snapshot_id))?;
        let envelope_value = snapshot
            .replay_input
            .get("envelope")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("replay snapshot missing envelope payload"))?;
        let mut envelope = serde_json::from_value::<TaskEnvelope>(envelope_value)?;
        let replay_suffix = format!("replay:{}", current_time_ms());
        envelope.trace_id = format!("{}:{replay_suffix}", envelope.trace_id).into();
        envelope.task_id = format!("{}:{replay_suffix}", envelope.task_id).into();
        let actor_id = snapshot
            .replay_input
            .get("actor_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("replay-runner")
            .to_string();
        let preferred_model_owned = snapshot
            .replay_input
            .get("preferred_model")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        let preferred_model = preferred_model_owned.as_deref();
        let manifest_owned = tools
            .manifests()
            .into_iter()
            .find(|item| item.registered_tool_name == envelope.capability_id.to_string());
        let manifest = manifest_owned.as_ref();
        let mut route_model_changed = false;
        if envelope.capability_id.as_ref().starts_with("provider:") {
            let messages = if let Ok(messages) =
                serde_json::from_value::<Vec<ChatMessage>>(envelope.payload.clone())
            {
                messages
            } else if let Some(text) = envelope.payload.as_str() {
                vec![ChatMessage {
                    role: "user".into(),
                    content: text.to_string(),
                }]
            } else {
                vec![ChatMessage {
                    role: "user".into(),
                    content: envelope.payload.to_string(),
                }]
            };
            let replay_route = providers.route_for_messages(&messages, preferred_model);
            if let Some(original_model) = &snapshot.route_model {
                route_model_changed = replay_route.model != *original_model;
            }
        }
        let execute_result = self
            .execute(
                db,
                tools,
                providers,
                &actor_id,
                &envelope,
                manifest,
                preferred_model,
            )
            .await?;
        let replay_output_digest = digest_text(&execute_result.content);
        let matched = replay_output_digest == snapshot.output_digest;
        let mut deviations = Vec::new();
        if !matched {
            deviations.push(ReplayDeviation {
                field: "output_digest".into(),
                expected: snapshot.output_digest.clone(),
                actual: replay_output_digest.clone(),
                severity: if snapshot.boundary.mode == "strict" {
                    "high".into()
                } else {
                    "medium".into()
                },
                explanation: "Runtime replay produced a different output hash for the same snapshot input."
                    .into(),
            });
        }
        if route_model_changed {
            deviations.push(ReplayDeviation {
                field: "route_model".into(),
                expected: snapshot.route_model.clone().unwrap_or_default(),
                actual: "changed".into(),
                severity: "medium".into(),
                explanation: "Provider route model differs from the original snapshot route.".into(),
            });
        }
        let deterministic_boundary_respected = if snapshot.boundary.mode == "strict" {
            matched && !route_model_changed
        } else if snapshot.boundary.external_dependencies.is_empty() {
            matched
        } else {
            true
        };
        let mut notes = Vec::new();
        if !snapshot.boundary.external_dependencies.is_empty() {
            notes.push(format!(
                "external dependencies detected: {}",
                snapshot.boundary.external_dependencies.join(", ")
            ));
        }
        if !snapshot.boundary.non_deterministic_steps.is_empty() {
            notes.push(format!(
                "non-deterministic steps: {}",
                snapshot.boundary.non_deterministic_steps.join(", ")
            ));
        }
        let report = ReplayRunReport {
            snapshot_id: snapshot.snapshot_id.clone(),
            session_id: snapshot.session_id.clone(),
            trace_id: snapshot.trace_id.clone(),
            task_id: snapshot.task_id.clone(),
            capability_id: snapshot.capability_id.clone(),
            matched,
            deterministic_boundary_respected,
            original_output_digest: snapshot.output_digest.clone(),
            replay_output_digest: replay_output_digest.clone(),
            route_model_changed,
            deviations: deviations.clone(),
            notes: notes.clone(),
            created_at_ms: current_time_ms(),
        };
        let analysis = ReplayAnalysisReport {
            snapshot_id: report.snapshot_id.clone(),
            session_id: report.session_id.clone(),
            trace_id: report.trace_id.clone(),
            replay_output_digest,
            matched: report.matched,
            deterministic_boundary_respected: report.deterministic_boundary_respected,
            deviations,
            notes,
            created_at_ms: report.created_at_ms,
        };
        let _ = persist_replay_analysis(db, &analysis).await;
        Ok(report)
    }

    fn should_enforce_gate(
        &self,
        session_id: &crate::contracts::ids::SessionId,
        task_id: &crate::contracts::ids::TaskId,
    ) -> bool {
        match self.gate_mode {
            RuntimeGateMode::Shadow => false,
            RuntimeGateMode::Full => true,
            RuntimeGateMode::Canary => {
                let key = format!("{session_id}:{task_id}");
                let mut hash = 1469598103934665603u64;
                for byte in key.as_bytes() {
                    hash ^= *byte as u64;
                    hash = hash.wrapping_mul(1099511628211);
                }
                let bucket = (hash % 10_000) as f32 / 10_000.0;
                bucket < self.gate_enforce_ratio
            }
        }
    }

    fn effective_contract_version(&self) -> &str {
        self.rollback_contract_version
            .as_deref()
            .unwrap_or(crate::contracts::version::CONTRACT_VERSION)
    }

    async fn precharge_budget(
        &self,
        db: &SpacetimeDb,
        envelope: &TaskEnvelope,
        estimated_tokens: u32,
    ) -> Result<Option<BudgetReservation>> {
        if !self.budget_enforced {
            return Ok(None);
        }
        let _guard = self.budget_lock.lock().await;
        let now = current_time_ms();
        let account_id = self.account_id_for(envelope);
        let mut account = db
            .get_budget_account(&envelope.identity.tenant_id, &account_id)
            .await?
            .unwrap_or(BudgetAccount {
                account_id: account_id.clone(),
                tenant_id: envelope.identity.tenant_id.clone(),
                principal_id: envelope.identity.principal_id.clone(),
                policy_id: envelope.identity.policy_id.clone(),
                total_budget_micros: self.default_budget_micros,
                reserved_micros: 0,
                spent_micros: 0,
                blocked_count: 0,
                updated_at_ms: now,
            });

        let estimated = self.estimate_precharge_micros(envelope, estimated_tokens);
        let window_id = self.window_id_for(now);
        let (window_start_ms, window_end_ms) = self.window_bounds(now);
        let mut window = db
            .get_quota_window(&envelope.identity.tenant_id, &account_id, &window_id)
            .await?
            .unwrap_or(QuotaWindow {
                window_id: window_id.clone(),
                tenant_id: envelope.identity.tenant_id.clone(),
                account_id: account_id.clone(),
                window_start_ms,
                window_end_ms,
                window_budget_micros: self.quota_window_budget_micros,
                consumed_micros: 0,
                blocked_count: 0,
                updated_at_ms: now,
            });

        let account_remaining = account
            .total_budget_micros
            .saturating_sub(account.spent_micros.saturating_add(account.reserved_micros));
        let window_remaining = window.window_budget_micros.saturating_sub(window.consumed_micros);
        if estimated > account_remaining || estimated > window_remaining {
            account.blocked_count = account.blocked_count.saturating_add(1);
            account.updated_at_ms = now;
            window.blocked_count = window.blocked_count.saturating_add(1);
            window.updated_at_ms = now;
            db.upsert_budget_account(account).await?;
            db.upsert_quota_window(window).await?;
            let _ = db
                .append_spend_ledger(SpendLedger {
                    ledger_id: format!(
                        "blocked:{}:{}:{}",
                        envelope.trace_id,
                        envelope.task_id,
                        now
                    ),
                    tenant_id: envelope.identity.tenant_id.clone(),
                    account_id: account_id.clone(),
                    session_id: envelope.session_id.to_string(),
                    trace_id: envelope.trace_id.to_string(),
                    task_id: envelope.task_id.to_string(),
                    capability_id: envelope.capability_id.to_string(),
                    kind: SpendLedgerKind::Blocked,
                    amount_micros: estimated as i64,
                    token_cost_micros: estimated_tokens as u64 * 10,
                    tool_cost_micros: if envelope.capability_id.as_ref().starts_with("provider:") {
                        200
                    } else {
                        500
                    },
                    duration_cost_micros: 0,
                    reason: "budget_precharge_exceeded".into(),
                    created_at_ms: now,
                })
                .await;
            bail!("budget precharge exceeded");
        }

        account.reserved_micros = account.reserved_micros.saturating_add(estimated);
        account.updated_at_ms = now;
        window.consumed_micros = window.consumed_micros.saturating_add(estimated);
        window.updated_at_ms = now;
        db.upsert_budget_account(account).await?;
        db.upsert_quota_window(window).await?;
        let reservation_id = format!("reserve:{}:{}:{}", envelope.trace_id, envelope.task_id, now);
        db.append_spend_ledger(SpendLedger {
            ledger_id: reservation_id.clone(),
            tenant_id: envelope.identity.tenant_id.clone(),
            account_id: account_id.clone(),
            session_id: envelope.session_id.to_string(),
            trace_id: envelope.trace_id.to_string(),
            task_id: envelope.task_id.to_string(),
            capability_id: envelope.capability_id.to_string(),
            kind: SpendLedgerKind::Reserve,
            amount_micros: estimated as i64,
            token_cost_micros: estimated_tokens as u64 * 10,
            tool_cost_micros: if envelope.capability_id.as_ref().starts_with("provider:") {
                200
            } else {
                500
            },
            duration_cost_micros: 0,
            reason: "precharge".into(),
            created_at_ms: now,
        })
        .await?;

        Ok(Some(BudgetReservation {
            reservation_id,
            account_id,
            tenant_id: envelope.identity.tenant_id.clone(),
            principal_id: envelope.identity.principal_id.clone(),
            policy_id: envelope.identity.policy_id.clone(),
            reserved_micros: estimated,
            started_at_ms: now,
        }))
    }

    async fn settle_budget(
        &self,
        db: &SpacetimeDb,
        envelope: &TaskEnvelope,
        reservation: &BudgetReservation,
        provider_tokens: u32,
        tool_invocations: u32,
        duration_ms: u64,
    ) -> Result<CostBreakdown> {
        if !self.budget_enforced {
            return Ok(CostBreakdown {
                token_cost_micros: 0,
                tool_cost_micros: 0,
                duration_cost_micros: 0,
                total_cost_micros: 0,
            });
        }
        let _guard = self.budget_lock.lock().await;
        let now = current_time_ms();
        let mut account = db
            .get_budget_account(&reservation.tenant_id, &reservation.account_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("budget account missing during settle"))?;
        let window_id = self.window_id_for(reservation.started_at_ms);
        let mut window = db
            .get_quota_window(&reservation.tenant_id, &reservation.account_id, &window_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("quota window missing during settle"))?;

        let breakdown = self.cost_breakdown(provider_tokens, tool_invocations, duration_ms);
        account.reserved_micros = account
            .reserved_micros
            .saturating_sub(reservation.reserved_micros);
        account.spent_micros = account.spent_micros.saturating_add(breakdown.total_cost_micros);
        account.updated_at_ms = now;
        window.consumed_micros = window
            .consumed_micros
            .saturating_sub(reservation.reserved_micros)
            .saturating_add(breakdown.total_cost_micros);
        window.updated_at_ms = now;
        db.upsert_budget_account(account).await?;
        db.upsert_quota_window(window).await?;

        db.append_spend_ledger(SpendLedger {
            ledger_id: format!("settle:{}:{}:{}", envelope.trace_id, envelope.task_id, now),
            tenant_id: reservation.tenant_id.clone(),
            account_id: reservation.account_id.clone(),
            session_id: envelope.session_id.to_string(),
            trace_id: envelope.trace_id.to_string(),
            task_id: envelope.task_id.to_string(),
            capability_id: envelope.capability_id.to_string(),
            kind: SpendLedgerKind::Settle,
            amount_micros: breakdown.total_cost_micros as i64,
            token_cost_micros: breakdown.token_cost_micros,
            tool_cost_micros: breakdown.tool_cost_micros,
            duration_cost_micros: breakdown.duration_cost_micros,
            reason: format!("settled_from:{}", reservation.reservation_id),
            created_at_ms: now,
        })
        .await?;

        if reservation.reserved_micros > breakdown.total_cost_micros {
            let refund = reservation
                .reserved_micros
                .saturating_sub(breakdown.total_cost_micros);
            db.append_spend_ledger(SpendLedger {
                ledger_id: format!("refund:{}:{}:{}", envelope.trace_id, envelope.task_id, now),
                tenant_id: reservation.tenant_id.clone(),
                account_id: reservation.account_id.clone(),
                session_id: envelope.session_id.to_string(),
                trace_id: envelope.trace_id.to_string(),
                task_id: envelope.task_id.to_string(),
                capability_id: envelope.capability_id.to_string(),
                kind: SpendLedgerKind::Refund,
                amount_micros: -(refund as i64),
                token_cost_micros: 0,
                tool_cost_micros: 0,
                duration_cost_micros: 0,
                reason: format!("refund_from:{}", reservation.reservation_id),
                created_at_ms: now.saturating_add(1),
            })
            .await?;
        }

        db.upsert_cost_attribution(CostAttribution {
            attribution_id: format!("{}:{}:{}", envelope.trace_id, envelope.task_id, now),
            tenant_id: reservation.tenant_id.clone(),
            principal_id: reservation.principal_id.clone(),
            policy_id: reservation.policy_id.clone(),
            session_id: envelope.session_id.to_string(),
            trace_id: envelope.trace_id.to_string(),
            task_id: envelope.task_id.to_string(),
            capability_id: envelope.capability_id.to_string(),
            provider_tokens,
            tool_invocations,
            duration_ms,
            token_cost_micros: breakdown.token_cost_micros,
            tool_cost_micros: breakdown.tool_cost_micros,
            duration_cost_micros: breakdown.duration_cost_micros,
            total_cost_micros: breakdown.total_cost_micros,
            settled_at_ms: now,
        })
        .await?;

        Ok(breakdown)
    }

    async fn rollback_budget(
        &self,
        db: &SpacetimeDb,
        envelope: &TaskEnvelope,
        reservation: &BudgetReservation,
        reason: &str,
    ) -> Result<()> {
        if !self.budget_enforced {
            return Ok(());
        }
        let _guard = self.budget_lock.lock().await;
        let now = current_time_ms();
        let mut account = db
            .get_budget_account(&reservation.tenant_id, &reservation.account_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("budget account missing during rollback"))?;
        let window_id = self.window_id_for(reservation.started_at_ms);
        let mut window = db
            .get_quota_window(&reservation.tenant_id, &reservation.account_id, &window_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("quota window missing during rollback"))?;
        account.reserved_micros = account
            .reserved_micros
            .saturating_sub(reservation.reserved_micros);
        account.updated_at_ms = now;
        window.consumed_micros = window
            .consumed_micros
            .saturating_sub(reservation.reserved_micros);
        window.updated_at_ms = now;
        db.upsert_budget_account(account).await?;
        db.upsert_quota_window(window).await?;
        db.append_spend_ledger(SpendLedger {
            ledger_id: format!("rollback:{}:{}:{}", envelope.trace_id, envelope.task_id, now),
            tenant_id: reservation.tenant_id.clone(),
            account_id: reservation.account_id.clone(),
            session_id: envelope.session_id.to_string(),
            trace_id: envelope.trace_id.to_string(),
            task_id: envelope.task_id.to_string(),
            capability_id: envelope.capability_id.to_string(),
            kind: SpendLedgerKind::Refund,
            amount_micros: -(reservation.reserved_micros as i64),
            token_cost_micros: 0,
            tool_cost_micros: 0,
            duration_cost_micros: 0,
            reason: format!("rollback:{reason}"),
            created_at_ms: now,
        })
        .await?;
        Ok(())
    }

    fn estimate_precharge_micros(&self, envelope: &TaskEnvelope, estimated_tokens: u32) -> u64 {
        let tool_cost = if envelope.capability_id.as_ref().starts_with("provider:") {
            200
        } else {
            500
        };
        let token_cost = estimated_tokens as u64 * 10;
        let duration_cost = envelope.constraints.timeout_ms.min(5_000) * 2;
        token_cost
            .saturating_add(tool_cost)
            .saturating_add(duration_cost)
            .max(1_000)
    }

    fn cost_breakdown(
        &self,
        provider_tokens: u32,
        tool_invocations: u32,
        duration_ms: u64,
    ) -> CostBreakdown {
        let token_cost_micros = provider_tokens as u64 * 10;
        let tool_cost_micros = tool_invocations as u64 * 500;
        let duration_cost_micros = duration_ms.max(1).saturating_mul(2);
        let total_cost_micros = token_cost_micros
            .saturating_add(tool_cost_micros)
            .saturating_add(duration_cost_micros);
        CostBreakdown {
            token_cost_micros,
            tool_cost_micros,
            duration_cost_micros,
            total_cost_micros,
        }
    }

    fn account_id_for(&self, envelope: &TaskEnvelope) -> String {
        format!(
            "{}:{}:{}",
            envelope.identity.tenant_id, envelope.identity.principal_id, envelope.identity.policy_id
        )
    }

    fn window_id_for(&self, timestamp_ms: u64) -> String {
        (timestamp_ms / self.quota_window_ms).to_string()
    }

    fn window_bounds(&self, timestamp_ms: u64) -> (u64, u64) {
        let start = (timestamp_ms / self.quota_window_ms) * self.quota_window_ms;
        let end = start.saturating_add(self.quota_window_ms);
        (start, end)
    }

    async fn validate_execution_identity(
        &self,
        db: &SpacetimeDb,
        envelope: &TaskEnvelope,
    ) -> Result<()> {
        let tenant = db
            .get_tenant(&envelope.identity.tenant_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("tenant not found"))?;
        if tenant.status != "active" {
            bail!("tenant is not active");
        }
        let principal = db
            .get_principal(&envelope.identity.tenant_id, &envelope.identity.principal_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("principal not found"))?;
        if principal.status != "active" {
            bail!("principal is not active");
        }
        let role_binding = db
            .get_role_binding(&envelope.identity.tenant_id, &envelope.identity.principal_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("role binding not found"))?;
        let policy = db
            .get_policy_binding(&envelope.identity.tenant_id, &envelope.identity.policy_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("policy binding not found"))?;
        if role_binding.role != policy.role {
            bail!("role downgraded or mismatched with policy");
        }
        let lease = db
            .get_session_lease(envelope.session_id.as_ref())
            .await?
            .ok_or_else(|| anyhow::anyhow!("session lease not found"))?;
        let now = current_time_ms();
        if lease.expires_at_ms <= now {
            bail!("session lease expired");
        }
        if lease.lease_token != envelope.identity.lease_token
            || lease.tenant_id != envelope.identity.tenant_id
            || lease.principal_id != envelope.identity.principal_id
            || lease.policy_id != envelope.identity.policy_id
        {
            bail!("session lease identity mismatch");
        }
        if !policy
            .capability_prefixes
            .iter()
            .any(|prefix| envelope.capability_id.as_ref().starts_with(prefix))
        {
            bail!("capability not allowed by policy");
        }
        if envelope.constraints.max_memory_mb > policy.max_memory_mb {
            bail!("requested memory exceeds policy max");
        }
        if envelope.constraints.max_tokens > policy.max_tokens {
            bail!("requested tokens exceeds policy max");
        }
        Ok(())
    }

    fn default_circuit_state(&self, scope_key: String, is_mcp: bool) -> CircuitState {
        CircuitState {
            scope_key,
            failure_count: 0,
            success_count: 0,
            phase: CircuitPhase::Closed,
            opened_at_ms: None,
            last_failure_ms: None,
            last_success_ms: None,
            cooldown_ms: if is_mcp {
                self.mcp.mcp_breaker_cooldown_ms
            } else {
                self.mcp.tool_breaker_cooldown_ms
            },
            threshold: if is_mcp {
                self.mcp.mcp_breaker_failure_threshold
            } else {
                self.mcp.tool_breaker_failure_threshold
            },
            last_reason: None,
        }
    }

    fn transition_circuit_state(
        &self,
        mut state: CircuitState,
        succeeded: bool,
        now_ms: u64,
        reason: String,
    ) -> CircuitState {
        if succeeded {
            state.success_count = state.success_count.saturating_add(1);
            state.failure_count = 0;
            state.phase = CircuitPhase::Closed;
            state.last_success_ms = Some(now_ms);
            state.opened_at_ms = None;
            state.last_reason = Some("execution succeeded and circuit closed".into());
            return state;
        }

        state.failure_count = state.failure_count.saturating_add(1);
        state.last_failure_ms = Some(now_ms);
        state.last_reason = Some(reason);
        if state.failure_count >= state.threshold {
            state.phase = CircuitPhase::Open;
            state.opened_at_ms = Some(now_ms);
        } else if state.phase == CircuitPhase::HalfOpen {
            state.phase = CircuitPhase::Open;
            state.opened_at_ms = Some(now_ms);
        }
        state
    }

    fn circuit_block_reason(&self, state: &CircuitState, now_ms: u64) -> Option<String> {
        match state.phase {
            CircuitPhase::Closed => None,
            CircuitPhase::HalfOpen => None,
            CircuitPhase::Open => {
                let opened_at = state.opened_at_ms.unwrap_or(now_ms);
                if now_ms.saturating_sub(opened_at) >= state.cooldown_ms {
                    None
                } else {
                    Some(format!(
                        "cooldown active for {} ms (failures: {})",
                        state.cooldown_ms.saturating_sub(now_ms.saturating_sub(opened_at)),
                        state.failure_count
                    ))
                }
            }
        }
    }

    async fn load_circuit_state(
        &self,
        db: &SpacetimeDb,
        key: &str,
    ) -> Result<Option<CircuitState>> {
        let state = db
            .get_knowledge(key)
            .await?
            .and_then(|record| serde_json::from_str::<CircuitState>(&record.value).ok())
            .map(|mut state| {
                if state.phase == CircuitPhase::Open {
                    if let Some(opened_at) = state.opened_at_ms {
                        if current_time_ms().saturating_sub(opened_at) >= state.cooldown_ms {
                            state.phase = CircuitPhase::HalfOpen;
                        }
                    }
                }
                state
            });
        Ok(state)
    }

    async fn persist_circuit_state(
        &self,
        db: &SpacetimeDb,
        state: &CircuitState,
    ) -> Result<()> {
        db.upsert_knowledge(
            state.scope_key.clone(),
            serde_json::to_string(state)?,
            "runtime-circuit".into(),
        )
        .await?;
        Ok(())
    }

    fn tool_circuit_key(&self, tool_name: &str) -> String {
        format!("metrics:circuit:tool:{tool_name}")
    }

    fn server_circuit_key(&self, server_name: &str) -> String {
        format!("metrics:circuit:server:{server_name}")
    }

    fn sandbox_policy_for(
        &self,
        tool_name: &str,
        manifest: &ForgedMcpToolManifest,
    ) -> SandboxPolicy {
        let mut filesystem_allow = vec![".".into(), "./workspace".into()];
        if manifest.scope == crate::tools::CapabilityScope::Session {
            filesystem_allow.push("./workspace/session".into());
        }
        if let Some(working_directory) = &manifest.working_directory {
            filesystem_allow.push(working_directory.clone());
        }
        let mut filesystem_deny = vec!["./.git".into(), "./deploy/secrets".into()];
        if manifest.risk == CapabilityRisk::High || tool_name.contains("deploy") {
            filesystem_deny.push("/".into());
        }
        SandboxPolicy {
            filesystem_allow,
            filesystem_deny,
            cpu_budget_ms: if manifest.risk == CapabilityRisk::High { 15_000 } else { 6_000 },
            memory_budget_mb: if manifest.risk == CapabilityRisk::High {
                self.limits.max_memory_mb.min(768)
            } else {
                self.limits.max_memory_mb.min(384)
            },
        }
    }

    pub fn evaluation_protocol(&self) -> EvaluationProtocol {
        EvaluationProtocol {
            protocol_name: "immutable-objective-protocol".into(),
            metric_name: "objective_score".into(),
            time_budget_secs: 300,
            mutable_by_agent: false,
            acceptance_checks: vec![
                "acceptance-criteria-coverage".into(),
                "task-level-judge".into(),
                "route-correctness".into(),
                "capability-regression".into(),
            ],
            required_verifiers: vec![
                "verifier-agent".into(),
                "task-judge".into(),
                "route-auditor".into(),
                "capability-regression-suite".into(),
            ],
            immutable_artifacts: vec![
                "acceptance_criteria".into(),
                "routing_catalog".into(),
                "guard_decision".into(),
            ],
        }
    }

    pub fn evaluate_candidate(
        &self,
        baseline_score: f32,
        candidate_score: f32,
    ) -> EvaluationResult {
        EvaluationResult {
            metric_name: self.evaluation_protocol().metric_name,
            score: candidate_score,
            summary: format!(
                "Immutable protocol compared candidate {:.6} against baseline {:.6}. Lower is better.",
                candidate_score, baseline_score
            ),
        }
    }

    pub async fn run_iteration_loop(
        &self,
        tools: &ToolRegistry,
        actions: &[ExecutionStep],
        baseline_score: f32,
        candidate_score: f32,
    ) -> Result<IterationRecord> {
        let action_results = tools.execute_plan(actions).await?;
        let evaluation = self.evaluate_candidate(baseline_score, candidate_score);
        let keep = evaluation.score < baseline_score;
        let rollback_reason = (!keep).then(|| {
            format!(
                "candidate {:.6} did not improve over baseline {:.6}",
                evaluation.score, baseline_score
            )
        });

        Ok(IterationRecord {
            actions: action_results,
            evaluation,
            keep,
            rollback_reason,
        })
    }

    pub fn learn_from_iteration_failure(&self, record: &IterationRecord) -> Option<LearningTask> {
        if record.keep {
            return None;
        }

        Some(LearningTask {
            hook_name: "optimization-reflexion".into(),
            anchor: "iteration-regression".into(),
            reason: record
                .rollback_reason
                .clone()
                .unwrap_or_else(|| "iteration failed immutable objective".into()),
            priority: "high".into(),
        })
    }

    pub fn verify_swarm_outcome(
        &self,
        brief: &RequirementBrief,
        routing: &RoutingContext,
        reports: &[ExecutionReport],
        tools: &ToolRegistry,
    ) -> VerifierReport {
        let task_judgements = self.judge_tasks(brief, reports);
        let route_reports = self.judge_routes(routing, reports, tools);
        let capability_regression = self.run_capability_regression_suite(tools);

        let task_score = average_score(task_judgements.iter().map(|item| item.score));
        let route_score = average_score(route_reports.iter().map(|item| item.score));
        let acceptance_coverage = acceptance_coverage(brief, reports);
        let overall_score = ((task_score + route_score + acceptance_coverage + capability_regression.score)
            / 4.0)
            .clamp(0.0, 1.0);

        let mut recommended_actions = Vec::new();
        if acceptance_coverage < 0.8 {
            recommended_actions.push("verifier: expand task evidence for frozen acceptance criteria".into());
        }
        if route_score < 0.65 {
            recommended_actions.push("verifier: audit route selection against catalog and graph signals".into());
        }
        if !capability_regression.all_passed {
            recommended_actions.push("verifier: deprecate or roll back failing capabilities".into());
        }
        if routing.pending_event_count > 0 {
            recommended_actions.push("verifier: drain pending scheduled events before completion".into());
        }

        let verdict = if !capability_regression.all_passed {
            VerifierVerdict::Reject
        } else if routing.pending_event_count > 0
            || acceptance_coverage < 0.75
            || task_score < 0.6
            || route_score < 0.6
        {
            VerifierVerdict::NeedsIteration
        } else {
            VerifierVerdict::Pass
        };

        VerifierReport {
            verifier_name: "verifier-agent".into(),
            verdict: verdict.clone(),
            overall_score,
            summary: format!(
                "Verifier {:?}: acceptance {:.2}, task {:.2}, route {:.2}, capability {:.2}.",
                verdict, acceptance_coverage, task_score, route_score, capability_regression.score
            ),
            task_judgements,
            route_reports,
            capability_regression,
            recommended_actions,
        }
    }

    fn judge_tasks(
        &self,
        brief: &RequirementBrief,
        reports: &[ExecutionReport],
    ) -> Vec<TaskLevelJudgement> {
        reports
            .iter()
            .map(|report| {
                let output = report.output.to_ascii_lowercase();
                let positive_signal = report.outcome_score > 0
                    && !output.contains("blocked")
                    && !output.contains("requires approval");
                let criterion_hits = brief
                    .acceptance_criteria
                    .iter()
                    .filter(|criterion| output.contains(&criterion.to_ascii_lowercase()))
                    .count();
                let criteria_score = if brief.acceptance_criteria.is_empty() {
                    1.0
                } else {
                    criterion_hits as f32 / brief.acceptance_criteria.len() as f32
                };
                let score = ((if positive_signal { 0.6 } else { 0.1 }) + criteria_score * 0.4)
                    .clamp(0.0, 1.0);
                TaskLevelJudgement {
                    task_role: report.task.role.clone(),
                    satisfied: positive_signal && score >= 0.55,
                    score,
                    summary: format!(
                        "{} satisfied={} criterion_hits={}/{} outcome_score={}",
                        report.task.agent_name,
                        positive_signal && score >= 0.55,
                        criterion_hits,
                        brief.acceptance_criteria.len(),
                        report.outcome_score
                    ),
                }
            })
            .collect()
    }

    fn judge_routes(
        &self,
        routing: &RoutingContext,
        reports: &[ExecutionReport],
        tools: &ToolRegistry,
    ) -> Vec<RouteCorrectnessReport> {
        reports
            .iter()
            .map(|report| {
                let aligned_with_catalog = report.tool_used.as_ref().is_none_or(|tool_name| {
                    if report.task.role != "Execution" {
                        return true;
                    }
                    tools.forged_tool_names().iter().any(|name| name == tool_name)
                        || tool_name == "cli::forge_mcp_tool"
                });
                let aligned_with_graph = match report.tool_used.as_deref() {
                    Some(tool_name) if tool_name == "cli::forge_mcp_tool" => {
                        routing.forged_tool_coverage == 0 || routing.graph_signals.prefers_cli_execution
                    }
                    Some(tool_name) if tool_name.starts_with("mcp::") => {
                        routing.graph_signals.prefers_mcp_execution || routing.forged_tool_coverage > 0
                    }
                    Some(_) => routing.graph_signals.prefers_cli_execution,
                    None => true,
                };
                let guard_ok =
                    report.guard_decision.eq_ignore_ascii_case("allow")
                        || report.guard_decision.eq_ignore_ascii_case("provider");
                let mut score = 0.2f32;
                if aligned_with_catalog {
                    score += 0.35;
                }
                if aligned_with_graph {
                    score += 0.25;
                }
                if guard_ok {
                    score += 0.2;
                }
                RouteCorrectnessReport {
                    task_role: report.task.role.clone(),
                    tool_name: report.tool_used.clone(),
                    route_variant: report.route_variant.clone(),
                    aligned_with_catalog,
                    aligned_with_graph,
                    guard_ok,
                    score: score.clamp(0.0, 1.0),
                    summary: format!(
                        "catalog={} graph={} guard={} variant={}",
                        aligned_with_catalog, aligned_with_graph, guard_ok, report.route_variant
                    ),
                }
            })
            .collect()
    }

    pub fn run_capability_regression_suite(&self, tools: &ToolRegistry) -> CapabilityRegressionSuite {
        let cases = tools
            .manifests()
            .into_iter()
            .map(|manifest| {
                let passed = manifest.status == CapabilityStatus::Active
                    && manifest.approval_status == crate::tools::ApprovalStatus::Verified
                    && manifest.health_score >= 0.55
                    && !(manifest.risk == CapabilityRisk::High && manifest.health_score < 0.7);
                let summary = if passed {
                    "capability satisfies executable governance baseline".to_string()
                } else {
                    format!(
                        "status={:?} approval={:?} health={:.2} risk={:?}",
                        manifest.status, manifest.approval_status, manifest.health_score, manifest.risk
                    )
                };
                CapabilityRegressionCase {
                    tool_name: manifest.registered_tool_name,
                    capability_id: manifest.capability_id,
                    version: manifest.version,
                    status: format!("{:?}", manifest.status),
                    approval_status: format!("{:?}", manifest.approval_status),
                    health_score: manifest.health_score,
                    passed,
                    summary,
                }
            })
            .collect::<Vec<_>>();
        let failing_tools = cases
            .iter()
            .filter(|case| !case.passed)
            .map(|case| case.tool_name.clone())
            .collect::<Vec<_>>();
        let passed_count = cases.iter().filter(|case| case.passed).count();
        let score = if cases.is_empty() {
            1.0
        } else {
            passed_count as f32 / cases.len() as f32
        };

        CapabilityRegressionSuite {
            suite_name: "capability-regression-suite".into(),
            all_passed: failing_tools.is_empty(),
            score,
            failing_tools,
            summary: if cases.is_empty() {
                "No forged capabilities were registered, so the regression suite is vacuously green.".into()
            } else {
                format!(
                    "{} of {} capabilities satisfy the verifier baseline.",
                    passed_count,
                    cases.len()
                )
            },
            cases,
        }
    }
}

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn resolve_working_directory(working_directory: Option<&str>) -> Result<PathBuf> {
    let requested = working_directory.unwrap_or(".");
    let path = Path::new(requested);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    Ok(absolute)
}

fn validate_command_spec(
    spec: &RenderedCommandSpec,
    policy: &SandboxPolicy,
) -> Result<()> {
    let executable = spec.executable.to_ascii_lowercase();
    let blocked_executables = ["powershell", "pwsh", "cmd", "bash", "sh"];
    if blocked_executables
        .iter()
        .any(|blocked| executable.ends_with(blocked) || executable.contains(&format!("{blocked}.")))
    {
        bail!("sandbox blocked interpreter-style executable: {}", spec.executable);
    }

    let denied_prefixes = policy
        .filesystem_deny
        .iter()
        .map(|entry| entry.to_ascii_lowercase())
        .collect::<Vec<_>>();
    for argument in &spec.args {
        let lowered = argument.to_ascii_lowercase();
        if denied_prefixes.iter().any(|prefix| lowered.contains(prefix)) {
            bail!("sandbox blocked denied path-like argument: {argument}");
        }
    }
    Ok(())
}

fn enforce_working_directory_policy(path: &Path, policy: &SandboxPolicy) -> Result<()> {
    let normalized = path.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
    let allowed = policy.filesystem_allow.iter().any(|entry| {
        let candidate = entry.replace('\\', "/").to_ascii_lowercase();
        let candidate = candidate.trim_start_matches("./");
        candidate.is_empty()
            || normalized.contains(candidate)
            || normalized.ends_with(candidate)
    });
    if !allowed {
        bail!("sandbox blocked working directory outside allowlist: {}", path.display());
    }
    let denied = policy.filesystem_deny.iter().any(|entry| {
        let candidate = entry.replace('\\', "/").to_ascii_lowercase();
        normalized.contains(candidate.trim_start_matches("./"))
    });
    if denied {
        bail!("sandbox blocked working directory inside denylist: {}", path.display());
    }
    Ok(())
}

fn server_name_for(
    tool_name: &str,
    manifest: Option<&ForgedMcpToolManifest>,
) -> Option<String> {
    manifest
        .map(|manifest| manifest.server.clone())
        .or_else(|| {
            let mut segments = tool_name.split("::");
            match (segments.next(), segments.next()) {
                (Some("mcp"), Some(server)) => Some(server.to_string()),
                _ => None,
            }
        })
}

fn average_score(values: impl Iterator<Item = f32>) -> f32 {
    let values = values.collect::<Vec<_>>();
    if values.is_empty() {
        1.0
    } else {
        values.iter().sum::<f32>() / values.len() as f32
    }
}

fn estimate_tokens(messages: &[ChatMessage]) -> u32 {
    let total_chars = messages
        .iter()
        .map(|message| message.role.len() + message.content.len())
        .sum::<usize>();
    // Simple hard gate heuristic for OpenAI-compatible tokenization.
    ((total_chars as f32) / 4.0).ceil() as u32
}

fn fallback_provider_response(messages: &[ChatMessage], error: &str) -> LlmResponse {
    let user_text = messages
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map(|message| message.content.clone())
        .unwrap_or_else(|| "request".into());
    LlmResponse {
        content: Some(format!(
            "degraded-provider-fallback: temporarily unavailable upstream provider; partial response kept for availability. request=\"{}\" error=\"{}\"",
            user_text,
            error
        )),
        tool_calls: Vec::new(),
    }
}

fn acceptance_coverage(brief: &RequirementBrief, reports: &[ExecutionReport]) -> f32 {
    if brief.acceptance_criteria.is_empty() {
        return 1.0;
    }
    let combined = reports
        .iter()
        .map(|report| report.output.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    let hits = brief
        .acceptance_criteria
        .iter()
        .filter(|criterion| combined.contains(&criterion.to_ascii_lowercase()))
        .count();
    hits as f32 / brief.acceptance_criteria.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use autoloop_spacetimedb_adapter::{
        PolicyBinding, Principal, RoleBinding, SessionLease, SpacetimeBackend, SpacetimeDbConfig,
        Tenant,
    };

    use crate::{
        config::AppConfig,
        contracts::{
            ids::{CapabilityId, SessionId, TaskId, TraceId},
            types::{ConstraintSet, ExecutionIdentity, TaskEnvelope},
        },
        orchestration::{ExecutionReport, RequirementBrief, RoutingContext, SwarmTask},
        providers::{ChatMessage, ProviderRegistry},
        tools::{ApprovalStatus, CapabilityRisk, CapabilityScope, CapabilityStatus, CliOutputMode, ForgedMcpToolManifest, TrustStatus},
    };
    use serde_json::json;

    fn manifest(risk: CapabilityRisk, approval_status: ApprovalStatus, status: CapabilityStatus) -> ForgedMcpToolManifest {
        ForgedMcpToolManifest {
            capability_id: "capability:test".into(),
            registered_tool_name: "mcp::local-mcp::test".into(),
            delegate_tool_name: "mcp::local-mcp::invoke".into(),
            server: "local-mcp".into(),
            capability_name: "test".into(),
            purpose: "test purpose".into(),
            executable: "test-cli".into(),
            command_template: "test-cli run".into(),
            payload_template: json!({}),
            output_mode: CliOutputMode::Json,
            working_directory: Some(".".into()),
            success_signal: Some("completed".into()),
            help_text: "help".into(),
            skill_markdown: "# skill".into(),
            examples: vec![],
            version: 1,
            lineage_key: "capability:test".into(),
            status,
            approval_status,
            health_score: 0.7,
            scope: CapabilityScope::TaskFamily,
            tags: vec![],
            risk,
            requested_by: "cli-agent".into(),
            created_at_ms: 0,
            updated_at_ms: 0,
            approved_at_ms: None,
            rollback_to_version: None,
            ..ForgedMcpToolManifest::default()
        }
    }

    async fn provision_identity(
        db: &SpacetimeDb,
        session_id: &str,
        tenant_id: &str,
        principal_id: &str,
        policy_id: &str,
        capability_prefixes: Vec<String>,
    ) -> ExecutionIdentity {
        let now = current_time_ms();
        db.upsert_tenant(Tenant {
            tenant_id: tenant_id.into(),
            name: tenant_id.into(),
            status: "active".into(),
            created_at_ms: now,
        })
        .await
        .expect("tenant");
        db.upsert_principal(Principal {
            principal_id: principal_id.into(),
            tenant_id: tenant_id.into(),
            principal_type: "user".into(),
            status: "active".into(),
            created_at_ms: now,
        })
        .await
        .expect("principal");
        db.upsert_role_binding(RoleBinding {
            tenant_id: tenant_id.into(),
            principal_id: principal_id.into(),
            role: "operator".into(),
            updated_at_ms: now,
        })
        .await
        .expect("role");
        db.upsert_policy_binding(PolicyBinding {
            policy_id: policy_id.into(),
            tenant_id: tenant_id.into(),
            role: "operator".into(),
            allowed_actions: vec![],
            capability_prefixes,
            max_memory_mb: 1024,
            max_tokens: 4096,
            updated_at_ms: now,
        })
        .await
        .expect("policy");
        let lease_token = format!("lease:{session_id}");
        db.upsert_session_lease(SessionLease {
            lease_token: lease_token.clone(),
            session_id: session_id.into(),
            tenant_id: tenant_id.into(),
            principal_id: principal_id.into(),
            policy_id: policy_id.into(),
            expires_at_ms: now.saturating_add(60_000),
            issued_at_ms: now,
        })
        .await
        .expect("lease");
        ExecutionIdentity {
            tenant_id: tenant_id.into(),
            principal_id: principal_id.into(),
            policy_id: policy_id.into(),
            lease_token,
        }
    }

    #[test]
    fn runtime_guard_requires_approval_for_high_risk_capability() {
        let runtime = RuntimeKernel::from_config(&AppConfig::default().runtime);
        let report = runtime.guard_tool_execution(
            "session-1",
            "mcp::local-mcp::test",
            Some(&manifest(
                CapabilityRisk::High,
                ApprovalStatus::Verified,
                CapabilityStatus::Active,
            )),
        );
        assert_eq!(report.decision, GuardDecision::RequiresApproval);
    }

    #[test]
    fn runtime_guard_blocks_unverified_capability() {
        let runtime = RuntimeKernel::from_config(&AppConfig::default().runtime);
        let report = runtime.guard_tool_execution(
            "session-1",
            "mcp::local-mcp::test",
            Some(&manifest(
                CapabilityRisk::Low,
                ApprovalStatus::Pending,
                CapabilityStatus::PendingVerification,
            )),
        );
        assert_eq!(report.decision, GuardDecision::Blocked);
    }

    #[test]
    fn runtime_guard_blocks_untrusted_capability() {
        let runtime = RuntimeKernel::from_config(&AppConfig::default().runtime);
        let mut forged = manifest(
            CapabilityRisk::Low,
            ApprovalStatus::Verified,
            CapabilityStatus::Active,
        );
        forged.trust_status = TrustStatus::Rejected;
        forged.trust_findings = vec!["signature verification failed".into()];
        let report =
            runtime.guard_tool_execution("session-1", "mcp::local-mcp::test", Some(&forged));
        assert_eq!(report.decision, GuardDecision::Blocked);
        assert!(report.reason.contains("not trusted"));
    }

    #[test]
    fn verifier_rejects_capability_regression_failures() {
        let config = AppConfig::default();
        let runtime = RuntimeKernel::from_config(&config.runtime);
        let tools = ToolRegistry::from_config(&config.tools);
        tools.hydrate_manifest(manifest(
            CapabilityRisk::High,
            ApprovalStatus::Pending,
            CapabilityStatus::PendingVerification,
        ));
        let brief = RequirementBrief {
            anchor_id: "anchor:session-1".into(),
            original_request: "execute via catalog".into(),
            clarified_goal: "execute via catalog".into(),
            frozen_scope: "capability-catalog execution".into(),
            open_questions: vec![],
            acceptance_criteria: vec!["mcp".into()],
            clarification_turns: vec![],
            confirmation_required: false,
        };
        let routing = RoutingContext {
            history_records: vec![],
            execution_metrics: vec![],
            graph_signals: Default::default(),
            pending_event_count: 0,
            agent_reputations: HashMap::new(),
            learning_evidence: vec![],
            skill_success_rate: 0.0,
            causal_confidence: 0.0,
            forged_tool_coverage: 0,
            session_ab_stats: None,
            task_ab_stats: Default::default(),
            tool_ab_stats: Default::default(),
            server_ab_stats: Default::default(),
            route_biases: vec![],
        };
        let reports = vec![ExecutionReport {
            task: SwarmTask {
                task_id: "execution-catalog-requires-approval".into(),
                agent_name: "execution-agent".into(),
                role: "Execution".into(),
                objective: "execute via catalog".into(),
                depends_on: Vec::new(),
            },
            output: "execution requires approval".into(),
            tool_used: Some("mcp::local-mcp::test".into()),
            mcp_server: Some("local-mcp".into()),
            invocation_payload: Some("{}".into()),
            outcome_score: -6,
            route_variant: "control".into(),
            control_score: 1,
            treatment_score: 1,
            guard_decision: "RequiresApproval".into(),
        }];

        let verifier = runtime.verify_swarm_outcome(&brief, &routing, &reports, &tools);

        assert_eq!(verifier.verdict, VerifierVerdict::Reject);
        assert!(!verifier.capability_regression.all_passed);
        assert!(!verifier.capability_regression.failing_tools.is_empty());
    }

    #[test]
    fn verifier_passes_healthy_verified_catalog_execution() {
        let config = AppConfig::default();
        let runtime = RuntimeKernel::from_config(&config.runtime);
        let tools = ToolRegistry::from_config(&config.tools);
        tools.hydrate_manifest(manifest(
            CapabilityRisk::Low,
            ApprovalStatus::Verified,
            CapabilityStatus::Active,
        ));
        let brief = RequirementBrief {
            anchor_id: "anchor:session-1".into(),
            original_request: "execute via catalog".into(),
            clarified_goal: "execute via catalog".into(),
            frozen_scope: "capability-catalog execution".into(),
            open_questions: vec![],
            acceptance_criteria: vec!["mcp".into(), "completed".into()],
            clarification_turns: vec![],
            confirmation_required: false,
        };
        let mut graph_signals = crate::rag::GraphRoutingSignals::default();
        graph_signals.prefers_mcp_execution = true;
        let routing = RoutingContext {
            history_records: vec![],
            execution_metrics: vec![],
            graph_signals,
            pending_event_count: 0,
            agent_reputations: HashMap::new(),
            learning_evidence: vec![],
            skill_success_rate: 0.0,
            causal_confidence: 0.0,
            forged_tool_coverage: 1,
            session_ab_stats: None,
            task_ab_stats: Default::default(),
            tool_ab_stats: Default::default(),
            server_ab_stats: Default::default(),
            route_biases: vec![],
        };
        let reports = vec![ExecutionReport {
            task: SwarmTask {
                task_id: "execution-catalog-healthy".into(),
                agent_name: "execution-agent".into(),
                role: "Execution".into(),
                objective: "execute via catalog".into(),
                depends_on: Vec::new(),
            },
            output: "mcp execution completed successfully".into(),
            tool_used: Some("mcp::local-mcp::test".into()),
            mcp_server: Some("local-mcp".into()),
            invocation_payload: Some("{}".into()),
            outcome_score: 4,
            route_variant: "control".into(),
            control_score: 4,
            treatment_score: 4,
            guard_decision: "Allow".into(),
        }];

        let verifier = runtime.verify_swarm_outcome(&brief, &routing, &reports, &tools);

        assert_eq!(verifier.verdict, VerifierVerdict::Pass);
        assert!(verifier.capability_regression.all_passed);
        assert!(verifier.overall_score > 0.7);
    }

    #[tokio::test]
    async fn circuit_state_opens_after_threshold_failures() {
        let mut config = AppConfig::default();
        config.runtime.tool_breaker_failure_threshold = 2;
        config.runtime.tool_breaker_cooldown_ms = 60_000;
        let runtime = RuntimeKernel::from_config(&config.runtime);
        let db = SpacetimeDb::from_config(&SpacetimeDbConfig {
            enabled: true,
            backend: SpacetimeBackend::InMemory,
            uri: "http://spacetimedb:3000".into(),
            module_name: "autoloop_core".into(),
            namespace: "autoloop".into(),
            pool_size: 4,
        });
        let executable_manifest = manifest(
            CapabilityRisk::Low,
            ApprovalStatus::Verified,
            CapabilityStatus::Active,
        );
        let failure = ExecutionReport {
            task: SwarmTask {
                task_id: "breaker-failure".into(),
                agent_name: "execution-agent".into(),
                role: "Execution".into(),
                objective: "execute failing tool".into(),
                depends_on: Vec::new(),
            },
            output: "failed with an error".into(),
            tool_used: Some("mcp::local-mcp::test".into()),
            mcp_server: Some("local-mcp".into()),
            invocation_payload: Some("{}".into()),
            outcome_score: -5,
            route_variant: "control".into(),
            control_score: -5,
            treatment_score: -5,
            guard_decision: "Allow".into(),
        };

        runtime
            .record_execution_outcome(&db, &failure)
            .await
            .expect("first failure");
        runtime
            .record_execution_outcome(&db, &failure)
            .await
            .expect("second failure");

        let guard = runtime
            .guard_tool_execution_with_state(
                &db,
                "session-1",
                "mcp::local-mcp::test",
                Some(&executable_manifest),
            )
            .await
            .expect("guard");

        assert_eq!(guard.decision, GuardDecision::Blocked);
        assert!(guard.reason.contains("circuit open"));
    }

    #[tokio::test]
    async fn circuit_state_recovers_into_half_open_after_cooldown() {
        let mut config = AppConfig::default();
        config.runtime.tool_breaker_failure_threshold = 1;
        config.runtime.tool_breaker_cooldown_ms = 1;
        let runtime = RuntimeKernel::from_config(&config.runtime);
        let db = SpacetimeDb::from_config(&SpacetimeDbConfig {
            enabled: true,
            backend: SpacetimeBackend::InMemory,
            uri: "http://spacetimedb:3000".into(),
            module_name: "autoloop_core".into(),
            namespace: "autoloop".into(),
            pool_size: 4,
        });
        let executable_manifest = manifest(
            CapabilityRisk::Low,
            ApprovalStatus::Verified,
            CapabilityStatus::Active,
        );
        let failure = ExecutionReport {
            task: SwarmTask {
                task_id: "breaker-half-open".into(),
                agent_name: "execution-agent".into(),
                role: "Execution".into(),
                objective: "execute failing tool".into(),
                depends_on: Vec::new(),
            },
            output: "failed with an error".into(),
            tool_used: Some("mcp::local-mcp::test".into()),
            mcp_server: Some("local-mcp".into()),
            invocation_payload: Some("{}".into()),
            outcome_score: -5,
            route_variant: "control".into(),
            control_score: -5,
            treatment_score: -5,
            guard_decision: "Allow".into(),
        };

        runtime
            .record_execution_outcome(&db, &failure)
            .await
            .expect("failure");
        std::thread::sleep(std::time::Duration::from_millis(2));

        let guard = runtime
            .guard_tool_execution_with_state(
                &db,
                "session-2",
                "mcp::local-mcp::test",
                Some(&executable_manifest),
            )
            .await
            .expect("guard");

        assert_eq!(guard.decision, GuardDecision::Allow);
        assert_eq!(guard.attempts_allowed, 1);
        assert!(guard.reason.contains("half-open"));
    }

    #[tokio::test]
    async fn sandbox_executor_runs_real_command_for_verified_manifest() {
        let runtime = RuntimeKernel::from_config(&AppConfig::default().runtime);
        let mut executable_manifest = manifest(
            CapabilityRisk::Low,
            ApprovalStatus::Verified,
            CapabilityStatus::Active,
        );
        executable_manifest.executable = "rustc".into();
        executable_manifest.command_template = "rustc --version".into();
        executable_manifest.working_directory = Some(".".into());
        let policy = runtime.sandbox_policy_for("mcp::local-mcp::test", &executable_manifest);

        let result = runtime
            .execute_sandboxed_manifest(&executable_manifest, "{}", &policy)
            .await
            .expect("sandbox execution");

        assert!(!result.timed_out);
        assert_eq!(result.exit_code, Some(0));
        assert!(result.stdout.to_ascii_lowercase().contains("rustc"));
    }

    #[tokio::test]
    async fn sandbox_executor_blocks_interpreter_style_commands() {
        let runtime = RuntimeKernel::from_config(&AppConfig::default().runtime);
        let mut executable_manifest = manifest(
            CapabilityRisk::Low,
            ApprovalStatus::Verified,
            CapabilityStatus::Active,
        );
        executable_manifest.executable = "powershell".into();
        executable_manifest.command_template = "powershell -Command Get-Date".into();
        executable_manifest.working_directory = Some(".".into());
        let policy = runtime.sandbox_policy_for("mcp::local-mcp::test", &executable_manifest);

        let error = runtime
            .execute_sandboxed_manifest(&executable_manifest, "{}", &policy)
            .await
            .expect_err("interpreter should be blocked");

        assert!(error.to_string().contains("interpreter-style executable"));
    }

    #[tokio::test]
    async fn execute_provider_blocks_when_token_budget_exceeded() {
        let runtime = RuntimeKernel::from_config(&AppConfig::default().runtime);
        let providers = ProviderRegistry::from_config(&AppConfig::default().providers);
        let tools = ToolRegistry::from_config(&AppConfig::default().tools);
        let db = SpacetimeDb::from_config(&SpacetimeDbConfig {
            enabled: true,
            backend: SpacetimeBackend::InMemory,
            uri: "http://spacetimedb:3000".into(),
            module_name: "autoloop_core".into(),
            namespace: "autoloop".into(),
            pool_size: 4,
        });
        let messages = vec![
            ChatMessage {
                role: "system".into(),
                content: "system".into(),
            },
            ChatMessage {
                role: "user".into(),
                content: "this payload should exceed a tiny token budget".into(),
            },
        ];
        db.upsert_tenant(Tenant {
            tenant_id: "tenant:test".into(),
            name: "tenant:test".into(),
            status: "active".into(),
            created_at_ms: 1,
        })
        .await
        .expect("tenant");
        db.upsert_principal(Principal {
            principal_id: "principal:test".into(),
            tenant_id: "tenant:test".into(),
            principal_type: "user".into(),
            status: "active".into(),
            created_at_ms: 1,
        })
        .await
        .expect("principal");
        db.upsert_role_binding(RoleBinding {
            tenant_id: "tenant:test".into(),
            principal_id: "principal:test".into(),
            role: "operator".into(),
            updated_at_ms: 1,
        })
        .await
        .expect("role");
        db.upsert_policy_binding(PolicyBinding {
            policy_id: "policy:test".into(),
            tenant_id: "tenant:test".into(),
            role: "operator".into(),
            allowed_actions: vec![],
            capability_prefixes: vec!["provider:".into()],
            max_memory_mb: 1024,
            max_tokens: 4096,
            updated_at_ms: 1,
        })
        .await
        .expect("policy");
        db.upsert_session_lease(SessionLease {
            lease_token: "lease:test".into(),
            session_id: "session-provider-budget".into(),
            tenant_id: "tenant:test".into(),
            principal_id: "principal:test".into(),
            policy_id: "policy:test".into(),
            expires_at_ms: current_time_ms().saturating_add(60_000),
            issued_at_ms: 1,
        })
        .await
        .expect("lease");
        let envelope = TaskEnvelope {
            session_id: SessionId::from("session-provider-budget"),
            trace_id: TraceId::from("trace-provider-budget"),
            task_id: TaskId::from("provider-budget"),
            capability_id: CapabilityId::from("provider:default"),
            identity: ExecutionIdentity {
                tenant_id: "tenant:test".into(),
                principal_id: "principal:test".into(),
                policy_id: "policy:test".into(),
                lease_token: "lease:test".into(),
            },
            payload: serde_json::json!({}),
            constraints: ConstraintSet {
                max_cpu_percent: 80,
                max_memory_mb: 512,
                timeout_ms: 60_000,
                max_retries: 1,
                max_tokens: 2,
                io_allow_paths: vec![".".into()],
                io_deny_paths: vec![],
                sandbox_profile: "provider".into(),
                requires_human_approval: false,
            },
        };

        let error = runtime
            .execute(
                &db,
                &tools,
                &providers,
                "session-provider-budget",
                &TaskEnvelope {
                    payload: serde_json::to_value(&messages).expect("messages payload"),
                    ..envelope.clone()
                },
                None,
                None,
            )
            .await
            .expect_err("token budget should block provider execution");
        assert!(error
            .to_string()
            .contains("provider token budget exceeded"));
    }

    #[tokio::test]
    async fn p10_replay_same_input_matches_for_deterministic_tool_path() {
        let runtime = RuntimeKernel::from_config(&AppConfig::default().runtime);
        let providers = ProviderRegistry::from_config(&AppConfig::default().providers);
        let tools = ToolRegistry::from_config(&AppConfig::default().tools);
        let db = SpacetimeDb::from_config(&SpacetimeDbConfig {
            enabled: true,
            backend: SpacetimeBackend::InMemory,
            uri: "http://spacetimedb:3000".into(),
            module_name: "autoloop_core".into(),
            namespace: "autoloop".into(),
            pool_size: 4,
        });
        let now = current_time_ms();
        db.upsert_tenant(Tenant {
            tenant_id: "tenant:p10".into(),
            name: "tenant:p10".into(),
            status: "active".into(),
            created_at_ms: now,
        })
        .await
        .expect("tenant");
        db.upsert_principal(Principal {
            principal_id: "principal:p10".into(),
            tenant_id: "tenant:p10".into(),
            principal_type: "user".into(),
            status: "active".into(),
            created_at_ms: now,
        })
        .await
        .expect("principal");
        db.upsert_role_binding(RoleBinding {
            tenant_id: "tenant:p10".into(),
            principal_id: "principal:p10".into(),
            role: "operator".into(),
            updated_at_ms: now,
        })
        .await
        .expect("role");
        db.upsert_policy_binding(PolicyBinding {
            policy_id: "policy:p10".into(),
            tenant_id: "tenant:p10".into(),
            role: "operator".into(),
            allowed_actions: vec![],
            capability_prefixes: vec!["read_file".into()],
            max_memory_mb: 1024,
            max_tokens: 4096,
            updated_at_ms: now,
        })
        .await
        .expect("policy");
        db.upsert_session_lease(SessionLease {
            lease_token: "lease:p10".into(),
            session_id: "session-p10-replay".into(),
            tenant_id: "tenant:p10".into(),
            principal_id: "principal:p10".into(),
            policy_id: "policy:p10".into(),
            expires_at_ms: now.saturating_add(60_000),
            issued_at_ms: now,
        })
        .await
        .expect("lease");
        let envelope = TaskEnvelope {
            session_id: SessionId::from("session-p10-replay"),
            trace_id: TraceId::from("trace-p10-replay"),
            task_id: TaskId::from("task-p10-replay"),
            capability_id: CapabilityId::from("read_file"),
            identity: ExecutionIdentity {
                tenant_id: "tenant:p10".into(),
                principal_id: "principal:p10".into(),
                policy_id: "policy:p10".into(),
                lease_token: "lease:p10".into(),
            },
            payload: serde_json::json!({"path":"README.md"}),
            constraints: ConstraintSet {
                max_cpu_percent: 80,
                max_memory_mb: 512,
                timeout_ms: 30_000,
                max_retries: 1,
                max_tokens: 256,
                io_allow_paths: vec![".".into()],
                io_deny_paths: vec![],
                sandbox_profile: "deterministic".into(),
                requires_human_approval: false,
            },
        };

        runtime
            .execute(
                &db,
                &tools,
                &providers,
                "runtime:p10",
                &envelope,
                None,
                None,
            )
            .await
            .expect("first execution");

        let snapshots = crate::observability::event_stream::list_replay_snapshots(
            &db,
            "session-p10-replay",
        )
        .await
        .expect("snapshots");
        let snapshot = snapshots.last().expect("at least one snapshot");
        let report = runtime
            .replay_from_snapshot(
                &db,
                &tools,
                &providers,
                &ReplayRunRequest {
                    snapshot_id: snapshot.snapshot_id.clone(),
                },
            )
            .await
            .expect("replay");

        assert!(report.matched);
        assert!(report.deterministic_boundary_respected);
        assert!(report.deviations.is_empty());
    }

    #[tokio::test]
    async fn p10_external_dependency_drift_is_explained_in_replay_report() {
        let runtime = RuntimeKernel::from_config(&AppConfig::default().runtime);
        let providers = ProviderRegistry::from_config(&AppConfig::default().providers);
        let tools = ToolRegistry::from_config(&AppConfig::default().tools);
        let db = SpacetimeDb::from_config(&SpacetimeDbConfig {
            enabled: true,
            backend: SpacetimeBackend::InMemory,
            uri: "http://spacetimedb:3000".into(),
            module_name: "autoloop_core".into(),
            namespace: "autoloop".into(),
            pool_size: 4,
        });
        let now = current_time_ms();
        db.upsert_tenant(Tenant {
            tenant_id: "tenant:p10d".into(),
            name: "tenant:p10d".into(),
            status: "active".into(),
            created_at_ms: now,
        })
        .await
        .expect("tenant");
        db.upsert_principal(Principal {
            principal_id: "principal:p10d".into(),
            tenant_id: "tenant:p10d".into(),
            principal_type: "user".into(),
            status: "active".into(),
            created_at_ms: now,
        })
        .await
        .expect("principal");
        db.upsert_role_binding(RoleBinding {
            tenant_id: "tenant:p10d".into(),
            principal_id: "principal:p10d".into(),
            role: "operator".into(),
            updated_at_ms: now,
        })
        .await
        .expect("role");
        db.upsert_policy_binding(PolicyBinding {
            policy_id: "policy:p10d".into(),
            tenant_id: "tenant:p10d".into(),
            role: "operator".into(),
            allowed_actions: vec![],
            capability_prefixes: vec!["provider:".into()],
            max_memory_mb: 1024,
            max_tokens: 4096,
            updated_at_ms: now,
        })
        .await
        .expect("policy");
        db.upsert_session_lease(SessionLease {
            lease_token: "lease:p10d".into(),
            session_id: "session-p10-drift".into(),
            tenant_id: "tenant:p10d".into(),
            principal_id: "principal:p10d".into(),
            policy_id: "policy:p10d".into(),
            expires_at_ms: now.saturating_add(60_000),
            issued_at_ms: now,
        })
        .await
        .expect("lease");
        let envelope = TaskEnvelope {
            session_id: SessionId::from("session-p10-drift"),
            trace_id: TraceId::from("trace-p10-drift"),
            task_id: TaskId::from("task-p10-drift"),
            capability_id: CapabilityId::from("provider:default"),
            identity: ExecutionIdentity {
                tenant_id: "tenant:p10d".into(),
                principal_id: "principal:p10d".into(),
                policy_id: "policy:p10d".into(),
                lease_token: "lease:p10d".into(),
            },
            payload: serde_json::to_value(vec![ChatMessage {
                role: "user".into(),
                content: "hello drift".into(),
            }])
            .expect("messages"),
            constraints: ConstraintSet {
                max_cpu_percent: 80,
                max_memory_mb: 512,
                timeout_ms: 30_000,
                max_retries: 1,
                max_tokens: 512,
                io_allow_paths: vec![".".into()],
                io_deny_paths: vec![],
                sandbox_profile: "provider".into(),
                requires_human_approval: false,
            },
        };

        runtime
            .execute(
                &db,
                &tools,
                &providers,
                "runtime:p10d",
                &envelope,
                None,
                None,
            )
            .await
            .expect("provider execution");
        let snapshots = crate::observability::event_stream::list_replay_snapshots(
            &db,
            "session-p10-drift",
        )
        .await
        .expect("snapshots");
        let mut snapshot = snapshots.last().expect("snapshot").clone();
        snapshot.output_digest = "forced-drift".into();
        crate::observability::event_stream::persist_replay_snapshot(&db, snapshot.clone())
            .await
            .expect("overwrite snapshot");

        let report = runtime
            .replay_from_snapshot(
                &db,
                &tools,
                &providers,
                &ReplayRunRequest {
                    snapshot_id: snapshot.snapshot_id.clone(),
                },
            )
            .await
            .expect("replay");

        assert!(!report.matched);
        assert!(!report.deviations.is_empty());
        assert!(report
            .notes
            .iter()
            .any(|note| note.contains("external dependencies")));
    }

    #[tokio::test]
    async fn p11_provider_outage_switches_to_degrade_fallback() {
        let runtime = RuntimeKernel::from_config(&AppConfig::default().runtime);
        let mut provider_config = AppConfig::default();
        provider_config.providers.builtin.clear();
        provider_config.providers.mcp_servers.clear();
        let providers = ProviderRegistry::from_config(&provider_config.providers);
        let tools = ToolRegistry::from_config(&AppConfig::default().tools);
        let db = SpacetimeDb::from_config(&SpacetimeDbConfig {
            enabled: true,
            backend: SpacetimeBackend::InMemory,
            uri: "http://spacetimedb:3000".into(),
            module_name: "autoloop_core".into(),
            namespace: "autoloop".into(),
            pool_size: 4,
        });
        let identity = provision_identity(
            &db,
            "session-p11-provider",
            "tenant:p11",
            "principal:p11",
            "policy:p11",
            vec!["provider:".into()],
        )
        .await;
        let envelope = TaskEnvelope {
            session_id: SessionId::from("session-p11-provider"),
            trace_id: TraceId::from("trace-p11-provider"),
            task_id: TaskId::from("task-p11-provider"),
            capability_id: CapabilityId::from("provider:default"),
            identity,
            payload: serde_json::to_value(vec![ChatMessage {
                role: "user".into(),
                content: "provider outage scenario".into(),
            }])
            .expect("payload"),
            constraints: ConstraintSet {
                max_cpu_percent: 80,
                max_memory_mb: 512,
                timeout_ms: 30_000,
                max_retries: 1,
                max_tokens: 1024,
                io_allow_paths: vec![".".into()],
                io_deny_paths: vec![],
                sandbox_profile: "provider".into(),
                requires_human_approval: false,
            },
        };
        let result = runtime
            .execute(
                &db,
                &tools,
                &providers,
                "runtime:p11-provider",
                &envelope,
                None,
                None,
            )
            .await
            .expect("fallback result");
        assert!(result
            .content
            .contains("degraded-provider-fallback"));
        let active = runtime
            .active_degrade_profile(&db, "session-p11-provider")
            .await
            .expect("active profile")
            .expect("degrade active");
        assert_eq!(active.kind, DegradeProfileKind::ProviderFallback);
    }

    #[tokio::test]
    async fn p11_mcp_failure_switches_to_conservative_degrade() {
        let runtime = RuntimeKernel::from_config(&AppConfig::default().runtime);
        let mut cfg = AppConfig::default();
        cfg.tools.allow_shell = true;
        let tools = ToolRegistry::from_config(&cfg.tools);
        let providers = ProviderRegistry::from_config(&cfg.providers);
        let db = SpacetimeDb::from_config(&SpacetimeDbConfig {
            enabled: true,
            backend: SpacetimeBackend::InMemory,
            uri: "http://spacetimedb:3000".into(),
            module_name: "autoloop_core".into(),
            namespace: "autoloop".into(),
            pool_size: 4,
        });
        let identity = provision_identity(
            &db,
            "session-p11-mcp",
            "tenant:p11m",
            "principal:p11m",
            "policy:p11m",
            vec!["mcp::".into()],
        )
        .await;

        let mut mcp_manifest = manifest(
            CapabilityRisk::Low,
            ApprovalStatus::Verified,
            CapabilityStatus::Active,
        );
        mcp_manifest.registered_tool_name = "mcp::local-mcp::timeout".into();
        mcp_manifest.capability_id = "mcp::local-mcp::timeout".into();
        mcp_manifest.delegate_tool_name = "mcp::local-mcp::invoke".into();
        mcp_manifest.executable = "autoloop-nonexistent-bin".into();
        mcp_manifest.command_template = "autoloop-nonexistent-bin --run".into();
        mcp_manifest.server = "local-mcp".into();
        tools.hydrate_manifest(mcp_manifest.clone());

        let envelope = TaskEnvelope {
            session_id: SessionId::from("session-p11-mcp"),
            trace_id: TraceId::from("trace-p11-mcp"),
            task_id: TaskId::from("task-p11-mcp"),
            capability_id: CapabilityId::from("mcp::local-mcp::timeout"),
            identity,
            payload: serde_json::json!({"arg":"x"}),
            constraints: ConstraintSet {
                max_cpu_percent: 80,
                max_memory_mb: 256,
                timeout_ms: 5_000,
                max_retries: 1,
                max_tokens: 128,
                io_allow_paths: vec![".".into()],
                io_deny_paths: vec![],
                sandbox_profile: "mcp".into(),
                requires_human_approval: false,
            },
        };
        let result = runtime
            .execute(
                &db,
                &tools,
                &providers,
                "runtime:p11-mcp",
                &envelope,
                Some(&mcp_manifest),
                None,
            )
            .await
            .expect("degraded result");
        assert!(result
            .content
            .contains("mcp degraded mode activated"));
        let active = runtime
            .active_degrade_profile(&db, "session-p11-mcp")
            .await
            .expect("active")
            .expect("degrade active");
        assert_eq!(active.kind, DegradeProfileKind::McpConservative);
    }

    #[tokio::test]
    async fn p11_recover_marks_failover_with_mttr() {
        let runtime = RuntimeKernel::from_config(&AppConfig::default().runtime);
        let db = SpacetimeDb::from_config(&SpacetimeDbConfig {
            enabled: true,
            backend: SpacetimeBackend::InMemory,
            uri: "http://spacetimedb:3000".into(),
            module_name: "autoloop_core".into(),
            namespace: "autoloop".into(),
            pool_size: 4,
        });
        runtime
            .apply_degrade_profile(
                &db,
                "session-p11-recover",
                "trigger:p11",
                DegradeProfileKind::QueueThrottle,
                "queue congestion",
            )
            .await
            .expect("degrade");
        let recovered = runtime
            .recover_from_degrade(&db, "session-p11-recover", "queue drained")
            .await
            .expect("recover")
            .expect("record");
        assert!(recovered.recovered);
        assert!(recovered.mttr_ms.is_some());
    }

    #[tokio::test]
    async fn p11_chaos_case_records_failover() {
        let runtime = RuntimeKernel::from_config(&AppConfig::default().runtime);
        let db = SpacetimeDb::from_config(&SpacetimeDbConfig {
            enabled: true,
            backend: SpacetimeBackend::InMemory,
            uri: "http://spacetimedb:3000".into(),
            module_name: "autoloop_core".into(),
            namespace: "autoloop".into(),
            pool_size: 4,
        });
        let record = runtime
            .run_chaos_case(
                &db,
                "session-p11-chaos",
                ChaosCase {
                    case_id: "db-unavailable".into(),
                    name: "db unavailable".into(),
                    fault: "db_unavailable".into(),
                    expected_profile: DegradeProfileKind::ReadOnly,
                    target: "spacetimedb".into(),
                    injected_at_ms: current_time_ms(),
                },
            )
            .await
            .expect("chaos record");
        assert_eq!(record.profile, DegradeProfileKind::ReadOnly);
        let profile = runtime
            .active_degrade_profile(&db, "session-p11-chaos")
            .await
            .expect("active")
            .expect("profile");
        assert!(profile.read_only_mode);
    }
}
