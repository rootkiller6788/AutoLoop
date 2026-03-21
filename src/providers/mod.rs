use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    env,
    hash::{Hash, Hasher},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use parking_lot::RwLock;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};

use crate::adaptive_framework::{PromptTemplateProfile, build_prompt_template_bundle};
use crate::config::ProviderConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationSignal {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationProposal {
    pub title: String,
    pub change_target: String,
    pub hypothesis: String,
    pub expected_gain: String,
    pub risk: String,
    pub patch_outline: Vec<String>,
    pub evaluation_focus: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PromptPolicyOverlay {
    pub preferred_model: Option<String>,
    pub directives: Vec<String>,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRouteStage {
    Screening,
    Reasoning,
    Judge,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRouteDecision {
    pub stage: ProviderRouteStage,
    pub model: String,
    pub rationale: String,
    pub cache_hit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderChatTrace {
    pub response: LlmResponse,
    pub route: ProviderRouteDecision,
    pub cache_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderProvenance {
    pub provider_name: String,
    pub source: String,
    pub version_ref: String,
    pub trusted: bool,
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    async fn chat(&self, messages: &[ChatMessage], model: &str) -> Result<LlmResponse>;
}

#[derive(Debug)]
pub struct StubProvider {
    name: String,
}

impl StubProvider {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

#[async_trait]
impl Provider for StubProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn chat(&self, messages: &[ChatMessage], model: &str) -> Result<LlmResponse> {
        let last = messages.iter().rev().find(|msg| msg.role == "user");
        let content = last.map(|msg| format!("[provider:{}:{}] {}", self.name, model, msg.content));
        Ok(LlmResponse {
            content,
            tool_calls: Vec::new(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAiChatRequest {
    model: String,
    messages: Vec<OpenAiChatMessage>,
    temperature: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAiChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAiChatResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAiChoice {
    message: OpenAiAssistantMessage,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAiAssistantMessage {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<OpenAiToolCall>,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAiToolCall {
    id: String,
    #[serde(default)]
    function: Option<OpenAiFunctionCall>,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAiFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug)]
pub struct OpenAiCompatibleProvider {
    name: String,
    client: reqwest::Client,
    base_url: String,
}

impl OpenAiCompatibleProvider {
    pub fn try_new(config: &ProviderConfig) -> Result<Self> {
        let api_key = env::var(&config.api_key_env)
            .with_context(|| format!("missing provider api key env {}", config.api_key_env))?;
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {api_key}"))
                .context("invalid provider api key for authorization header")?,
        );
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(config.request_timeout_secs))
            .build()
            .context("failed to build openai-compatible client")?;

        Ok(Self {
            name: "openai-compatible".into(),
            client,
            base_url: config.api_base_url.trim_end_matches('/').into(),
        })
    }
}

#[async_trait]
impl Provider for OpenAiCompatibleProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn chat(&self, messages: &[ChatMessage], model: &str) -> Result<LlmResponse> {
        let request = OpenAiChatRequest {
            model: model.to_string(),
            messages: messages
                .iter()
                .map(|message| OpenAiChatMessage {
                    role: message.role.clone(),
                    content: message.content.clone(),
                })
                .collect(),
            temperature: 0.2,
        };
        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .json(&request)
            .send()
            .await
            .context("provider request failed")?
            .error_for_status()
            .context("provider returned non-success status")?;
        let body = response
            .json::<OpenAiChatResponse>()
            .await
            .context("failed to decode provider response")?;
        let choice = body
            .choices
            .into_iter()
            .next()
            .context("provider returned no choices")?;

        Ok(LlmResponse {
            content: choice.message.content,
            tool_calls: choice
                .message
                .tool_calls
                .into_iter()
                .filter_map(|call| {
                    call.function.map(|function| ToolCall {
                        id: call.id,
                        name: function.name,
                        arguments: function.arguments,
                    })
                })
                .collect(),
        })
    }
}

#[derive(Debug)]
pub struct McpProviderAdapter {
    name: String,
    server: String,
}

impl McpProviderAdapter {
    pub fn new(server: impl Into<String>) -> Self {
        let server = server.into();
        Self {
            name: format!("mcp:{server}"),
            server,
        }
    }
}

#[async_trait]
impl Provider for McpProviderAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    async fn chat(&self, messages: &[ChatMessage], model: &str) -> Result<LlmResponse> {
        let last = messages.iter().rev().find(|msg| msg.role == "user");
        let content = last.map(|msg| format!("[mcp-provider:{}:{}] {}", self.server, model, msg.content));
        Ok(LlmResponse {
            content,
            tool_calls: Vec::new(),
        })
    }
}

#[derive(Clone)]
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn Provider>>,
    provider_provenance: HashMap<String, ProviderProvenance>,
    default_provider: String,
    pub default_model: String,
    screening_model: String,
    reasoning_model: String,
    judge_model: String,
    enable_tiered_routing: bool,
    prompt_cache_capacity: usize,
    prompt_cache: Arc<RwLock<HashMap<String, LlmResponse>>>,
}

impl ProviderRegistry {
    pub fn from_config(config: &ProviderConfig) -> Self {
        let mut providers: HashMap<String, Arc<dyn Provider>> = HashMap::new();
        let mut provider_provenance: HashMap<String, ProviderProvenance> = HashMap::new();

        for name in &config.builtin {
            if name == "openai-compatible" {
                match OpenAiCompatibleProvider::try_new(config) {
                    Ok(provider) => {
                        providers.insert(name.clone(), Arc::new(provider));
                        provider_provenance.insert(
                            name.clone(),
                            ProviderProvenance {
                                provider_name: name.clone(),
                                source: config.api_base_url.clone(),
                                version_ref: "openai-compatible-http".into(),
                                trusted: config.api_base_url.starts_with("https://"),
                            },
                        );
                    }
                    Err(_) => {
                        providers.insert(name.clone(), Arc::new(StubProvider::new(name.clone())));
                        provider_provenance.insert(
                            name.clone(),
                            ProviderProvenance {
                                provider_name: name.clone(),
                                source: "stub-fallback".into(),
                                version_ref: "stub".into(),
                                trusted: true,
                            },
                        );
                    }
                }
            } else {
                providers.insert(name.clone(), Arc::new(StubProvider::new(name.clone())));
                provider_provenance.insert(
                    name.clone(),
                    ProviderProvenance {
                        provider_name: name.clone(),
                        source: "builtin".into(),
                        version_ref: "stub".into(),
                        trusted: true,
                    },
                );
            }
        }

        for server in &config.mcp_servers {
            let provider = Arc::new(McpProviderAdapter::new(server.clone()));
            providers.insert(provider.name().to_string(), provider);
            provider_provenance.insert(
                format!("mcp:{server}"),
                ProviderProvenance {
                    provider_name: format!("mcp:{server}"),
                    source: format!("mcp://{server}"),
                    version_ref: "adapter-v1".into(),
                    trusted: true,
                },
            );
        }

        let default_provider = config
            .builtin
            .first()
            .cloned()
            .or_else(|| config.mcp_servers.first().map(|server| format!("mcp:{server}")))
            .unwrap_or_else(|| "openai-compatible".into());

        Self {
            providers,
            provider_provenance,
            default_provider,
            default_model: config.default_model.clone(),
            screening_model: config.screening_model.clone(),
            reasoning_model: config.reasoning_model.clone(),
            judge_model: config.judge_model.clone(),
            enable_tiered_routing: config.enable_tiered_routing,
            prompt_cache_capacity: config.prompt_cache_capacity,
            prompt_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.providers.is_empty() {
            bail!("at least one provider must be registered");
        }
        if self.default_model.trim().is_empty() {
            bail!("providers.default_model must not be empty");
        }
        if self.prompt_cache_capacity == 0 {
            bail!("providers.prompt_cache_capacity must be greater than 0");
        }
        if !self.providers.contains_key(&self.default_provider) {
            bail!("default provider '{}' is not registered", self.default_provider);
        }
        if let Some(default_meta) = self.provider_provenance.get(&self.default_provider) {
            if !default_meta.trusted {
                bail!(
                    "default provider '{}' is not trusted by provenance policy",
                    self.default_provider
                );
            }
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.providers.len()
    }

    pub fn default_provider(&self) -> &str {
        &self.default_provider
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Provider>> {
        self.providers.get(name).cloned()
    }

    pub fn provenance(&self, name: &str) -> Option<&ProviderProvenance> {
        self.provider_provenance.get(name)
    }

    pub async fn chat(&self, messages: &[ChatMessage]) -> Result<LlmResponse> {
        self.chat_with_policy(messages, None).await
    }

    pub async fn chat_with_policy(
        &self,
        messages: &[ChatMessage],
        preferred_model: Option<&str>,
    ) -> Result<LlmResponse> {
        Ok(self
            .chat_with_trace(messages, preferred_model)
            .await?
            .response)
    }

    pub async fn chat_with_trace(
        &self,
        messages: &[ChatMessage],
        preferred_model: Option<&str>,
    ) -> Result<ProviderChatTrace> {
        let normalized_messages = normalize_messages(messages);
        let mut route = self.route_for_messages(&normalized_messages, preferred_model);
        let cache_key = self.cache_key(&normalized_messages, &route.model);
        if let Some(cached) = self.prompt_cache.read().get(&cache_key).cloned() {
            route.cache_hit = true;
            return Ok(ProviderChatTrace {
                response: cached,
                route,
                cache_key,
            });
        }
        let provider = self
            .get(&self.default_provider)
            .ok_or_else(|| anyhow::anyhow!("provider '{}' not found", self.default_provider))?;
        let response = provider.chat(&normalized_messages, &route.model).await?;
        self.insert_cache(cache_key.clone(), response.clone());
        route.cache_hit = false;
        Ok(ProviderChatTrace {
            response,
            route,
            cache_key,
        })
    }

    pub fn derive_prompt_policy(
        &self,
        objective: &str,
        evolution_summary: Option<&str>,
        research_summary: Option<&str>,
        capability_hints: &[String],
    ) -> PromptPolicyOverlay {
        let bundle = build_prompt_template_bundle(
            Some(PromptTemplateProfile {
                stage: "api-policy-adaptation".into(),
                adaptation_type: "prompt-route-tool-policy".into(),
                preferred_surface: "provider-api".into(),
                rollout_budget_ms: 1,
            }),
            evolution_summary,
            research_summary,
            capability_hints,
        );
        let mut directives = bundle.all_directives();

        if objective.to_ascii_lowercase().contains("research")
            || objective.to_ascii_lowercase().contains("crawl")
        {
            directives.push(
                "Bias toward evidence expansion, freshness checks, and official sources before finalizing the answer."
                    .into(),
            );
        }

        PromptPolicyOverlay {
            preferred_model: None,
            directives: dedupe_directives(&directives),
            rationale: if bundle.all_rationales().is_empty() {
                "Derived from self-evolution reports, research evidence, and active capability catalog."
                    .into()
            } else {
                bundle.all_rationales().join(" ")
            },
        }
    }

    pub fn route_for_messages(
        &self,
        messages: &[ChatMessage],
        preferred_model: Option<&str>,
    ) -> ProviderRouteDecision {
        if let Some(model) = preferred_model {
            return ProviderRouteDecision {
                stage: ProviderRouteStage::Reasoning,
                model: model.to_string(),
                rationale: "explicit preferred model requested".into(),
                cache_hit: false,
            };
        }

        if !self.enable_tiered_routing {
            return ProviderRouteDecision {
                stage: ProviderRouteStage::Reasoning,
                model: self.default_model.clone(),
                rationale: "tiered routing disabled".into(),
                cache_hit: false,
            };
        }

        let combined = messages
            .iter()
            .map(|message| message.content.to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join("\n");
        let total_chars = combined.len();
        let stage = if combined.contains("judge")
            || combined.contains("verifier")
            || combined.contains("arbitrate")
            || combined.contains("regression")
        {
            ProviderRouteStage::Judge
        } else if total_chars > 1400
            || combined.contains("graph")
            || combined.contains("research")
            || combined.contains("plan")
            || combined.contains("swarm")
            || combined.contains("capability")
        {
            ProviderRouteStage::Reasoning
        } else {
            ProviderRouteStage::Screening
        };

        let model = match stage {
            ProviderRouteStage::Screening => self.screening_model.clone(),
            ProviderRouteStage::Reasoning => self.reasoning_model.clone(),
            ProviderRouteStage::Judge => self.judge_model.clone(),
        };

        ProviderRouteDecision {
            stage,
            model,
            rationale: format!(
                "tiered routing selected model using {} characters of prompt context",
                total_chars
            ),
            cache_hit: false,
        }
    }

    fn cache_key(&self, messages: &[ChatMessage], model: &str) -> String {
        let mut hasher = DefaultHasher::new();
        model.hash(&mut hasher);
        for message in messages {
            message.role.hash(&mut hasher);
            message.content.hash(&mut hasher);
        }
        format!("provider-cache:{:x}", hasher.finish())
    }

    fn insert_cache(&self, key: String, response: LlmResponse) {
        let mut cache = self.prompt_cache.write();
        if cache.len() >= self.prompt_cache_capacity {
            if let Some(oldest_key) = cache.keys().next().cloned() {
                cache.remove(&oldest_key);
            }
        }
        cache.insert(key, response);
    }

    pub async fn propose_next_iteration(
        &self,
        objective: &str,
        history_summary: &str,
        optimization_signals: &[OptimizationSignal],
    ) -> Result<OptimizationProposal> {
        let signal_text = optimization_signals
            .iter()
            .map(|signal| format!("{}={}", signal.key, signal.value))
            .collect::<Vec<_>>()
            .join(", ");
        let prompt = vec![
            ChatMessage {
                role: "system".into(),
                content: "You are the strategy layer for autonomous optimization. Propose the next iteration, focusing on the highest-leverage bounded change, expected gain, risk, and evaluation focus.".into(),
            },
            ChatMessage {
                role: "user".into(),
                content: format!(
                    "Objective: {objective}\nHistory: {history_summary}\nSignals: {signal_text}"
                ),
            },
        ];
        let response = self.chat(&prompt).await?;
        let fallback_hypothesis = response.content.unwrap_or_else(|| {
            format!(
                "Use strategy signals ({signal_text}) to improve the fixed-budget evaluation outcome."
            )
        });

        Ok(OptimizationProposal {
            title: "Autonomous optimization iteration".into(),
            change_target: "current target surface".into(),
            hypothesis: fallback_hypothesis,
            expected_gain:
                "Improve the immutable objective while preserving simplicity and bounded risk."
                    .into(),
            risk: if history_summary.to_ascii_lowercase().contains("crash") {
                "Medium: recent failures suggest guarding against unstable structural changes.".into()
            } else {
                "Low to medium: prefer bounded changes within the current evaluation budget.".into()
            },
            patch_outline: vec![
                "Adjust one coherent slice of the system.".into(),
                "Keep the evaluation protocol read-only.".into(),
                "Capture observable outcomes and compare against the current incumbent.".into(),
            ],
            evaluation_focus:
                "immutable evaluation protocol or equivalent fixed success criteria".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_prompt_policy_uses_evolution_research_and_capabilities() {
        let registry = ProviderRegistry::from_config(&crate::config::AppConfig::default().providers);
        let overlay = registry.derive_prompt_policy(
            "research anchor drift",
            Some("improve verifier score with bounded prompt changes"),
            Some("official sources were recovered"),
            &["mcp::local-mcp::deploy".into()],
        );

        assert!(!overlay.directives.is_empty());
        assert!(overlay
            .directives
            .iter()
            .any(|line| line.contains("Self-evolution guidance")));
        assert!(overlay
            .directives
            .iter()
            .any(|line| line.contains("Verified capability hints")));
    }

    #[test]
    fn provider_routes_small_and_judge_prompts_to_different_models() {
        let registry = ProviderRegistry::from_config(&crate::config::AppConfig::default().providers);
        let screening = registry.route_for_messages(
            &[ChatMessage {
                role: "user".into(),
                content: "say hello".into(),
            }],
            None,
        );
        let judge = registry.route_for_messages(
            &[ChatMessage {
                role: "user".into(),
                content: "Judge route correctness and verifier regression output".into(),
            }],
            None,
        );

        assert_eq!(screening.stage, ProviderRouteStage::Screening);
        assert_eq!(judge.stage, ProviderRouteStage::Judge);
        assert_ne!(screening.model, judge.model);
    }
}

fn normalize_messages(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    let mut normalized = Vec::new();
    for message in messages {
        let normalized_content = dedupe_text(&message.content);
        if normalized
            .last()
            .is_some_and(|last: &ChatMessage| {
                last.role == message.role && last.content == normalized_content
            })
        {
            continue;
        }
        normalized.push(ChatMessage {
            role: message.role.clone(),
            content: compress_text(&normalized_content, 1800),
        });
    }
    normalized
}

fn dedupe_directives(directives: &[String]) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    directives
        .iter()
        .filter_map(|directive| {
            let normalized = directive.trim().to_string();
            if normalized.is_empty() || !seen.insert(normalized.clone()) {
                None
            } else {
                Some(normalized)
            }
        })
        .collect()
}

fn dedupe_text(text: &str) -> String {
    let mut seen = std::collections::BTreeSet::new();
    text.lines()
        .filter_map(|line| {
            let normalized = line.trim();
            if normalized.is_empty() || !seen.insert(normalized.to_string()) {
                None
            } else {
                Some(normalized.to_string())
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn compress_text(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        text.to_string()
    } else {
        format!("{}...", &text[..limit.saturating_sub(3)])
    }
}
