use std::collections::HashMap;
use std::path::Path;
use crate::Result;
use handlebars::Handlebars;
use thiserror::Error;
use crate::empack::config::PackMetadata;

/// Template system errors
#[derive(Debug, Error)]
pub enum TemplateError {
    #[error("IO error: {source}")]
    IoError {
        #[from]
        source: std::io::Error,
    },

    #[error("Template rendering error: {message}")]
    RenderError { message: String },

    #[error("Template not found: {name}")]
    TemplateNotFound { name: String },

    #[error("Pack.toml parsing error: {source}")]
    PackTomlError {
        #[from]
        source: toml::de::Error,
    },
}

/// Template system for V1-compatible modpack initialization
/// 
/// Uses embedded templates with handlebars engine for compatibility
/// with V1's `{{VARIABLE}}` pattern.
pub struct TemplateEngine {
    handlebars: Handlebars<'static>,
    variables: HashMap<String, String>,
}

impl Default for TemplateEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TemplateEngine {
    /// Create new template engine with embedded V1-compatible templates
    pub fn new() -> Self {
        let mut handlebars = Handlebars::new();
        
        // Configure for V1 compatibility
        handlebars.set_strict_mode(false); // Allow missing variables
        
        // Register embedded templates using include_str!
        let _ = handlebars.register_template_string("gitignore", include_str!("../../templates/config/gitignore-clean.template"));
        let _ = handlebars.register_template_string("packwizignore", include_str!("../../templates/config/packwizignore.template"));
        let _ = handlebars.register_template_string("instance.cfg", include_str!("../../templates/client/instance.cfg.template"));
        let _ = handlebars.register_template_string("install_pack.sh", include_str!("../../templates/server/install_pack.sh.template"));
        let _ = handlebars.register_template_string("server.properties", include_str!("../../templates/server/server.properties.template"));
        let _ = handlebars.register_template_string("validate.yml", include_str!("../../templates/github/validate.yml.template"));
        
        Self {
            handlebars,
            variables: HashMap::new(),
        }
    }
    
    /// Set template variable for substitution
    pub fn set_variable<K, V>(&mut self, key: K, value: V) 
    where 
        K: Into<String>,
        V: Into<String>,
    {
        self.variables.insert(key.into(), value.into());
    }
    
    /// Set multiple template variables from HashMap
    pub fn set_variables(&mut self, vars: HashMap<String, String>) {
        self.variables.extend(vars);
    }
    
    /// Set default V1-compatible variables for modpack initialization
    pub fn set_pack_variables(&mut self, name: &str, author: &str, mc_version: &str, version: &str) {
        self.set_variable("NAME", name);
        self.set_variable("AUTHOR", author);
        self.set_variable("MC_VERSION", mc_version);
        self.set_variable("VERSION", version);
        
        // Generate safe identifiers
        let safe_name = name.to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>();
        self.set_variable("SAFE_NAME", safe_name);
    }
    
    /// Load V1-compatible variables from pack.toml for build-time rendering
    pub fn load_from_pack_toml<P: AsRef<Path>>(&mut self, pack_toml_path: P) -> Result<()> {
        let content = std::fs::read_to_string(&pack_toml_path)
            .with_context(|| format!("Failed to read pack.toml: {}", pack_toml_path.as_ref().display()))?;
        
        let pack: PackMetadata = toml::from_str(&content)
            .with_context(|| format!("Failed to parse pack.toml: {}", pack_toml_path.as_ref().display()))?;
        
        // Extract V1-compatible template variables (NAME, AUTHOR, VERSION only)
        self.set_variable("NAME", &pack.name);
        if let Some(ref author) = pack.author {
            self.set_variable("AUTHOR", author);
        }
        if let Some(ref version) = pack.version {
            self.set_variable("VERSION", version);
        }
        
        // Add modloader info from pack.toml for build-time context
        self.extract_modloader_info(&pack);
        
        Ok(())
    }
    
    /// Extract modloader information from PackMetadata (all 4 supported: NeoForge > Fabric > Quilt > Forge)
    fn extract_modloader_info(&mut self, pack: &PackMetadata) {
        // V1 preference order: NeoForge > Fabric > Quilt > Forge
        if let Some(neoforge_version) = pack.versions.loader_versions.get("neoforge") {
            self.set_variable("MODLOADER_NAME", "neoforge");
            self.set_variable("MODLOADER_VERSION", neoforge_version);
        } else if let Some(fabric_version) = pack.versions.loader_versions.get("fabric") {
            self.set_variable("MODLOADER_NAME", "fabric");
            self.set_variable("MODLOADER_VERSION", fabric_version);
        } else if let Some(quilt_version) = pack.versions.loader_versions.get("quilt") {
            self.set_variable("MODLOADER_NAME", "quilt");
            self.set_variable("MODLOADER_VERSION", quilt_version);
        } else if let Some(forge_version) = pack.versions.loader_versions.get("forge") {
            self.set_variable("MODLOADER_NAME", "forge");
            self.set_variable("MODLOADER_VERSION", forge_version);
        }
        
        // Always set MC version from pack.toml
        self.set_variable("MC_VERSION", &pack.versions.minecraft);
    }
    
    /// Render named template with current variables
    pub fn render_template(&self, template_name: &str) -> Result<String, TemplateError> {
        self.handlebars
            .render(template_name, &self.variables)
            .map_err(|e| TemplateError::RenderError {
                message: format!("Failed to render template '{}': {}", template_name, e),
            })
    }
    
    /// Get list of available template names
    pub fn template_names(&self) -> Vec<String> {
        self.handlebars.get_templates().keys().cloned().collect()
    }
    
    /// Get current template variables (for debugging)
    pub fn variables(&self) -> &HashMap<String, String> {
        &self.variables
    }
}

/// Template installer for V1-compatible modpack setup
pub struct TemplateInstaller {
    engine: TemplateEngine,
}

impl Default for TemplateInstaller {
    fn default() -> Self {
        Self::new()
    }
}

impl TemplateInstaller {
    /// Create new template installer with embedded templates
    pub fn new() -> Self {
        Self {
            engine: TemplateEngine::new(),
        }
    }
    
    /// Configure template variables for modpack
    pub fn configure(&mut self, name: &str, author: &str, mc_version: &str, version: &str) {
        self.engine.set_pack_variables(name, author, mc_version, version);
    }
    
    /// Configure template variables from pack.toml for build-time rendering
    pub fn configure_from_pack_toml<P: AsRef<Path>>(&mut self, pack_toml_path: P) -> Result<(), TemplateError> {
        let content = std::fs::read_to_string(&pack_toml_path)?;
        let pack: PackMetadata = toml::from_str(&content)?;
        
        // Extract V1-compatible template variables
        self.engine.set_variable("NAME", &pack.name);
        if let Some(ref author) = pack.author {
            self.engine.set_variable("AUTHOR", author);
        }
        if let Some(ref version) = pack.version {
            self.engine.set_variable("VERSION", version);
        }
        
        // Add modloader info
        if let Some(neoforge_version) = pack.versions.loader_versions.get("neoforge") {
            self.engine.set_variable("MODLOADER_NAME", "neoforge");
            self.engine.set_variable("MODLOADER_VERSION", neoforge_version);
        } else if let Some(fabric_version) = pack.versions.loader_versions.get("fabric") {
            self.engine.set_variable("MODLOADER_NAME", "fabric");
            self.engine.set_variable("MODLOADER_VERSION", fabric_version);
        } else if let Some(quilt_version) = pack.versions.loader_versions.get("quilt") {
            self.engine.set_variable("MODLOADER_NAME", "quilt");
            self.engine.set_variable("MODLOADER_VERSION", quilt_version);
        } else if let Some(forge_version) = pack.versions.loader_versions.get("forge") {
            self.engine.set_variable("MODLOADER_NAME", "forge");
            self.engine.set_variable("MODLOADER_VERSION", forge_version);
        }
        
        self.engine.set_variable("MC_VERSION", &pack.versions.minecraft);
        
        Ok(())
    }

    /// Render template by name
    pub fn render_template(&self, template_name: &str) -> Result<String, TemplateError> {
        self.engine.render_template(template_name)
    }
    
    /// Install config templates (.gitignore, .packwizignore)
    pub fn install_config_templates<P: AsRef<Path>>(&self, target_dir: P) -> Result<()> {
        let base = target_dir.as_ref();
        
        // .gitignore
        let gitignore_content = self.engine.render_template("gitignore")?;
        std::fs::write(base.join(".gitignore"), gitignore_content)?;
        
        // pack/.packwizignore
        std::fs::create_dir_all(base.join("pack"))?;
        let packwizignore_content = self.engine.render_template("packwizignore")?;
        std::fs::write(base.join("pack/.packwizignore"), packwizignore_content)?;
        
        Ok(())
    }
    
    /// Install GitHub workflow templates
    pub fn install_github_templates<P: AsRef<Path>>(&self, target_dir: P) -> Result<()> {
        let base = target_dir.as_ref();
        std::fs::create_dir_all(base.join(".github/workflows"))?;
        
        let validate_content = self.engine.render_template("validate.yml")?;
        std::fs::write(base.join(".github/workflows/validate.yml"), validate_content)?;
        
        Ok(())
    }
    
    /// Install client build templates
    pub fn install_client_templates<P: AsRef<Path>>(&self, target_dir: P) -> Result<()> {
        let base = target_dir.as_ref();
        std::fs::create_dir_all(base.join("templates/client"))?;
        
        let instance_content = self.engine.render_template("instance.cfg")?;
        std::fs::write(base.join("templates/client/instance.cfg.template"), instance_content)?;
        
        Ok(())
    }
    
    /// Install server build templates
    pub fn install_server_templates<P: AsRef<Path>>(&self, target_dir: P) -> Result<()> {
        let base = target_dir.as_ref();
        std::fs::create_dir_all(base.join("templates/server"))?;
        
        let install_script_content = self.engine.render_template("install_pack.sh")?;
        std::fs::write(base.join("templates/server/install_pack.sh.template"), install_script_content)?;
        
        let server_props_content = self.engine.render_template("server.properties")?;
        std::fs::write(base.join("templates/server/server.properties.template"), server_props_content)?;
        
        Ok(())
    }
    
    /// Create layer_1-compatible directory structure
    pub fn create_directory_structure<P: AsRef<Path>>(&self, target_dir: P) -> Result<()> {
        let base = target_dir.as_ref();
        
        // Build output directories with .gitkeep files
        let build_dirs = ["dist/client", "dist/client-full", "dist/server", "dist/server-full"];
        for dir in &build_dirs {
            let dir_path = base.join(dir);
            std::fs::create_dir_all(&dir_path)?;
            std::fs::write(dir_path.join(".gitkeep"), "")?;
        }
        
        // Template directories
        let template_dirs = ["templates/client", "templates/server"];
        for dir in &template_dirs {
            std::fs::create_dir_all(base.join(dir))?;
        }
        
        // GitHub directories
        let github_dirs = [".github/workflows"];
        for dir in &github_dirs {
            std::fs::create_dir_all(base.join(dir))?;
        }
        
        // Installer and pack directories
        std::fs::create_dir_all(base.join("installer"))?;
        std::fs::create_dir_all(base.join("pack"))?;
        
        Ok(())
    }
    
    /// Install all templates and create complete layer_1-compatible structure
    pub fn install_all<P: AsRef<Path>>(&self, target_dir: P) -> Result<()> {
        self.create_directory_structure(&target_dir)?;
        self.install_config_templates(&target_dir)?;
        self.install_github_templates(&target_dir)?;
        self.install_client_templates(&target_dir)?;
        self.install_server_templates(&target_dir)?;
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    include!("templates.test.rs");
}
