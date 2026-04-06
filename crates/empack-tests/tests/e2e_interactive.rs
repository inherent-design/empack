use empack_tests::e2e::{empack_bin, TestProject};
use expectrl::{Expect, Regex, Session};
use std::process::Command;
use std::time::Duration;

// Reference only: kept as a manual verification aid, not run in CI.
//
// The interactive init flow relies on dialoguer terminal widgets (FuzzySelect,
// Input) which render escape sequences that are difficult to match reliably
// across terminal emulators and CI environments. This test exercises the prompt
// sequence through a PTY but dialoguer rendering varies across platforms and
// CI runners, so it remains permanently ignored.
#[test]
#[ignore]
fn e2e_init_interactive_prompts() {
    empack_tests::skip_if_no_packwiz!();

    let project = TestProject::new();
    let bin = empack_bin();

    let mut cmd = Command::new(bin);
    cmd.args(["init", "test-pack"]);
    cmd.current_dir(project.dir());
    cmd.env("NO_COLOR", "1");

    let mut session = Session::spawn(cmd).expect("failed to spawn empack init");
    session.set_expect_timeout(Some(Duration::from_secs(30)));

    // Prompt 1: Modpack name (dialoguer Input, default: "test-pack")
    let _ = session
        .expect(Regex("(?i)modpack.*name|name"))
        .expect("expected modpack name prompt");
    session.send_line("").expect("failed to accept default name");

    // Prompt 2: Author (dialoguer Input, default: git user.name)
    let _ = session
        .expect(Regex("(?i)author"))
        .expect("expected author prompt");
    session
        .send_line("")
        .expect("failed to accept default author");

    // Prompt 3: Version (dialoguer Input, default: "1.0.0")
    let _ = session
        .expect(Regex("(?i)version"))
        .expect("expected version prompt");
    session
        .send_line("")
        .expect("failed to accept default version");

    // Network fetch for Minecraft versions happens here.
    // Prompt 4: Minecraft version (dialoguer FuzzySelect)
    let _ = session
        .expect(Regex("(?i)minecraft.*version|version"))
        .expect("expected minecraft version prompt");
    session
        .send_line("1.21.1")
        .expect("failed to send minecraft version");

    // Prompt 5: Mod loader (dialoguer Select or FuzzySelect)
    match session.expect(Regex("(?i)loader|compatible")) {
        Ok(_) => {
            session
                .send_line("fabric")
                .expect("failed to send loader selection");
        }
        Err(_) => {
            // Process may have chosen defaults or exited; continue.
        }
    }

    // Prompt 6: Loader version (dialoguer FuzzySelect, if shown)
    match session.expect(Regex("(?i)loader.*version|fabric")) {
        Ok(_) => {
            let _ = session.send_line("");
        }
        Err(_) => {
            // May not appear if the process already completed.
        }
    }

    // Wait for the process to produce final output or exit.
    let _ = session.expect(Regex("(?i)initialized|created|successfully"));

    let pack_dir = project.dir().join("test-pack");
    assert!(
        pack_dir.join("empack.yml").exists(),
        "empack.yml not found after interactive init"
    );
}
