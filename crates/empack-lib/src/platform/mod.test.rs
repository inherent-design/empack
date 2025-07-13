use super::*;

#[test]
fn test_system_resources_detection() {
    let resources = SystemResources::detect();

    // Should successfully detect on all supported platforms
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    {
        let resources = resources.expect("Should detect system resources");
        assert!(resources.cpu_cores > 0, "Should detect at least 1 CPU core");
        assert!(resources.total_memory > 0, "Should detect total memory");
        assert!(
            resources.memory_pressure >= 0.0 && resources.memory_pressure <= 1.0,
            "Memory pressure should be between 0.0 and 1.0"
        );
    }
}

#[test]
fn test_optimal_jobs_calculation() {
    let resources = SystemResources {
        cpu_cores: 8,
        memory_pressure: 0.3,
        total_memory: 16_000_000_000,     // 16 GB
        available_memory: 11_200_000_000, // ~70% available
    };

    let jobs = resources.calculate_optimal_jobs(None);
    assert!(jobs >= 1, "Should calculate at least 1 job");
    assert!(jobs <= resources.cpu_cores, "Should not exceed CPU cores");

    // Test with max limit
    let limited_jobs = resources.calculate_optimal_jobs(Some(4));
    assert!(limited_jobs <= 4, "Should respect max jobs limit");
}

#[test]
fn test_memory_pressure_edge_cases() {
    // High memory pressure
    let high_pressure = SystemResources {
        cpu_cores: 4,
        memory_pressure: 0.9,
        total_memory: 8_000_000_000,
        available_memory: 800_000_000, // Only 10% available
    };

    let jobs = high_pressure.calculate_optimal_jobs(None);
    assert!(
        jobs >= 1,
        "Should still allow at least 1 job under high pressure"
    );

    // Zero memory pressure (edge case)
    let zero_pressure = SystemResources {
        cpu_cores: 8,
        memory_pressure: 0.0,
        total_memory: 16_000_000_000,
        available_memory: 16_000_000_000,
    };

    let jobs = zero_pressure.calculate_optimal_jobs(None);
    assert!(jobs >= 1, "Should handle zero memory pressure gracefully");
}
