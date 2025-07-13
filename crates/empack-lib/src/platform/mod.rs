pub mod capabilities;

use std::sync::OnceLock;
use thiserror::Error;

pub use capabilities::*;

/// Platform-specific system resource detection errors
#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("Failed to detect CPU cores: {reason}")]
    CpuDetectionFailed { reason: String },

    #[error("Failed to detect memory information: {reason}")]
    MemoryDetectionFailed { reason: String },

    #[error("Platform not supported: {platform}")]
    UnsupportedPlatform { platform: String },

    #[error("System call failed: {call} - {reason}")]
    SystemCallFailed { call: String, reason: String },
}

/// System resource information
#[derive(Debug, Clone)]
pub struct SystemResources {
    /// Number of logical CPU cores
    pub cpu_cores: u32,
    /// Memory pressure as a ratio (0.0 = no pressure, 1.0 = maximum pressure)
    pub memory_pressure: f32,
    /// Total system memory in bytes
    pub total_memory: u64,
    /// Available memory in bytes
    pub available_memory: u64,
}

impl SystemResources {
    /// Detect current system resources
    pub fn detect() -> Result<Self, PlatformError> {
        let cpu_cores = detect_cpu_cores()?;
        let (total_memory, available_memory) = detect_memory_info()?;

        // Calculate memory pressure (0.0 to 1.0)
        let memory_pressure = if total_memory > 0 {
            1.0 - (available_memory as f32 / total_memory as f32)
        } else {
            0.0
        };

        Ok(Self {
            cpu_cores,
            memory_pressure,
            total_memory,
            available_memory,
        })
    }

    /// Calculate optimal job parallelism based on resources
    /// More memory pressure = fewer jobs. Formula: j = 1 + cores/pressure
    pub fn calculate_optimal_jobs(&self, max_jobs: Option<u32>) -> u32 {
        if let Some(logger) = crate::logger::Logger::global() {
            logger.trace(
                &format!(
                    "Starting job calculation: {} CPU cores, {:.3} memory pressure, max_jobs: {:?}",
                    self.cpu_cores, self.memory_pressure, max_jobs
                ),
                None,
            );
        }

        // Ensure minimum memory pressure to avoid division issues
        let effective_memory_pressure = self.memory_pressure.max(0.01);
        if let Some(logger) = crate::logger::Logger::global() {
            logger.trace(
                &format!(
                    "Effective memory pressure: {:.3} (original: {:.3})",
                    effective_memory_pressure, self.memory_pressure
                ),
                None,
            );
        }

        // Scale CPU cores - more aggressive scaling under memory pressure
        let cpu_scaling_factor = if self.memory_pressure > 0.7 {
            // High memory pressure: be conservative
            let factor = self.cpu_cores as f32 * 0.5;
            if let Some(logger) = crate::logger::Logger::global() {
                logger.trace(
                    &format!(
                        "High memory pressure (>0.7): CPU scaling factor = {} * 0.5 = {}",
                        self.cpu_cores, factor
                    ),
                    None,
                );
            }
            factor
        } else if self.memory_pressure > 0.4 {
            // Moderate memory pressure: scale down
            let factor = self.cpu_cores as f32 * 0.75;
            if let Some(logger) = crate::logger::Logger::global() {
                logger.trace(
                    &format!(
                        "Moderate memory pressure (>0.4): CPU scaling factor = {} * 0.75 = {}",
                        self.cpu_cores, factor
                    ),
                    None,
                );
            }
            factor
        } else {
            // Low memory pressure: use most cores
            let factor = self.cpu_cores as f32;
            if let Some(logger) = crate::logger::Logger::global() {
                logger.trace(
                    &format!(
                        "Low memory pressure (≤0.4): CPU scaling factor = {} * 1.0 = {}",
                        self.cpu_cores, factor
                    ),
                    None,
                );
            }
            factor
        };

        // Jobs = 1 + cores/pressure
        let ratio = cpu_scaling_factor / effective_memory_pressure;
        let raw_optimal = 1.0 + ratio;
        if let Some(logger) = crate::logger::Logger::global() {
            logger.trace(
                &format!(
                    "Formula calculation: 1 + ({:.1} / {:.3}) = 1 + {:.2} = {:.2}",
                    cpu_scaling_factor, effective_memory_pressure, ratio, raw_optimal
                ),
                None,
            );
        }

        let optimal_jobs = raw_optimal
            .max(1.0) // At least 1 job
            .min(self.cpu_cores as f32) // No more than CPU cores
            as u32;

        if let Some(logger) = crate::logger::Logger::global() {
            logger.trace(
                &format!(
                    "After bounds: {:.2} -> max(1.0) -> min({}) -> {}",
                    raw_optimal, self.cpu_cores, optimal_jobs
                ),
                None,
            );
        }

        // Apply user-specified maximum if provided
        let final_jobs = if let Some(max) = max_jobs {
            let limited = optimal_jobs.min(max);
            if let Some(logger) = crate::logger::Logger::global() {
                logger.trace(
                    &format!(
                        "User max limit: {} -> min({}) = {}",
                        optimal_jobs, max, limited
                    ),
                    None,
                );
            }
            limited
        } else {
            if let Some(logger) = crate::logger::Logger::global() {
                logger.trace(
                    &format!("No user limit, final jobs: {}", optimal_jobs),
                    None,
                );
            }
            optimal_jobs
        };

        if let Some(logger) = crate::logger::Logger::global() {
            logger.trace(
                &format!(
                    "Job calculation complete: {} cores, {:.1}% memory pressure -> {} jobs",
                    self.cpu_cores,
                    self.memory_pressure * 100.0,
                    final_jobs
                ),
                None,
            );
        }

        final_jobs
    }
}

/// Global system resources cache (initialized on first use)
static SYSTEM_RESOURCES: OnceLock<SystemResources> = OnceLock::new();

/// Get cached system resources (auto-detects on first call)
pub fn system_resources() -> Result<&'static SystemResources, PlatformError> {
    SYSTEM_RESOURCES.get_or_init(|| {
        SystemResources::detect().unwrap_or_else(|_| {
            // Fallback with basic detection if full detection fails
            SystemResources {
                cpu_cores: std::thread::available_parallelism()
                    .map(|n| n.get() as u32)
                    .unwrap_or(1),
                memory_pressure: 0.0,
                total_memory: 0,
                available_memory: 0,
            }
        })
    });

    // Return cached result
    Ok(SYSTEM_RESOURCES.get().unwrap())
}

/// Force refresh of system resources cache
pub fn refresh_system_resources() -> Result<&'static SystemResources, PlatformError> {
    // OnceLock can't refresh - detection runs once per program
    system_resources()
}

// ============================================================================
// PLATFORM-SPECIFIC IMPLEMENTATIONS
// ============================================================================

#[cfg(target_family = "unix")]
fn detect_cpu_cores() -> Result<u32, PlatformError> {
    // Try sysconf first
    let cores = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
    if cores > 0 {
        return Ok(cores as u32);
    }

    // Fallback to std implementation
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .map_err(|e| PlatformError::CpuDetectionFailed {
            reason: format!("sysconf failed and available_parallelism failed: {}", e),
        })
}

#[cfg(target_family = "unix")]
fn detect_memory_info() -> Result<(u64, u64), PlatformError> {
    detect_memory_info_unix()
}

#[cfg(target_family = "windows")]
fn detect_memory_info() -> Result<(u64, u64), PlatformError> {
    detect_memory_info_windows()
}

// Unix memory detection with platform optimizations
fn detect_memory_info_unix() -> Result<(u64, u64), PlatformError> {
    // Platform-specific implementations with conditional compilation
    #[cfg(target_os = "macos")]
    {
        return detect_memory_info_macos();
    }

    #[cfg(target_os = "linux")]
    {
        return detect_memory_info_linux();
    }

    #[cfg(target_os = "freebsd")]
    {
        return detect_memory_info_freebsd();
    }

    #[cfg(target_os = "openbsd")]
    {
        return detect_memory_info_openbsd();
    }

    #[cfg(target_os = "netbsd")]
    {
        return detect_memory_info_netbsd();
    }

    // Generic Unix fallback for Solaris, AIX, DragonFly BSD, etc.
    #[cfg(not(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    )))]
    {
        detect_memory_info_unix_generic()
    }
}

#[cfg(target_os = "linux")]
fn detect_memory_info_linux() -> Result<(u64, u64), PlatformError> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").map_err(|e| {
        PlatformError::MemoryDetectionFailed {
            reason: format!("Failed to read /proc/meminfo: {}", e),
        }
    })?;

    let mut total_kb = None;
    let mut available_kb = None;

    for line in meminfo.lines() {
        if line.starts_with("MemTotal:") {
            total_kb = parse_meminfo_line(line, "MemTotal:");
        } else if line.starts_with("MemAvailable:") {
            available_kb = parse_meminfo_line(line, "MemAvailable:");
        }

        if total_kb.is_some() && available_kb.is_some() {
            break;
        }
    }

    let total = total_kb.ok_or_else(|| PlatformError::MemoryDetectionFailed {
        reason: "MemTotal not found in /proc/meminfo".to_string(),
    })? * 1024; // Convert KB to bytes

    let available = available_kb.ok_or_else(|| PlatformError::MemoryDetectionFailed {
        reason: "MemAvailable not found in /proc/meminfo".to_string(),
    })? * 1024; // Convert KB to bytes

    Ok((total, available))
}

#[cfg(target_os = "linux")]
fn parse_meminfo_line(line: &str, prefix: &str) -> Option<u64> {
    line.strip_prefix(prefix)?
        .trim()
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

#[cfg(target_os = "macos")]
fn detect_memory_info_macos() -> Result<(u64, u64), PlatformError> {
    use std::mem;
    use std::process::Command;
    use std::ptr;

    // Get total memory via sysctl
    let mut size = mem::size_of::<u64>();
    let mut total_memory: u64 = 0;

    let result = unsafe {
        libc::sysctlbyname(
            b"hw.memsize\0".as_ptr() as *const i8,
            &mut total_memory as *mut _ as *mut libc::c_void,
            &mut size,
            ptr::null_mut(),
            0,
        )
    };

    if result != 0 {
        return Err(PlatformError::SystemCallFailed {
            call: "sysctlbyname(hw.memsize)".to_string(),
            reason: "Failed to get total memory".to_string(),
        });
    }

    // Try using memory_pressure command first (most accurate)
    if let Ok(output) = Command::new("memory_pressure").output() {
        if output.status.success() {
            let output_str = String::from_utf8_lossy(&output.stdout);

            // Look for the official free percentage line
            for line in output_str.lines() {
                if line.contains("System-wide memory free percentage:") {
                    if let Some(percentage_str) = line.split(':').nth(1) {
                        if let Some(num_str) = percentage_str.trim().strip_suffix('%') {
                            if let Ok(free_percentage) = num_str.parse::<f64>() {
                                let available_memory =
                                    (total_memory as f64 * free_percentage / 100.0) as u64;
                                return Ok((total_memory, available_memory));
                            }
                        }
                    }
                }
            }

            // Fallback to status-based estimation if percentage not found
            if let Some(line) = output_str.lines().next() {
                if line.contains("normal") {
                    // Low pressure - estimate ~80% available
                    let available_memory = (total_memory as f64 * 0.8) as u64;
                    return Ok((total_memory, available_memory));
                } else if line.contains("warn") {
                    // Medium pressure - estimate ~30% available
                    let available_memory = (total_memory as f64 * 0.3) as u64;
                    return Ok((total_memory, available_memory));
                } else if line.contains("urgent") || line.contains("critical") {
                    // High pressure - estimate ~10% available
                    let available_memory = (total_memory as f64 * 0.1) as u64;
                    return Ok((total_memory, available_memory));
                }
            }
        }
    }

    // Fallback to vm_stat command (more accurate than mach APIs for pressure)
    if let Ok(output) = Command::new("vm_stat").output() {
        if output.status.success() {
            let output_str = String::from_utf8_lossy(&output.stdout);
            let mut pages_active = 0u64;
            let mut pages_free = 0u64;
            let mut pages_inactive = 0u64;
            let mut pages_wired = 0u64;

            for line in output_str.lines() {
                if line.starts_with("Pages active:") {
                    if let Some(val) = line.split_whitespace().nth(2) {
                        pages_active = val.trim_end_matches('.').parse().unwrap_or(0);
                    }
                } else if line.starts_with("Pages free:") {
                    if let Some(val) = line.split_whitespace().nth(2) {
                        pages_free = val.trim_end_matches('.').parse().unwrap_or(0);
                    }
                } else if line.starts_with("Pages inactive:") {
                    if let Some(val) = line.split_whitespace().nth(2) {
                        pages_inactive = val.trim_end_matches('.').parse().unwrap_or(0);
                    }
                } else if line.starts_with("Pages wired down:") {
                    if let Some(val) = line.split_whitespace().nth(3) {
                        pages_wired = val.trim_end_matches('.').parse().unwrap_or(0);
                    }
                }
            }

            if pages_active > 0 || pages_free > 0 {
                // macOS uses 16KB pages on Apple Silicon, 4KB on Intel
                let page_size = if std::env::consts::ARCH == "aarch64" {
                    16384
                } else {
                    4096
                };

                // More conservative calculation: free + half of inactive
                let available_pages = pages_free + (pages_inactive / 2);
                let available_memory = available_pages * page_size;

                return Ok((total_memory, available_memory));
            }
        }
    }

    // Final fallback to mach APIs (least accurate but always available)
    let mut vm_stat: libc::vm_statistics64 = unsafe { mem::zeroed() };
    let mut count = std::mem::size_of::<libc::vm_statistics64>() as libc::mach_msg_type_number_t
        / std::mem::size_of::<libc::integer_t>() as libc::mach_msg_type_number_t;

    let result = unsafe {
        libc::host_statistics64(
            libc::mach_host_self(),
            libc::HOST_VM_INFO64,
            &mut vm_stat as *mut _ as *mut libc::integer_t,
            &mut count,
        )
    };

    if result != libc::KERN_SUCCESS {
        return Err(PlatformError::SystemCallFailed {
            call: "host_statistics64".to_string(),
            reason: "Failed to get VM statistics".to_string(),
        });
    }

    // Conservative calculation: only count truly free + half of purgeable
    let page_size = if std::env::consts::ARCH == "aarch64" {
        16384
    } else {
        4096
    };
    let available_pages = vm_stat.free_count + (vm_stat.purgeable_count / 2);
    let available_memory = available_pages as u64 * page_size;

    Ok((total_memory, available_memory))
}

// FreeBSD memory detection using sysctl
fn detect_memory_info_freebsd() -> Result<(u64, u64), PlatformError> {
    use std::process::Command;

    // Get total memory via sysctl
    let total_output = Command::new("sysctl")
        .arg("-n")
        .arg("hw.physmem")
        .output()
        .map_err(|e| PlatformError::MemoryDetectionFailed {
            reason: format!("Failed to run sysctl hw.physmem: {}", e),
        })?;

    let total_memory: u64 = String::from_utf8_lossy(&total_output.stdout)
        .trim()
        .parse()
        .map_err(|e| PlatformError::MemoryDetectionFailed {
            reason: format!("Failed to parse total memory: {}", e),
        })?;

    // Get available memory via sysctl (free + inactive + cached)
    let free_output = Command::new("sysctl")
        .arg("-n")
        .arg("vm.stats.vm.v_free_count")
        .output()
        .map_err(|e| PlatformError::MemoryDetectionFailed {
            reason: format!("Failed to run sysctl vm.stats.vm.v_free_count: {}", e),
        })?;

    let inactive_output = Command::new("sysctl")
        .arg("-n")
        .arg("vm.stats.vm.v_inactive_count")
        .output()
        .map_err(|e| PlatformError::MemoryDetectionFailed {
            reason: format!("Failed to run sysctl vm.stats.vm.v_inactive_count: {}", e),
        })?;

    let free_pages: u64 = String::from_utf8_lossy(&free_output.stdout)
        .trim()
        .parse()
        .unwrap_or(0);

    let inactive_pages: u64 = String::from_utf8_lossy(&inactive_output.stdout)
        .trim()
        .parse()
        .unwrap_or(0);

    // FreeBSD typically uses 4KB pages
    let page_size = 4096u64;
    let available_memory = (free_pages + inactive_pages) * page_size;

    Ok((total_memory, available_memory))
}

// OpenBSD memory detection using sysctl
fn detect_memory_info_openbsd() -> Result<(u64, u64), PlatformError> {
    use std::process::Command;

    // Get total memory via sysctl
    let total_output = Command::new("sysctl")
        .arg("-n")
        .arg("hw.physmem")
        .output()
        .map_err(|e| PlatformError::MemoryDetectionFailed {
            reason: format!("Failed to run sysctl hw.physmem: {}", e),
        })?;

    let total_memory: u64 = String::from_utf8_lossy(&total_output.stdout)
        .trim()
        .parse()
        .map_err(|e| PlatformError::MemoryDetectionFailed {
            reason: format!("Failed to parse total memory: {}", e),
        })?;

    // OpenBSD: estimate available memory as 80% of total (conservative)
    // More sophisticated detection would require parsing uvmexp via sysctl
    let available_memory = (total_memory as f64 * 0.8) as u64;

    Ok((total_memory, available_memory))
}

// NetBSD memory detection using sysctl
fn detect_memory_info_netbsd() -> Result<(u64, u64), PlatformError> {
    use std::process::Command;

    // Get total memory via sysctl
    let total_output = Command::new("sysctl")
        .arg("-n")
        .arg("hw.physmem64")
        .output()
        .map_err(|e| PlatformError::MemoryDetectionFailed {
            reason: format!("Failed to run sysctl hw.physmem64: {}", e),
        })?;

    let total_memory: u64 = String::from_utf8_lossy(&total_output.stdout)
        .trim()
        .parse()
        .map_err(|e| PlatformError::MemoryDetectionFailed {
            reason: format!("Failed to parse total memory: {}", e),
        })?;

    // NetBSD: estimate available memory as 80% of total (conservative)
    // More sophisticated detection would require parsing uvm stats
    let available_memory = (total_memory as f64 * 0.8) as u64;

    Ok((total_memory, available_memory))
}

// Generic Unix fallback for Solaris, AIX, DragonFly BSD, etc.
fn detect_memory_info_unix_generic() -> Result<(u64, u64), PlatformError> {
    use std::process::Command;

    // Try common Unix commands for memory detection

    // Try getconf for total memory (POSIX)
    if let Ok(output) = Command::new("getconf").arg("_PHYS_PAGES").output() {
        if let Ok(pages_str) = String::from_utf8(output.stdout) {
            if let Ok(pages) = pages_str.trim().parse::<u64>() {
                if let Ok(output) = Command::new("getconf").arg("PAGESIZE").output() {
                    if let Ok(page_size_str) = String::from_utf8(output.stdout) {
                        if let Ok(page_size) = page_size_str.trim().parse::<u64>() {
                            let total_memory = pages * page_size;
                            // Conservative estimate: 70% available
                            let available_memory = (total_memory as f64 * 0.7) as u64;
                            return Ok((total_memory, available_memory));
                        }
                    }
                }
            }
        }
    }

    // Fallback: use std::thread::available_parallelism to estimate and assume 8GB
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);

    // Very conservative fallback: assume 2GB per core with 60% available
    let estimated_total = (cores as u64) * 2 * 1024 * 1024 * 1024;
    let estimated_available = (estimated_total as f64 * 0.6) as u64;

    Ok((estimated_total, estimated_available))
}

#[cfg(target_family = "windows")]
fn detect_cpu_cores() -> Result<u32, PlatformError> {
    use windows_sys::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};

    let mut system_info: SYSTEM_INFO = unsafe { std::mem::zeroed() };
    unsafe {
        GetSystemInfo(&mut system_info);
    }

    let logical_processors = system_info.dwNumberOfProcessors;
    if logical_processors > 0 {
        Ok(logical_processors)
    } else {
        // Fallback to std::thread::available_parallelism
        std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .map_err(|e| PlatformError::CpuDetectionFailed {
                reason: format!(
                    "GetSystemInfo returned 0 and available_parallelism failed: {}",
                    e
                ),
            })
    }
}

#[cfg(target_family = "windows")]
fn detect_memory_info_windows() -> Result<(u64, u64), PlatformError> {
    use windows_core::BOOL;
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    let mut memory_status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    memory_status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;

    let result: BOOL = unsafe { BOOL(GlobalMemoryStatusEx(&mut memory_status)) };

    if result == false {
        return Err(PlatformError::SystemCallFailed {
            call: "GlobalMemoryStatusEx".to_string(),
            reason: "Failed to get memory status".to_string(),
        });
    }

    let total_memory = memory_status.ullTotalPhys;
    let available_memory = memory_status.ullAvailPhys;

    Ok((total_memory, available_memory))
}

#[cfg(test)]
mod tests {
    include!("mod.test.rs");
}


