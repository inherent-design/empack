use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Invalid resolution: {0}. Expected formats: '64', '64x', or '64x64'")]
    Resolution(String),

    #[error("Invalid shader loader: {0}. Expected: canvas, iris, optifine, or vanilla")]
    ShaderLoader(String),

    #[error("Invalid mod loader: {0}. Expected: neoforge, fabric, quilt, or forge")]
    ModLoader(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResourcePackResolution {
    X16,
    X32,
    X64,
    X128,
    X256,
    X512,
    X1024,
}

impl ResourcePackResolution {
    pub fn parse(input: &str) -> Result<Self, ParseError> {
        match input.trim().to_lowercase().as_str() {
            "16" | "16x" | "16x16" => Ok(Self::X16),
            "32" | "32x" | "32x32" => Ok(Self::X32),
            "64" | "64x" | "64x64" => Ok(Self::X64),
            "128" | "128x" | "128x128" => Ok(Self::X128),
            "256" | "256x" | "256x256" => Ok(Self::X256),
            "512" | "512x" | "512x512" => Ok(Self::X512),
            "1024" | "1024x" | "1024x1024" => Ok(Self::X1024),
            _ => Err(ParseError::Resolution(input.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ShaderLoader {
    Canvas,
    Iris,
    Optifine,
    Vanilla,
}

impl ShaderLoader {
    pub fn parse(input: &str) -> Result<Self, ParseError> {
        match input.trim().to_lowercase().as_str() {
            "canvas" => Ok(Self::Canvas),
            "iris" => Ok(Self::Iris),
            "optifine" => Ok(Self::Optifine),
            "vanilla" => Ok(Self::Vanilla),
            _ => Err(ParseError::ShaderLoader(input.to_string())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModLoader {
    #[serde(rename = "neoforge")]
    NeoForge,
    Fabric,
    Quilt,
    Forge,
}

impl ModLoader {
    pub fn parse(input: &str) -> Result<Self, ParseError> {
        match input.trim().to_lowercase().as_str() {
            "neoforge" => Ok(Self::NeoForge),
            "fabric" => Ok(Self::Fabric),
            "quilt" => Ok(Self::Quilt),
            "forge" => Ok(Self::Forge),
            _ => Err(ParseError::ModLoader(input.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("parsing.test.rs");
}
