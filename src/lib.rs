pub mod agent;
pub mod adaptive_framework;
pub mod agentevolver_task_core;
pub mod bus;
pub mod config;
pub mod contracts;
pub mod dashboard_server;
pub mod evolution;
pub mod hooks;
pub mod memory;
pub mod module_bindings;
pub mod observability;
pub mod orchestration;
pub mod providers;
pub mod rag;
pub mod research;
pub mod runtime;
pub mod security;
pub mod session;
pub mod tools;

pub use autoloop_spacetimedb_adapter as spacetimedb_adapter;
pub use spacetimedb_sdk;

use std::{fs, path::PathBuf};

use anyhow::Result;
use agent::AgentRuntime;
use autoloop_spacetimedb_adapter::SpacetimeDb;
use bus::MessageBus;
use config::AppConfig;
use evolution::SelfEvolutionKernel;
use hooks::HookRegistry;
use memory::{CausalEdge, MemorySubsystem, ReflexionEpisode, SkillRecord, WitnessLog};
use observability::ObservabilityKernel;
use observability::event_stream::{ReplayAnalysisReport, append_event};
use orchestration::{
    AbRoutingStats, ExecutionStats, OrchestrationKernel, current_time_ms, parse_mcp_server,
    update_ab_routing_stats,
    update_execution_stats,
};
use providers::ProviderRegistry;
use rag::RagSubsystem;
use research::ResearchKernel;
use runtime::{ReplayRunRequest, RuntimeKernel};
use security::SecurityPolicy;
use session::{SessionIdentity, SessionStore};
use tools::{ForgedMcpToolManifest, ToolRegistry};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DashboardSessionSnapshot {
    pub session_id: String,
    pub anchor: String,
    pub ceo_summary: String,
    pub validation_summary: String,
    pub route_treatment_share: f32,
    pub readiness: bool,
    pub capability_catalog: Vec<DashboardCapabilityRecord>,
    pub proxy_forensics: serde_json::Value,
    pub research_health: serde_json::Value,
    pub graph: DashboardGraphLens,
    pub verifier: DashboardVerifierLens,
    pub business: DashboardBusinessLens,
    pub work_orders: Vec<serde_json::Value>,
    pub revenue_events: Vec<serde_json::Value>,
    pub operations_notes: Vec<String>,
    pub capability_lifecycle: serde_json::Value,
    pub runtime_circuits: serde_json::Value,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DashboardCapabilityRecord {
    pub name: String,
    pub status: String,
    pub approval: String,
    pub health: f32,
    pub scope: String,
    pub risk: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DashboardGraphLens {
    pub entities: usize,
    pub relationships: usize,
    pub communities: usize,
    pub forged_capability_count: usize,
    pub top_entities: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DashboardVerifierLens {
    pub verdict: String,
    pub score: f32,
    pub summary: String,
    pub failing_tools: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DashboardBusinessLens {
    pub revenue_micros: u64,
    pub cost_micros: u64,
    pub profit_micros: i64,
    pub margin_ratio: f32,
    pub sla_success_ratio: f32,
    pub breached_orders: usize,
    pub risk_summary: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionReplaySnapshot {
    pub session_id: String,
    pub deliberation: Option<serde_json::Value>,
    pub execution_feedback: Vec<serde_json::Value>,
    pub traces: Vec<serde_json::Value>,
    pub route_analytics: Option<serde_json::Value>,
    pub failure_forensics: Option<serde_json::Value>,
}

#[derive(Debug)]
pub struct BootstrapReport {
    pub app_name: String,
    pub provider_count: usize,
    pub tool_count: usize,
    pub hook_count: usize,
    pub memory_targets: usize,
    pub rag_strategies: usize,
}

pub struct AutoLoopApp {
    pub config: AppConfig,
    pub runtime: RuntimeKernel,
    pub security: SecurityPolicy,
    pub memory: MemorySubsystem,
    pub evolution: SelfEvolutionKernel,
    pub rag: RagSubsystem,
    pub research: ResearchKernel,
    pub providers: ProviderRegistry,
    pub tools: ToolRegistry,
    pub hooks: HookRegistry,
    pub observability: ObservabilityKernel,
    pub orchestration: OrchestrationKernel,
    pub sessions: SessionStore,
    pub bus: MessageBus,
    pub spacetimedb: SpacetimeDb,
    pub agent: AgentRuntime,
}

impl AutoLoopApp {
    pub fn try_new(config: AppConfig) -> Result<Self> {
        let runtime = RuntimeKernel::from_config(&config.runtime);
        let security = SecurityPolicy::from_config(&config.security);
        let memory = MemorySubsystem::from_config(&config.memory, &config.learning);
        let evolution = SelfEvolutionKernel::new();
        let rag = RagSubsystem::from_config(&config.rag);
        let research = ResearchKernel::from_config(&config.research);
        let providers = ProviderRegistry::from_config(&config.providers);
        let tools = ToolRegistry::from_config(&config.tools);
        let hooks = HookRegistry::from_config(&config.hooks);
        let observability = ObservabilityKernel::from_config(&config.observability, &config.deployment);
        let sessions = SessionStore::new(config.agent.memory_window);
        let bus = MessageBus::default();
        let spacetimedb = SpacetimeDb::try_from_config(&config.spacetimedb)?;
        tools.attach_spacetimedb(spacetimedb.clone());
        let orchestration = OrchestrationKernel::new(
            providers.clone(),
            tools.clone(),
            sessions.clone(),
            memory.clone(),
            rag.clone(),
            runtime.clone(),
            spacetimedb.clone(),
            config.learning.gray_routing_ratio,
            config.learning.routing_takeover_threshold,
        );
        let agent = AgentRuntime::new(
            config.agent.clone(),
            providers.clone(),
            tools.clone(),
            sessions.clone(),
            memory.clone(),
            hooks.clone(),
            security.clone(),
            runtime.clone(),
            spacetimedb.clone(),
        );

        Ok(Self {
            config,
            runtime,
            security,
            memory,
            evolution,
            rag,
            research,
            providers,
            tools,
            hooks,
            observability,
            orchestration,
            sessions,
            bus,
            spacetimedb,
            agent,
        })
    }

    pub fn new(config: AppConfig) -> Self {
        Self::try_new(config).expect("failed to construct AutoLoopApp")
    }

    pub async fn bootstrap(&self) -> Result<BootstrapReport> {
        let _ = self.tools.restore_persisted_manifests().await?;
        self.runtime.validate()?;
        self.security.validate(&self.runtime, &self.tools)?;
        self.memory.validate()?;
        self.rag.validate()?;
        self.research.validate()?;
        self.providers.validate()?;
        self.tools.validate()?;
        self.hooks.validate()?;
        self.observability.validate()?;
        self.spacetimedb.validate()?;
        self.agent.validate()?;

        Ok(BootstrapReport {
            app_name: self.config.app.name.clone(),
            provider_count: self.providers.len(),
            tool_count: self.tools.len(),
            hook_count: self.hooks.len(),
            memory_targets: self.memory.load_targets().len(),
            rag_strategies: self.rag.strategies().len(),
        })
    }

    pub async fn process_direct(&self, session_id: &str, content: &str) -> Result<String> {
        self.ensure_default_session_identity(session_id).await?;
        self.agent.process_message(session_id, content).await
    }

    pub async fn ensure_session_identity(
        &self,
        session_id: &str,
        tenant_id: &str,
        principal_id: &str,
        policy_id: &str,
        lease_ttl_ms: u64,
    ) -> Result<SessionIdentity> {
        let identity = self
            .security
            .issue_session_identity(
                &self.spacetimedb,
                session_id,
                tenant_id,
                principal_id,
                policy_id,
                lease_ttl_ms,
            )
            .await?;
        let expires_at_ms = current_time_ms().saturating_add(lease_ttl_ms);
        let session_identity = SessionIdentity {
            tenant_id: identity.tenant_id,
            principal_id: identity.principal_id,
            policy_id: identity.policy_id,
            lease_token: identity.lease_token,
            expires_at_ms,
        };
        self.sessions
            .bind_identity(session_id, session_identity.clone())
            .await;
        Ok(session_identity)
    }

    async fn ensure_default_session_identity(&self, session_id: &str) -> Result<SessionIdentity> {
        if let Some(identity) = self.sessions.identity(session_id).await {
            let now = current_time_ms();
            if identity.expires_at_ms > now {
                return Ok(identity);
            }
        }
        self.ensure_session_identity(
            session_id,
            "tenant:default",
            &format!("principal:{session_id}"),
            "policy:default",
            3_600_000,
        )
        .await
    }

    pub async fn process_requirement_swarm(&self, session_id: &str, content: &str) -> Result<String> {
        self.ensure_default_session_identity(session_id).await?;
        let research_report = self
            .research
            .run_anchor_research(&self.spacetimedb, session_id, content)
            .await?;
        let scheduled_research_tasks = self
            .research
            .schedule_follow_up_research(&self.spacetimedb, session_id, session_id, &research_report)
            .await
            .unwrap_or(0);
        let outcome = self
            .orchestration
            .run_requirement_swarm(session_id, content)
            .await?;

        self.spacetimedb
            .upsert_knowledge(
                format!("conversation:{session_id}:brief"),
                serde_json::to_string(&outcome.brief)?,
                "requirement-agent".into(),
            )
            .await?;
        self.spacetimedb
            .upsert_knowledge(
                format!("{}:brief", outcome.brief.anchor_id),
                serde_json::to_string(&outcome.brief)?,
                "requirement-agent".into(),
            )
            .await?;
        self.spacetimedb
            .upsert_knowledge(
                format!("conversation:{session_id}:ceo"),
                outcome.ceo_summary.clone(),
                "ceo-agent".into(),
            )
            .await?;
        self.spacetimedb
            .upsert_knowledge(
                format!("{}:ceo", outcome.brief.anchor_id),
                outcome.ceo_summary.clone(),
                "ceo-agent".into(),
            )
            .await?;
        self.spacetimedb
            .upsert_json_knowledge(
                format!("conversation:{session_id}:deliberation"),
                &outcome.deliberation,
                "swarm-judge",
            )
            .await?;
        self.spacetimedb
            .upsert_json_knowledge(
                format!("conversation:{session_id}:optimization"),
                &outcome.optimization_proposal,
                "optimization-agent",
            )
            .await?;
        self.spacetimedb
            .upsert_json_knowledge(
                format!("conversation:{session_id}:research"),
                &research_report,
                "autonomous-research",
            )
            .await?;
        self.spacetimedb
            .upsert_knowledge(
                format!("research:{session_id}:follow-up-status"),
                serde_json::json!({
                    "scheduled_tasks": scheduled_research_tasks,
                    "backend_used": research_report.backend_used,
                    "knowledge_gaps": research_report.knowledge_gaps,
                })
                .to_string(),
                "autonomous-research".into(),
            )
            .await?;
        let capability_lifecycle = self.tools.lifecycle_report();
        let verifier_queue_depth = self
            .spacetimedb
            .list_schedule_events(session_id)
            .await?
            .into_iter()
            .filter(|event| event.topic.contains("verifier"))
            .count();
        self.observability
            .persist_swarm_observability(
                &self.spacetimedb,
                session_id,
                &outcome,
                &capability_lifecycle,
                verifier_queue_depth,
            )
            .await?;
        self.spacetimedb
            .upsert_knowledge(
                format!("conversation:{session_id}:swarm"),
                serde_json::to_string(&outcome.execution_reports)?,
                "swarm-execution".into(),
            )
            .await?;
        self.spacetimedb
            .upsert_json_knowledge(
                format!("protocol:{session_id}:verifier-report"),
                &outcome.verifier_report,
                "verifier-agent",
            )
            .await?;
        self.spacetimedb
            .upsert_json_knowledge(
                format!("protocol:{session_id}:capability-regression"),
                &outcome.verifier_report.capability_regression,
                "capability-regression-suite",
            )
            .await?;
        for case in &outcome.verifier_report.capability_regression.cases {
            if case.passed {
                let _ = self.tools.verify_capability(&case.tool_name).await?;
            } else {
                let _ = self
                    .tools
                    .deprecate_capability(&case.tool_name, case.health_score.min(0.35))
                    .await?;
            }
        }
        for (index, report) in outcome.execution_reports.iter().enumerate() {
            let observed_at_ms = current_time_ms().saturating_add(index as u64);
            self.spacetimedb
                .upsert_knowledge(
                    format!("conversation:{session_id}:execution-feedback:{index}"),
                    serde_json::json!({
                        "tool": report.tool_used,
                        "mcp_server": report.mcp_server,
                        "payload": report.invocation_payload,
                        "outcome_score": report.outcome_score,
                        "route_variant": report.route_variant,
                        "control_score": report.control_score,
                        "treatment_score": report.treatment_score,
                        "output": report.output,
                    })
                    .to_string(),
                    "execution-feedback".into(),
                )
                .await?;

            let stats_key = match report.tool_used.as_deref() {
                Some(tool_name) if tool_name.starts_with("mcp::") => {
                    format!(
                        "metrics:execution:mcp:{}",
                        report
                            .mcp_server
                            .clone()
                            .or_else(|| parse_mcp_server(tool_name))
                            .unwrap_or_else(|| "unknown".into())
                    )
                }
                Some(tool_name) => format!("metrics:execution:tool:{tool_name}"),
                None => "metrics:execution:provider-only".into(),
            };
            let existing_stats = self
                .spacetimedb
                .get_knowledge(&stats_key)
                .await?
                .and_then(|record| serde_json::from_str::<ExecutionStats>(&record.value).ok());
            let updated_stats = update_execution_stats(existing_stats, report, observed_at_ms);
            self.spacetimedb
                .upsert_knowledge(
                    stats_key,
                    serde_json::to_string(&updated_stats)?,
                    "execution-stats".into(),
                )
                .await?;
            let session_ab_key = format!("metrics:ab:session:{session_id}");
            let existing_session_ab = self
                .spacetimedb
                .get_knowledge(&session_ab_key)
                .await?
                .and_then(|record| serde_json::from_str::<AbRoutingStats>(&record.value).ok());
            let mut updated_session_ab =
                update_ab_routing_stats(existing_session_ab, report, observed_at_ms);
            updated_session_ab.scope = session_ab_key.clone();
            self.spacetimedb
                .upsert_knowledge(
                    session_ab_key,
                    serde_json::to_string(&updated_session_ab)?,
                    "ab-routing-stats".into(),
                )
                .await?;
            let task_ab_key = format!("metrics:ab:task:{}", report.task.role.to_ascii_lowercase());
            let existing_task_ab = self
                .spacetimedb
                .get_knowledge(&task_ab_key)
                .await?
                .and_then(|record| serde_json::from_str::<AbRoutingStats>(&record.value).ok());
            let mut updated_task_ab =
                update_ab_routing_stats(existing_task_ab, report, observed_at_ms);
            updated_task_ab.scope = task_ab_key.clone();
            self.spacetimedb
                .upsert_knowledge(
                    task_ab_key,
                    serde_json::to_string(&updated_task_ab)?,
                    "ab-routing-stats".into(),
                )
                .await?;
            if let Some(tool_name) = &report.tool_used {
                let tool_ab_key = format!("metrics:ab:tool:{tool_name}");
                let existing_tool_ab = self
                    .spacetimedb
                    .get_knowledge(&tool_ab_key)
                    .await?
                    .and_then(|record| serde_json::from_str::<AbRoutingStats>(&record.value).ok());
                let mut updated_tool_ab =
                    update_ab_routing_stats(existing_tool_ab, report, observed_at_ms);
                updated_tool_ab.scope = tool_ab_key.clone();
                self.spacetimedb
                    .upsert_knowledge(
                        tool_ab_key,
                        serde_json::to_string(&updated_tool_ab)?,
                        "ab-routing-stats".into(),
                    )
                    .await?;
            }
            if let Some(server) = &report.mcp_server {
                let server_ab_key = format!("metrics:ab:server:{server}");
                let existing_server_ab = self
                    .spacetimedb
                    .get_knowledge(&server_ab_key)
                    .await?
                    .and_then(|record| serde_json::from_str::<AbRoutingStats>(&record.value).ok());
                let mut updated_server_ab =
                    update_ab_routing_stats(existing_server_ab, report, observed_at_ms);
                updated_server_ab.scope = server_ab_key.clone();
                self.spacetimedb
                    .upsert_knowledge(
                        server_ab_key,
                        serde_json::to_string(&updated_server_ab)?,
                        "ab-routing-stats".into(),
                    )
                    .await?;
            }
            self.memory
                .persist_witness_log(
                    &self.spacetimedb,
                    session_id,
                    &WitnessLog {
                        source: report
                            .tool_used
                            .clone()
                            .unwrap_or_else(|| "provider-only".into()),
                        observation: report.output.clone(),
                        metric_name: "outcome_score".into(),
                    metric_value: report.outcome_score as f32,
                },
            )
            .await?;
            self.memory
                .persist_learning_event(
                    &self.spacetimedb,
                    &memory::LearningEvent {
                        event_kind: memory::LearningEventKind::RouteDecision,
                        session_id: session_id.to_string(),
                        source: report
                            .tool_used
                            .clone()
                            .unwrap_or_else(|| "provider-only".into()),
                        summary: format!(
                            "variant={} control={} treatment={}",
                            report.route_variant, report.control_score, report.treatment_score
                        ),
                        score: report.outcome_score as f32,
                    },
                )
                .await?;
        }
        let circuit_snapshot = self.runtime.circuit_snapshot(&self.spacetimedb).await?;
        self.spacetimedb
            .upsert_knowledge(
                format!("observability:{session_id}:runtime-circuits"),
                serde_json::to_string(&circuit_snapshot)?,
                "runtime-circuit".into(),
            )
            .await?;
        let forged_manifests = self
            .spacetimedb
            .list_knowledge_by_prefix(ToolRegistry::FORGED_TOOL_PREFIX)
            .await?
            .into_iter()
            .filter_map(|record| serde_json::from_str::<ForgedMcpToolManifest>(&record.value).ok())
            .collect::<Vec<_>>();
        let prior_snapshot = self
            .spacetimedb
            .get_knowledge(&format!("graph:{session_id}:snapshot"))
            .await?
            .map(|record| record.value);
        let augmented_snapshot = self
            .rag
            .augment_snapshot_with_forged_capabilities(
                &outcome.knowledge_update.snapshot_json,
                &forged_manifests,
            );
        let merged_snapshot = self.rag.merge_incremental_snapshot(
            prior_snapshot.as_deref(),
            &augmented_snapshot,
            &outcome.tasks,
            &forged_manifests,
        );
        self.spacetimedb
            .upsert_knowledge(
                format!("graph:{session_id}:snapshot"),
                merged_snapshot,
                "graph-rag".into(),
            )
            .await?;
        let global_snapshot = self.merge_global_graph_snapshot(session_id, &forged_manifests).await?;
        self.spacetimedb
            .upsert_knowledge(
                "graph:global:snapshot".into(),
                global_snapshot,
                "graph-rag-global".into(),
            )
            .await?;
        self.memory
            .persist_learning_session(
                &self.spacetimedb,
                autoloop_spacetimedb_adapter::LearningSessionRecord {
                    id: format!("iteration:{session_id}:{}", current_time_ms()),
                    session_id: session_id.to_string(),
                    objective: outcome.brief.clarified_goal.clone(),
                    status: if outcome.validation.ready {
                        "completed".into()
                    } else {
                        "needs_follow_up".into()
                    },
                    priority: if outcome.validation.ready { 0.3 } else { 0.9 },
                    summary: outcome.validation.summary.clone(),
                    started_at_ms: current_time_ms(),
                    completed_at_ms: Some(current_time_ms()),
                },
            )
            .await?;
        self.memory
            .persist_reflexion_episode(
                &self.spacetimedb,
                session_id,
                &ReflexionEpisode {
                    proposal_id: session_key(session_id),
                    hypothesis: outcome.optimization_proposal.hypothesis.clone(),
                    outcome: outcome.validation.summary.clone(),
                    lesson: if outcome.validation.ready {
                        "Keep iterations that improve immutable objectives with bounded complexity."
                            .into()
                    } else {
                        "Rollback or revise iterations that regress immutable objectives or leave unresolved follow-up work."
                            .into()
                    },
                },
            )
            .await?;
        self.memory
            .persist_skill(
                &self.spacetimedb,
                session_id,
                &SkillRecord {
                    name: "autonomy-optimization-loop".into(),
                    trigger: "Need a bounded iteration under immutable evaluation".into(),
                    procedure:
                        "propose -> apply -> execute -> parse immutable metric -> keep/discard -> rollback"
                            .into(),
                    confidence: 0.78,
                },
            )
            .await?;
        self.memory
            .persist_causal_edge(
                &self.spacetimedb,
                session_id,
                &CausalEdge {
                    cause: "immutable-objective-regression".into(),
                    effect: "rollback-iteration".into(),
                    evidence: outcome.validation.summary.clone(),
                    strength: if outcome.validation.ready { 0.2 } else { 0.9 },
                },
            )
            .await?;
        self.memory
            .persist_learning_event(
                &self.spacetimedb,
                &memory::LearningEvent {
                    event_kind: if outcome.validation.ready {
                        memory::LearningEventKind::Success
                    } else {
                        memory::LearningEventKind::Failure
                    },
                    session_id: session_id.to_string(),
                    source: "validation".into(),
                    summary: outcome.validation.summary.clone(),
                    score: if outcome.validation.ready { 1.0 } else { 0.0 },
                },
            )
            .await?;
        let consolidation = self
            .memory
            .consolidate_learning(&self.spacetimedb, session_id)
            .await?;
        let evolution_report = self
            .evolution
            .run(session_id, &consolidation, outcome.verifier_report.overall_score);
        let retired_capabilities = self.tools.auto_retire_unhealthy_capabilities().await?;
        self.spacetimedb
            .upsert_json_knowledge(
                format!("memory:{session_id}:consolidation"),
                &consolidation,
                "learning-consolidation",
            )
            .await?;
        self.spacetimedb
            .upsert_json_knowledge(
                format!("memory:{session_id}:self-evolution"),
                &evolution_report,
                "self-evolution",
            )
            .await?;
        for proposal in &consolidation.capability_improvements {
            self.spacetimedb
                .upsert_json_knowledge(
                    format!(
                        "memory:{session_id}:capability-improvement:{}",
                        proposal.tool_name
                    ),
                    proposal,
                    "learning-consolidation",
                )
                .await?;
            self.spacetimedb
                .create_schedule_event(
                    session_id.to_string(),
                    "verifier.capability_improvement".into(),
                    "cli::verify_capability".into(),
                    serde_json::to_string(proposal)?,
                    session_id.to_string(),
                )
                .await?;
        }
        for proposal in &evolution_report.capability_proposals {
            self.spacetimedb
                .upsert_json_knowledge(
                    format!("memory:{session_id}:evolution-proposal:{}", proposal.tool_name),
                    proposal,
                    "self-evolution",
                )
                .await?;
            self.memory
                .persist_witness_log(
                    &self.spacetimedb,
                    session_id,
                    &WitnessLog {
                        source: proposal.tool_name.clone(),
                        observation: proposal.rationale.clone(),
                        metric_name: "expected_lift".into(),
                        metric_value: proposal.expected_lift,
                    },
                )
                .await?;
        }
        self.memory
            .persist_learning_session(
                &self.spacetimedb,
                autoloop_spacetimedb_adapter::LearningSessionRecord {
                    id: format!("evolution:{session_id}:{}", current_time_ms()),
                    session_id: session_id.to_string(),
                    objective: "self-evolution-policy-adaptation".into(),
                    status: if evolution_report.evolved_score > evolution_report.baseline_score {
                        "improved".into()
                    } else {
                        "stable".into()
                    },
                    priority: 0.75,
                    summary: evolution_report.summary.clone(),
                    started_at_ms: current_time_ms(),
                    completed_at_ms: Some(current_time_ms()),
                },
            )
            .await?;
        self.spacetimedb
            .upsert_json_knowledge(
                format!("protocol:{session_id}:immutable-eval"),
                &self.runtime.evaluation_protocol(),
                "evaluation-protocol",
            )
            .await?;
        self.spacetimedb
            .upsert_json_knowledge(
                format!("observability:{session_id}:capability-lifecycle"),
                &capability_lifecycle,
                "capability-governance",
            )
            .await?;
        self.spacetimedb
            .upsert_json_knowledge(
                format!("observability:{session_id}:retired-capabilities"),
                &retired_capabilities,
                "capability-governance",
            )
            .await?;
        let dashboard_snapshot = self.export_dashboard_snapshot(session_id).await?;
        let replay_snapshot = self.export_session_replay(session_id).await?;
        self.persist_runtime_artifact("dashboard", session_id, &dashboard_snapshot)?;
        self.persist_runtime_artifact("replay", session_id, &replay_snapshot)?;

        if !outcome.validation.ready {
            self.spacetimedb
                .create_schedule_event(
                    session_id.to_string(),
                    "validation.iteration".into(),
                    "mcp::local-mcp::invoke".into(),
                    serde_json::to_string(&outcome.validation.follow_up_tasks)?,
                    session_id.to_string(),
                )
                .await?;
        }

        Ok(format!(
            "Requirement brief: {}\nResearch autonomy: {:.2}\nResearch follow-ups: {}\nCEO: {}\nVerifier: {}\nSwarm tasks: {}\nValidation: {}\nEvolution: {}\nRetired capabilities: {}\nGraph update: {}",
            outcome.brief.clarified_goal,
            research_report.autonomy_score,
            scheduled_research_tasks,
            outcome.ceo_summary,
            outcome.verifier_report.summary,
            outcome.tasks.len(),
            outcome.validation.summary,
            evolution_report.summary,
            retired_capabilities.len(),
            outcome.knowledge_update.global_context_summary
        ))
    }

    pub async fn list_focus_anchors(&self) -> Result<Vec<String>> {
        let deleted = self
            .spacetimedb
            .list_knowledge_by_prefix("anchor:")
            .await?
            .into_iter()
            .filter_map(|record| record.key.strip_suffix(":deleted").map(|prefix| prefix.to_string()))
            .collect::<std::collections::HashSet<_>>();
        let mut anchors = self
            .spacetimedb
            .list_knowledge_by_prefix("anchor:")
            .await?
            .into_iter()
            .filter_map(|record| {
                record
                    .key
                    .strip_suffix(":brief")
                    .map(|prefix| prefix.to_string())
            })
            .filter(|anchor| !deleted.contains(anchor))
            .collect::<Vec<_>>();
        anchors.sort();
        anchors.dedup();
        Ok(anchors)
    }

    pub async fn focus_status(&self, anchor_or_session: &str) -> Result<String> {
        let anchor_id = normalize_anchor(anchor_or_session);
        let brief = self
            .spacetimedb
            .get_knowledge(&format!("{anchor_id}:brief"))
            .await?;
        let dashboard = self
            .spacetimedb
            .get_knowledge(&format!(
                "observability:{}:dashboard",
                anchor_or_session.trim_start_matches("anchor:")
            ))
            .await?;
        let research = self
            .spacetimedb
            .get_knowledge(&format!(
                "research:{}:report",
                anchor_or_session.trim_start_matches("anchor:")
            ))
            .await?;
        let follow_up = self
            .spacetimedb
            .get_knowledge(&format!(
                "research:{}:follow-up-status",
                anchor_or_session.trim_start_matches("anchor:")
            ))
            .await?;
        let deleted = self
            .spacetimedb
            .get_knowledge(&format!("{anchor_id}:deleted"))
            .await?;
        let graph = self
            .spacetimedb
            .get_knowledge(&format!(
                "graph:{}:snapshot",
                anchor_or_session.trim_start_matches("anchor:")
            ))
            .await?;
        let verifier = self
            .spacetimedb
            .get_knowledge(&format!(
                "protocol:{}:verifier-report",
                anchor_or_session.trim_start_matches("anchor:")
            ))
            .await?;
        let proxy_forensics = self
            .spacetimedb
            .get_knowledge(&format!(
                "research:{}:proxy-forensics",
                anchor_or_session.trim_start_matches("anchor:")
            ))
            .await?;
        let global_graph = self
            .spacetimedb
            .get_knowledge("graph:global:snapshot")
            .await?;
        Ok(serde_json::json!({
            "anchor_id": anchor_id,
            "brief": brief.map(|record| record.value),
            "dashboard": dashboard.map(|record| record.value),
            "research": research.map(|record| record.value),
            "research_follow_up": follow_up.map(|record| record.value),
            "deleted": deleted.map(|record| record.value),
            "graph_snapshot": graph.map(|record| record.value),
            "global_graph_snapshot": global_graph.map(|record| record.value),
            "verifier": verifier.map(|record| record.value),
            "proxy_forensics": proxy_forensics.map(|record| record.value),
        })
        .to_string())
    }

    async fn merge_global_graph_snapshot(
        &self,
        current_session_id: &str,
        forged_manifests: &[ForgedMcpToolManifest],
    ) -> Result<String> {
        let existing_global = self
            .spacetimedb
            .get_knowledge("graph:global:snapshot")
            .await?
            .map(|record| record.value);
        let current = self
            .spacetimedb
            .get_knowledge(&format!("graph:{current_session_id}:snapshot"))
            .await?
            .map(|record| record.value)
            .unwrap_or_else(|| "{}".into());
        let related_graphs = self
            .spacetimedb
            .list_knowledge_by_prefix("graph:")
            .await?
            .into_iter()
            .filter(|record| record.key.ends_with(":snapshot") && record.key != "graph:global:snapshot")
            .take(6)
            .collect::<Vec<_>>();

        let mut merged = self.rag.merge_incremental_snapshot(
            existing_global.as_deref(),
            &current,
            &[],
            forged_manifests,
        );
        for record in related_graphs {
            if record.key == format!("graph:{current_session_id}:snapshot") {
                continue;
            }
            merged = self.rag.merge_incremental_snapshot(
                Some(&merged),
                &record.value,
                &[],
                forged_manifests,
            );
        }
        Ok(merged)
    }

    pub async fn delete_focus_anchor(&self, anchor_or_session: &str) -> Result<String> {
        let anchor_id = normalize_anchor(anchor_or_session);
        self.spacetimedb
            .upsert_json_knowledge(
                format!("{anchor_id}:deleted"),
                &serde_json::json!({
                    "anchor_id": anchor_id,
                    "status": "deleted",
                    "deleted_at_ms": current_time_ms(),
                }),
                "autocog-focus",
            )
            .await?;
        Ok(serde_json::json!({
            "status": "deleted",
            "anchor_id": anchor_or_session,
        })
        .to_string())
    }

    pub async fn export_mcp_catalog(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(&self.tools.manifests())?)
    }

    pub async fn export_dashboard_snapshot(&self, anchor_or_session: &str) -> Result<String> {
        let session_id = anchor_or_session.trim_start_matches("anchor:").to_string();
        let anchor = normalize_anchor(anchor_or_session);
        let ceo_summary = self
            .spacetimedb
            .get_knowledge(&format!("conversation:{session_id}:ceo"))
            .await?
            .map(|record| record.value)
            .unwrap_or_default();
        let verifier_json = self
            .spacetimedb
            .get_knowledge(&format!("protocol:{session_id}:verifier-report"))
            .await?
            .and_then(|record| serde_json::from_str::<serde_json::Value>(&record.value).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        let graph_json = self
            .spacetimedb
            .get_knowledge(&format!("graph:{session_id}:snapshot"))
            .await?
            .and_then(|record| serde_json::from_str::<serde_json::Value>(&record.value).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        let route_analytics = self
            .spacetimedb
            .get_knowledge(&format!("observability:{session_id}:route-analytics"))
            .await?
            .and_then(|record| serde_json::from_str::<serde_json::Value>(&record.value).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        let operations_report = self
            .spacetimedb
            .get_knowledge(&format!("observability:{session_id}:operations-report"))
            .await?
            .and_then(|record| serde_json::from_str::<serde_json::Value>(&record.value).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        let business_report = self
            .spacetimedb
            .get_knowledge(&format!("observability:{session_id}:business-report"))
            .await?
            .and_then(|record| serde_json::from_str::<serde_json::Value>(&record.value).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        let work_orders = self
            .spacetimedb
            .list_knowledge_by_prefix(&format!("business:work-order:{session_id}:"))
            .await?
            .into_iter()
            .filter_map(|record| serde_json::from_str::<serde_json::Value>(&record.value).ok())
            .collect::<Vec<_>>();
        let revenue_events = self
            .spacetimedb
            .list_knowledge_by_prefix(&format!("business:revenue-event:{session_id}:"))
            .await?
            .into_iter()
            .filter_map(|record| serde_json::from_str::<serde_json::Value>(&record.value).ok())
            .collect::<Vec<_>>();
        let margin_report = business_report
            .get("margin")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        let sla_report = business_report
            .get("sla")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        let snapshot = DashboardSessionSnapshot {
            session_id: session_id.clone(),
            anchor,
            ceo_summary,
            validation_summary: verifier_json
                .get("summary")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("no verifier summary")
                .to_string(),
            route_treatment_share: route_analytics
                .get("treatment_share")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0) as f32,
            readiness: verifier_json
                .get("verdict")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|verdict| verdict == "pass"),
            capability_catalog: self
                .tools
                .manifests()
                .into_iter()
                .map(|manifest| DashboardCapabilityRecord {
                    name: manifest.registered_tool_name,
                    status: format!("{:?}", manifest.status).to_ascii_lowercase(),
                    approval: format!("{:?}", manifest.approval_status).to_ascii_lowercase(),
                    health: manifest.health_score,
                    scope: format!("{:?}", manifest.scope).to_ascii_lowercase(),
                    risk: format!("{:?}", manifest.risk).to_ascii_lowercase(),
                })
                .collect(),
            proxy_forensics: self
                .spacetimedb
                .get_knowledge(&format!("research:{session_id}:proxy-forensics"))
                .await?
                .and_then(|record| serde_json::from_str::<serde_json::Value>(&record.value).ok())
                .unwrap_or_else(|| serde_json::json!({})),
            research_health: serde_json::to_value(self.research.health_report())?,
            graph: DashboardGraphLens {
                entities: graph_json
                    .get("entities")
                    .and_then(serde_json::Value::as_array)
                    .map(|items| items.len())
                    .unwrap_or(0),
                relationships: graph_json
                    .get("relationships")
                    .and_then(serde_json::Value::as_array)
                    .map(|items| items.len())
                    .unwrap_or(0),
                communities: graph_json
                    .get("communities")
                    .and_then(serde_json::Value::as_array)
                    .map(|items| items.len())
                    .unwrap_or(0),
                forged_capability_count: self.tools.forged_tool_names().len(),
                top_entities: graph_json
                    .get("entities")
                    .and_then(serde_json::Value::as_array)
                    .map(|items| {
                        items.iter()
                            .filter_map(|item| {
                                item.get("canonical_name")
                                    .and_then(serde_json::Value::as_str)
                                    .map(str::to_string)
                            })
                            .take(8)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
            },
            verifier: DashboardVerifierLens {
                verdict: verifier_json
                    .get("verdict")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown")
                    .to_string(),
                score: verifier_json
                    .get("overall_score")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.0) as f32,
                summary: verifier_json
                    .get("summary")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("no verifier summary")
                    .to_string(),
                failing_tools: verifier_json
                    .get("capability_regression")
                    .and_then(|value| value.get("failing_tools"))
                    .and_then(serde_json::Value::as_array)
                    .map(|items| {
                        items.iter()
                            .filter_map(|item| item.as_str().map(str::to_string))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
            },
            business: DashboardBusinessLens {
                revenue_micros: margin_report
                    .get("recognized_revenue_micros")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
                cost_micros: margin_report
                    .get("allocated_cost_micros")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
                profit_micros: margin_report
                    .get("gross_profit_micros")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0),
                margin_ratio: margin_report
                    .get("gross_margin_ratio")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.0) as f32,
                sla_success_ratio: sla_report
                    .get("sla_success_ratio")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(1.0) as f32,
                breached_orders: sla_report
                    .get("breached_orders")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as usize,
                risk_summary: business_report
                    .get("risk_summary")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("business risk unavailable")
                    .to_string(),
            },
            work_orders,
            revenue_events,
            operations_notes: operations_report
                .get("task_summary")
                .and_then(serde_json::Value::as_array)
                .map(|items| {
                    items.iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .take(8)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            capability_lifecycle: self
                .spacetimedb
                .get_knowledge(&format!("observability:{session_id}:capability-lifecycle"))
                .await?
                .and_then(|record| serde_json::from_str::<serde_json::Value>(&record.value).ok())
                .unwrap_or_else(|| serde_json::json!({})),
            runtime_circuits: self
                .spacetimedb
                .get_knowledge(&format!("observability:{session_id}:runtime-circuits"))
                .await?
                .and_then(|record| serde_json::from_str::<serde_json::Value>(&record.value).ok())
                .unwrap_or_else(|| serde_json::json!({})),
        };

        Ok(serde_json::to_string_pretty(&snapshot)?)
    }

    pub async fn export_session_replay(&self, anchor_or_session: &str) -> Result<String> {
        let session_id = anchor_or_session.trim_start_matches("anchor:").to_string();
        let replay = SessionReplaySnapshot {
            session_id: session_id.clone(),
            deliberation: self
                .spacetimedb
                .get_knowledge(&format!("conversation:{session_id}:deliberation"))
                .await?
                .and_then(|record| serde_json::from_str::<serde_json::Value>(&record.value).ok()),
            execution_feedback: self
                .spacetimedb
                .list_knowledge_by_prefix(&format!("conversation:{session_id}:execution-feedback:"))
                .await?
                .into_iter()
                .filter_map(|record| serde_json::from_str::<serde_json::Value>(&record.value).ok())
                .collect(),
            traces: self
                .spacetimedb
                .list_knowledge_by_prefix(&format!("observability:{session_id}:trace:"))
                .await?
                .into_iter()
                .filter_map(|record| serde_json::from_str::<serde_json::Value>(&record.value).ok())
                .collect(),
            route_analytics: self
                .spacetimedb
                .get_knowledge(&format!("observability:{session_id}:route-analytics"))
                .await?
                .and_then(|record| serde_json::from_str::<serde_json::Value>(&record.value).ok()),
            failure_forensics: self
                .spacetimedb
                .get_knowledge(&format!("observability:{session_id}:failure-forensics"))
                .await?
                .and_then(|record| serde_json::from_str::<serde_json::Value>(&record.value).ok()),
        };
        Ok(serde_json::to_string_pretty(&replay)?)
    }

    pub async fn run_replay_snapshot(&self, snapshot_id: &str) -> Result<String> {
        let report = self
            .runtime
            .replay_from_snapshot(
                &self.spacetimedb,
                &self.tools,
                &self.providers,
                &ReplayRunRequest {
                    snapshot_id: snapshot_id.to_string(),
                },
            )
            .await?;
        Ok(serde_json::to_string_pretty(&report)?)
    }

    pub async fn export_replay_report(
        &self,
        anchor_or_session: &str,
        snapshot_id: Option<&str>,
    ) -> Result<String> {
        let session_id = anchor_or_session.trim_start_matches("anchor:");
        let prefix = if let Some(snapshot_id) = snapshot_id {
            format!("replay:analysis:{snapshot_id}:")
        } else {
            "replay:analysis:".to_string()
        };
        let mut reports = self
            .spacetimedb
            .list_knowledge_by_prefix(&prefix)
            .await?
            .into_iter()
            .filter_map(|record| serde_json::from_str::<ReplayAnalysisReport>(&record.value).ok())
            .filter(|report| report.session_id == session_id)
            .collect::<Vec<_>>();
        reports.sort_by_key(|report| report.created_at_ms);
        let latest = reports.last().cloned();
        Ok(serde_json::to_string_pretty(&serde_json::json!({
            "session_id": session_id,
            "snapshot_id": snapshot_id,
            "count": reports.len(),
            "latest": latest,
            "reports": reports,
        }))?)
    }

    pub async fn import_mcp_catalog(&self, raw: &str) -> Result<String> {
        let manifests = serde_json::from_str::<Vec<ForgedMcpToolManifest>>(raw)?;
        let mut imported = 0usize;
        for manifest in manifests {
            self.tools.hydrate_manifest(manifest.clone());
            self.tools.persist_manifest(&manifest).await?;
            imported += 1;
        }
        Ok(serde_json::json!({
            "status": "imported",
            "count": imported,
        })
        .to_string())
    }

    pub async fn govern_mcp_capability(&self, action: &str, tool_name: &str) -> Result<String> {
        let result = match action {
            "verify" => self.tools.verify_capability(tool_name).await?,
            "rollback" => self.tools.rollback_capability(tool_name).await?,
            "deprecate" => self.tools.deprecate_capability(tool_name, 0.25).await?,
            _ => None,
        };
        Ok(serde_json::to_string_pretty(&serde_json::json!({
            "action": action,
            "tool": tool_name,
            "result": result,
        }))?)
    }

    pub async fn operator_decision(
        &self,
        session_id: &str,
        approved: bool,
        reason: &str,
    ) -> Result<String> {
        let now = orchestration::current_time_ms();
        let trace_id = format!("operator:{session_id}:{now}");
        let decision = if approved { "approved" } else { "rejected" };
        self.spacetimedb
            .upsert_json_knowledge(
                format!("policy:{session_id}:decision:{now}"),
                &serde_json::json!({
                    "session_id": session_id,
                    "trace_id": trace_id,
                    "decision": decision,
                    "reason": reason,
                    "created_at_ms": now,
                }),
                "operator-control",
            )
            .await?;
        let _ = append_event(
            &self.spacetimedb,
            "policy_decisions",
            trace_id,
            session_id.to_string(),
            None,
            None,
            contracts::version::CONTRACT_VERSION,
            serde_json::json!({
                "decision": decision,
                "reason": reason,
            }),
        )
        .await;
        Ok(serde_json::json!({
            "status": decision,
            "session_id": session_id,
            "reason": reason
        })
        .to_string())
    }

    pub async fn export_knowledge(&self, anchor_or_session: &str, export_type: &str) -> Result<String> {
        let session_id = anchor_or_session.trim_start_matches("anchor:");
        if export_type == "index" {
            let mut keys = Vec::new();
            for prefix in [
                format!("anchor:{session_id}"),
                format!("graph:{session_id}:"),
                format!("research:{session_id}:"),
                format!("memory:{session_id}:"),
                format!("conversation:{session_id}:"),
                format!("protocol:{session_id}:"),
                format!("observability:{session_id}:"),
                format!("business:work-order:{session_id}:"),
                format!("business:revenue-event:{session_id}:"),
                "replay:analysis:".to_string(),
            ] {
                keys.extend(
                    self.spacetimedb
                        .list_knowledge_by_prefix(&prefix)
                        .await?
                        .into_iter()
                        .map(|record| record.key),
                );
            }
            keys.sort();
            keys.dedup();
            return Ok(serde_json::to_string_pretty(&keys)?);
        }
        let key = match export_type {
            "graph" => format!("graph:{session_id}:snapshot"),
            "brief" => format!("anchor:{session_id}:brief"),
            "research" => format!("research:{session_id}:report"),
            "research-follow-up" => format!("research:{session_id}:follow-up-status"),
            "research-proxy" => format!("research:{session_id}:proxy-forensics"),
            "resilience" => format!("observability:{session_id}:resilience"),
            "business" => format!("observability:{session_id}:business-report"),
            "margin" => format!("observability:{session_id}:margin-report"),
            "sla" => format!("observability:{session_id}:sla-report"),
            "work-orders" => {
                let work_orders = self
                    .spacetimedb
                    .list_knowledge_by_prefix(&format!("business:work-order:{session_id}:"))
                    .await?
                    .into_iter()
                    .filter_map(|record| serde_json::from_str::<serde_json::Value>(&record.value).ok())
                    .collect::<Vec<_>>();
                return Ok(serde_json::to_string_pretty(&work_orders)?);
            }
            "revenue" => {
                let revenue_events = self
                    .spacetimedb
                    .list_knowledge_by_prefix(&format!("business:revenue-event:{session_id}:"))
                    .await?
                    .into_iter()
                    .filter_map(|record| serde_json::from_str::<serde_json::Value>(&record.value).ok())
                    .collect::<Vec<_>>();
                return Ok(serde_json::to_string_pretty(&revenue_events)?);
            }
            "strategy-layers" => {
                let value = self.memory.strategy_memory_layers(&self.spacetimedb, session_id).await?;
                return Ok(serde_json::to_string_pretty(&value)?);
            }
            "dashboard" => return self.export_dashboard_snapshot(session_id).await,
            "replay" => return self.export_session_replay(session_id).await,
            "replay-report" => return self.export_replay_report(session_id, None).await,
            "deliberation" => format!("conversation:{session_id}:deliberation"),
            "consolidation" => format!("memory:{session_id}:consolidation"),
            "self-evolution" => format!("memory:{session_id}:self-evolution"),
            "global-graph" => "graph:global:snapshot".into(),
            _ => format!("graph:{session_id}:snapshot"),
        };
        Ok(self
            .spacetimedb
            .get_knowledge(&key)
            .await?
            .map(|record| record.value)
            .unwrap_or_else(|| "{}".into()))
    }

    pub async fn system_status(&self) -> Result<String> {
        let report = self.bootstrap().await?;
        let anchor_count = self.list_focus_anchors().await?.len();
        let manifests = self.tools.manifests();
        let verified_count = manifests
            .iter()
            .filter(|manifest| manifest.approval_status == crate::tools::ApprovalStatus::Verified)
            .count();
        let active_count = manifests
            .iter()
            .filter(|manifest| manifest.status == crate::tools::CapabilityStatus::Active)
            .count();
        Ok(serde_json::json!({
            "app": report.app_name,
            "anchors": anchor_count,
            "providers": report.provider_count,
            "tools": report.tool_count,
            "hooks": report.hook_count,
            "memory_targets": report.memory_targets,
            "rag_strategies": report.rag_strategies,
            "deployment_profile": self.config.deployment.profile,
            "observability_enabled": self.config.observability.enabled,
            "catalog_capabilities": manifests.len(),
            "catalog_active": active_count,
            "catalog_verified": verified_count,
            "dashboard_snapshot_dir": self.runtime_artifact_dir("dashboard").to_string_lossy(),
            "session_replay_dir": self.runtime_artifact_dir("replay").to_string_lossy(),
        })
        .to_string())
    }

    pub fn plugin_list(&self) -> Result<String> {
        Ok(serde_json::json!({
            "builtin_tools": self.tools.names(),
            "forged_capabilities": self.tools.forged_tool_names(),
        })
        .to_string())
    }

    pub fn plugin_status(&self, plugin: &str) -> Result<String> {
        let matched = self
            .tools
            .manifests()
            .into_iter()
            .find(|manifest| manifest.registered_tool_name == plugin || manifest.capability_name == plugin);
        let verifier_ready = matched
            .as_ref()
            .map(|manifest| manifest.is_executable())
            .unwrap_or(false);
        Ok(serde_json::to_string_pretty(&serde_json::json!({
            "plugin": plugin,
            "known_builtin": self.tools.has_tool(plugin),
            "forged_capability": matched,
            "verifier_ready": verifier_ready,
        }))?)
    }

    fn persist_runtime_artifact(&self, category: &str, session_id: &str, body: &str) -> Result<()> {
        let directory = self.runtime_artifact_dir(category);
        fs::create_dir_all(&directory)?;
        fs::write(directory.join(format!("{session_id}.json")), body)?;
        Ok(())
    }

    fn runtime_artifact_dir(&self, category: &str) -> PathBuf {
        PathBuf::from("D:\\AutoLoop\\autoloop-app\\deploy\\runtime").join(category)
    }
}

fn session_key(session_id: &str) -> String {
    session_id
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .collect()
}

fn normalize_anchor(value: &str) -> String {
    if value.starts_with("anchor:") {
        value.to_string()
    } else {
        format!("anchor:{value}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::McpDispatchRequest;
    use autoloop_spacetimedb_adapter::PermissionAction;

    #[tokio::test]
    async fn bootstrap_default_config() {
        let app = AutoLoopApp::new(AppConfig::default());
        let report = app.bootstrap().await.expect("bootstrap");
        assert_eq!(report.app_name, "autoloop");
        assert!(report.provider_count >= 1);
    }

    #[tokio::test]
    async fn mcp_dispatch_event_calls_spacetimedb() {
        let app = AutoLoopApp::new(AppConfig::default());
        app.spacetimedb
            .grant_permissions("scheduler", vec![PermissionAction::Dispatch, PermissionAction::Write])
            .await
            .expect("grant");

        let event = app
            .runtime
            .dispatch_mcp_event(
                &app.spacetimedb,
                McpDispatchRequest {
                    session_id: "session-1".into(),
                    tool_name: "mcp::local-mcp::invoke".into(),
                    payload: "{\"job\":\"reindex\"}".into(),
                    actor_id: "scheduler".into(),
                },
            )
            .await
            .expect("dispatch");

        assert_eq!(event.topic, "mcp.dispatch");
        assert_eq!(event.status, "queued");
    }

    #[tokio::test]
    async fn single_agent_closed_loop_example() {
        let app = AutoLoopApp::new(AppConfig::default());
        app.bootstrap().await.expect("bootstrap");

        app.spacetimedb
            .grant_permissions(
                "agent-1",
                vec![
                    PermissionAction::Read,
                    PermissionAction::Write,
                    PermissionAction::Dispatch,
                ],
            )
            .await
            .expect("grant");

        let event = app
            .runtime
            .dispatch_mcp_event(
                &app.spacetimedb,
                McpDispatchRequest {
                    session_id: "agent-session".into(),
                    tool_name: "mcp::local-mcp::invoke".into(),
                    payload: "{\"anchor\":\"spacetimedb\"}".into(),
                    actor_id: "agent-1".into(),
                },
            )
            .await
            .expect("dispatch");

        let answer = app
            .process_direct("agent-session", "Explain how the spacetimedb anchor should be stored.")
            .await
            .expect("agent response");

        app.spacetimedb
            .upsert_agent_state(
                "agent-session".into(),
                "Explain how the spacetimedb anchor should be stored.".into(),
                Some(answer.clone()),
            )
            .await
            .expect("state upsert");

        app.spacetimedb
            .upsert_knowledge(
                "anchor:spacetimedb".into(),
                answer.clone(),
                "single-agent-test".into(),
            )
            .await
            .expect("knowledge upsert");
        app.spacetimedb
            .update_schedule_status(event.id, "completed")
            .await
            .expect("status update");

        let state = app
            .spacetimedb
            .get_agent_state("agent-session")
            .await
            .expect("state get")
            .expect("state exists");
        let record = app
            .spacetimedb
            .get_knowledge("anchor:spacetimedb")
            .await
            .expect("knowledge get")
            .expect("knowledge exists");
        let events = app
            .spacetimedb
            .list_schedule_events("agent-session")
            .await
            .expect("events");

        assert_eq!(state.session_id, "agent-session");
        assert!(record.value.contains("spacetimedb"));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].status, "completed");
    }

    #[tokio::test]
    async fn requirement_swarm_persists_knowledge_records() {
        let app = AutoLoopApp::new(AppConfig::default());
        app.spacetimedb
            .grant_permissions(
                "swarm-session",
                vec![
                    PermissionAction::Read,
                    PermissionAction::Write,
                    PermissionAction::Dispatch,
                ],
            )
            .await
            .expect("grant");

        let summary = app
            .process_requirement_swarm(
                "swarm-session",
                "Need a CEO agent that forms a swarm and stores all discussion in graph memory with MCP execution.",
            )
            .await
            .expect("swarm");

        let brief = app
            .spacetimedb
            .get_knowledge("conversation:swarm-session:brief")
            .await
            .expect("brief")
            .expect("brief exists");
        let graph = app
            .spacetimedb
            .get_knowledge("graph:swarm-session:snapshot")
            .await
            .expect("graph")
            .expect("graph exists");
        let dashboard = app
            .spacetimedb
            .get_knowledge("observability:swarm-session:dashboard")
            .await
            .expect("dashboard")
            .expect("dashboard exists");
        let deliberation = app
            .spacetimedb
            .get_knowledge("conversation:swarm-session:deliberation")
            .await
            .expect("deliberation")
            .expect("deliberation exists");
        let research = app
            .spacetimedb
            .get_knowledge("research:swarm-session:report")
            .await
            .expect("research")
            .expect("research exists");
        let consolidation = app
            .spacetimedb
            .get_knowledge("memory:swarm-session:consolidation")
            .await
            .expect("consolidation")
            .expect("consolidation exists");
        let evolution = app
            .spacetimedb
            .get_knowledge("memory:swarm-session:self-evolution")
            .await
            .expect("evolution")
            .expect("evolution exists");
        let research_follow_up = app
            .spacetimedb
            .get_knowledge("research:swarm-session:follow-up-status")
            .await
            .expect("research follow-up")
            .expect("research follow-up exists");
        let stats = app
            .spacetimedb
            .get_knowledge("metrics:execution:mcp:local-mcp")
            .await
            .expect("stats");

        assert!(summary.contains("CEO"));
        assert!(brief.value.contains("clarified_goal"));
        assert!(graph.value.contains("entities"));
        assert!(dashboard.value.contains("route_analytics"));
        assert!(deliberation.value.contains("planner_notes"));
        assert!(research.value.contains("autonomy_score"));
        assert!(consolidation.value.contains("capability_improvements"));
        assert!(evolution.value.contains("evolved_score"));
        assert!(research_follow_up.value.contains("scheduled_tasks"));
        assert!(stats.is_some());
    }

    #[tokio::test]
    async fn requirement_swarm_persists_structured_execution_feedback() {
        let app = AutoLoopApp::new(AppConfig::default());
        app.spacetimedb
            .grant_permissions(
                "routing-session",
                vec![
                    PermissionAction::Read,
                    PermissionAction::Write,
                    PermissionAction::Dispatch,
                ],
            )
            .await
            .expect("grant");

        app.process_requirement_swarm(
            "routing-session",
            "Use MCP execution and graph memory to plan and execute the swarm.",
        )
        .await
        .expect("swarm");

        let feedback = app
            .spacetimedb
            .list_knowledge_by_prefix("conversation:routing-session:execution-feedback:")
            .await
            .expect("feedback");
        let stats = app
            .spacetimedb
            .get_knowledge("metrics:execution:mcp:local-mcp")
            .await
            .expect("stats")
            .expect("stats exist");

        assert!(!feedback.is_empty());
        assert!(feedback.iter().any(|record| record.value.contains("\"mcp_server\":\"local-mcp\"")));
        assert!(stats.value.contains("\"attempts\":"));
        assert!(stats.value.contains("\"success_rate\":"));
    }
}
