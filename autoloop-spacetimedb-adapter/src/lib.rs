use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        Mutex as StdMutex,
    },
};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use tokio::task;
use tokio::sync::RwLock;

#[path = "../../src/module_bindings/generated/mod.rs"]
pub mod generated_bindings;

pub mod sdk {
    use anyhow::Result;
    use spacetimedb_sdk::{DbContext, Table};

    use super::{
        AgentState,
        CausalEdgeRecord,
        KnowledgeRecord,
        LearningEventKind,
        LearningSessionRecord,
        PermissionAction,
        PermissionGrant,
        ReflexionEpisodeRecord,
        ScheduleEvent,
        SkillLibraryRecord,
        SpacetimeDbConfig,
        WitnessLogRecord,
        generated_bindings::{
            self,
            AgentStateTableAccess,
            CausalEdgeRecordTableAccess,
            DbConnection,
            KnowledgeRecordTableAccess,
            LearningEventKind as GeneratedLearningEventKind,
            LearningSessionRecordTableAccess,
            PermissionAction as GeneratedPermissionAction,
            PermissionGrantTableAccess,
            ReflexionEpisodeTableAccess,
            ScheduleEventTableAccess,
            SkillLibraryRecordTableAccess,
            WitnessLogRecordTableAccess,
            append_witness_log_record as AppendWitnessLogRecordExt,
            create_schedule_event as CreateScheduleEventExt,
            grant_permissions as GrantPermissionsExt,
            upsert_causal_edge_record as UpsertCausalEdgeRecordExt,
            upsert_learning_session_record as UpsertLearningSessionRecordExt,
            update_schedule_status as UpdateScheduleStatusExt,
            upsert_agent_state as UpsertAgentStateExt,
            upsert_knowledge as UpsertKnowledgeExt,
            upsert_reflexion_episode as UpsertReflexionEpisodeExt,
            upsert_skill_library_record as UpsertSkillLibraryRecordExt,
        },
    };

    pub struct GeneratedModuleClient {
        connection: DbConnection,
    }

    impl GeneratedModuleClient {
        pub fn connect(config: &SpacetimeDbConfig) -> Result<Self> {
            let connection = DbConnection::builder()
                .with_uri(config.uri.clone())
                .with_database_name(config.module_name.clone())
                .build()?;

            Ok(Self { connection })
        }

        pub fn connection(&self) -> &DbConnection {
            &self.connection
        }

        pub fn subscribe_all_tables(&self) {
            self.connection.subscription_builder().subscribe_to_all_tables();
        }

        pub fn frame_tick(&self) -> Result<()> {
            self.connection.frame_tick()?;
            Ok(())
        }

        pub fn grant_permissions(
            &self,
            actor_id: impl Into<String>,
            permissions: Vec<PermissionAction>,
        ) -> Result<()> {
            self.connection.reducers.grant_permissions(
                actor_id.into(),
                permissions.into_iter().map(into_generated_permission).collect(),
            )?;
            Ok(())
        }

        pub fn create_schedule_event(&self, event: ScheduleEvent) -> Result<()> {
            self.connection.reducers.create_schedule_event(
                event.id,
                event.session_id,
                event.topic,
                event.tool_name,
                event.payload,
                event.actor_id,
                event.status,
            )?;
            Ok(())
        }

        pub fn update_schedule_status(&self, event_id: u64, status: impl Into<String>) -> Result<()> {
            self.connection
                .reducers
                .update_schedule_status(event_id, status.into())?;
            Ok(())
        }

        pub fn upsert_agent_state(&self, state: AgentState) -> Result<()> {
            self.connection.reducers.upsert_agent_state(
                state.session_id,
                state.last_user_message,
                state.last_assistant_message,
            )?;
            Ok(())
        }

        pub fn upsert_knowledge(&self, record: KnowledgeRecord) -> Result<()> {
            self.connection
                .reducers
                .upsert_knowledge(record.key, record.value, record.source)?;
            Ok(())
        }

        pub fn list_schedule_events(&self) -> Vec<ScheduleEvent> {
            self.connection
                .db
                .schedule_event()
                .iter()
                .map(|row| ScheduleEvent {
                    id: row.id,
                    session_id: row.session_id,
                    topic: row.topic,
                    tool_name: row.tool_name,
                    payload: row.payload,
                    actor_id: row.actor_id,
                    status: row.status,
                })
                .collect()
        }

        pub fn get_agent_state(&self, session_id: &str) -> Option<AgentState> {
            self.connection.db.agent_state().session_id().find(&session_id.to_string()).map(
                |row| AgentState {
                    id: 0,
                    session_id: row.session_id,
                    last_user_message: row.last_user_message,
                    last_assistant_message: row.last_assistant_message,
                },
            )
        }

        pub fn get_knowledge(&self, key: &str) -> Option<KnowledgeRecord> {
            self.connection.db.knowledge_record().key().find(&key.to_string()).map(|row| {
                KnowledgeRecord {
                    id: 0,
                    key: row.key,
                    value: row.value,
                    source: row.source,
                }
            })
        }

        pub fn list_knowledge_by_prefix(&self, prefix: &str) -> Vec<KnowledgeRecord> {
            self.connection
                .db
                .knowledge_record()
                .iter()
                .filter(|row| row.key.starts_with(prefix))
                .map(|row| KnowledgeRecord {
                    id: 0,
                    key: row.key,
                    value: row.value,
                    source: row.source,
                })
                .collect()
        }

        pub fn get_permission_grant(&self, actor_id: &str) -> Option<PermissionGrant> {
            self.connection
                .db
                .permission_grant()
                .actor_id()
                .find(&actor_id.to_string())
                .map(|row| PermissionGrant {
                    actor_id: row.actor_id,
                    permissions: row
                        .permissions
                        .into_iter()
                        .map(from_generated_permission)
                        .collect(),
                })
        }

        pub fn upsert_reflexion_episode(&self, record: ReflexionEpisodeRecord) -> Result<()> {
            self.connection.reducers.upsert_reflexion_episode(
                record.id,
                record.session_id,
                record.objective,
                record.hypothesis,
                record.outcome,
                record.lesson,
                record.status,
                record.score,
                record.created_at_ms,
            )?;
            Ok(())
        }

        pub fn list_reflexion_episodes(&self) -> Vec<ReflexionEpisodeRecord> {
            self.connection
                .db
                .reflexion_episode()
                .iter()
                .map(|row| ReflexionEpisodeRecord {
                    id: row.id,
                    session_id: row.session_id,
                    objective: row.objective,
                    hypothesis: row.hypothesis,
                    outcome: row.outcome,
                    lesson: row.lesson,
                    status: row.status,
                    score: row.score,
                    created_at_ms: row.created_at_ms,
                })
                .collect()
        }

        pub fn upsert_skill_library_record(&self, record: SkillLibraryRecord) -> Result<()> {
            self.connection.reducers.upsert_skill_library_record(
                record.id,
                record.session_id,
                record.name,
                record.trigger,
                record.procedure,
                record.confidence,
                record.success_rate,
                record.evidence_count,
                record.created_at_ms,
                record.updated_at_ms,
            )?;
            Ok(())
        }

        pub fn list_skill_library_records(&self) -> Vec<SkillLibraryRecord> {
            self.connection
                .db
                .skill_library_record()
                .iter()
                .map(|row| SkillLibraryRecord {
                    id: row.id,
                    session_id: row.session_id,
                    name: row.name,
                    trigger: row.trigger,
                    procedure: row.procedure,
                    confidence: row.confidence,
                    success_rate: row.success_rate,
                    evidence_count: row.evidence_count,
                    created_at_ms: row.created_at_ms,
                    updated_at_ms: row.updated_at_ms,
                })
                .collect()
        }

        pub fn upsert_causal_edge_record(&self, record: CausalEdgeRecord) -> Result<()> {
            self.connection.reducers.upsert_causal_edge_record(
                record.id,
                record.session_id,
                record.cause,
                record.effect,
                record.evidence,
                record.strength,
                record.confidence,
                record.created_at_ms,
            )?;
            Ok(())
        }

        pub fn list_causal_edge_records(&self) -> Vec<CausalEdgeRecord> {
            self.connection
                .db
                .causal_edge_record()
                .iter()
                .map(|row| CausalEdgeRecord {
                    id: row.id,
                    session_id: row.session_id,
                    cause: row.cause,
                    effect: row.effect,
                    evidence: row.evidence,
                    strength: row.strength,
                    confidence: row.confidence,
                    created_at_ms: row.created_at_ms,
                })
                .collect()
        }

        pub fn upsert_learning_session_record(&self, record: LearningSessionRecord) -> Result<()> {
            self.connection.reducers.upsert_learning_session_record(
                record.id,
                record.session_id,
                record.objective,
                record.status,
                record.priority,
                record.summary,
                record.started_at_ms,
                record.completed_at_ms,
            )?;
            Ok(())
        }

        pub fn list_learning_session_records(&self) -> Vec<LearningSessionRecord> {
            self.connection
                .db
                .learning_session_record()
                .iter()
                .map(|row| LearningSessionRecord {
                    id: row.id,
                    session_id: row.session_id,
                    objective: row.objective,
                    status: row.status,
                    priority: row.priority,
                    summary: row.summary,
                    started_at_ms: row.started_at_ms,
                    completed_at_ms: row.completed_at_ms,
                })
                .collect()
        }

        pub fn append_witness_log_record(&self, record: WitnessLogRecord) -> Result<()> {
            self.connection.reducers.append_witness_log_record(
                record.id,
                record.session_id,
                into_generated_learning_event(record.event_type),
                record.source,
                record.detail,
                record.score,
                record.created_at_ms,
                record.metadata_json,
            )?;
            Ok(())
        }

        pub fn list_witness_log_records(&self) -> Vec<WitnessLogRecord> {
            self.connection
                .db
                .witness_log_record()
                .iter()
                .map(|row| WitnessLogRecord {
                    id: row.id,
                    session_id: row.session_id,
                    event_type: from_generated_learning_event(row.event_type),
                    source: row.source,
                    detail: row.detail,
                    score: row.score,
                    created_at_ms: row.created_at_ms,
                    metadata_json: row.metadata_json,
                })
                .collect()
        }
    }

    fn into_generated_permission(action: PermissionAction) -> GeneratedPermissionAction {
        match action {
            PermissionAction::Read => GeneratedPermissionAction::Read,
            PermissionAction::Write => GeneratedPermissionAction::Write,
            PermissionAction::Dispatch => GeneratedPermissionAction::Dispatch,
            PermissionAction::Admin => GeneratedPermissionAction::Admin,
        }
    }

    fn from_generated_permission(action: generated_bindings::PermissionAction) -> PermissionAction {
        match action {
            generated_bindings::PermissionAction::Read => PermissionAction::Read,
            generated_bindings::PermissionAction::Write => PermissionAction::Write,
            generated_bindings::PermissionAction::Dispatch => PermissionAction::Dispatch,
            generated_bindings::PermissionAction::Admin => PermissionAction::Admin,
        }
    }

    fn into_generated_learning_event(action: LearningEventKind) -> GeneratedLearningEventKind {
        match action {
            LearningEventKind::Failure => GeneratedLearningEventKind::Failure,
            LearningEventKind::Success => GeneratedLearningEventKind::Success,
            LearningEventKind::ToolCall => GeneratedLearningEventKind::ToolCall,
            LearningEventKind::RouteDecision => GeneratedLearningEventKind::RouteDecision,
            LearningEventKind::Audit => GeneratedLearningEventKind::Audit,
        }
    }

    fn from_generated_learning_event(action: GeneratedLearningEventKind) -> LearningEventKind {
        match action {
            GeneratedLearningEventKind::Failure => LearningEventKind::Failure,
            GeneratedLearningEventKind::Success => LearningEventKind::Success,
            GeneratedLearningEventKind::ToolCall => LearningEventKind::ToolCall,
            GeneratedLearningEventKind::RouteDecision => LearningEventKind::RouteDecision,
            GeneratedLearningEventKind::Audit => LearningEventKind::Audit,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpacetimeBackend {
    InMemory,
    Sdk,
}

fn default_backend() -> SpacetimeBackend {
    SpacetimeBackend::InMemory
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpacetimeDbConfig {
    pub enabled: bool,
    #[serde(default = "default_backend")]
    pub backend: SpacetimeBackend,
    pub uri: String,
    pub module_name: String,
    pub namespace: String,
    pub pool_size: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PermissionAction {
    Read,
    Write,
    Dispatch,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LearningEventKind {
    Failure,
    Success,
    ToolCall,
    RouteDecision,
    Audit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleEvent {
    pub id: u64,
    pub session_id: String,
    pub topic: String,
    pub tool_name: String,
    pub payload: String,
    pub actor_id: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    pub id: u64,
    pub session_id: String,
    pub last_user_message: String,
    pub last_assistant_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeRecord {
    pub id: u64,
    pub key: String,
    pub value: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionGrant {
    pub actor_id: String,
    pub permissions: Vec<PermissionAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tenant {
    pub tenant_id: String,
    pub name: String,
    pub status: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Principal {
    pub principal_id: String,
    pub tenant_id: String,
    pub principal_type: String,
    pub status: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleBinding {
    pub tenant_id: String,
    pub principal_id: String,
    pub role: String,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyBinding {
    pub policy_id: String,
    pub tenant_id: String,
    pub role: String,
    pub allowed_actions: Vec<PermissionAction>,
    pub capability_prefixes: Vec<String>,
    pub max_memory_mb: u32,
    pub max_tokens: u32,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionLease {
    pub lease_token: String,
    pub session_id: String,
    pub tenant_id: String,
    pub principal_id: String,
    pub policy_id: String,
    pub expires_at_ms: u64,
    pub issued_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetAccount {
    pub account_id: String,
    pub tenant_id: String,
    pub principal_id: String,
    pub policy_id: String,
    pub total_budget_micros: u64,
    pub reserved_micros: u64,
    pub spent_micros: u64,
    pub blocked_count: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SpendLedgerKind {
    Reserve,
    Settle,
    Refund,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpendLedger {
    pub ledger_id: String,
    pub tenant_id: String,
    pub account_id: String,
    pub session_id: String,
    pub trace_id: String,
    pub task_id: String,
    pub capability_id: String,
    pub kind: SpendLedgerKind,
    pub amount_micros: i64,
    pub token_cost_micros: u64,
    pub tool_cost_micros: u64,
    pub duration_cost_micros: u64,
    pub reason: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaWindow {
    pub window_id: String,
    pub tenant_id: String,
    pub account_id: String,
    pub window_start_ms: u64,
    pub window_end_ms: u64,
    pub window_budget_micros: u64,
    pub consumed_micros: u64,
    pub blocked_count: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostAttribution {
    pub attribution_id: String,
    pub tenant_id: String,
    pub principal_id: String,
    pub policy_id: String,
    pub session_id: String,
    pub trace_id: String,
    pub task_id: String,
    pub capability_id: String,
    pub provider_tokens: u32,
    pub tool_invocations: u32,
    pub duration_ms: u64,
    pub token_cost_micros: u64,
    pub tool_cost_micros: u64,
    pub duration_cost_micros: u64,
    pub total_cost_micros: u64,
    pub settled_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflexionEpisodeRecord {
    pub id: String,
    pub session_id: String,
    pub objective: String,
    pub hypothesis: String,
    pub outcome: String,
    pub lesson: String,
    pub status: String,
    pub score: f32,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillLibraryRecord {
    pub id: String,
    pub session_id: String,
    pub name: String,
    pub trigger: String,
    pub procedure: String,
    pub confidence: f32,
    pub success_rate: f32,
    pub evidence_count: u32,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalEdgeRecord {
    pub id: String,
    pub session_id: String,
    pub cause: String,
    pub effect: String,
    pub evidence: String,
    pub strength: f32,
    pub confidence: f32,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningSessionRecord {
    pub id: String,
    pub session_id: String,
    pub objective: String,
    pub status: String,
    pub priority: f32,
    pub summary: String,
    pub started_at_ms: u64,
    pub completed_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitnessLogRecord {
    pub id: String,
    pub session_id: String,
    pub event_type: LearningEventKind,
    pub source: String,
    pub detail: String,
    pub score: f32,
    pub created_at_ms: u64,
    pub metadata_json: String,
}

#[async_trait::async_trait]
pub trait SpacetimeRepository: Send + Sync {
    async fn create_schedule_event(
        &self,
        session_id: String,
        topic: String,
        tool_name: String,
        payload: String,
        actor_id: String,
    ) -> Result<ScheduleEvent>;
    async fn update_schedule_status(&self, event_id: u64, status: String) -> Result<()>;
    async fn list_schedule_events(&self, session_id: &str) -> Result<Vec<ScheduleEvent>>;
    async fn upsert_agent_state(
        &self,
        session_id: String,
        last_user_message: String,
        last_assistant_message: Option<String>,
    ) -> Result<AgentState>;
    async fn get_agent_state(&self, session_id: &str) -> Result<Option<AgentState>>;
    async fn upsert_knowledge(
        &self,
        key: String,
        value: String,
        source: String,
    ) -> Result<KnowledgeRecord>;
    async fn get_knowledge(&self, key: &str) -> Result<Option<KnowledgeRecord>>;
    async fn list_knowledge_by_prefix(&self, prefix: &str) -> Result<Vec<KnowledgeRecord>>;
    async fn grant_permissions(
        &self,
        actor_id: String,
        permissions: Vec<PermissionAction>,
    ) -> Result<PermissionGrant>;
    async fn has_permission(&self, actor_id: &str, action: PermissionAction) -> Result<bool>;
    async fn upsert_reflexion_episode(
        &self,
        record: ReflexionEpisodeRecord,
    ) -> Result<ReflexionEpisodeRecord>;
    async fn list_reflexion_episodes(&self, session_id: &str) -> Result<Vec<ReflexionEpisodeRecord>>;
    async fn upsert_skill_library_record(
        &self,
        record: SkillLibraryRecord,
    ) -> Result<SkillLibraryRecord>;
    async fn list_skill_library_records(&self, session_id: &str) -> Result<Vec<SkillLibraryRecord>>;
    async fn upsert_causal_edge_record(
        &self,
        record: CausalEdgeRecord,
    ) -> Result<CausalEdgeRecord>;
    async fn list_causal_edge_records(&self, session_id: &str) -> Result<Vec<CausalEdgeRecord>>;
    async fn upsert_learning_session_record(
        &self,
        record: LearningSessionRecord,
    ) -> Result<LearningSessionRecord>;
    async fn list_learning_session_records(
        &self,
        session_id: &str,
    ) -> Result<Vec<LearningSessionRecord>>;
    async fn append_witness_log_record(
        &self,
        record: WitnessLogRecord,
    ) -> Result<WitnessLogRecord>;
    async fn list_witness_log_records(&self, session_id: &str) -> Result<Vec<WitnessLogRecord>>;
}

#[derive(Default)]
struct InMemoryState {
    events: HashMap<u64, ScheduleEvent>,
    events_by_session: HashMap<String, Vec<u64>>,
    agent_state: HashMap<String, AgentState>,
    knowledge: HashMap<String, KnowledgeRecord>,
    permissions: HashMap<String, HashSet<PermissionAction>>,
    reflexion_episodes: HashMap<String, ReflexionEpisodeRecord>,
    skills: HashMap<String, SkillLibraryRecord>,
    causal_edges: HashMap<String, CausalEdgeRecord>,
    learning_sessions: HashMap<String, LearningSessionRecord>,
    witness_logs: HashMap<String, WitnessLogRecord>,
}

#[derive(Default)]
pub struct InMemorySpacetimeRepository {
    state: Arc<RwLock<InMemoryState>>,
    next_id: AtomicU64,
}

impl InMemorySpacetimeRepository {
    fn alloc_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst) + 1
    }
}

pub struct SdkSpacetimeRepository {
    client: Arc<StdMutex<sdk::GeneratedModuleClient>>,
    next_event_id: AtomicU64,
}

impl SdkSpacetimeRepository {
    pub fn connect(config: &SpacetimeDbConfig) -> Result<Self> {
        let client = sdk::GeneratedModuleClient::connect(config)?;
        client.subscribe_all_tables();
        let _ = client.frame_tick();

        Ok(Self {
            client: Arc::new(StdMutex::new(client)),
            next_event_id: AtomicU64::new(0),
        })
    }

    fn alloc_event_id(&self) -> u64 {
        self.next_event_id.fetch_add(1, Ordering::SeqCst) + 1
    }

    async fn with_client<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut sdk::GeneratedModuleClient) -> Result<T> + Send + 'static,
    {
        let client = Arc::clone(&self.client);
        task::spawn_blocking(move || {
            let mut guard = client.lock().expect("spacetimedb sdk client mutex poisoned");
            f(&mut guard)
        })
        .await
        .map_err(|error| anyhow::anyhow!("spacetimedb sdk task join error: {error}"))?
    }
}

#[async_trait::async_trait]
impl SpacetimeRepository for InMemorySpacetimeRepository {
    async fn create_schedule_event(
        &self,
        session_id: String,
        topic: String,
        tool_name: String,
        payload: String,
        actor_id: String,
    ) -> Result<ScheduleEvent> {
        let id = self.alloc_id();
        let event = ScheduleEvent {
            id,
            session_id: session_id.clone(),
            topic,
            tool_name,
            payload,
            actor_id,
            status: "queued".into(),
        };

        let mut state = self.state.write().await;
        state.events.insert(id, event.clone());
        state.events_by_session.entry(session_id).or_default().push(id);
        Ok(event)
    }

    async fn update_schedule_status(&self, event_id: u64, status: String) -> Result<()> {
        let mut state = self.state.write().await;
        let event = state
            .events
            .get_mut(&event_id)
            .ok_or_else(|| anyhow::anyhow!("event {event_id} not found"))?;
        event.status = status;
        Ok(())
    }

    async fn list_schedule_events(&self, session_id: &str) -> Result<Vec<ScheduleEvent>> {
        let state = self.state.read().await;
        let ids = state.events_by_session.get(session_id).cloned().unwrap_or_default();
        Ok(ids
            .into_iter()
            .filter_map(|id| state.events.get(&id).cloned())
            .collect())
    }

    async fn upsert_agent_state(
        &self,
        session_id: String,
        last_user_message: String,
        last_assistant_message: Option<String>,
    ) -> Result<AgentState> {
        let mut state = self.state.write().await;
        let id = state
            .agent_state
            .get(&session_id)
            .map(|current| current.id)
            .unwrap_or_else(|| self.alloc_id());
        let snapshot = AgentState {
            id,
            session_id: session_id.clone(),
            last_user_message,
            last_assistant_message,
        };
        state.agent_state.insert(session_id, snapshot.clone());
        Ok(snapshot)
    }

    async fn get_agent_state(&self, session_id: &str) -> Result<Option<AgentState>> {
        let state = self.state.read().await;
        Ok(state.agent_state.get(session_id).cloned())
    }

    async fn upsert_knowledge(
        &self,
        key: String,
        value: String,
        source: String,
    ) -> Result<KnowledgeRecord> {
        let mut state = self.state.write().await;
        let id = state
            .knowledge
            .get(&key)
            .map(|current| current.id)
            .unwrap_or_else(|| self.alloc_id());
        let record = KnowledgeRecord {
            id,
            key: key.clone(),
            value,
            source,
        };
        state.knowledge.insert(key, record.clone());
        Ok(record)
    }

    async fn get_knowledge(&self, key: &str) -> Result<Option<KnowledgeRecord>> {
        let state = self.state.read().await;
        Ok(state.knowledge.get(key).cloned())
    }

    async fn list_knowledge_by_prefix(&self, prefix: &str) -> Result<Vec<KnowledgeRecord>> {
        let state = self.state.read().await;
        let mut records = state
            .knowledge
            .values()
            .filter(|record| record.key.starts_with(prefix))
            .cloned()
            .collect::<Vec<_>>();
        records.sort_by(|left, right| left.key.cmp(&right.key));
        Ok(records)
    }

    async fn grant_permissions(
        &self,
        actor_id: String,
        permissions: Vec<PermissionAction>,
    ) -> Result<PermissionGrant> {
        let mut state = self.state.write().await;
        let entry = state.permissions.entry(actor_id.clone()).or_default();
        for permission in &permissions {
            entry.insert(*permission);
        }
        Ok(PermissionGrant {
            actor_id,
            permissions: entry.iter().copied().collect(),
        })
    }

    async fn has_permission(&self, actor_id: &str, action: PermissionAction) -> Result<bool> {
        let state = self.state.read().await;
        Ok(state.permissions.get(actor_id).is_some_and(|grants| {
            grants.contains(&action) || grants.contains(&PermissionAction::Admin)
        }))
    }

    async fn upsert_reflexion_episode(
        &self,
        record: ReflexionEpisodeRecord,
    ) -> Result<ReflexionEpisodeRecord> {
        let mut state = self.state.write().await;
        state
            .reflexion_episodes
            .insert(record.id.clone(), record.clone());
        Ok(record)
    }

    async fn list_reflexion_episodes(&self, session_id: &str) -> Result<Vec<ReflexionEpisodeRecord>> {
        let state = self.state.read().await;
        Ok(state
            .reflexion_episodes
            .values()
            .filter(|record| record.session_id == session_id)
            .cloned()
            .collect())
    }

    async fn upsert_skill_library_record(
        &self,
        record: SkillLibraryRecord,
    ) -> Result<SkillLibraryRecord> {
        let mut state = self.state.write().await;
        state.skills.insert(record.id.clone(), record.clone());
        Ok(record)
    }

    async fn list_skill_library_records(&self, session_id: &str) -> Result<Vec<SkillLibraryRecord>> {
        let state = self.state.read().await;
        Ok(state
            .skills
            .values()
            .filter(|record| record.session_id == session_id)
            .cloned()
            .collect())
    }

    async fn upsert_causal_edge_record(
        &self,
        record: CausalEdgeRecord,
    ) -> Result<CausalEdgeRecord> {
        let mut state = self.state.write().await;
        state
            .causal_edges
            .insert(record.id.clone(), record.clone());
        Ok(record)
    }

    async fn list_causal_edge_records(&self, session_id: &str) -> Result<Vec<CausalEdgeRecord>> {
        let state = self.state.read().await;
        Ok(state
            .causal_edges
            .values()
            .filter(|record| record.session_id == session_id)
            .cloned()
            .collect())
    }

    async fn upsert_learning_session_record(
        &self,
        record: LearningSessionRecord,
    ) -> Result<LearningSessionRecord> {
        let mut state = self.state.write().await;
        state
            .learning_sessions
            .insert(record.id.clone(), record.clone());
        Ok(record)
    }

    async fn list_learning_session_records(
        &self,
        session_id: &str,
    ) -> Result<Vec<LearningSessionRecord>> {
        let state = self.state.read().await;
        Ok(state
            .learning_sessions
            .values()
            .filter(|record| record.session_id == session_id)
            .cloned()
            .collect())
    }

    async fn append_witness_log_record(
        &self,
        record: WitnessLogRecord,
    ) -> Result<WitnessLogRecord> {
        let mut state = self.state.write().await;
        state
            .witness_logs
            .insert(record.id.clone(), record.clone());
        Ok(record)
    }

    async fn list_witness_log_records(&self, session_id: &str) -> Result<Vec<WitnessLogRecord>> {
        let state = self.state.read().await;
        Ok(state
            .witness_logs
            .values()
            .filter(|record| record.session_id == session_id)
            .cloned()
            .collect())
    }
}

#[async_trait::async_trait]
impl SpacetimeRepository for SdkSpacetimeRepository {
    async fn create_schedule_event(
        &self,
        session_id: String,
        topic: String,
        tool_name: String,
        payload: String,
        actor_id: String,
    ) -> Result<ScheduleEvent> {
        let event = ScheduleEvent {
            id: self.alloc_event_id(),
            session_id,
            topic,
            tool_name,
            payload,
            actor_id,
            status: "queued".into(),
        };

        let event_for_write = event.clone();
        self.with_client(move |client| {
            client.create_schedule_event(event_for_write)?;
            let _ = client.frame_tick();
            Ok(())
        })
        .await?;

        Ok(event)
    }

    async fn update_schedule_status(&self, event_id: u64, status: String) -> Result<()> {
        self.with_client(move |client| {
            client.update_schedule_status(event_id, status)?;
            let _ = client.frame_tick();
            Ok(())
        })
        .await
    }

    async fn list_schedule_events(&self, session_id: &str) -> Result<Vec<ScheduleEvent>> {
        let session_id = session_id.to_string();
        self.with_client(move |client| {
            let _ = client.frame_tick();
            Ok(client
                .list_schedule_events()
                .into_iter()
                .filter(|event| event.session_id == session_id)
                .collect())
        })
        .await
    }

    async fn upsert_agent_state(
        &self,
        session_id: String,
        last_user_message: String,
        last_assistant_message: Option<String>,
    ) -> Result<AgentState> {
        let snapshot = AgentState {
            id: 0,
            session_id,
            last_user_message,
            last_assistant_message,
        };
        let snapshot_for_write = snapshot.clone();

        self.with_client(move |client| {
            client.upsert_agent_state(snapshot_for_write)?;
            let _ = client.frame_tick();
            Ok(())
        })
        .await?;

        Ok(snapshot)
    }

    async fn get_agent_state(&self, session_id: &str) -> Result<Option<AgentState>> {
        let session_id = session_id.to_string();
        self.with_client(move |client| {
            let _ = client.frame_tick();
            Ok(client.get_agent_state(&session_id))
        })
        .await
    }

    async fn upsert_knowledge(
        &self,
        key: String,
        value: String,
        source: String,
    ) -> Result<KnowledgeRecord> {
        let record = KnowledgeRecord {
            id: 0,
            key,
            value,
            source,
        };
        let record_for_write = record.clone();

        self.with_client(move |client| {
            client.upsert_knowledge(record_for_write)?;
            let _ = client.frame_tick();
            Ok(())
        })
        .await?;

        Ok(record)
    }

    async fn get_knowledge(&self, key: &str) -> Result<Option<KnowledgeRecord>> {
        let key = key.to_string();
        self.with_client(move |client| {
            let _ = client.frame_tick();
            Ok(client.get_knowledge(&key))
        })
        .await
    }

    async fn list_knowledge_by_prefix(&self, prefix: &str) -> Result<Vec<KnowledgeRecord>> {
        let prefix = prefix.to_string();
        self.with_client(move |client| {
            let _ = client.frame_tick();
            Ok(client.list_knowledge_by_prefix(&prefix))
        })
        .await
    }

    async fn grant_permissions(
        &self,
        actor_id: String,
        permissions: Vec<PermissionAction>,
    ) -> Result<PermissionGrant> {
        let permissions_for_result = permissions.clone();
        self.with_client(move |client| {
            client.grant_permissions(actor_id.clone(), permissions)?;
            let _ = client.frame_tick();
            Ok(PermissionGrant {
                actor_id,
                permissions: permissions_for_result,
            })
        })
        .await
    }

    async fn has_permission(&self, actor_id: &str, action: PermissionAction) -> Result<bool> {
        let actor_id = actor_id.to_string();
        self.with_client(move |client| {
            let _ = client.frame_tick();
            Ok(client.get_permission_grant(&actor_id).is_some_and(|grant| {
                grant.permissions.contains(&action) || grant.permissions.contains(&PermissionAction::Admin)
            }))
        })
        .await
    }

    async fn upsert_reflexion_episode(
        &self,
        record: ReflexionEpisodeRecord,
    ) -> Result<ReflexionEpisodeRecord> {
        let value = record.clone();
        self.with_client(move |client| {
            client.upsert_reflexion_episode(value)?;
            let _ = client.frame_tick();
            Ok(())
        })
        .await?;
        Ok(record)
    }

    async fn list_reflexion_episodes(&self, session_id: &str) -> Result<Vec<ReflexionEpisodeRecord>> {
        let session_id = session_id.to_string();
        self.with_client(move |client| {
            let _ = client.frame_tick();
            Ok(client
                .list_reflexion_episodes()
                .into_iter()
                .filter(|record| record.session_id == session_id)
                .collect())
        })
        .await
    }

    async fn upsert_skill_library_record(
        &self,
        record: SkillLibraryRecord,
    ) -> Result<SkillLibraryRecord> {
        let value = record.clone();
        self.with_client(move |client| {
            client.upsert_skill_library_record(value)?;
            let _ = client.frame_tick();
            Ok(())
        })
        .await?;
        Ok(record)
    }

    async fn list_skill_library_records(&self, session_id: &str) -> Result<Vec<SkillLibraryRecord>> {
        let session_id = session_id.to_string();
        self.with_client(move |client| {
            let _ = client.frame_tick();
            Ok(client
                .list_skill_library_records()
                .into_iter()
                .filter(|record| record.session_id == session_id)
                .collect())
        })
        .await
    }

    async fn upsert_causal_edge_record(
        &self,
        record: CausalEdgeRecord,
    ) -> Result<CausalEdgeRecord> {
        let value = record.clone();
        self.with_client(move |client| {
            client.upsert_causal_edge_record(value)?;
            let _ = client.frame_tick();
            Ok(())
        })
        .await?;
        Ok(record)
    }

    async fn list_causal_edge_records(&self, session_id: &str) -> Result<Vec<CausalEdgeRecord>> {
        let session_id = session_id.to_string();
        self.with_client(move |client| {
            let _ = client.frame_tick();
            Ok(client
                .list_causal_edge_records()
                .into_iter()
                .filter(|record| record.session_id == session_id)
                .collect())
        })
        .await
    }

    async fn upsert_learning_session_record(
        &self,
        record: LearningSessionRecord,
    ) -> Result<LearningSessionRecord> {
        let value = record.clone();
        self.with_client(move |client| {
            client.upsert_learning_session_record(value)?;
            let _ = client.frame_tick();
            Ok(())
        })
        .await?;
        Ok(record)
    }

    async fn list_learning_session_records(
        &self,
        session_id: &str,
    ) -> Result<Vec<LearningSessionRecord>> {
        let session_id = session_id.to_string();
        self.with_client(move |client| {
            let _ = client.frame_tick();
            Ok(client
                .list_learning_session_records()
                .into_iter()
                .filter(|record| record.session_id == session_id)
                .collect())
        })
        .await
    }

    async fn append_witness_log_record(
        &self,
        record: WitnessLogRecord,
    ) -> Result<WitnessLogRecord> {
        let value = record.clone();
        self.with_client(move |client| {
            client.append_witness_log_record(value)?;
            let _ = client.frame_tick();
            Ok(())
        })
        .await?;
        Ok(record)
    }

    async fn list_witness_log_records(&self, session_id: &str) -> Result<Vec<WitnessLogRecord>> {
        let session_id = session_id.to_string();
        self.with_client(move |client| {
            let _ = client.frame_tick();
            Ok(client
                .list_witness_log_records()
                .into_iter()
                .filter(|record| record.session_id == session_id)
                .collect())
        })
        .await
    }
}

#[derive(Clone)]
pub struct SpacetimeDb {
    config: SpacetimeDbConfig,
    repo: Arc<dyn SpacetimeRepository>,
}

impl SpacetimeDb {
    pub fn try_from_config(config: &SpacetimeDbConfig) -> Result<Self> {
        let repo: Arc<dyn SpacetimeRepository> = match config.backend {
            SpacetimeBackend::InMemory => Arc::new(InMemorySpacetimeRepository::default()),
            SpacetimeBackend::Sdk => Arc::new(SdkSpacetimeRepository::connect(config)?),
        };

        Ok(Self {
            config: config.clone(),
            repo,
        })
    }

    pub fn from_config(config: &SpacetimeDbConfig) -> Self {
        Self::try_from_config(config).expect("failed to initialize SpacetimeDb backend")
    }

    pub fn validate(&self) -> Result<()> {
        if self.config.enabled && self.config.uri.trim().is_empty() {
            bail!("spacetimedb.uri must not be empty when enabled");
        }
        if self.config.enabled && self.config.pool_size == 0 {
            bail!("spacetimedb.pool_size must be greater than 0");
        }
        if self.config.enabled && matches!(self.config.backend, SpacetimeBackend::Sdk) && self.config.module_name.trim().is_empty() {
            bail!("spacetimedb.module_name must not be empty when sdk backend is enabled");
        }
        Ok(())
    }

    pub async fn enforce_permission(&self, actor_id: &str, action: PermissionAction) -> Result<()> {
        if !self.repo.has_permission(actor_id, action).await? {
            bail!("actor '{actor_id}' does not have permission '{action:?}'");
        }
        Ok(())
    }

    pub async fn has_permission(&self, actor_id: &str, action: PermissionAction) -> Result<bool> {
        self.repo.has_permission(actor_id, action).await
    }

    pub async fn create_schedule_event(
        &self,
        session_id: String,
        topic: String,
        tool_name: String,
        payload: String,
        actor_id: String,
    ) -> Result<ScheduleEvent> {
        self.repo
            .create_schedule_event(session_id, topic, tool_name, payload, actor_id)
            .await
    }

    pub async fn update_schedule_status(&self, event_id: u64, status: impl Into<String>) -> Result<()> {
        self.repo.update_schedule_status(event_id, status.into()).await
    }

    pub async fn list_schedule_events(&self, session_id: &str) -> Result<Vec<ScheduleEvent>> {
        self.repo.list_schedule_events(session_id).await
    }

    pub async fn upsert_agent_state(
        &self,
        session_id: String,
        last_user_message: String,
        last_assistant_message: Option<String>,
    ) -> Result<AgentState> {
        self.repo
            .upsert_agent_state(session_id, last_user_message, last_assistant_message)
            .await
    }

    pub async fn get_agent_state(&self, session_id: &str) -> Result<Option<AgentState>> {
        self.repo.get_agent_state(session_id).await
    }

    pub async fn upsert_knowledge(
        &self,
        key: String,
        value: String,
        source: String,
    ) -> Result<KnowledgeRecord> {
        self.repo.upsert_knowledge(key, value, source).await
    }

    pub async fn upsert_json_knowledge<T: Serialize>(
        &self,
        key: impl Into<String>,
        value: &T,
        source: impl Into<String>,
    ) -> Result<KnowledgeRecord> {
        self.upsert_knowledge(
            key.into(),
            serde_json::to_string(value)?,
            source.into(),
        )
        .await
    }

    pub async fn get_knowledge(&self, key: &str) -> Result<Option<KnowledgeRecord>> {
        self.repo.get_knowledge(key).await
    }

    pub async fn list_knowledge_by_prefix(&self, prefix: &str) -> Result<Vec<KnowledgeRecord>> {
        self.repo.list_knowledge_by_prefix(prefix).await
    }

    pub async fn grant_permissions(
        &self,
        actor_id: impl Into<String>,
        permissions: Vec<PermissionAction>,
    ) -> Result<PermissionGrant> {
        self.repo.grant_permissions(actor_id.into(), permissions).await
    }

    pub async fn upsert_tenant(&self, tenant: Tenant) -> Result<Tenant> {
        self.upsert_json_knowledge(
            format!("identity:tenant:{}", tenant.tenant_id),
            &tenant,
            "identity",
        )
        .await?;
        Ok(tenant)
    }

    pub async fn get_tenant(&self, tenant_id: &str) -> Result<Option<Tenant>> {
        Ok(self
            .get_knowledge(&format!("identity:tenant:{tenant_id}"))
            .await?
            .and_then(|record| serde_json::from_str::<Tenant>(&record.value).ok()))
    }

    pub async fn upsert_principal(&self, principal: Principal) -> Result<Principal> {
        self.upsert_json_knowledge(
            format!("identity:principal:{}:{}", principal.tenant_id, principal.principal_id),
            &principal,
            "identity",
        )
        .await?;
        Ok(principal)
    }

    pub async fn get_principal(
        &self,
        tenant_id: &str,
        principal_id: &str,
    ) -> Result<Option<Principal>> {
        Ok(self
            .get_knowledge(&format!("identity:principal:{tenant_id}:{principal_id}"))
            .await?
            .and_then(|record| serde_json::from_str::<Principal>(&record.value).ok()))
    }

    pub async fn upsert_role_binding(&self, binding: RoleBinding) -> Result<RoleBinding> {
        self.upsert_json_knowledge(
            format!(
                "identity:role-binding:{}:{}",
                binding.tenant_id, binding.principal_id
            ),
            &binding,
            "identity",
        )
        .await?;
        Ok(binding)
    }

    pub async fn get_role_binding(
        &self,
        tenant_id: &str,
        principal_id: &str,
    ) -> Result<Option<RoleBinding>> {
        Ok(self
            .get_knowledge(&format!("identity:role-binding:{tenant_id}:{principal_id}"))
            .await?
            .and_then(|record| serde_json::from_str::<RoleBinding>(&record.value).ok()))
    }

    pub async fn upsert_policy_binding(&self, binding: PolicyBinding) -> Result<PolicyBinding> {
        self.upsert_json_knowledge(
            format!("identity:policy-binding:{}:{}", binding.tenant_id, binding.policy_id),
            &binding,
            "identity",
        )
        .await?;
        Ok(binding)
    }

    pub async fn get_policy_binding(
        &self,
        tenant_id: &str,
        policy_id: &str,
    ) -> Result<Option<PolicyBinding>> {
        Ok(self
            .get_knowledge(&format!("identity:policy-binding:{tenant_id}:{policy_id}"))
            .await?
            .and_then(|record| serde_json::from_str::<PolicyBinding>(&record.value).ok()))
    }

    pub async fn upsert_session_lease(&self, lease: SessionLease) -> Result<SessionLease> {
        self.upsert_json_knowledge(
            format!("identity:session-lease:{}", lease.session_id),
            &lease,
            "identity",
        )
        .await?;
        Ok(lease)
    }

    pub async fn get_session_lease(&self, session_id: &str) -> Result<Option<SessionLease>> {
        Ok(self
            .get_knowledge(&format!("identity:session-lease:{session_id}"))
            .await?
            .and_then(|record| serde_json::from_str::<SessionLease>(&record.value).ok()))
    }

    pub async fn upsert_budget_account(&self, account: BudgetAccount) -> Result<BudgetAccount> {
        self.upsert_json_knowledge(
            format!("billing:budget-account:{}:{}", account.tenant_id, account.account_id),
            &account,
            "billing",
        )
        .await?;
        Ok(account)
    }

    pub async fn get_budget_account(
        &self,
        tenant_id: &str,
        account_id: &str,
    ) -> Result<Option<BudgetAccount>> {
        Ok(self
            .get_knowledge(&format!("billing:budget-account:{tenant_id}:{account_id}"))
            .await?
            .and_then(|record| serde_json::from_str::<BudgetAccount>(&record.value).ok()))
    }

    pub async fn append_spend_ledger(&self, entry: SpendLedger) -> Result<SpendLedger> {
        let key = format!(
            "billing:spend-ledger:{}:{}:{}",
            entry.tenant_id, entry.account_id, entry.ledger_id
        );
        if self.get_knowledge(&key).await?.is_some() {
            bail!("spend ledger {} already exists", entry.ledger_id);
        }
        self.upsert_json_knowledge(key, &entry, "billing").await?;
        Ok(entry)
    }

    pub async fn list_spend_ledger(
        &self,
        tenant_id: &str,
        account_id: &str,
    ) -> Result<Vec<SpendLedger>> {
        let prefix = format!("billing:spend-ledger:{tenant_id}:{account_id}:");
        let mut entries = self
            .list_knowledge_by_prefix(&prefix)
            .await?
            .into_iter()
            .filter_map(|record| serde_json::from_str::<SpendLedger>(&record.value).ok())
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.created_at_ms);
        Ok(entries)
    }

    pub async fn list_spend_ledger_by_task(
        &self,
        session_id: &str,
        task_id: &str,
    ) -> Result<Vec<SpendLedger>> {
        let mut entries = self
            .list_knowledge_by_prefix("billing:spend-ledger:")
            .await?
            .into_iter()
            .filter_map(|record| serde_json::from_str::<SpendLedger>(&record.value).ok())
            .filter(|entry| entry.session_id == session_id && entry.task_id == task_id)
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.created_at_ms);
        Ok(entries)
    }

    pub async fn upsert_quota_window(&self, window: QuotaWindow) -> Result<QuotaWindow> {
        self.upsert_json_knowledge(
            format!("billing:quota-window:{}:{}:{}", window.tenant_id, window.account_id, window.window_id),
            &window,
            "billing",
        )
        .await?;
        Ok(window)
    }

    pub async fn get_quota_window(
        &self,
        tenant_id: &str,
        account_id: &str,
        window_id: &str,
    ) -> Result<Option<QuotaWindow>> {
        Ok(self
            .get_knowledge(&format!(
                "billing:quota-window:{tenant_id}:{account_id}:{window_id}"
            ))
            .await?
            .and_then(|record| serde_json::from_str::<QuotaWindow>(&record.value).ok()))
    }

    pub async fn upsert_cost_attribution(
        &self,
        attribution: CostAttribution,
    ) -> Result<CostAttribution> {
        self.upsert_json_knowledge(
            format!(
                "billing:cost-attribution:{}:{}:{}",
                attribution.tenant_id, attribution.session_id, attribution.attribution_id
            ),
            &attribution,
            "billing",
        )
        .await?;
        Ok(attribution)
    }

    pub async fn get_cost_attribution(
        &self,
        tenant_id: &str,
        session_id: &str,
        attribution_id: &str,
    ) -> Result<Option<CostAttribution>> {
        Ok(self
            .get_knowledge(&format!(
                "billing:cost-attribution:{tenant_id}:{session_id}:{attribution_id}"
            ))
            .await?
            .and_then(|record| serde_json::from_str::<CostAttribution>(&record.value).ok()))
    }

    pub async fn list_cost_attribution_by_session(
        &self,
        tenant_id: &str,
        session_id: &str,
    ) -> Result<Vec<CostAttribution>> {
        let prefix = format!("billing:cost-attribution:{tenant_id}:{session_id}:");
        let mut records = self
            .list_knowledge_by_prefix(&prefix)
            .await?
            .into_iter()
            .filter_map(|record| serde_json::from_str::<CostAttribution>(&record.value).ok())
            .collect::<Vec<_>>();
        records.sort_by_key(|record| record.settled_at_ms);
        Ok(records)
    }

    pub async fn upsert_reflexion_episode(
        &self,
        record: ReflexionEpisodeRecord,
    ) -> Result<ReflexionEpisodeRecord> {
        self.repo.upsert_reflexion_episode(record).await
    }

    pub async fn list_reflexion_episodes(&self, session_id: &str) -> Result<Vec<ReflexionEpisodeRecord>> {
        self.repo.list_reflexion_episodes(session_id).await
    }

    pub async fn upsert_skill_library_record(
        &self,
        record: SkillLibraryRecord,
    ) -> Result<SkillLibraryRecord> {
        self.repo.upsert_skill_library_record(record).await
    }

    pub async fn list_skill_library_records(&self, session_id: &str) -> Result<Vec<SkillLibraryRecord>> {
        self.repo.list_skill_library_records(session_id).await
    }

    pub async fn upsert_causal_edge_record(
        &self,
        record: CausalEdgeRecord,
    ) -> Result<CausalEdgeRecord> {
        self.repo.upsert_causal_edge_record(record).await
    }

    pub async fn list_causal_edge_records(&self, session_id: &str) -> Result<Vec<CausalEdgeRecord>> {
        self.repo.list_causal_edge_records(session_id).await
    }

    pub async fn upsert_learning_session_record(
        &self,
        record: LearningSessionRecord,
    ) -> Result<LearningSessionRecord> {
        self.repo.upsert_learning_session_record(record).await
    }

    pub async fn list_learning_session_records(
        &self,
        session_id: &str,
    ) -> Result<Vec<LearningSessionRecord>> {
        self.repo.list_learning_session_records(session_id).await
    }

    pub async fn append_witness_log_record(
        &self,
        record: WitnessLogRecord,
    ) -> Result<WitnessLogRecord> {
        self.repo.append_witness_log_record(record).await
    }

    pub async fn list_witness_log_records(&self, session_id: &str) -> Result<Vec<WitnessLogRecord>> {
        self.repo.list_witness_log_records(session_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn spacetimedb_crud_is_type_safe_and_thread_safe() {
        let db = SpacetimeDb::from_config(&SpacetimeDbConfig {
            enabled: true,
            backend: SpacetimeBackend::InMemory,
            uri: "http://spacetimedb:3000".into(),
            module_name: "autoloop_core".into(),
            namespace: "autoloop".into(),
            pool_size: 4,
        });

        db.grant_permissions("agent-1", vec![PermissionAction::Read, PermissionAction::Write])
            .await
            .expect("grant");
        db.enforce_permission("agent-1", PermissionAction::Read)
            .await
            .expect("read permission");

        let knowledge = db
            .upsert_knowledge(
                "anchor:rust".into(),
                "Rust is the systems substrate for AutoLoop.".into(),
                "test".into(),
            )
            .await
            .expect("knowledge upsert");

        assert_eq!(knowledge.key, "anchor:rust");
        assert!(db.get_knowledge("anchor:rust").await.expect("knowledge read").is_some());
    }

    #[tokio::test]
    async fn identity_schema_crud_works_for_tenant_principal_policy_and_lease() {
        let db = SpacetimeDb::from_config(&SpacetimeDbConfig {
            enabled: true,
            backend: SpacetimeBackend::InMemory,
            uri: "http://spacetimedb:3000".into(),
            module_name: "autoloop_core".into(),
            namespace: "autoloop".into(),
            pool_size: 4,
        });
        let tenant = db
            .upsert_tenant(Tenant {
                tenant_id: "tenant-a".into(),
                name: "Tenant A".into(),
                status: "active".into(),
                created_at_ms: 1,
            })
            .await
            .expect("tenant");
        db.upsert_principal(Principal {
            principal_id: "principal-a".into(),
            tenant_id: tenant.tenant_id.clone(),
            principal_type: "user".into(),
            status: "active".into(),
            created_at_ms: 2,
        })
        .await
        .expect("principal");
        db.upsert_role_binding(RoleBinding {
            tenant_id: tenant.tenant_id.clone(),
            principal_id: "principal-a".into(),
            role: "operator".into(),
            updated_at_ms: 3,
        })
        .await
        .expect("role");
        db.upsert_policy_binding(PolicyBinding {
            policy_id: "policy-a".into(),
            tenant_id: tenant.tenant_id.clone(),
            role: "operator".into(),
            allowed_actions: vec![PermissionAction::Read, PermissionAction::Dispatch],
            capability_prefixes: vec!["provider:".into(), "mcp::local-mcp::".into()],
            max_memory_mb: 1024,
            max_tokens: 8000,
            updated_at_ms: 4,
        })
        .await
        .expect("policy");
        db.upsert_session_lease(SessionLease {
            lease_token: "lease-a".into(),
            session_id: "session-a".into(),
            tenant_id: tenant.tenant_id.clone(),
            principal_id: "principal-a".into(),
            policy_id: "policy-a".into(),
            expires_at_ms: 9_999,
            issued_at_ms: 5,
        })
        .await
        .expect("lease");

        assert!(db
            .get_tenant("tenant-a")
            .await
            .expect("tenant get")
            .is_some());
        assert!(db
            .get_principal("tenant-a", "principal-a")
            .await
            .expect("principal get")
            .is_some());
        assert!(db
            .get_policy_binding("tenant-a", "policy-a")
            .await
            .expect("policy get")
            .is_some());
        assert!(db
            .get_session_lease("session-a")
            .await
            .expect("lease get")
            .is_some());
    }

    #[tokio::test]
    async fn billing_schema_append_only_and_replay_work() {
        let db = SpacetimeDb::from_config(&SpacetimeDbConfig {
            enabled: true,
            backend: SpacetimeBackend::InMemory,
            uri: "http://spacetimedb:3000".into(),
            module_name: "autoloop_core".into(),
            namespace: "autoloop".into(),
            pool_size: 4,
        });
        db.upsert_budget_account(BudgetAccount {
            account_id: "account-a".into(),
            tenant_id: "tenant-a".into(),
            principal_id: "principal-a".into(),
            policy_id: "policy-a".into(),
            total_budget_micros: 100_000,
            reserved_micros: 20_000,
            spent_micros: 10_000,
            blocked_count: 0,
            updated_at_ms: 1,
        })
        .await
        .expect("budget account");
        db.append_spend_ledger(SpendLedger {
            ledger_id: "l1".into(),
            tenant_id: "tenant-a".into(),
            account_id: "account-a".into(),
            session_id: "session-a".into(),
            trace_id: "trace-a".into(),
            task_id: "task-a".into(),
            capability_id: "provider:default".into(),
            kind: SpendLedgerKind::Reserve,
            amount_micros: 5_000,
            token_cost_micros: 3_000,
            tool_cost_micros: 1_000,
            duration_cost_micros: 1_000,
            reason: "precharge".into(),
            created_at_ms: 2,
        })
        .await
        .expect("ledger reserve");
        db.append_spend_ledger(SpendLedger {
            ledger_id: "l2".into(),
            tenant_id: "tenant-a".into(),
            account_id: "account-a".into(),
            session_id: "session-a".into(),
            trace_id: "trace-a".into(),
            task_id: "task-a".into(),
            capability_id: "provider:default".into(),
            kind: SpendLedgerKind::Settle,
            amount_micros: 4_500,
            token_cost_micros: 3_000,
            tool_cost_micros: 500,
            duration_cost_micros: 1_000,
            reason: "settled".into(),
            created_at_ms: 3,
        })
        .await
        .expect("ledger settle");
        db.upsert_quota_window(QuotaWindow {
            window_id: "w1".into(),
            tenant_id: "tenant-a".into(),
            account_id: "account-a".into(),
            window_start_ms: 0,
            window_end_ms: 10_000,
            window_budget_micros: 50_000,
            consumed_micros: 4_500,
            blocked_count: 0,
            updated_at_ms: 3,
        })
        .await
        .expect("quota");
        db.upsert_cost_attribution(CostAttribution {
            attribution_id: "a1".into(),
            tenant_id: "tenant-a".into(),
            principal_id: "principal-a".into(),
            policy_id: "policy-a".into(),
            session_id: "session-a".into(),
            trace_id: "trace-a".into(),
            task_id: "task-a".into(),
            capability_id: "provider:default".into(),
            provider_tokens: 300,
            tool_invocations: 0,
            duration_ms: 120,
            token_cost_micros: 3_000,
            tool_cost_micros: 500,
            duration_cost_micros: 1_000,
            total_cost_micros: 4_500,
            settled_at_ms: 3,
        })
        .await
        .expect("attribution");

        let replay = db
            .list_spend_ledger("tenant-a", "account-a")
            .await
            .expect("ledger replay");
        assert_eq!(replay.len(), 2);
        assert_eq!(replay[0].ledger_id, "l1");
        assert_eq!(replay[1].ledger_id, "l2");
        assert_eq!(replay[1].kind, SpendLedgerKind::Settle);
        assert!(db
            .append_spend_ledger(SpendLedger {
                ledger_id: "l2".into(),
                tenant_id: "tenant-a".into(),
                account_id: "account-a".into(),
                session_id: "session-a".into(),
                trace_id: "trace-a".into(),
                task_id: "task-a".into(),
                capability_id: "provider:default".into(),
                kind: SpendLedgerKind::Settle,
                amount_micros: 1,
                token_cost_micros: 0,
                tool_cost_micros: 0,
                duration_cost_micros: 0,
                reason: "duplicate".into(),
                created_at_ms: 4,
            })
            .await
            .is_err());
    }
}
