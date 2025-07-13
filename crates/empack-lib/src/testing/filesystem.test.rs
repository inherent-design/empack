use super::*;

#[test]
fn test_creates_temp_dir_and_writes_file() -> Result<(), Box<dyn std::error::Error>> {
    // Create temporary directory fixture with automatic cleanup
    let fixture = TempDirFixture::new()?;
    
    // Create config subdirectory
    fixture.create_dir("config")?;
    
    // Write empack.toml file with pack configuration
    let config_content = "[pack]";
    fixture.write_file("config/empack.toml", config_content)?;
    
    // Assert file was created successfully
    assert!(fixture.file_exists("config/empack.toml"));
    
    // Assert content is correct
    let actual_content = fixture.read_file("config/empack.toml")?;
    assert_eq!(actual_content, config_content);
    
    // Verify path structure
    let config_path = fixture.path().join("config");
    assert!(config_path.exists());
    assert!(config_path.is_dir());
    
    let file_path = config_path.join("empack.toml");
    assert!(file_path.exists());
    assert!(file_path.is_file());
    
    Ok(())
    // Temporary directory automatically cleaned up when fixture drops
}

#[test]
fn test_temp_dir_isolation() -> Result<(), Box<dyn std::error::Error>> {
    // Create two separate fixtures to verify isolation
    let fixture1 = TempDirFixture::new()?;
    let fixture2 = TempDirFixture::new()?;
    
    // Verify they have different paths
    assert_ne!(fixture1.path(), fixture2.path());
    
    // Write different files to each
    fixture1.write_file("test1.txt", "content1")?;
    fixture2.write_file("test2.txt", "content2")?;
    
    // Verify isolation - each fixture only sees its own file
    assert!(fixture1.file_exists("test1.txt"));
    assert!(!fixture1.file_exists("test2.txt"));
    
    assert!(fixture2.file_exists("test2.txt"));
    assert!(!fixture2.file_exists("test1.txt"));
    
    Ok(())
}

#[test]
fn test_nested_directory_creation() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TempDirFixture::new()?;
    
    // Create deeply nested directory structure
    fixture.write_file("deep/nested/structure/test.txt", "nested content")?;
    
    // Verify the entire structure was created
    assert!(fixture.file_exists("deep/nested/structure/test.txt"));
    
    let content = fixture.read_file("deep/nested/structure/test.txt")?;
    assert_eq!(content, "nested content");
    
    Ok(())
}

#[test]
fn test_empack_config_pattern() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TempDirFixture::new()?;
    
    // Simulate typical empack project structure
    fixture.write_file("empack.yml", "name: test-pack\nauthor: test-author")?;
    fixture.write_file("pack.toml", "[pack]\nname = \"test-pack\"\nauthor = \"test-author\"")?;
    fixture.create_dir("mods")?;
    fixture.create_dir(".empack")?;
    fixture.write_file(".empack/state", "configured")?;
    
    // Verify empack project structure
    assert!(fixture.file_exists("empack.yml"));
    assert!(fixture.file_exists("pack.toml"));
    assert!(fixture.file_exists(".empack/state"));
    
    // Verify state content
    let state = fixture.read_file(".empack/state")?;
    assert_eq!(state, "configured");
    
    Ok(())
}
