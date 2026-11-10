//! Finite, attributable Java-major policy, independent of installed JVMs and I/O.
//!
//! A profile ceiling is an admission policy, not proof that every newer JVM fails.
//! Unknown games have no recommendation; an explicit preference remains unqualified.

use crate::model::ModelError;
use alloc::{format, vec::Vec};

/// Identifies the policy embedded in resolved and distributed runtime requirements.
pub const POLICY_ID: &str = "empack-java-1";
/// Exact upstream snapshot underlying the initial admitted-major profiles.
pub const METADATA_REVISION: &str = "b8e2a3e1ba241414f8b861f13c9a6fd6bc5d922d";
const PROFILES: &str = include_str!("java_profiles.csv");

/// Known hard lower bound and finite admitted majors for one exact game identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaPolicy {
    historical_minimum: Option<u16>,
    required_minimum: u16,
    supported: Option<Vec<u16>>,
}

impl JavaPolicy {
    /// Look up an exact release/snapshot. No version-prefix or chronological guess.
    pub fn for_game(game: &str) -> Self {
        for line in PROFILES.lines().filter(|line| !line.starts_with('#')) {
            let mut fields = line.split('|');
            if fields.next() != Some(game) {
                continue;
            }
            let historical = fields.next().and_then(|value| value.parse::<u16>().ok());
            let supported = fields
                .next()
                .map(|value| value.split(',').filter_map(|n| n.parse().ok()).collect());
            return Self {
                historical_minimum: historical,
                required_minimum: historical.unwrap_or(8).max(8),
                supported,
            };
        }
        Self {
            historical_minimum: None,
            required_minimum: 8,
            supported: None,
        }
    }

    /// Historical game minimum; it can predate empack's Java 8 execution floor.
    pub fn historical_minimum(&self) -> Option<u16> {
        self.historical_minimum
    }

    /// Hard minimum from the game, native execution floor, and applied constraints.
    pub fn required_minimum(&self) -> u16 {
        self.required_minimum
    }

    /// Finite supported set, or unknown. A missing set never means all versions.
    pub fn supported(&self) -> Option<&[u16]> {
        self.supported.as_deref()
    }

    /// Highest admitted major. Host installations never alter this recommendation.
    pub fn recommended(&self) -> Option<u16> {
        self.supported
            .as_ref()
            .and_then(|values| values.last().copied())
    }

    /// Narrow an existing profile with a verified loader/mod/author constraint.
    /// Unknown upper support remains unknown; requirements cannot establish it.
    pub fn constrain(
        &self,
        minimum: u16,
        maximum: Option<u16>,
        source: &str,
    ) -> Result<Self, ModelError> {
        let minimum = minimum.max(self.required_minimum);
        if maximum.is_some_and(|maximum| maximum < minimum) {
            return Err(ModelError(format!(
                "Conflicting Java requirements from {source}: minimum {minimum}, maximum {maximum:?}"
            )));
        }
        let mut result = self.clone();
        result.required_minimum = minimum;
        if let Some(supported) = &mut result.supported {
            supported.retain(|major| *major >= minimum && maximum.is_none_or(|max| *major <= max));
            if supported.is_empty() {
                return Err(ModelError(format!(
                    "No admitted Java major satisfies {source}; profile supports {:?}, constraint is {minimum}..{maximum:?}",
                    self.supported
                )));
            }
        } else if maximum.is_some() {
            // The explicit bound remains relevant even without a known game profile.
            // Refuse to promote an arbitrary interval to supported game compatibility.
            return Err(ModelError(format!(
                "Cannot establish a supported Java range for this unknown runtime from {source} alone"
            )));
        }
        Ok(result)
    }

    /// Choose an admitted override or the recommendation. Unknown games require
    /// an explicit major, which is preserved as unqualified rather than inferred.
    pub fn select(&self, preferred: Option<u16>) -> Result<u16, ModelError> {
        let major = preferred.or_else(|| self.recommended()).ok_or_else(|| {
            ModelError("Java compatibility is unknown for this exact runtime; specify --java explicitly after checking its requirements".into())
        })?;
        if major < self.required_minimum {
            return Err(ModelError(format!(
                "Java {major} is below the required minimum {}",
                self.required_minimum
            )));
        }
        if self
            .supported
            .as_ref()
            .is_some_and(|values| !values.contains(&major))
        {
            return Err(ModelError(format!(
                "Java {major} is outside the admitted runtime policy {:?}; recommended Java {}. This is a support boundary, not proof every excluded JVM fails",
                self.supported,
                self.recommended().unwrap_or(major)
            )));
        }
        Ok(major)
    }

    /// Validate actual execution. Unknown profiles admit only the explicitly
    /// selected major and do not silently accept every newer installed Java.
    pub fn check_actual(&self, actual: u16, preferred: u16) -> Result<(), ModelError> {
        self.select(Some(preferred))?;
        if self.supported.is_none() && actual != preferred {
            return Err(ModelError(format!(
                "Java compatibility is unqualified; explicitly selected Java {preferred}, actual Java {actual}"
            )));
        }
        self.select(Some(actual)).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn historical_boundaries_and_recommendations() {
        for (game, historical, supported, recommended) in [
            ("1.0", 5, &[8][..], 8),
            ("1.5.2", 5, &[8][..], 8),
            ("1.6.1", 6, &[8][..], 8),
            ("1.11.2", 6, &[8][..], 8),
            ("1.12", 8, &[8][..], 8),
            ("1.16.5", 8, &[8][..], 8),
            ("1.17", 16, &[16, 17][..], 17),
            ("1.17.1", 16, &[16, 17][..], 17),
            ("1.18", 17, &[17][..], 17),
            ("1.20.4", 17, &[17][..], 17),
            ("1.20.5", 21, &[21][..], 21),
            ("1.21.11", 21, &[21][..], 21),
            ("26.1", 25, &[25][..], 25),
            ("26.3", 25, &[25][..], 25),
        ] {
            let policy = JavaPolicy::for_game(game);
            assert_eq!(policy.historical_minimum(), Some(historical), "{game}");
            assert_eq!(policy.supported(), Some(supported), "{game}");
            assert_eq!(policy.select(None).unwrap(), recommended, "{game}");
        }
    }

    #[test]
    fn exact_snapshot_boundaries_do_not_use_semver_or_prefix_guesses() {
        for (game, expected) in [
            ("21w18a", 8),
            ("21w19a", 17),
            ("1.18-pre1", 17),
            ("1.18-pre2", 17),
            ("24w13a", 17),
            ("24w14potato", 17),
            ("24w14a", 21),
            ("26.4-snapshot-8", 25),
        ] {
            assert_eq!(
                JavaPolicy::for_game(game).select(None).unwrap(),
                expected,
                "{game}"
            );
        }
        for unknown in [
            "1.17.99",
            "1.21.12",
            "26.9",
            "26.4-snapshot-999",
            "1.18-pre999",
            "custom",
        ] {
            let policy = JavaPolicy::for_game(unknown);
            assert_eq!(policy.supported(), None, "{unknown}");
            assert!(policy.select(None).unwrap_err().0.contains("unknown"));
            assert_eq!(policy.select(Some(21)).unwrap(), 21);
            assert!(policy.check_actual(25, 21).is_err());
        }
    }

    #[test]
    fn overrides_accept_supported_lower_majors_and_refuse_unsupported_versions() {
        let policy = JavaPolicy::for_game("1.17.1");
        assert_eq!(policy.select(Some(16)).unwrap(), 16);
        assert_eq!(policy.select(Some(17)).unwrap(), 17);
        assert!(policy.check_actual(16, 17).is_ok());
        assert!(policy.check_actual(17, 16).is_ok());
        for major in [0, 7, 8, 15, 18, 21, 25] {
            assert!(policy.select(Some(major)).is_err(), "{major}");
        }
    }

    #[test]
    fn constraints_narrow_without_recommending_outside_the_intersection() {
        let policy = JavaPolicy::for_game("1.17.1");
        let loader = policy.constrain(16, Some(16), "selected loader").unwrap();
        assert_eq!(loader.recommended(), Some(16));
        assert!(loader.select(Some(17)).is_err());
        assert!(
            loader
                .constrain(17, None, "selected mod")
                .unwrap_err()
                .0
                .contains("selected mod")
        );
        assert!(policy.constrain(18, None, "selected mod").is_err());
        let unknown = JavaPolicy::for_game("custom")
            .constrain(17, None, "verified metadata")
            .unwrap();
        assert!(unknown.select(None).is_err());
        assert!(unknown.select(Some(16)).is_err());
        assert_eq!(unknown.select(Some(21)).unwrap(), 21);
    }

    #[test]
    fn checked_in_profiles_are_unique_ordered_bounded_and_hash_attributed() {
        assert_eq!(
            PROFILES.lines().next().unwrap(),
            format!("# PrismLauncher/meta-launcher {METADATA_REVISION}")
        );
        let mut seen = alloc::collections::BTreeSet::new();
        for row in PROFILES.lines().filter(|line| !line.starts_with('#')) {
            let fields = row.split('|').collect::<Vec<_>>();
            assert_eq!(fields.len(), 4);
            assert!(seen.insert(fields[0]));
            assert_eq!(fields[3].len(), 64);
            assert!(fields[3].bytes().all(|b| b.is_ascii_hexdigit()));
            let policy = JavaPolicy::for_game(fields[0]);
            let values = policy.supported().unwrap();
            assert!(!values.is_empty());
            assert!(values.windows(2).all(|pair| pair[0] < pair[1]));
            for value in values {
                assert!(*value >= policy.required_minimum());
                policy
                    .check_actual(*value, policy.recommended().unwrap())
                    .unwrap();
            }
        }
        assert!(seen.len() >= 100);
    }
}
