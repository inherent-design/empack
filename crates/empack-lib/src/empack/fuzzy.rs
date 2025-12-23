//! Fuzzy matching algorithms for project search with confidence-based validation
//!
//! Port of v2 fuzzy matching algorithm using Levenshtein distance for string similarity.
//! Implements multi-factor confidence scoring to prevent false positives.
//!
//! # Algorithm Overview
//!
//! Confidence score combines:
//! - **String similarity** (70% weight) - Normalized Levenshtein distance
//! - **Download confidence** (30% weight) - Logarithmic popularity scaling
//!
//! Platform-specific thresholds:
//! - **Modrinth**: 90% confidence required (stricter, preferred platform)
//! - **CurseForge**: 85% confidence required (lower bar for fallback)
//!
//! # Examples
//!
//! ```
//! use empack_lib::empack::fuzzy::{calculate_confidence, has_extra_words, meets_threshold};
//! use empack_lib::primitives::ModPlatform;
//!
//! // Exact match
//! let conf = calculate_confidence("JEI", "JEI", "jei", 1000000, 1000000);
//! assert!(conf.score >= 0.9);
//!
//! // Extra words rejection
//! assert!(has_extra_words("Apotheosis", "Apotheosis Ascended"));
//! assert!(!has_extra_words("JEI", "Just Enough Items")); // JEI is acronym, not extra
//!
//! // Platform thresholds
//! assert!(meets_threshold(0.91, ModPlatform::Modrinth));
//! assert!(!meets_threshold(0.89, ModPlatform::Modrinth));
//! ```

use crate::primitives::ModPlatform;
use strsim::normalized_levenshtein;

/// Fuzzy match confidence score
///
/// Contains overall score and individual factor scores for debugging.
#[derive(Debug, Clone, PartialEq)]
pub struct FuzzyMatch {
    /// Overall confidence score (0.0-1.0)
    pub score: f64,
    /// String similarity component (0.0-1.0)
    pub string_similarity: f64,
    /// Download confidence component (0.0-1.0)
    pub download_confidence: f64,
}

/// Calculate fuzzy match confidence for a search result
///
/// # Algorithm
///
/// 1. **String similarity** (70% weight):
///    - Compare query against both title and slug
///    - Use normalized Levenshtein distance (0.0-1.0)
///    - Take maximum of title/slug similarity
///
/// 2. **Download confidence** (30% weight):
///    - Logarithmic scaling relative to max downloads
///    - Helps disambiguate mods with similar names
///    - Higher downloads = more likely correct match
///
/// 3. **Combined score**:
///    - `(string_similarity * 0.7) + (download_confidence * 0.3)`
///
/// # Arguments
///
/// * `query` - Search query string (user intent)
/// * `result_title` - Result's display title
/// * `result_slug` - Result's URL slug (often more normalized)
/// * `downloads` - Result's download count
/// * `max_downloads` - Maximum downloads across all results (for normalization)
///
/// # Examples
///
/// ```
/// use empack_lib::empack::fuzzy::calculate_confidence;
///
/// // Exact match with high downloads
/// let conf = calculate_confidence("Citadel", "Citadel", "citadel", 50_000_000, 50_000_000);
/// assert!(conf.score >= 0.95);
///
/// // Partial match with lower downloads
/// let conf = calculate_confidence("Apotheosis", "Apotheosis Ascended", "apotheosis-ascended", 5_000_000, 50_000_000);
/// assert!(conf.score < 0.90); // Should fail Modrinth threshold
/// ```
pub fn calculate_confidence(
    query: &str,
    result_title: &str,
    result_slug: &str,
    downloads: u64,
    max_downloads: u64,
) -> FuzzyMatch {
    // String similarity (70% weight)
    let title_similarity = normalized_levenshtein(query, result_title);
    let slug_similarity = normalized_levenshtein(query, result_slug);
    let string_similarity = title_similarity.max(slug_similarity);

    // Download confidence (30% weight) - logarithmic scale
    let download_confidence = if max_downloads > 0 && downloads > 0 {
        (downloads as f64).log10() / (max_downloads as f64).log10()
    } else {
        0.0
    };

    // Combined score: 70% string, 30% downloads
    let score = (string_similarity * 0.7) + (download_confidence * 0.3);

    FuzzyMatch {
        score,
        string_similarity,
        download_confidence,
    }
}

/// Reject matches with extra words beyond the query
///
/// Prevents false positives where result contains query as prefix/subset:
/// - "Apotheosis" vs "Apotheosis Ascended" → REJECT (extra words)
/// - "JEI" vs "Just Enough Items" → ACCEPT (acronym expansion, not extra words)
/// - "Create" vs "Create: Steam 'n' Rails" → REJECT (extra words)
///
/// # Algorithm
///
/// 1. Split query and result into words
/// 2. If result has more words than query:
///    - Check if all query words are present in result
///    - If yes → REJECT (extra words beyond query)
/// 3. Otherwise → ACCEPT
///
/// # Examples
///
/// ```
/// use empack_lib::empack::fuzzy::has_extra_words;
///
/// // Variants with extra words
/// assert!(has_extra_words("Apotheosis", "Apotheosis Ascended"));
/// assert!(has_extra_words("Create", "Create: Steam 'n' Rails"));
///
/// // Exact matches
/// assert!(!has_extra_words("JEI", "JEI"));
/// assert!(!has_extra_words("Fabric API", "Fabric API"));
///
/// // Acronym expansion (not extra words)
/// assert!(!has_extra_words("JEI", "Just Enough Items")); // Different words, not extra
/// ```
pub fn has_extra_words(query: &str, result: &str) -> bool {
    let query_lower = query.to_lowercase();
    let result_lower = result.to_lowercase();

    // Split into words
    let query_words: Vec<&str> = query_lower.split_whitespace().collect();
    let result_words: Vec<&str> = result_lower.split_whitespace().collect();

    // If result has more words than query, check if it's a variant
    if result_words.len() > query_words.len() {
        // Check if all query words are present in result
        

        // If query words are present but result has more words → extra words
        query_words.iter().all(|qw| {
            result_words
                .iter()
                .any(|rw| rw.contains(qw) || qw.contains(rw))
        })
    } else {
        false
    }
}

/// Apply platform-specific confidence threshold
///
/// Different platforms have different quality bars:
/// - **Modrinth**: 90% required (preferred platform, stricter)
/// - **CurseForge**: 85% required (fallback platform, lower bar)
///
/// # Examples
///
/// ```
/// use empack_lib::empack::fuzzy::meets_threshold;
/// use empack_lib::primitives::ModPlatform;
///
/// // Modrinth threshold (90%)
/// assert!(meets_threshold(0.91, ModPlatform::Modrinth));
/// assert!(!meets_threshold(0.89, ModPlatform::Modrinth));
///
/// // CurseForge threshold (85%)
/// assert!(meets_threshold(0.86, ModPlatform::CurseForge));
/// assert!(!meets_threshold(0.84, ModPlatform::CurseForge));
/// ```
pub fn meets_threshold(confidence: f64, platform: ModPlatform) -> bool {
    match platform {
        ModPlatform::Modrinth => confidence >= 0.90,
        ModPlatform::CurseForge => confidence >= 0.85,
    }
}

#[cfg(test)]
mod tests {
    include!("fuzzy.test.rs");
}
