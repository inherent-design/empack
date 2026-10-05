use empack_core::{
    digest::ContentId,
    files::*,
    model::ContentLayer,
    path::{PathSyntax, PortableRelPath},
};
use std::collections::{BTreeMap, BTreeSet};
fn path(name: &str) -> ManagedPath {
    ManagedPath::Content {
        layer: ContentLayer::Common,
        path: PortableRelPath::parse(name, PathSyntax::ProjectContent).unwrap(),
    }
}
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
#[test]
fn reconciliation_retains_unlisted_files_and_converges() {
    let observed = BTreeMap::from([
        (path("root"), ObservedPath::File(content(1))),
        (path("transitive"), ObservedPath::File(content(2))),
    ]);
    let desired = BTreeMap::from([(path("root"), content(3))]);
    let plan = FilePlan::prepare(&observed, &desired, &BTreeSet::new()).unwrap();
    assert_eq!(plan.changes().len(), 1);
    assert_eq!(plan.expected()[&path("transitive")], content(2));
    let observed = plan
        .expected()
        .iter()
        .map(|(p, c)| (p.clone(), ObservedPath::File(c.clone())))
        .collect();
    let converged = FilePlan::prepare(&observed, &desired, &BTreeSet::new()).unwrap();
    assert!(converged.changes().is_empty());
    assert_eq!(converged.expected(), plan.expected());
}
#[test]
fn removals_require_exact_observation_and_never_authorize_a_directory() {
    let target = path("selected");
    let removals = BTreeSet::from([target.clone()]);
    assert!(FilePlan::prepare(&BTreeMap::new(), &BTreeMap::new(), &removals).is_err());
    let observed = BTreeMap::from([(target.clone(), ObservedPath::Directory)]);
    assert!(FilePlan::prepare(&observed, &BTreeMap::new(), &removals).is_err());
    assert!(
        FilePlan::prepare(
            &observed,
            &BTreeMap::from([(target.clone(), content(1))]),
            &BTreeSet::new()
        )
        .is_err()
    );
    let observed = BTreeMap::from([(target.clone(), ObservedPath::File(content(1)))]);
    let plan = FilePlan::prepare(&observed, &BTreeMap::new(), &removals).unwrap();
    assert!(
        matches!(&plan.changes()[0], FileChange::Remove { before, .. } if before == &content(1))
    );
    assert!(plan.expected().is_empty());
    assert!(
        FilePlan::prepare(
            &observed,
            &BTreeMap::from([(target, content(2))]),
            &removals
        )
        .is_err()
    );
}
#[test]
fn absence_and_permissions_are_part_of_the_plan() {
    let target = ManagedPath::IntentDocument;
    let desired = BTreeMap::from([(target.clone(), content(1))]);
    assert!(FilePlan::prepare(&BTreeMap::new(), &desired, &BTreeSet::new()).is_err());
    let observed = BTreeMap::from([(target.clone(), ObservedPath::Absent)]);
    let plan = FilePlan::prepare(&observed, &desired, &BTreeSet::new()).unwrap();
    assert!(matches!(
        &plan.changes()[0],
        FileChange::Replace {
            before: ObservedPath::Absent,
            ..
        }
    ));
    let observed = BTreeMap::from([(target.clone(), ObservedPath::File(content(1)))]);
    let mut after = content(1);
    after.permissions.executable = true;
    assert_eq!(
        FilePlan::prepare(
            &observed,
            &BTreeMap::from([(target, after)]),
            &BTreeSet::new()
        )
        .unwrap()
        .changes()
        .len(),
        1
    );
}
