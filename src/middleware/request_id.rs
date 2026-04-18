use uuid::Uuid;

/// A per-request unique ID.
#[derive(Debug, Clone)]
pub struct RequestId(pub String);

impl RequestId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    #[allow(dead_code)]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for RequestId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
