//! Ranked search integration combining SearchProvider with fuzzy matching
//!
//! Provides confidence-based search with platform-specific thresholds and extra words filtering.

use crate::api::search::{SearchError, SearchProvider, SearchResult};
use crate::empack::fuzzy::{calculate_confidence, has_extra_words, meets_threshold, FuzzyMatch};

/// Search result with confidence score
#[derive(Debug, Clone)]
pub struct RankedSearchResult {
    /// Original search result
    pub result: SearchResult,
    /// Fuzzy match confidence
    pub confidence: FuzzyMatch,
}

impl RankedSearchResult {
    /// Create new ranked result
    pub fn new(result: SearchResult, confidence: FuzzyMatch) -> Self {
        Self { result, confidence }
    }

    /// Check if result meets platform threshold
    pub fn meets_threshold(&self) -> bool {
        meets_threshold(self.confidence.score, self.result.platform)
    }
}

/// Search with confidence filtering and ranking
///
/// # Algorithm
///
/// 1. Execute platform search via SearchProvider
/// 2. Calculate confidence for each result (fuzzy matching)
/// 3. Apply extra words rejection filter
/// 4. Apply platform-specific confidence thresholds
/// 5. Sort by confidence descending (highest first)
///
/// # Arguments
///
/// * `provider` - SearchProvider implementation (Modrinth, CurseForge, etc.)
/// * `query` - Search query string
/// * `limit` - Maximum results before filtering
///
/// # Returns
///
/// Ranked results sorted by confidence (high → low), filtered by threshold
///
/// # Examples
///
/// ```no_run
/// use empack_lib::api::ranked_search::search_with_confidence;
/// use empack_lib::api::search::MockSearchProvider;
/// use empack_lib::primitives::ModPlatform;
///
/// # tokio_test::block_on(async {
/// let provider = MockSearchProvider::new(ModPlatform::Modrinth);
/// let results = search_with_confidence(&provider, "JEI", 10).await.unwrap();
///
/// // Results are sorted by confidence
/// for (i, ranked) in results.iter().enumerate() {
///     if i > 0 {
///         assert!(ranked.confidence.score <= results[i-1].confidence.score);
///     }
/// }
/// # });
/// ```
pub async fn search_with_confidence<P>(
    provider: &P,
    query: &str,
    limit: u32,
) -> Result<Vec<RankedSearchResult>, SearchError>
where
    P: SearchProvider,
{
    // Execute search
    let results = provider.search(query, limit, 0).await?;

    if results.is_empty() {
        return Ok(Vec::new());
    }

    // Find max downloads for confidence calculation
    let max_downloads = results
        .iter()
        .map(|r| r.downloads)
        .max()
        .unwrap_or(0);

    // Calculate confidence for each result
    let mut ranked: Vec<RankedSearchResult> = results
        .into_iter()
        .map(|result| {
            let confidence = calculate_confidence(
                query,
                &result.title,
                &result.slug,
                result.downloads,
                max_downloads,
            );

            RankedSearchResult::new(result, confidence)
        })
        .collect();

    // Filter: Remove results with extra words
    ranked.retain(|r| !has_extra_words(query, &r.result.title));

    // Filter: Apply platform-specific confidence threshold
    let platform = provider.platform();
    ranked.retain(|r| meets_threshold(r.confidence.score, platform));

    // Sort by confidence descending (highest first)
    ranked.sort_by(|a, b| {
        b.confidence
            .score
            .partial_cmp(&a.confidence.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(ranked)
}

#[cfg(test)]
mod tests {
    include!("ranked_search.test.rs");
}
