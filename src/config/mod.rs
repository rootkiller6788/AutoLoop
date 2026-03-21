use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;
use autoloop_spacetimedb_adapter::{SpacetimeBackend, SpacetimeDbConfig};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSection {
    pub name: String,
    pub workspace_root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeGateMode {
    Shadow,
    Canary,
    Full,
}

fn default_runtime_gate_mode() -> RuntimeGateMode {
    RuntimeGateMode::Shadow
}

fn default_runtime_gate_enforce_ratio() -> f32 {
    0.2
}

fn default_runtime_budget_enforced() -> bool {
    true
}

fn default_runtime_default_budget_micros() -> u64 {
    5_000_000
}

fn default_runtime_quota_window_ms() -> u64 {
    3_600_000
}

fn default_runtime_quota_window_budget_micros() -> u64 {
    1_000_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub max_parallel_agents: usize,
    pub max_memory_mb: u32,
    pub mcp_enabled: bool,
    pub allow_network_tools: bool,
    pub tool_breaker_failure_threshold: u32,
    pub tool_breaker_cooldown_ms: u64,
    pub mcp_breaker_failure_threshold: u32,
    pub mcp_breaker_cooldown_ms: u64,
    #[serde(default = "default_runtime_gate_mode")]
    pub gate_mode: RuntimeGateMode,
    #[serde(default = "default_runtime_gate_enforce_ratio")]
    pub gate_enforce_ratio: f32,
    #[serde(default)]
    pub rollback_contract_version: Option<String>,
    #[serde(default = "default_runtime_budget_enforced")]
    pub budget_enforced: bool,
    #[serde(default = "default_runtime_default_budget_micros")]
    pub default_budget_micros: u64,
    #[serde(default = "default_runtime_quota_window_ms")]
    pub quota_window_ms: u64,
    #[serde(default = "default_runtime_quota_window_budget_micros")]
    pub quota_window_budget_micros: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub profile: String,
    pub require_approval_for_exec: bool,
    pub ironclaw_compatible_rules: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub load_identity: bool,
    pub load_memory_md: bool,
    pub load_history_md: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearningIndexKind {
    Flat,
    Hnsw,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuantizationMode {
    None,
    Scalar,
    Product,
    Binary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningConfig {
    pub enabled: bool,
    pub sidecar_enabled: bool,
    pub index_kind: LearningIndexKind,
    pub embedding_dimensions: usize,
    pub top_k: usize,
    pub gray_routing_ratio: f32,
    pub routing_takeover_threshold: f32,
    pub hot_window_days: u32,
    pub cold_quantization: QuantizationMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagConfig {
    pub enable_graph_rag: bool,
    pub enable_first_principles_extraction: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResearchBackend {
    Auto,
    BrowserFetch,
    PlaywrightCli,
    Firecrawl,
    SelfHostedScraper,
    WebFetch,
    Synthetic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchConfig {
    pub enabled: bool,
    pub backend: ResearchBackend,
    pub live_fetch_enabled: bool,
    pub max_queries_per_anchor: usize,
    pub max_candidates_per_query: usize,
    pub prefer_official_sources: bool,
    pub prefer_dynamic_render: bool,
    pub browser_render_url: Option<String>,
    pub browser_session_pool: Vec<String>,
    pub proxy_provider_name: String,
    pub proxy_pool: Vec<String>,
    pub anti_bot_profile: String,
    pub rotate_proxy_per_request: bool,
    pub proxy_request_quota_per_proxy: usize,
    pub proxy_breaker_failure_threshold: usize,
    pub proxy_breaker_cooldown_ms: u64,
    pub playwright_node_binary: String,
    pub playwright_launch_timeout_secs: u64,
    pub firecrawl_api_url: String,
    pub firecrawl_api_key_env: String,
    pub self_hosted_scraper_url: Option<String>,
    pub request_timeout_secs: u64,
    pub retry_attempts: usize,
    pub stability_backoff_ms: u64,
    pub user_agent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub builtin: Vec<String>,
    pub default_model: String,
    pub screening_model: String,
    pub reasoning_model: String,
    pub judge_model: String,
    pub enable_tiered_routing: bool,
    pub prompt_cache_capacity: usize,
    pub api_base_url: String,
    pub api_key_env: String,
    pub request_timeout_secs: u64,
    pub mcp_servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsConfig {
    pub builtin: Vec<String>,
    pub allow_shell: bool,
    pub mcp_servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HooksConfig {
    pub builtin: Vec<String>,
    pub learning_hooks_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityConfig {
    pub enabled: bool,
    pub route_analytics_enabled: bool,
    pub failure_forensics_enabled: bool,
    pub dashboard_enabled: bool,
    pub report_top_k: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentConfig {
    pub profile: String,
    pub enable_container_assets: bool,
    pub config_dir: PathBuf,
    pub backup_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub max_iterations: usize,
    pub memory_window: usize,
    pub system_prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub app: AppSection,
    pub agent: AgentConfig,
    pub runtime: RuntimeConfig,
    pub security: SecurityConfig,
    pub memory: MemoryConfig,
    pub learning: LearningConfig,
    pub research: ResearchConfig,
    pub rag: RagConfig,
    pub providers: ProviderConfig,
    pub tools: ToolsConfig,
    pub hooks: HooksConfig,
    pub observability: ObservabilityConfig,
    pub deployment: DeploymentConfig,
    pub spacetimedb: SpacetimeDbConfig,
}

impl AppConfig {
    pub fn load_from_path(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path)?;
        Ok(toml::from_str(&raw)?)
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            app: AppSection {
                name: "autoloop".into(),
                workspace_root: PathBuf::from("."),
            },
            agent: AgentConfig {
                max_iterations: 12,
                memory_window: 100,
                system_prompt: "You are AutoLoop, a lightweight autonomous assistant runtime.".into(),
            },
            runtime: RuntimeConfig {
                max_parallel_agents: 4,
                max_memory_mb: 512,
                mcp_enabled: true,
                allow_network_tools: false,
                tool_breaker_failure_threshold: 3,
                tool_breaker_cooldown_ms: 180_000,
                mcp_breaker_failure_threshold: 2,
                mcp_breaker_cooldown_ms: 300_000,
                gate_mode: RuntimeGateMode::Shadow,
                gate_enforce_ratio: 0.2,
                rollback_contract_version: None,
                budget_enforced: true,
                default_budget_micros: 5_000_000,
                quota_window_ms: 3_600_000,
                quota_window_budget_micros: 1_000_000,
            },
            security: SecurityConfig {
                profile: "minimal-ironclaw".into(),
                require_approval_for_exec: true,
                ironclaw_compatible_rules: true,
            },
            memory: MemoryConfig {
                load_identity: true,
                load_memory_md: true,
                load_history_md: false,
            },
            learning: LearningConfig {
                enabled: true,
                sidecar_enabled: true,
                index_kind: LearningIndexKind::Flat,
                embedding_dimensions: 128,
                top_k: 4,
                gray_routing_ratio: 0.2,
                routing_takeover_threshold: 2.0,
                hot_window_days: 14,
                cold_quantization: QuantizationMode::Scalar,
            },
            research: ResearchConfig {
                enabled: true,
                backend: ResearchBackend::Auto,
                live_fetch_enabled: false,
                max_queries_per_anchor: 3,
                max_candidates_per_query: 4,
                prefer_official_sources: true,
                prefer_dynamic_render: true,
                browser_render_url: None,
                browser_session_pool: vec!["default-browser-session".into()],
                proxy_provider_name: "local-static-pool".into(),
                proxy_pool: Vec::new(),
                anti_bot_profile: "balanced-stealth".into(),
                rotate_proxy_per_request: true,
                proxy_request_quota_per_proxy: 12,
                proxy_breaker_failure_threshold: 3,
                proxy_breaker_cooldown_ms: 300_000,
                playwright_node_binary: "node".into(),
                playwright_launch_timeout_secs: 25,
                firecrawl_api_url: "https://api.firecrawl.dev".into(),
                firecrawl_api_key_env: "FIRECRAWL_API_KEY".into(),
                self_hosted_scraper_url: None,
                request_timeout_secs: 20,
                retry_attempts: 2,
                stability_backoff_ms: 350,
                user_agent: "autoloop-research/0.1".into(),
            },
            rag: RagConfig {
                enable_graph_rag: true,
                enable_first_principles_extraction: true,
            },
            providers: ProviderConfig {
                builtin: vec!["openai-compatible".into()],
                default_model: "gpt-4.1-mini".into(),
                screening_model: "gpt-4.1-nano".into(),
                reasoning_model: "gpt-4.1-mini".into(),
                judge_model: "gpt-5".into(),
                enable_tiered_routing: true,
                prompt_cache_capacity: 256,
                api_base_url: "https://api.openai.com/v1".into(),
                api_key_env: "OPENAI_API_KEY".into(),
                request_timeout_secs: 60,
                mcp_servers: vec!["local-mcp".into()],
            },
            tools: ToolsConfig {
                builtin: vec!["read_file".into(), "write_file".into(), "web_fetch".into()],
                allow_shell: false,
                mcp_servers: vec!["local-mcp".into()],
            },
            hooks: HooksConfig {
                builtin: vec!["self-learn".into()],
                learning_hooks_enabled: true,
            },
            observability: ObservabilityConfig {
                enabled: true,
                route_analytics_enabled: true,
                failure_forensics_enabled: true,
                dashboard_enabled: true,
                report_top_k: 8,
            },
            deployment: DeploymentConfig {
                profile: "production-ready".into(),
                enable_container_assets: true,
                config_dir: PathBuf::from("deploy/config"),
                backup_dir: PathBuf::from("deploy/backup"),
            },
            spacetimedb: SpacetimeDbConfig {
                enabled: true,
                backend: SpacetimeBackend::InMemory,
                uri: "http://spacetimedb:3000".into(),
                module_name: "autoloop_core".into(),
                namespace: "autoloop".into(),
                pool_size: 8,
            },
        }
    }
}
