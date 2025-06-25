use crate::core::{AppConfig, primitives::*};

#[derive(Debug, Clone)]
pub struct TerminalCapabilities {
    pub color: TerminalColorCaps,
    pub unicode: TerminalUnicodeCaps,
    pub graphics: TerminalGraphicsCaps,
    pub width: Option<u16>,
    pub height: Option<u16>,
    pub is_tty: bool,
}

impl TerminalCapabilities {
    // pub fn detect_from_config(config: &AppConfig) -> Self {
    //     use console::Term;
    // }
}
