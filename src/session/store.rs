use std::{collections::HashMap, sync::Arc};

use tokio::sync::RwLock;

use crate::providers::ChatMessage;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionIdentity {
    pub tenant_id: String,
    pub principal_id: String,
    pub policy_id: String,
    pub lease_token: String,
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub key: String,
    pub history: Vec<ChatMessage>,
}

impl Session {
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            history: Vec::new(),
        }
    }

    pub fn push(&mut self, role: impl Into<String>, content: impl Into<String>) {
        self.history.push(ChatMessage {
            role: role.into(),
            content: content.into(),
        });
    }

    pub fn recent_history(&self, max_messages: usize) -> Vec<ChatMessage> {
        let start = self.history.len().saturating_sub(max_messages);
        self.history[start..].to_vec()
    }
}

#[derive(Clone)]
pub struct SessionStore {
    inner: Arc<RwLock<HashMap<String, Session>>>,
    identities: Arc<RwLock<HashMap<String, SessionIdentity>>>,
    memory_window: usize,
}

impl SessionStore {
    pub fn new(memory_window: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            identities: Arc::new(RwLock::new(HashMap::new())),
            memory_window,
        }
    }

    pub async fn append_user_message(&self, session_id: &str, content: &str) {
        let mut sessions = self.inner.write().await;
        let session = sessions
            .entry(session_id.to_string())
            .or_insert_with(|| Session::new(session_id));
        session.push("user", content);
    }

    pub async fn append_assistant_message(&self, session_id: &str, content: &str) {
        let mut sessions = self.inner.write().await;
        let session = sessions
            .entry(session_id.to_string())
            .or_insert_with(|| Session::new(session_id));
        session.push("assistant", content);
    }

    pub async fn append_tool_message(&self, session_id: &str, tool_name: &str, content: &str) {
        let mut sessions = self.inner.write().await;
        let session = sessions
            .entry(session_id.to_string())
            .or_insert_with(|| Session::new(session_id));
        session.push("tool", format!("{tool_name}: {content}"));
    }

    pub async fn history(&self, session_id: &str) -> Vec<ChatMessage> {
        let sessions = self.inner.read().await;
        sessions
            .get(session_id)
            .map(|session| session.recent_history(self.memory_window))
            .unwrap_or_default()
    }

    pub async fn bind_identity(&self, session_id: &str, identity: SessionIdentity) {
        self.identities
            .write()
            .await
            .insert(session_id.to_string(), identity);
    }

    pub async fn identity(&self, session_id: &str) -> Option<SessionIdentity> {
        self.identities.read().await.get(session_id).cloned()
    }
}
