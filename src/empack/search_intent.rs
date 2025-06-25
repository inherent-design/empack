use crate::empack::parsing::{ModLoader, ResourcePackResolution, ShaderLoader};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};

#[derive(Debug, Clone)]
pub struct BaseSearchIntent {
    pub query_raw: String,
    pub minecraft_version: Option<String>,
}

impl BaseSearchIntent {
    pub fn new(query: &str, minecraft_version: Option<String>) -> Self {
        Self {
            query_raw: query.to_string(),
            minecraft_version,
        }
    }

    /// URL-encoded version for API calls
    pub fn query_encoded(&self) -> String {
        utf8_percent_encode(&self.query_raw, NON_ALPHANUMERIC).to_string()
    }

    /// Display-safe version (strips potential XSS but keeps readable)
    pub fn query_display(&self) -> String {
        html_escape::encode_text(&self.query_raw).to_string()
    }
}

#[derive(Debug, Clone)]
pub enum ProjectSearchIntent {
    Mod(ModSearchIntent),
    ResourcePack(ResourcePackSearchIntent),
    DataPack(DataPackSearchIntent),
    Shader(ShaderSearchIntent),
}

impl ProjectSearchIntent {
    /// Convenience constructor that defaults to Mod
    pub fn new(query: &str) -> Self {
        Self::Mod(ModSearchIntent {
            base: BaseSearchIntent::new(query, None),
            mod_loader: None,
        })
    }

    /// Explicit constructor for shader search
    pub fn shader(query: &str) -> Self {
        Self::Shader(ShaderSearchIntent {
            base: BaseSearchIntent::new(query, None),
            shader_loader: None,
        })
    }

    /// Explicit constructor for resource pack search
    pub fn resource_pack(query: &str) -> Self {
        Self::ResourcePack(ResourcePackSearchIntent {
            base: BaseSearchIntent::new(query, None),
            resolution: None,
        })
    }

    /// Explicit constructor for data pack search
    pub fn data_pack(query: &str) -> Self {
        Self::DataPack(DataPackSearchIntent {
            base: BaseSearchIntent::new(query, None),
        })
    }
}

#[derive(Debug, Clone)]
pub struct ModSearchIntent {
    pub base: BaseSearchIntent,
    pub mod_loader: Option<ModLoader>,
}

#[derive(Debug, Clone)]
pub struct ResourcePackSearchIntent {
    pub base: BaseSearchIntent,
    pub resolution: Option<ResourcePackResolution>,
}

#[derive(Debug, Clone)]
pub struct DataPackSearchIntent {
    pub base: BaseSearchIntent,
}

#[derive(Debug, Clone)]
pub struct ShaderSearchIntent {
    pub base: BaseSearchIntent,
    pub shader_loader: Option<ShaderLoader>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_search_intent_sanitization() {
        let intent = BaseSearchIntent::new("Create & <script>alert('xss')</script>", None);

        // Raw query preserved
        assert_eq!(intent.query_raw, "Create & <script>alert('xss')</script>");

        // URL encoded for API safety
        let encoded = intent.query_encoded();
        assert!(encoded.contains("%26")); // & becomes %26
        assert!(encoded.contains("%3C")); // < becomes %3C

        // HTML escaped for display safety
        let display = intent.query_display();
        assert!(display.contains("&amp;")); // & becomes &amp;
        assert!(display.contains("&lt;")); // < becomes &lt;
    }

    #[test]
    fn test_project_search_intent_defaults_to_mod() {
        let intent = ProjectSearchIntent::new("Create");

        match intent {
            ProjectSearchIntent::Mod(mod_intent) => {
                assert_eq!(mod_intent.base.query_raw, "Create");
                assert!(mod_intent.mod_loader.is_none());
            }
            _ => panic!("Expected Mod variant"),
        }
    }

    #[test]
    fn test_explicit_shader_constructor() {
        let intent = ProjectSearchIntent::shader("BSL Shaders");

        match intent {
            ProjectSearchIntent::Shader(shader_intent) => {
                assert_eq!(shader_intent.base.query_raw, "BSL Shaders");
                assert!(shader_intent.shader_loader.is_none());
            }
            _ => panic!("Expected Shader variant"),
        }
    }
}
