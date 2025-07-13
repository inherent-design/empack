//! Structured output display (tables, lists)
//!
//! Provides table and list formatting that adapts to terminal capabilities.

use super::styling::StyleManager;
use crate::terminal::TerminalCapabilities;
use std::cmp;

/// Structured display manager for tables and lists
pub struct StructuredDisplay<'a> {
    styling: &'a StyleManager,
    capabilities: &'a TerminalCapabilities,
}

impl<'a> StructuredDisplay<'a> {
    pub(crate) fn new(styling: &'a StyleManager, capabilities: &'a TerminalCapabilities) -> Self {
        Self { styling, capabilities }
    }

    /// Create a simple table
    /// 
    /// Example:
    /// ```
    /// Display::table()
    ///     .header(&["Tool", "Status", "Version"])
    ///     .row(&["packwiz", "✓", "v0.16.1"])
    ///     .row(&["Go", "✗", "not found"])
    ///     .render();
    /// ```
    pub fn table(&self) -> TableDisplay {
        TableDisplay::new(self.styling, self.capabilities)
    }

    /// Create a key-value list
    /// 
    /// Example:
    /// ```
    /// Display::structured()
    ///     .pairs(&[
    ///         ("Project", "my-modpack"),
    ///         ("Minecraft", "1.21.6"),
    ///         ("Modloader", "Fabric"),
    ///     ]);
    /// ```
    pub fn pairs(&self, pairs: &[(&str, &str)]) {
        let max_key_len = pairs.iter()
            .map(|(key, _)| key.len())
            .max()
            .unwrap_or(0);

        for (key, value) in pairs {
            println!("{:width$} | {}", 
                self.styling.style_subtle(key),
                value,
                width = max_key_len
            );
        }
    }

    /// Create a bulleted list
    pub fn list(&self, items: &[&str]) {
        for item in items {
            println!("{} {}", 
                self.styling.bullet(), 
                item
            );
        }
    }

    /// Create a numbered list
    pub fn numbered_list(&self, items: &[&str]) {
        for (i, item) in items.iter().enumerate() {
            println!("{}. {}", 
                self.styling.style_subtle(&(i + 1).to_string()), 
                item
            );
        }
    }
}

/// Table display builder
pub struct TableDisplay<'a> {
    styling: &'a StyleManager,
    capabilities: &'a TerminalCapabilities,
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    max_width: Option<usize>,
}

impl<'a> TableDisplay<'a> {
    fn new(styling: &'a StyleManager, capabilities: &'a TerminalCapabilities) -> Self {
        Self {
            styling,
            capabilities,
            headers: Vec::new(),
            rows: Vec::new(),
            max_width: None,
        }
    }

    /// Set table headers
    pub fn header(mut self, headers: &[&str]) -> Self {
        self.headers = headers.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Add a table row
    pub fn row(mut self, cells: &[&str]) -> Self {
        self.rows.push(cells.iter().map(|s| s.to_string()).collect());
        self
    }

    /// Set maximum table width (auto-detected if not set)
    pub fn max_width(mut self, width: usize) -> Self {
        self.max_width = Some(width);
        self
    }

    /// Render the table
    pub fn render(self) {
        if self.headers.is_empty() && self.rows.is_empty() {
            return;
        }

        let terminal_width = self.max_width
            .unwrap_or(self.capabilities.dimensions.cols as usize);

        // Calculate column widths
        let num_cols = if !self.headers.is_empty() {
            self.headers.len()
        } else if !self.rows.is_empty() {
            self.rows[0].len()
        } else {
            return;
        };

        let mut col_widths = vec![0; num_cols];

        // Calculate minimum required widths
        if !self.headers.is_empty() {
            for (i, header) in self.headers.iter().enumerate() {
                col_widths[i] = cmp::max(col_widths[i], header.len());
            }
        }

        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                if i < col_widths.len() {
                    col_widths[i] = cmp::max(col_widths[i], cell.len());
                }
            }
        }

        // Adjust for terminal width (simple truncation for now)
        let total_width: usize = col_widths.iter().sum::<usize>() + (num_cols - 1) * 3; // 3 chars for " | "
        if total_width > terminal_width {
            let excess = total_width - terminal_width;
            let reduction_per_col = excess / num_cols;
            for width in &mut col_widths {
                *width = width.saturating_sub(reduction_per_col);
            }
        }

        // Render header
        if !self.headers.is_empty() {
            self.render_row(&self.headers, &col_widths, true);
            self.render_separator(&col_widths);
        }

        // Render rows
        for row in &self.rows {
            self.render_row(row, &col_widths, false);
        }
    }

    fn render_row(&self, cells: &[String], widths: &[usize], is_header: bool) {
        let mut output = String::new();
        
        for (i, cell) in cells.iter().enumerate() {
            if i < widths.len() {
                let width = widths[i];
                let truncated = if cell.len() > width {
                    format!("{}...", &cell[..width.saturating_sub(3)])
                } else {
                    cell.clone()
                };

                let styled_cell = if is_header {
                    self.styling.style_emphasis(&truncated)
                } else {
                    truncated
                };

                output.push_str(&format!("{:width$}", styled_cell, width = width));
                
                if i < cells.len() - 1 {
                    output.push_str(" | ");
                }
            }
        }
        
        println!("{}", output);
    }

    fn render_separator(&self, widths: &[usize]) {
        let mut output = String::new();
        
        for (i, &width) in widths.iter().enumerate() {
            output.push_str(&"-".repeat(width));
            if i < widths.len() - 1 {
                output.push_str("---");
            }
        }
        
        println!("{}", self.styling.style_subtle(&output));
    }
}

