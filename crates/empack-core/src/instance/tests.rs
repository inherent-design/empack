use super::*;
use crate::{digest::ContentId, files::FilePermissions};

const CAPS: FileCapabilities = FileCapabilities {
    executable_bits: true,
};
fn content(byte: u8) -> FileContent {
    FileContent {
        content: ContentId::from_sha256([byte; 32]),
        bytes: 1,
        permissions: FilePermissions {
            readonly: false,
            executable: false,
        },
    }
}
fn installed(byte: u8, policy: FilePolicy) -> InstalledFile {
    InstalledFile {
        baseline: content(byte),
        policy,
    }
}
fn release(byte: u8, policy: FilePolicy) -> ReleaseFile {
    ReleaseFile {
        content: content(byte),
        policy,
    }
}

#[test]
fn install_update_repair_and_retire_follow_the_installed_baseline() {
    let first = release(1, FilePolicy::Managed);
    assert_eq!(
        plan_file(None, &ObservedPath::Absent, Some(&first), CAPS),
        FileDecision::Create(content(1))
    );
    let previous = installed(1, FilePolicy::Managed);
    let actual = ObservedPath::File(content(1));
    assert_eq!(
        plan_file(Some(&previous), &actual, Some(&first), CAPS),
        FileDecision::Unchanged
    );
    let second = release(2, FilePolicy::Managed);
    assert_eq!(
        plan_file(Some(&previous), &actual, Some(&second), CAPS),
        FileDecision::Replace(content(2))
    );
    let completed = installed(2, FilePolicy::Managed);
    assert_eq!(
        plan_file(Some(&completed), &ObservedPath::Absent, Some(&second), CAPS),
        FileDecision::Create(content(2))
    );
    assert_eq!(
        plan_file(
            Some(&completed),
            &ObservedPath::File(content(2)),
            None,
            CAPS
        ),
        FileDecision::Remove
    );
    assert_eq!(
        plan_file(Some(&completed), &ObservedPath::Absent, None, CAPS),
        FileDecision::Unchanged
    );
}

#[test]
fn identical_unowned_bytes_do_not_authorize_adoption() {
    let incoming = release(1, FilePolicy::Managed);
    assert_eq!(
        plan_file(None, &ObservedPath::File(content(1)), Some(&incoming), CAPS),
        FileDecision::Conflict(FileConflict::UnownedDestination)
    );
}

#[test]
fn user_changes_block_replacement_and_retirement_but_accept_exact_desired_state() {
    let previous = installed(1, FilePolicy::Managed);
    let incoming = release(2, FilePolicy::Managed);
    let edited = ObservedPath::File(content(3));
    for incoming in [Some(&incoming), None] {
        assert_eq!(
            plan_file(Some(&previous), &edited, incoming, CAPS),
            FileDecision::Conflict(FileConflict::ModifiedManagedFile)
        );
    }
    assert_eq!(
        plan_file(
            Some(&previous),
            &ObservedPath::File(content(2)),
            Some(&incoming),
            CAPS
        ),
        FileDecision::Unchanged
    );
}

#[test]
fn seeds_preserve_edits_and_cannot_become_managed_without_a_decision() {
    let previous = installed(1, FilePolicy::Seed);
    let seed = release(2, FilePolicy::Seed);
    let edited = ObservedPath::File(content(3));
    assert_eq!(
        plan_file(Some(&previous), &edited, Some(&seed), CAPS),
        FileDecision::Preserve
    );
    assert_eq!(
        plan_file(None, &edited, Some(&seed), CAPS),
        FileDecision::Preserve
    );
    assert_eq!(
        plan_file(Some(&previous), &edited, None, CAPS),
        FileDecision::Preserve
    );
    assert_eq!(
        plan_file(None, &ObservedPath::Absent, Some(&seed), CAPS),
        FileDecision::Create(content(2))
    );
    let managed = release(2, FilePolicy::Managed);
    for current in [ObservedPath::Absent, edited] {
        assert_eq!(
            plan_file(Some(&previous), &current, Some(&managed), CAPS),
            FileDecision::Conflict(FileConflict::SeedOwnership)
        );
    }
}

#[test]
fn directories_never_become_file_mutations() {
    let previous = installed(1, FilePolicy::Managed);
    for policy in [FilePolicy::Managed, FilePolicy::Seed] {
        let incoming = release(2, policy);
        for previous in [Some(&previous), None] {
            assert_eq!(
                plan_file(previous, &ObservedPath::Directory, Some(&incoming), CAPS),
                FileDecision::Conflict(FileConflict::WrongKind)
            );
        }
    }
    assert_eq!(
        plan_file(Some(&previous), &ObservedPath::Directory, None, CAPS),
        FileDecision::Conflict(FileConflict::WrongKind)
    );
    assert_eq!(
        plan_file(None, &ObservedPath::Directory, None, CAPS),
        FileDecision::Preserve
    );
    let previous = installed(1, FilePolicy::Seed);
    assert_eq!(
        plan_file(Some(&previous), &ObservedPath::Directory, None, CAPS),
        FileDecision::Preserve
    );
}

#[test]
fn supported_permission_changes_are_local_edits() {
    let previous = installed(1, FilePolicy::Managed);
    let incoming = release(2, FilePolicy::Managed);
    let mut edited = content(1);
    edited.permissions.executable = true;
    let edited = ObservedPath::File(edited);
    assert_eq!(
        plan_file(Some(&previous), &edited, Some(&incoming), CAPS),
        FileDecision::Conflict(FileConflict::ModifiedManagedFile)
    );
    assert_eq!(
        plan_file(
            Some(&previous),
            &edited,
            Some(&incoming),
            FileCapabilities {
                executable_bits: false
            }
        ),
        FileDecision::Replace(content(2))
    );
    let mut edited = content(1);
    edited.permissions.readonly = true;
    assert_eq!(
        plan_file(Some(&previous), &ObservedPath::File(edited), None, CAPS),
        FileDecision::Conflict(FileConflict::ModifiedManagedFile)
    );
}

#[test]
fn no_destructive_decision_without_an_unchanged_managed_baseline() {
    let policies = [FilePolicy::Managed, FilePolicy::Seed];
    for previous_byte in 0..3 {
        for previous_policy in policies {
            let previous = installed(previous_byte, previous_policy);
            for current_byte in 0..3 {
                let current = ObservedPath::File(content(current_byte));
                for next_byte in 0..3 {
                    for next_policy in policies {
                        let incoming = release(next_byte, next_policy);
                        for previous in [None, Some(&previous)] {
                            for incoming in [None, Some(&incoming)] {
                                let result = plan_file(previous, &current, incoming, CAPS);
                                if matches!(result, FileDecision::Replace(_) | FileDecision::Remove)
                                {
                                    let previous =
                                        previous.expect("destruction requires ownership");
                                    assert_eq!(previous.policy, FilePolicy::Managed);
                                    assert_eq!(previous.baseline, content(current_byte));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
