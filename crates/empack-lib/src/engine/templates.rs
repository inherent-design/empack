//! Output-language helpers shared by captured builds and project scaffolding.
mod encoding;

pub(crate) fn register_helpers(handlebars: &mut handlebars::Handlebars<'_>) {
    handlebars::handlebars_helper!(shell_quote: |value: str| {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    });
    handlebars::handlebars_helper!(ini_quote: |value: str| encoding::ini_value(value));
    handlebars::handlebars_helper!(properties_value: |value: str| encoding::properties_value(value));
    handlebars.register_helper("shell_quote", Box::new(shell_quote));
    handlebars.register_helper("ini_quote", Box::new(ini_quote));
    handlebars.register_helper("properties_value", Box::new(properties_value));
}
