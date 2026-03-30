use crate::Result;
use crate::application::session::FileSystemProvider;
use crate::empack::config::PackMetadata;
use anyhow::Context;
use handlebars::Handlebars;
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

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

/// Template system for modpack initialization and build-time variable substitution
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
    /// Create new template engine with embedded templates
    pub fn new() -> Self {
        let mut handlebars = Handlebars::new();

        handlebars.set_strict_mode(false);
        handlebars.register_escape_fn(handlebars::no_escape);

        let _ = handlebars.register_template_string(
            "gitignore",
            include_str!("../../templates/config/gitignore-clean.template"),
        );
        let _ = handlebars.register_template_string(
            "packwizignore",
            include_str!("../../templates/config/packwizignore.template"),
        );
        let _ = handlebars.register_template_string(
            "instance.cfg",
            include_str!("../../templates/client/instance.cfg.template"),
        );
        let _ = handlebars.register_template_string(
            "install_pack.sh",
            include_str!("../../templates/server/install_pack.sh.template"),
        );
        let _ = handlebars.register_template_string(
            "server.properties",
            include_str!("../../templates/server/server.properties.template"),
        );
        let _ = handlebars.register_template_string(
            "validate.yml",
            include_str!("../../templates/github/validate.yml.template"),
        );
        let _ = handlebars.register_template_string(
            "release.yml",
            include_str!("../../templates/github/release.yml.template"),
        );

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

    /// Set standard template variables from modpack metadata
    pub fn set_pack_variables(
        &mut self,
        name: &str,
        author: &str,
        mc_version: &str,
        version: &str,
    ) {
        self.set_variable("NAME", name);
        self.set_variable("AUTHOR", author);
        self.set_variable("MC_VERSION", mc_version);
        self.set_variable("VERSION", version);

        let safe_name = name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>();
        self.set_variable("SAFE_NAME", safe_name);
    }

    /// Set modloader variables (called separately when modloader info is available)
    pub fn set_modloader_variables(&mut self, modloader_name: &str, modloader_version: &str) {
        self.set_variable("MODLOADER_NAME", modloader_name);
        self.set_variable("MODLOADER_VERSION", modloader_version);
    }

    /// Load template variables from pack.toml for build-time rendering
    pub fn load_from_pack_toml<P: AsRef<Path>>(
        &mut self,
        pack_toml_path: P,
        filesystem: &dyn FileSystemProvider,
    ) -> Result<()> {
        let content = filesystem.read_to_string(pack_toml_path.as_ref())?;

        let pack: PackMetadata = toml::from_str(&content).with_context(|| {
            format!(
                "Failed to parse pack.toml: {}",
                pack_toml_path.as_ref().display()
            )
        })?;

        self.set_variable("NAME", &pack.name);
        if let Some(ref author) = pack.author {
            self.set_variable("AUTHOR", author);
        }
        if let Some(ref version) = pack.version {
            self.set_variable("VERSION", version);
        }

        self.extract_modloader_info(&pack);

        Ok(())
    }

    /// Extract modloader information from PackMetadata (all 4 supported: NeoForge > Fabric > Quilt > Forge)
    fn extract_modloader_info(&mut self, pack: &PackMetadata) {
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

        self.set_variable("MC_VERSION", &pack.versions.minecraft);
    }

    /// Render named template with current variables
    pub fn render_template(&self, template_name: &str) -> Result<String> {
        self.handlebars
            .render(template_name, &self.variables)
            .map_err(|e| TemplateError::RenderError {
                message: format!("Failed to render template '{}': {}", template_name, e),
            })
            .map_err(Into::into)
    }

    /// Render an arbitrary template string with current variables.
    /// Used by build-time template processing for user-provided template files.
    pub fn render_string(&self, template_content: &str) -> Result<String> {
        self.handlebars
            .render_template(template_content, &self.variables)
            .map_err(|e| TemplateError::RenderError {
                message: format!("Failed to render template string: {}", e),
            })
            .map_err(Into::into)
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

/// Template installer for modpack project scaffolding
pub struct TemplateInstaller<'a> {
    engine: TemplateEngine,
    filesystem: &'a dyn FileSystemProvider,
}

impl<'a> TemplateInstaller<'a> {
    /// Create new template installer with embedded templates
    pub fn new(filesystem: &'a dyn FileSystemProvider) -> Self {
        Self {
            engine: TemplateEngine::new(),
            filesystem,
        }
    }

    /// Get mutable access to the underlying engine for setting additional variables
    pub fn engine_mut(&mut self) -> &mut TemplateEngine {
        &mut self.engine
    }

    /// Configure template variables for modpack
    pub fn configure(&mut self, name: &str, author: &str, mc_version: &str, version: &str) {
        self.engine
            .set_pack_variables(name, author, mc_version, version);
    }

    /// Configure template variables from pack.toml for build-time rendering
    pub fn configure_from_pack_toml<P: AsRef<Path>>(&mut self, pack_toml_path: P) -> Result<()> {
        self.engine
            .load_from_pack_toml(pack_toml_path, self.filesystem)
    }

    /// Render template by name
    pub fn render_template(&self, template_name: &str) -> Result<String> {
        self.engine.render_template(template_name)
    }

    /// Install config templates (.gitignore, .packwizignore)
    pub fn install_config_templates<P: AsRef<Path>>(&self, target_dir: P) -> Result<()> {
        let base = target_dir.as_ref();

        let gitignore_content = self.engine.render_template("gitignore")?;
        self.filesystem
            .write_file(&base.join(".gitignore"), &gitignore_content)?;

        self.filesystem.create_dir_all(&base.join("pack"))?;
        let packwizignore_content = self.engine.render_template("packwizignore")?;
        self.filesystem
            .write_file(&base.join("pack/.packwizignore"), &packwizignore_content)?;

        Ok(())
    }

    /// Install GitHub workflow templates
    pub fn install_github_templates<P: AsRef<Path>>(&self, target_dir: P) -> Result<()> {
        let base = target_dir.as_ref();
        self.filesystem
            .create_dir_all(&base.join(".github/workflows"))?;

        let validate_content = self.engine.render_template("validate.yml")?;
        self.filesystem.write_file(
            &base.join(".github/workflows/validate.yml"),
            &validate_content,
        )?;

        let release_content = self.engine.render_template("release.yml")?;
        self.filesystem.write_file(
            &base.join(".github/workflows/release.yml"),
            &release_content,
        )?;

        Ok(())
    }

    /// Install client build templates
    pub fn install_client_templates<P: AsRef<Path>>(&self, target_dir: P) -> Result<()> {
        let base = target_dir.as_ref();
        self.filesystem
            .create_dir_all(&base.join("templates/client"))?;

        let instance_content = self.engine.render_template("instance.cfg")?;
        self.filesystem.write_file(
            &base.join("templates/client/instance.cfg.template"),
            &instance_content,
        )?;

        Ok(())
    }

    /// Install server build templates
    pub fn install_server_templates<P: AsRef<Path>>(&self, target_dir: P) -> Result<()> {
        let base = target_dir.as_ref();
        self.filesystem
            .create_dir_all(&base.join("templates/server"))?;

        let install_script_content = self.engine.render_template("install_pack.sh")?;
        self.filesystem.write_file(
            &base.join("templates/server/install_pack.sh.template"),
            &install_script_content,
        )?;

        let server_props_content = self.engine.render_template("server.properties")?;
        self.filesystem.write_file(
            &base.join("templates/server/server.properties.template"),
            &server_props_content,
        )?;

        Ok(())
    }

    /// Create standard modpack directory structure
    pub fn create_directory_structure<P: AsRef<Path>>(&self, target_dir: P) -> Result<()> {
        let base = target_dir.as_ref();

        let build_dirs = [
            "dist/client",
            "dist/client-full",
            "dist/server",
            "dist/server-full",
        ];
        for dir in &build_dirs {
            let dir_path = base.join(dir);
            self.filesystem.create_dir_all(&dir_path)?;
            self.filesystem.write_file(&dir_path.join(".gitkeep"), "")?;
        }

        for dir in &["templates/client", "templates/server"] {
            self.filesystem.create_dir_all(&base.join(dir))?;
        }

        for dir in &[".github/workflows"] {
            self.filesystem.create_dir_all(&base.join(dir))?;
        }

        self.filesystem.create_dir_all(&base.join("pack"))?;

        Ok(())
    }

    /// Install all templates and create complete project structure
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
