use super::*;

#[test]
fn test_base_search_intent_sanitization() {
    let intent = BaseSearchIntent::new("Create & <script>alert('xss')</script>", None);

    // Raw query preserved
    assert_eq!(intent.query_raw, "Create & <script>alert('xss')</script>");

    // URL encoded for API safety
    let encoded = intent.query_encoded();
    assert!(encoded.contains("%26")); // & becomes %26
    assert!(encoded.contains("%3C")); // < becomes %3C

    // HTML escaped for display safety
    let display = intent.query_display();
    assert!(display.contains("&amp;")); // & becomes &amp;
    assert!(display.contains("&lt;")); // < becomes &lt;
}

#[test]
fn test_project_search_intent_defaults_to_mod() {
    let intent = ProjectSearchIntent::new("Create");

    match intent {
        ProjectSearchIntent::Mod(mod_intent) => {
            assert_eq!(mod_intent.base.query_raw, "Create");
            assert!(mod_intent.mod_loader.is_none());
        }
        _ => panic!("Expected Mod variant"),
    }
}

#[test]
fn test_explicit_shader_constructor() {
    let intent = ProjectSearchIntent::shader("BSL Shaders");

    match intent {
        ProjectSearchIntent::Shader(shader_intent) => {
            assert_eq!(shader_intent.base.query_raw, "BSL Shaders");
            assert!(shader_intent.shader_loader.is_none());
        }
        _ => panic!("Expected Shader variant"),
    }
}
