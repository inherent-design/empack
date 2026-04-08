use super::*;
use indicatif::MultiProgress;
use std::sync::Arc;

#[test]
fn test_live_display_provider_surface_methods() {
    let provider = LiveDisplayProvider::new();

    let status = provider.status();
    status.checking("loading");
    status.success("pack", "ready");
    status.error("pack", "failed");
    status.warning("be careful");
    status.info("info");
    status.message("hello");
    status.emphasis("important");
    status.subtle("quiet");
    status.list(&["one", "two"]);
    status.complete("loading");
    status.tool_check("cargo", true, "1.0");
    status.section("section");
    status.step(1, 2, "step");

    let progress = provider.progress();
    let bar = progress.bar(3);
    bar.set_position(1);
    bar.inc();
    bar.inc_by(1);
    bar.set_message("progress");
    bar.tick("tick");
    bar.finish("done");

    let abandoned = progress.bar(1);
    abandoned.abandon("abandoned");

    let cleared = progress.bar(1);
    cleared.finish_clear();

    let spinner = progress.spinner("spinner");
    spinner.set_message("spin");
    spinner.tick("spin tick");
    spinner.finish("spun");

    let spinner_abandoned = progress.spinner("spinner abandoned");
    spinner_abandoned.abandon("stopped");

    let multi = progress.multi();
    let multi_bar = multi.add_bar(2, "multi");
    multi_bar.inc();
    multi_bar.finish("multi done");
    let multi_spinner = multi.add_spinner("multi spinner");
    multi_spinner.finish_clear();
    multi.clear();

    let table = provider.table();
    table.table(&["Name", "Value"], &[vec!["one", "1"]]);
    table.list(&["alpha", "beta"]);
    table.properties(&[("key", "value")]);
}

#[test]
fn test_live_display_provider_new_with_arc() {
    let shared = Arc::new(MultiProgress::new());
    let provider = LiveDisplayProvider::new_with_arc(Arc::clone(&shared));

    let progress = provider.progress();
    let multi = progress.multi();
    let bar = multi.add_bar(1, "shared");
    bar.finish("shared done");
    multi.clear();
}
