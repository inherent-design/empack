/// Macro to generate FromStr implementations for ValueEnum types
#[macro_export]
macro_rules! impl_fromstr_for_value_enum {
    ($enum_type:ty, $error_reason:expr) => {
        impl FromStr for $enum_type {
            type Err = $crate::primitives::ConfigError;

            fn from_str(s: &str) -> Result<Self, $crate::primitives::ConfigError> {
                for variant in Self::value_variants() {
                    if let Some(possible_value) = variant.to_possible_value() {
                        if possible_value.matches(s, false) {
                            return Ok(*variant);
                        }
                    }
                }

                Err($crate::primitives::ConfigError::ParseError {
                    value: s.to_string(),
                    reason: $error_reason.to_string(),
                })
            }
        }
    };
}

// Re-export for internal use
pub(crate) use impl_fromstr_for_value_enum;
