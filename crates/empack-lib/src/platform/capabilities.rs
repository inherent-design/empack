//! Cross-platform program capability detection
//!
//! Provides composable APIs for detecting available programs and their capabilities
//! across Windows and Unix systems.

use std::ffi::OsStr;
use std::process::Command;

/// Program detection result
#[derive(Debug, Clone, PartialEq)]
pub struct ProgramInfo {
    /// Program name
    pub name: String,
    /// Whether the program is available
    pub available: bool,
    /// Program version if detectable
    pub version: Option<String>,
    /// Full path to program if found
    pub path: Option<String>,
}

/// Cross-platform program finder
pub struct ProgramFinder;

impl ProgramFinder {
    /// Check if a program exists and get basic info
    pub fn find(program: &str) -> ProgramInfo {
        let path = Self::find_program_path(program);
        let available = path.is_some();
        
        ProgramInfo {
            name: program.to_string(),
            available,
            version: None,
            path,
        }
    }

    /// Check if a program exists and attempt to get version
    pub fn find_with_version(program: &str, version_args: &[&str]) -> ProgramInfo {
        let mut info = Self::find(program);
        
        if info.available {
            info.version = Self::get_program_version(program, version_args);
        }
        
        info
    }

    /// Find program path using platform-appropriate method
    fn find_program_path(program: &str) -> Option<String> {
        #[cfg(windows)]
        {
            Self::find_program_windows(program)
        }
        #[cfg(unix)]
        {
            Self::find_program_unix(program)
        }
    }

    #[cfg(windows)]
    fn find_program_windows(program: &str) -> Option<String> {
        // Try program name as-is first
        if let Some(path) = Self::try_where_command(program) {
            return Some(path);
        }
        
        // Try with .exe extension
        let exe_name = format!("{}.exe", program);
        Self::try_where_command(&exe_name)
    }

    #[cfg(windows)]
    fn try_where_command(program: &str) -> Option<String> {
        Command::new("where")
            .arg(program)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| {
                let stdout = String::from_utf8_lossy(&output.stdout);
                stdout.lines().next().map(|line| line.trim().to_string())
            })
    }

    #[cfg(unix)]
    fn find_program_unix(program: &str) -> Option<String> {
        Command::new("which")
            .arg(program)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let path = stdout.trim();
                if path.is_empty() {
                    None
                } else {
                    Some(path.to_string())
                }
            })
    }

    /// Get program version using specified arguments
    fn get_program_version(program: &str, version_args: &[&str]) -> Option<String> {
        Command::new(program)
            .args(version_args)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let version_text = stdout.trim();
                if version_text.is_empty() {
                    None
                } else {
                    Some(version_text.to_string())
                }
            })
    }
}

/// Archiver capability detector
pub struct ArchiverCapabilities;

impl ArchiverCapabilities {
    /// Detect available archive creation capabilities
    pub fn detect_creation() -> Vec<ProgramInfo> {
        let mut programs = Vec::new();
        
        #[cfg(windows)]
        {
            // Check PowerShell Compress-Archive
            if Self::has_powershell_compress() {
                programs.push(ProgramInfo {
                    name: "powershell-compress".to_string(),
                    available: true,
                    version: None,
                    path: Some("powershell".to_string()),
                });
            }
            
            // Check zip.exe
            programs.push(ProgramFinder::find("zip"));
        }
        
        #[cfg(unix)]
        {
            programs.push(ProgramFinder::find("zip"));
            programs.push(ProgramFinder::find("tar"));
        }
        
        programs
    }

    /// Detect available archive extraction capabilities
    pub fn detect_extraction() -> Vec<ProgramInfo> {
        let mut programs = Vec::new();
        
        #[cfg(windows)]
        {
            // Check PowerShell Expand-Archive
            if Self::has_powershell_expand() {
                programs.push(ProgramInfo {
                    name: "powershell-expand".to_string(),
                    available: true,
                    version: None,
                    path: Some("powershell".to_string()),
                });
            }
            
            // Check unzip.exe
            programs.push(ProgramFinder::find("unzip"));
        }
        
        #[cfg(unix)]
        {
            programs.push(ProgramFinder::find("unzip"));
            programs.push(ProgramFinder::find("tar"));
        }
        
        programs
    }

    #[cfg(windows)]
    fn has_powershell_compress() -> bool {
        Command::new("powershell")
            .args(&["-Command", "Get-Command Compress-Archive -ErrorAction SilentlyContinue"])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    #[cfg(windows)]
    fn has_powershell_expand() -> bool {
        Command::new("powershell")
            .args(&["-Command", "Get-Command Expand-Archive -ErrorAction SilentlyContinue"])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }
}

/// Java runtime detector
pub struct JavaCapabilities;

impl JavaCapabilities {
    /// Detect Java runtime information
    pub fn detect() -> Vec<ProgramInfo> {
        let mut javas = Vec::new();
        
        // Check standard java command
        let java_info = ProgramFinder::find_with_version("java", &["--version"]);
        javas.push(java_info);
        
        // Check javac if java is available
        if javas[0].available {
            javas.push(ProgramFinder::find_with_version("javac", &["--version"]));
        }
        
        javas
    }
}

/// Go toolchain detector  
pub struct GoCapabilities;

impl GoCapabilities {
    /// Detect Go toolchain
    pub fn detect() -> ProgramInfo {
        ProgramFinder::find_with_version("go", &["version"])
    }
}

/// Batch capability checker for common development tools
pub struct ToolchainCapabilities;

impl ToolchainCapabilities {
    /// Check all common development tools
    pub fn detect_all() -> ToolchainSummary {
        ToolchainSummary {
            go: GoCapabilities::detect(),
            java: JavaCapabilities::detect(),
            archivers_create: ArchiverCapabilities::detect_creation(),
            archivers_extract: ArchiverCapabilities::detect_extraction(),
        }
    }
}

/// Summary of detected toolchain capabilities
#[derive(Debug)]
pub struct ToolchainSummary {
    pub go: ProgramInfo,
    pub java: Vec<ProgramInfo>,
    pub archivers_create: Vec<ProgramInfo>,
    pub archivers_extract: Vec<ProgramInfo>,
}

impl ToolchainSummary {
    /// Check if basic archiving is available
    pub fn can_create_archives(&self) -> bool {
        self.archivers_create.iter().any(|p| p.available)
    }
    
    /// Check if basic extraction is available
    pub fn can_extract_archives(&self) -> bool {
        self.archivers_extract.iter().any(|p| p.available)
    }
    
    /// Check if Go toolchain is available
    pub fn has_go(&self) -> bool {
        self.go.available
    }
    
    /// Check if Java runtime is available
    pub fn has_java(&self) -> bool {
        self.java.iter().any(|p| p.available)
    }
}