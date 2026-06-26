//! Search engine: queries the node repository and ranks results.

use dos_storage::NodeRepository;

use crate::{SearchError, SearchQuery, SearchResult};
use crate::query::score;

/// Executes device search queries against the node store.
pub struct SearchEngine<R: NodeRepository> {
    repository: R,
}

impl<R: NodeRepository> SearchEngine<R> {
    /// Create a new engine backed by the given repository.
    pub fn new(repository: R) -> Self {
        Self { repository }
    }

    /// Search for nodes matching `query`.
    ///
    /// Results are sorted by descending score; zero-score nodes are excluded.
    ///
    /// # Errors
    /// Returns [`SearchError::Storage`] if the repository cannot be queried.
    pub async fn search(&self, query: SearchQuery) -> Result<Vec<SearchResult>, SearchError> {
        let nodes = self.repository.list_all().await.map_err(|e| SearchError::Storage(e.to_string()))?;

        let mut results: Vec<SearchResult> = nodes
            .into_iter()
            .filter_map(|node| {
                let s = score(&node, &query);
                if s > 0.0 { Some(SearchResult { node, score: s }) } else { None }
            })
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        Ok(results)
    }
}
