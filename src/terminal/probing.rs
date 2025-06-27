use crate::primitives::*;
use std::io::{self, IsTerminal, Read, Write};
use std::mem;
use std::time::{Duration, Instant};

use super::graphics::{TerminalGraphicsCaps, KittyGraphicsCaps, SixelCaps, ITerm2Caps, GraphicsDetectionMethod};
use super::capabilities::*;

#[cfg(unix)]
use std::os::fd::AsRawFd;

// ============================================================================
// CAPABILITY PROBING SYSTEM
// ============================================================================

pub struct CapabilityProber {
    timeout: Duration,
    raw_mode_guard: Option<RawModeGuard>,
}

impl CapabilityProber {
    pub fn new(timeout: Duration) -> Result<Self, TerminalError> {
        // Check both stdin AND stdout are TTYs - we need stdin for input and stdout for output
        let raw_mode_guard = if io::stdin().is_terminal() && io::stdout().is_terminal() {
            Some(setup_raw_mode()?)
        } else {
            None
        };

        Ok(Self {
            timeout,
            raw_mode_guard,
        })
    }

    pub fn probe_color_support(&self) -> Result<TerminalColorCaps, TerminalError> {
        if self.raw_mode_guard.is_none() {
            return Err(TerminalError::NotInteractive);
        }

        // Progressive probing: true color -> 256 -> 16 -> 8
        if self.probe_true_color()? {
            return Ok(TerminalColorCaps::TrueColor);
        }

        if self.probe_256_color()? {
            return Ok(TerminalColorCaps::Ansi256);
        }

        if self.probe_16_color()? {
            return Ok(TerminalColorCaps::Ansi16);
        }

        Ok(TerminalColorCaps::None)
    }

    pub fn probe_graphics_support(&self) -> Result<TerminalGraphicsCaps, TerminalError> {
        if self.raw_mode_guard.is_none() {
            return Ok(TerminalGraphicsCaps::None);
        }

        // Try Kitty protocol first (most common)
        if let Ok(kitty_caps) = self.probe_kitty_graphics() {
            return Ok(TerminalGraphicsCaps::Kitty(kitty_caps));
        }

        // Try Sixel
        if self.probe_sixel_support()? {
            return Ok(TerminalGraphicsCaps::Sixel(SixelCaps::default()));
        }

        // Try iTerm2
        if self.probe_iterm2_support()? {
            return Ok(TerminalGraphicsCaps::ITerm2(ITerm2Caps::default()));
        }

        Ok(TerminalGraphicsCaps::None)
    }

    pub fn probe_dimensions(&self) -> Result<TerminalDimensions, TerminalError> {
        // Try platform-specific methods first
        #[cfg(unix)]
        {
            if let Ok(dims) = self.probe_unix_dimensions() {
                return Ok(dims);
            }
        }

        #[cfg(windows)]
        {
            if let Ok(dims) = self.probe_windows_dimensions() {
                return Ok(dims);
            }
        }

        // Fallback to CSI query (cross-platform)
        if self.raw_mode_guard.is_some() {
            if let Ok(dims) = self.probe_csi_dimensions() {
                return Ok(dims);
            }
        }

        // Environment variable fallback
        self.probe_env_dimensions()
    }

    // Individual probe methods
    fn probe_true_color(&self) -> Result<bool, TerminalError> {
        // Enhanced true color probing using OSC 11 query
        let test_sequence = "\x1b]11;?\x1b\\";
        self.send_and_wait_for_response(test_sequence, |response| {
            response.contains("rgb:") || response.contains("rgba:")
        })
    }

    fn probe_256_color(&self) -> Result<bool, TerminalError> {
        // Test 256-color support with color setting and query
        let test_sequence = "\x1b[48;5;196m\x1b]11;?\x1b\\";
        self.send_and_wait_for_response(test_sequence, |response| {
            response.contains("rgb:") && !response.is_empty()
        })
    }

    fn probe_16_color(&self) -> Result<bool, TerminalError> {
        // Test basic 16-color support
        let test_sequence = "\x1b[41m\x1b]11;?\x1b\\";
        self.send_and_wait_for_response(test_sequence, |response| !response.is_empty())
    }

    fn probe_kitty_graphics(&self) -> Result<KittyGraphicsCaps, TerminalError> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let query_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_millis() as u32
            % 9000
            + 1000;
        let test_sequence = format!("\x1b_Gi={},s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\", query_id);

        if self.send_and_wait_for_response(&test_sequence, |response| {
            response.contains(&format!("_Gi={};OK", query_id))
        })? {
            // Protocol supported - now probe transmission methods
            self.probe_kitty_transmission_methods(query_id)
        } else {
            Err(TerminalError::UnsupportedGraphics {
                protocol: "kitty".to_string(),
            })
        }
    }

    fn probe_kitty_transmission_methods(
        &self,
        base_id: u32,
    ) -> Result<KittyGraphicsCaps, TerminalError> {
        let mut caps = KittyGraphicsCaps::default();
        caps.detection_method = GraphicsDetectionMethod::ProtocolProbe;

        // Test file transmission
        if self
            .test_kitty_file_transmission(base_id + 1)
            .unwrap_or(false)
        {
            caps.supports_file = true;
        }

        // Test temp file transmission
        if self
            .test_kitty_temp_transmission(base_id + 2)
            .unwrap_or(false)
        {
            caps.supports_temp_file = true;
        }

        Ok(caps)
    }

    fn test_kitty_file_transmission(&self, test_id: u32) -> Result<bool, TerminalError> {
        // Create minimal test sequence for file transmission
        let test_sequence = format!("\x1b_Gi={},a=q,t=f\x1b\\", test_id);
        self.send_and_wait_for_response(&test_sequence, |response| {
            response.contains(&format!("_Gi={};OK", test_id))
        })
    }

    fn test_kitty_temp_transmission(&self, test_id: u32) -> Result<bool, TerminalError> {
        // Create minimal test sequence for temp file transmission
        let test_sequence = format!("\x1b_Gi={},a=q,t=t\x1b\\", test_id);
        self.send_and_wait_for_response(&test_sequence, |response| {
            response.contains(&format!("_Gi={};OK", test_id))
        })
    }

    fn probe_sixel_support(&self) -> Result<bool, TerminalError> {
        // Test Sixel support with device attributes query
        let test_sequence = "\x1b[c";
        self.send_and_wait_for_response(test_sequence, |response| {
            response.contains(";4;") // Sixel support indicator
        })
    }

    fn probe_iterm2_support(&self) -> Result<bool, TerminalError> {
        // Test iTerm2 inline images support
        let test_sequence = "\x1b]1337;File=inline=1:\x1b\\";
        self.send_and_wait_for_response(test_sequence, |_response| {
            true // iTerm2 typically doesn't respond, so any response indicates support
        })
    }

    fn send_and_wait_for_response<F>(
        &self,
        sequence: &str,
        validator: F,
    ) -> Result<bool, TerminalError>
    where
        F: Fn(&str) -> bool,
    {
        io::stdout().write_all(sequence.as_bytes())?;
        io::stdout().flush()?;

        let response = self.read_terminal_response()?;

        // Comprehensive terminal state cleanup to prevent corruption
        // Reset all formatting and clear any pending escape sequences
        io::stdout().write_all(b"\x1b[0m")?; // Reset all attributes (colors, styles)
        io::stdout().write_all(b"\x1b[?25h")?; // Show cursor (in case it was hidden)
        io::stdout().write_all(b"\x1b[49m")?; // Reset background color specifically
        io::stdout().write_all(b"\x1b[39m")?; // Reset foreground color specifically
        io::stdout().flush()?;

        Ok(validator(&response))
    }

    fn read_terminal_response(&self) -> Result<String, TerminalError> {
        let start = Instant::now();
        let mut buffer = Vec::new();
        let mut stdin = io::stdin();

        while start.elapsed() < self.timeout {
            let mut byte = [0u8; 1];
            match stdin.read(&mut byte) {
                Ok(1) => {
                    buffer.push(byte[0]);
                    // Check for common escape sequence terminators
                    if buffer.len() >= 2 {
                        let last_two = &buffer[buffer.len() - 2..];
                        if last_two == [0x1b, 0x5c] || // ESC \
                           last_two == [0x07, 0x00] || // BEL
                           (buffer.len() >= 3 && &buffer[buffer.len()-3..] == [0x1b, 0x5c, 0x1b])
                        {
                            break;
                        }
                    }
                }
                Ok(0) => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(e) => return Err(TerminalError::ResponseReadFailed { source: e }),
                _ => {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }

        String::from_utf8(buffer).map_err(|e| TerminalError::InvalidUtf8Response { source: e })
    }

    #[cfg(unix)]
    fn probe_unix_dimensions(&self) -> Result<TerminalDimensions, TerminalError> {
        let mut winsize: libc::winsize = unsafe { mem::zeroed() };

        if unsafe { libc::ioctl(io::stdout().as_raw_fd(), libc::TIOCGWINSZ, &mut winsize) } == 0 {
            Ok(TerminalDimensions {
                cols: winsize.ws_col,
                rows: winsize.ws_row,
                width_pixels: if winsize.ws_xpixel > 0 {
                    Some(winsize.ws_xpixel)
                } else {
                    None
                },
                height_pixels: if winsize.ws_ypixel > 0 {
                    Some(winsize.ws_ypixel)
                } else {
                    None
                },
                detection_source: DimensionSource::Tiocgwinsz,
            })
        } else {
            Err(TerminalError::DimensionDetectionFailed {
                reason: "TIOCGWINSZ ioctl failed".to_string(),
            })
        }
    }

    #[cfg(windows)]
    fn probe_windows_dimensions(&self) -> Result<TerminalDimensions, TerminalError> {
        // Windows 10+ supports ANSI sequences - use CSI query
        self.probe_csi_dimensions()
    }

    fn probe_csi_dimensions(&self) -> Result<TerminalDimensions, TerminalError> {
        // Query window size in pixels (CSI 14 t)
        let test_sequence = "\x1b[14t";

        if let Ok(response) = self.read_terminal_response_after_send(test_sequence) {
            // Response format: ESC[4;height;widtht
            if let Some(start) = response.find("[4;") {
                if let Some(end) = response[start..].find('t') {
                    let data = &response[start + 3..start + end];
                    if let Some(semicolon) = data.find(';') {
                        let height: u16 = data[..semicolon].parse().unwrap_or(24);
                        let width: u16 = data[semicolon + 1..].parse().unwrap_or(80);

                        // Query character cell size (CSI 18 t)
                        let cell_query = "\x1b[18t";
                        if let Ok(cell_response) =
                            self.read_terminal_response_after_send(cell_query)
                        {
                            // Response format: ESC[8;rows;colst
                            if let Some(cell_start) = cell_response.find("[8;") {
                                if let Some(cell_end) = cell_response[cell_start..].find('t') {
                                    let cell_data =
                                        &cell_response[cell_start + 3..cell_start + cell_end];
                                    if let Some(cell_semicolon) = cell_data.find(';') {
                                        let rows: u16 =
                                            cell_data[..cell_semicolon].parse().unwrap_or(24);
                                        let cols: u16 =
                                            cell_data[cell_semicolon + 1..].parse().unwrap_or(80);

                                        return Ok(TerminalDimensions {
                                            cols,
                                            rows,
                                            width_pixels: Some(width),
                                            height_pixels: Some(height),
                                            detection_source: DimensionSource::CsiQuery,
                                        });
                                    }
                                }
                            }
                        }

                        // Fallback with just pixel dimensions
                        return Ok(TerminalDimensions {
                            cols: 80,
                            rows: 24,
                            width_pixels: Some(width),
                            height_pixels: Some(height),
                            detection_source: DimensionSource::CsiQuery,
                        });
                    }
                }
            }
        }

        Err(TerminalError::DimensionDetectionFailed {
            reason: "CSI dimension query failed".to_string(),
        })
    }

    fn probe_env_dimensions(&self) -> Result<TerminalDimensions, TerminalError> {
        let cols = std::env::var("COLUMNS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(80);
        let rows = std::env::var("LINES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(24);

        Ok(TerminalDimensions {
            cols,
            rows,
            width_pixels: None,
            height_pixels: None,
            detection_source: DimensionSource::Environment,
        })
    }

    fn read_terminal_response_after_send(&self, sequence: &str) -> Result<String, TerminalError> {
        io::stdout().write_all(sequence.as_bytes())?;
        io::stdout().flush()?;
        let response = self.read_terminal_response()?;

        // Ensure terminal state is clean after dimension queries
        io::stdout().write_all(b"\x1b[0m")?;
        io::stdout().flush()?;

        Ok(response)
    }
}
