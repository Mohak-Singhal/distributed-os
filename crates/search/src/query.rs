//! Search query and result types.

use dos_core::Node;

/// A parsed search query.
#[derive(Debug, Clone)]
pub struct SearchQuery {
    raw: String,
}

impl SearchQuery {
    /// Parse and normalise a raw user query (trim, lowercase).
    pub fn new(raw: impl Into<String>) -> Self {
        Self { raw: raw.into().trim().to_lowercase() }
    }

    /// Returns the normalised query string.
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Returns `true` if the query is empty.
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }
}

/// A single scored search result.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// The matching node.
    pub node: Node,
    /// Relevance score in [0.0, 1.0]. Higher is better.
    pub score: f32,
}

/// Compute a relevance score for `node` against `query`.
pub(crate) fn score(node: &Node, query: &SearchQuery) -> f32 {
    if query.is_empty() {
        return 1.0;
    }
    let q = query.as_str();
    if node.status.to_string() == q { return 1.0; }
    if node.platform.to_string() == q { return 1.0; }
    if node.name.to_lowercase().contains(q) { return 0.8; }
    if node.platform.to_string().contains(q) { return 0.7; }
    if node.capabilities.iter().any(|c| c.to_string().contains(q)) { return 0.6; }
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use dos_core::{Node, NodeStatus, Platform};
    use uuid::Uuid;

    fn make_node(name: &str, platform: Platform, status: NodeStatus) -> Node {
        let mut n = Node::new(Uuid::new_v4(), name, platform, vec![], "key", "0.1.0");
        n.status = status;
        n
    }

    #[test]
    fn score_exact_platform() {
        let node = make_node("My Mac", Platform::Mac, NodeStatus::Online);
        assert_eq!(score(&node, &SearchQuery::new("mac")), 1.0);
    }

    #[test]
    fn score_status() {
        let node = make_node("Pi", Platform::RaspberryPi, NodeStatus::Offline);
        assert_eq!(score(&node, &SearchQuery::new("offline")), 1.0);
    }

    #[test]
    fn score_no_match() {
        let node = make_node("Pi", Platform::RaspberryPi, NodeStatus::Online);
        assert_eq!(score(&node, &SearchQuery::new("windows")), 0.0);
    }
}
