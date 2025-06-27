// Enhanced graphics capabilities with Kitty protocol support
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub enum TerminalGraphicsCaps {
    None,
    Kitty(KittyGraphicsCaps),
    Sixel(SixelCaps),
    ITerm2(ITerm2Caps),
}

impl Default for TerminalGraphicsCaps {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub struct KittyGraphicsCaps {
    pub supports_direct: bool,
    pub supports_file: bool,
    pub supports_temp_file: bool,
    pub supports_shared_memory: bool,
    pub supports_animation: bool,
    pub supports_unicode_placeholders: bool,
    pub supports_z_index: bool,
    pub cell_width_pixels: u16,
    pub cell_height_pixels: u16,
    pub max_image_width: Option<u32>,
    pub max_image_height: Option<u32>,
    pub protocol_version: u8,
    pub detection_method: GraphicsDetectionMethod,
}

impl Default for KittyGraphicsCaps {
    fn default() -> Self {
        Self {
            supports_direct: true,
            supports_file: false,
            supports_temp_file: false,
            supports_shared_memory: false,
            supports_animation: false,
            supports_unicode_placeholders: false,
            supports_z_index: false,
            cell_width_pixels: 0,
            cell_height_pixels: 0,
            max_image_width: None,
            max_image_height: None,
            protocol_version: 1,
            detection_method: GraphicsDetectionMethod::ProtocolProbe,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub struct SixelCaps {
    pub max_colors: u16,
    pub max_width: Option<u16>,
    pub max_height: Option<u16>,
}

impl Default for SixelCaps {
    fn default() -> Self {
        Self {
            max_colors: 256,
            max_width: None,
            max_height: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub struct ITerm2Caps {
    pub supports_inline: bool,
    pub supports_file_download: bool,
}

impl Default for ITerm2Caps {
    fn default() -> Self {
        Self {
            supports_inline: true,
            supports_file_download: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub enum GraphicsDetectionMethod {
    EnvironmentReliable,  // TERM_PROGRAM=kitty
    EnvironmentVariables, // KITTY_WINDOW_ID, etc.
    ProtocolProbe,        // Escape sequence query
    ProtocolProbeTimeout, // Probe with no response
}
