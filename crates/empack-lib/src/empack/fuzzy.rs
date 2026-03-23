//! Fuzzy string matching for project search confidence scoring
//!
//! Extracted from `search.rs` to isolate matching logic from HTTP/API concerns.

/// Configuration constants from bash implementation
pub const MODRINTH_CONFIDENCE_THRESHOLD: u8 = 90;
pub const CURSEFORGE_CONFIDENCE_THRESHOLD: u8 = 85;
pub const MIN_DOWNLOAD_THRESHOLD: u64 = 1000;
pub const EXTRA_WORDS_MAX_RATIO: u8 = 150;

pub fn calculate_confidence(query: &str, found_title: &str, downloads: u64) -> u8 {
    let query_lower = query.to_lowercase();
    let found_lower = found_title.to_lowercase();

    if query_lower == found_lower {
        return 100;
    }

    if found_lower.contains(&query_lower) || query_lower.contains(&found_lower) {
        let base_score = 85;
        let download_boost = if downloads >= MIN_DOWNLOAD_THRESHOLD {
            5
        } else {
            0
        };
        return std::cmp::min(100, base_score + download_boost);
    }

    let distance = levenshtein_distance(&query_lower, &found_lower);
    let max_len = std::cmp::max(query.chars().count(), found_title.chars().count());

    if max_len == 0 {
        return 0;
    }

    let similarity = 100 - ((distance * 100) / max_len);
    let download_boost = if downloads >= MIN_DOWNLOAD_THRESHOLD {
        5
    } else {
        0
    };

    std::cmp::min(100, similarity + download_boost) as u8
}

pub fn has_extra_words(query: &str, found_title: &str) -> bool {
    let norm_query = query
        .to_lowercase()
        .chars()
        .filter(|&c| c != ' ' && c != '-' && c != '_' && c != '.')
        .collect::<String>();
    let norm_found = found_title
        .to_lowercase()
        .chars()
        .filter(|&c| c != ' ' && c != '-' && c != '_' && c != '.')
        .collect::<String>();

    if norm_query.is_empty() {
        return false;
    }

    let ratio = (norm_found.chars().count() * 100) / norm_query.chars().count();
    ratio > EXTRA_WORDS_MAX_RATIO as usize
}

pub fn levenshtein_distance(s1: &str, s2: &str) -> usize {
    let len1 = s1.chars().count();
    let len2 = s2.chars().count();
    let mut matrix = vec![vec![0; len2 + 1]; len1 + 1];

    for (i, row) in matrix.iter_mut().enumerate().take(len1 + 1) {
        row[0] = i;
    }
    for (j, value) in matrix[0].iter_mut().enumerate().take(len2 + 1) {
        *value = j;
    }

    for (i, c1) in s1.chars().enumerate() {
        for (j, c2) in s2.chars().enumerate() {
            let cost = if c1 == c2 { 0 } else { 1 };
            matrix[i + 1][j + 1] = std::cmp::min(
                std::cmp::min(
                    matrix[i][j + 1] + 1, // deletion
                    matrix[i + 1][j] + 1, // insertion
                ),
                matrix[i][j] + cost, // substitution
            );
        }
    }

    matrix[len1][len2]
}

#[cfg(test)]
mod tests {
    include!("fuzzy.test.rs");
}
