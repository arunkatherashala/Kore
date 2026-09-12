use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub config: HashMap<String, String>,
    pub created_at: i64,
}

pub struct SessionManager {
    sessions: HashMap<String, Session>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    pub fn create_session(&mut self, name: String, config: HashMap<String, String>) -> String {
        let session_id = Uuid::new_v4().to_string();
        let session = Session {
            id: session_id.clone(),
            name,
            config,
            created_at: chrono::Utc::now().timestamp(),
        };
        self.sessions.insert(session_id.clone(), session);
        session_id
    }

    pub fn get_session(&self, session_id: &str) -> Option<Session> {
        self.sessions.get(session_id).cloned()
    }
}
