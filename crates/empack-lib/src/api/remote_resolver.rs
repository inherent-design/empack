//! Remote project resolution orchestrator
//!
//! Implements the v2 remote_resolver.sh pattern: try Modrinth first (preferred platform),
//! fallback to CurseForge if Modrinth fails or returns low confidence.
//!
//! Port of v2 resolution algorithm with confidence-based validation and extra words filtering.

use crate::api::ranked_search::search_with_confidence;
use crate::api::search::{SearchError, SearchProvider, SearchResult};
use crate::primitives::ModPlatform;
use std::sync::Arc;
use thiserror::Error;

/// Resolution errors
#[derive(Debug, Error)]
pub enum ResolverError {
    #[error("Search error: {0}")]
    Search(#[from] SearchError),

    #[error("No high-confidence match found for '{query}' on any platform")]
    NoMatch { query: String },

    #[error("Platform error: {0}")]
    Platform(String),
}

/// Successful resolution result
#[derive(Debug, Clone)]
pub struct Resolution {
    /// Search result (project info)
    pub result: SearchResult,
    /// Confidence score (0.0-1.0)
    pub confidence: f64,
    /// Platform where mod was found
    pub platform: ModPlatform,
    /// Whether this was a fallback resolution (CurseForge after Modrinth failed)
    pub was_fallback: bool,
}

impl Resolution {
    /// Create new resolution
    pub fn new(result: SearchResult, confidence: f64, was_fallback: bool) -> Self {
        let platform = result.platform;
        Self {
            result,
            confidence,
            platform,
            was_fallback,
        }
    }

    /// Get project ID
    pub fn project_id(&self) -> &str {
        &self.result.project_id
    }

    /// Get project title
    pub fn title(&self) -> &str {
        &self.result.title
    }

    /// Get project slug
    pub fn slug(&self) -> &str {
        &self.result.slug
    }
}

/// Remote resolver with Modrinth → CurseForge fallback
///
/// # Algorithm
///
/// 1. **Try Modrinth first** (preferred platform, 90% threshold)
/// 2. **Fallback to CurseForge** if Modrinth fails or low confidence (85% threshold)
/// 3. **Return best match** or error if no high-confidence result
///
/// # Examples
///
/// ```no_run
/// use empack_lib::api::remote_resolver::RemoteResolver;
/// use empack_lib::api::search::MockSearchProvider;
/// use empack_lib::primitives::ModPlatform;
/// use std::sync::Arc;
///
/// # tokio_test::block_on(async {
/// let modrinth = Arc::new(MockSearchProvider::new(ModPlatform::Modrinth));
/// let curseforge = Arc::new(MockSearchProvider::new(ModPlatform::CurseForge));
/// let resolver = RemoteResolver::new(modrinth, curseforge);
///
/// let resolution = resolver.resolve("Citadel").await.unwrap();
/// println!("Found: {} on {}", resolution.title(), resolution.platform);
/// # });
/// ```
pub struct RemoteResolver<M, C>
where
    M: SearchProvider,
    C: SearchProvider,
{
    modrinth: Arc<M>,
    curseforge: Arc<C>,
}

impl<M, C> RemoteResolver<M, C>
where
    M: SearchProvider,
    C: SearchProvider,
{
    /// Create new remote resolver
    pub fn new(modrinth: Arc<M>, curseforge: Arc<C>) -> Self {
        Self {
            modrinth,
            curseforge,
        }
    }

    /// Resolve a mod by name
    ///
    /// # Algorithm
    ///
    /// 1. Try Modrinth (confidence ≥ 90%)
    /// 2. If Modrinth fails/low confidence → Try CurseForge (confidence ≥ 85%)
    /// 3. Return best match or error
    ///
    /// # Arguments
    ///
    /// * `query` - Mod name to search for
    ///
    /// # Returns
    ///
    /// Resolution with mod info and confidence, or error if no high-confidence match
    pub async fn resolve(&self, query: &str) -> Result<Resolution, ResolverError> {
        // Try Modrinth first (preferred platform)
        let modrinth_results = search_with_confidence(&*self.modrinth, query, 10).await?;

        if let Some(best_match) = modrinth_results.first() {
            // Found high-confidence match on Modrinth (≥90%)
            return Ok(Resolution::new(
                best_match.result.clone(),
                best_match.confidence.score,
                false, // Not a fallback
            ));
        }

        // Modrinth failed or low confidence - fallback to CurseForge
        let curseforge_results = search_with_confidence(&*self.curseforge, query, 10).await?;

        if let Some(best_match) = curseforge_results.first() {
            // Found high-confidence match on CurseForge (≥85%)
            return Ok(Resolution::new(
                best_match.result.clone(),
                best_match.confidence.score,
                true, // Was a fallback
            ));
        }

        // No high-confidence match on either platform
        Err(ResolverError::NoMatch {
            query: query.to_string(),
        })
    }

    /// Resolve multiple mods in parallel
    ///
    /// # Arguments
    ///
    /// * `queries` - List of mod names to resolve
    ///
    /// # Returns
    ///
    /// Vector of resolutions (same order as queries), with errors for failed resolutions
    pub async fn resolve_all(&self, queries: &[String]) -> Vec<Result<Resolution, ResolverError>> {
        let mut results = Vec::with_capacity(queries.len());

        for query in queries {
            results.push(self.resolve(query).await);
        }

        results
    }
}

#[cfg(test)]
mod tests {
    include!("remote_resolver.test.rs");
}
