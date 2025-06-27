use std::str::FromStr;
use clap::ValueEnum;
use serde::Deserialize;
use thiserror::Error;

use crate::impl_fromstr_for_value_enum;
use crate::primitives::ConfigError;

/// Shared platform detection primitive types and behaviors
/// This module defines the interface layer for platform capabilities,
/// while platform/mod.rs handles the actual implementation and orchestration.

/// CPU core detection strategy for resource calculation
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CpuDetectionStrategy {
    /// Use system calls (sysconf on Unix, GetSystemInfo on Windows)
    #[value(alias = "system")]
    SystemCall,
    /// Use Rust standard library thread::available_parallelism
    #[value(alias = "std")]
    StandardLibrary,
    /// Try system calls first, fallback to standard library
    #[value(alias = "auto")]
    Automatic,
}

impl_fromstr_for_value_enum!(CpuDetectionStrategy, "cpu detection strategy");

impl Default for CpuDetectionStrategy {
    fn default() -> Self {
        Self::Automatic
    }
}

/// Memory pressure calculation method for resource management
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryPressureMethod {
    /// Calculate based on available vs total memory
    #[value(alias = "available")]
    Available,
    /// Use platform-specific pressure metrics (Linux: pressure stall info, macOS: VM stats)
    #[value(alias = "platform")]
    PlatformSpecific,
    /// Combine multiple metrics for comprehensive pressure assessment
    #[value(alias = "combined")]
    Combined,
}

impl_fromstr_for_value_enum!(MemoryPressureMethod, "memory pressure method");

impl Default for MemoryPressureMethod {
    fn default() -> Self {
        Self::Available
    }
}

/// Resource calculation algorithm variant
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceCalculationAlgorithm {
    /// Basic formula: j = 1 / (memory_pressure * cpu_scaling)
    #[value(alias = "basic")]
    Basic,
    /// Conservative approach: reduces jobs under memory pressure
    #[value(alias = "conservative")]
    Conservative,
    /// Aggressive approach: maximizes parallelism when possible
    #[value(alias = "aggressive")]
    Aggressive,
    /// Adaptive approach: adjusts based on system characteristics
    #[value(alias = "adaptive")]
    Adaptive,
}

impl_fromstr_for_value_enum!(ResourceCalculationAlgorithm, "resource calculation algorithm");

impl Default for ResourceCalculationAlgorithm {
    fn default() -> Self {
        Self::Conservative
    }
}

/// Platform capability flags for feature detection
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformCapabilities {
    /// Can detect CPU core count
    pub cpu_detection: bool,
    /// Can detect memory information
    pub memory_detection: bool,
    /// Supports real-time memory pressure monitoring
    pub memory_pressure_monitoring: bool,
    /// Can detect system load averages
    pub load_average_detection: bool,
    /// Platform-specific optimizations available
    pub platform_optimizations: bool,
}

impl Default for PlatformCapabilities {
    fn default() -> Self {
        Self {
            cpu_detection: true,
            memory_detection: true,
            memory_pressure_monitoring: false,
            load_average_detection: false,
            platform_optimizations: false,
        }
    }
}

/// Platform information summary for display and debugging
#[derive(Debug, Clone)]
pub struct PlatformInfo {
    /// Operating system name
    pub os_name: String,
    /// Architecture (x86_64, arm64, etc.)
    pub arch: String,
    /// Platform family (unix, windows)
    pub family: String,
    /// Detected capabilities
    pub capabilities: PlatformCapabilities,
}

impl PlatformInfo {
    /// Create platform info from environment
    pub fn detect() -> Self {
        Self {
            os_name: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            family: std::env::consts::FAMILY.to_string(),
            capabilities: PlatformCapabilities::default(),
        }
    }
    
    /// Get a human-readable platform description
    pub fn description(&self) -> String {
        format!("{}/{} ({})", self.os_name, self.arch, self.family)
    }
    
    /// Check if running on supported platform
    pub fn is_supported(&self) -> bool {
        matches!(self.family.as_str(), "unix" | "windows")
    }
}

/// Resource scaling configuration for job calculation
#[derive(Debug, Clone)]
pub struct ResourceScalingConfig {
    /// CPU detection strategy
    pub cpu_strategy: CpuDetectionStrategy,
    /// Memory pressure calculation method
    pub memory_method: MemoryPressureMethod,
    /// Resource calculation algorithm
    pub algorithm: ResourceCalculationAlgorithm,
    /// Minimum number of jobs to allow
    pub min_jobs: u32,
    /// Maximum memory pressure threshold (0.0-1.0)
    pub max_memory_pressure: f32,
    /// CPU scaling factor under high memory pressure
    pub memory_pressure_cpu_scaling: f32,
}

impl Default for ResourceScalingConfig {
    fn default() -> Self {
        Self {
            cpu_strategy: CpuDetectionStrategy::default(),
            memory_method: MemoryPressureMethod::default(),
            algorithm: ResourceCalculationAlgorithm::default(),
            min_jobs: 1,
            max_memory_pressure: 0.9,
            memory_pressure_cpu_scaling: 0.5,
        }
    }
}

/// Platform detection error categories
#[derive(Debug, Error)]
pub enum PlatformDetectionError {
    #[error("CPU detection failed: {reason}")]
    CpuDetectionFailed { reason: String },
    
    #[error("Memory detection failed: {reason}")]
    MemoryDetectionFailed { reason: String },
    
    #[error("Platform not supported: {platform}")]
    UnsupportedPlatform { platform: String },
    
    #[error("System call failed: {call} - {reason}")]
    SystemCallFailed { call: String, reason: String },
    
    #[error("Resource calculation invalid: {reason}")]
    InvalidResourceCalculation { reason: String },
}

/// Convert platform module errors to shared primitive errors
impl From<crate::platform::PlatformError> for PlatformDetectionError {
    fn from(err: crate::platform::PlatformError) -> Self {
        match err {
            crate::platform::PlatformError::CpuDetectionFailed { reason } => {
                Self::CpuDetectionFailed { reason }
            }
            crate::platform::PlatformError::MemoryDetectionFailed { reason } => {
                Self::MemoryDetectionFailed { reason }
            }
            crate::platform::PlatformError::UnsupportedPlatform { platform } => {
                Self::UnsupportedPlatform { platform }
            }
            crate::platform::PlatformError::SystemCallFailed { call, reason } => {
                Self::SystemCallFailed { call, reason }
            }
        }
    }
}

/// Resource metrics summary for display
#[derive(Debug, Clone)]
pub struct ResourceMetrics {
    /// Number of logical CPU cores
    pub cpu_cores: u32,
    /// Memory pressure ratio (0.0 to 1.0)
    pub memory_pressure: f32,
    /// Total system memory in bytes
    pub total_memory: u64,
    /// Available memory in bytes
    pub available_memory: u64,
    /// Calculated optimal job count
    pub optimal_jobs: u32,
}

impl ResourceMetrics {
    /// Format memory size for human display
    pub fn format_memory(bytes: u64) -> String {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
        let mut size = bytes as f64;
        let mut unit_index = 0;
        
        while size >= 1024.0 && unit_index < UNITS.len() - 1 {
            size /= 1024.0;
            unit_index += 1;
        }
        
        if unit_index == 0 {
            format!("{} {}", bytes, UNITS[unit_index])
        } else {
            format!("{:.1} {}", size, UNITS[unit_index])
        }
    }
    
    /// Get memory pressure percentage for display
    pub fn memory_pressure_percent(&self) -> f32 {
        self.memory_pressure * 100.0
    }
    
    /// Get memory utilization summary
    pub fn memory_summary(&self) -> String {
        format!(
            "{} total, {} available ({:.1}% pressure)",
            Self::format_memory(self.total_memory),
            Self::format_memory(self.available_memory),
            self.memory_pressure_percent()
        )
    }
}

/// Platform adapter functions for bridging with platform module
pub mod adapter {
    use super::*;
    
    /// Convert platform module SystemResources to shared ResourceMetrics
    pub fn system_resources_to_metrics(
        resources: &crate::platform::SystemResources,
        optimal_jobs: u32,
    ) -> ResourceMetrics {
        ResourceMetrics {
            cpu_cores: resources.cpu_cores,
            memory_pressure: resources.memory_pressure,
            total_memory: resources.total_memory,
            available_memory: resources.available_memory,
            optimal_jobs,
        }
    }
    
    /// Create platform info with detected capabilities
    pub fn detect_platform_info() -> PlatformInfo {
        let mut info = PlatformInfo::detect();
        
        // Update capabilities based on actual platform detection
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        {
            info.capabilities.cpu_detection = true;
            info.capabilities.memory_detection = true;
        }
        
        #[cfg(target_os = "linux")]
        {
            info.capabilities.memory_pressure_monitoring = true;
            info.capabilities.load_average_detection = true;
            info.capabilities.platform_optimizations = true;
        }
        
        #[cfg(target_os = "macos")]
        {
            info.capabilities.platform_optimizations = true;
        }
        
        info
    }
}