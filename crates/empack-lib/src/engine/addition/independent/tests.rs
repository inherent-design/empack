use super::*;
use crate::engine::mrpack::tests::{explicitly_placed, project};
use empack_core::{
    addition::AdditionPlan,
    path::{InstallDestination, PathSyntax, PortableRelPath},
};

fn selected(label: &str, destination: &str) -> ResolvedProject {
    let source = project(false, false);
    let mut intent = source.intent().clone();
    let mut root = intent.roots.values().next().unwrap().clone();
    let mut lock = source.lock().clone();
    let mut dependency = lock.dependencies.values().next().unwrap().clone();
    let key = DependencyKey::parse(label).unwrap();
    dependency.identity = ResolvedIdentity::Url(key.clone());
    let mut file = dependency.files.as_slice()[0].clone();
    let mut placement = file.placements.as_slice()[0].clone();
    placement.destination = InstallDestination::parse(destination).unwrap();
    file.placements = NonEmpty::new(vec![placement]).unwrap();
    dependency.files = NonEmpty::new(vec![file]).unwrap();
    root.placement = PlacementIntent::Automatic;
    intent.roots = [(key.clone(), root)].into();
    lock.dependencies = [(key.clone(), dependency)].into();
    lock.coverage = [(key, Coverage::CompleteForSelection)].into();
    explicitly_placed(intent, lock)
}
fn empty() -> ResolvedProject {
    let source = selected("a", "a.zip");
    let mut intent = source.intent().clone();
    let mut lock = source.lock().clone();
    intent.roots.clear();
    lock.dependencies.clear();
    lock.coverage.clear();
    explicitly_placed(intent, lock)
}
fn group(project: &ResolvedProject) -> AdditionGroup {
    AdditionGroup::from_resolved(project).unwrap()
}
fn combined(projects: &[ResolvedProject]) -> ResolvedProject {
    let current = empty();
    let groups: Vec<_> = projects.iter().map(group).collect();
    let combined = AdditionGroup::combine(&current, &groups.iter().collect::<Vec<_>>()).unwrap();
    AdditionPlan::prepare(&current, &combined)
        .unwrap()
        .resolve(current.lock().intent_revision)
        .unwrap()
}
#[test]
fn independent_groups_follow_current_required_chains_cycles_and_old_paths() {
    let a = selected("a", "a.zip");
    let b = selected("b", "b.zip");
    let c = selected("c", "c.zip");
    let requests = vec![group(&a), group(&b), group(&c)];
    assert_eq!(
        independent_components(&empty(), &requests).unwrap(),
        vec![vec![0], vec![1], vec![2]]
    );
    let current = combined(&[a.clone(), b.clone(), c.clone()]);
    let mut lock = current.lock().clone();
    for (from, to) in [("a", "b"), ("b", "c"), ("c", "a")] {
        lock.required_edges.insert(
            DependencyKey::parse(from).unwrap(),
            [DependencyKey::parse(to).unwrap()].into(),
        );
    }
    let linked = explicitly_placed(current.intent().clone(), lock);
    assert_eq!(
        independent_components(&linked, &requests).unwrap(),
        vec![vec![0, 1, 2]]
    );
    // Moving A does not make B's newly occupied old A path independent of A's deletion.
    let requests = vec![
        group(&selected("a", "new-a.zip")),
        group(&selected("b", "a.zip")),
    ];
    assert_eq!(
        independent_components(&current, &requests).unwrap(),
        vec![vec![0, 1]]
    );
}
#[test]
fn independent_groups_merge_required_closure_and_reject_conflicting_shared_records() {
    let a = selected("a", "a.zip");
    let b = selected("b", "b.zip");
    let c = selected("c", "c.zip");
    let closure = |root: &ResolvedProject| {
        let value = combined(&[root.clone(), b.clone()]);
        let mut intent = value.intent().clone();
        let b_key = DependencyKey::parse("b").unwrap();
        intent.roots.remove(&b_key);
        let mut lock = value.lock().clone();
        lock.required_edges
            .insert(intent.roots.keys().next().unwrap().clone(), [b_key].into());
        group(&explicitly_placed(intent, lock))
    };
    let requests = vec![closure(&a), closure(&c)];
    assert_eq!(
        independent_components(&empty(), &requests).unwrap(),
        vec![vec![0, 1]]
    );
    let merged = AdditionGroup::combine(&empty(), &[&requests[0], &requests[1]]).unwrap();
    assert_eq!(merged.dependencies().len(), 3);
    assert!(AdditionPlan::prepare(&empty(), &merged).is_ok());
    let changed = group(&selected("b", "different.zip"));
    assert!(AdditionGroup::combine(&empty(), &[&requests[0], &changed]).is_err());
    assert!(AdditionGroup::combine(&empty(), &[]).is_err());
}
#[test]
fn independent_groups_use_native_layer_paths_unicode_and_ancestor_collisions() {
    for (a, b) in [
        ("Config/é.txt", "config/e\u{301}.TXT"),
        ("config", "config/settings.toml"),
        ("mods/STRASSE.jar", "mods/straße.jar"),
    ] {
        let groups = vec![group(&selected("a", a)), group(&selected("b", b))];
        assert_eq!(
            independent_components(&empty(), &groups).unwrap(),
            vec![vec![0, 1]]
        );
    }
    let a = selected("a", "shared.zip");
    let b = selected("b", "shared.zip");
    let mut lock = b.lock().clone();
    let dependency = lock.dependencies.values_mut().next().unwrap();
    let mut file = dependency.files.as_slice()[0].clone();
    let mut placement = file.placements.as_slice()[0].clone();
    placement.layer = ContentLayer::Client;
    file.placements = NonEmpty::new(vec![placement]).unwrap();
    dependency.files = NonEmpty::new(vec![file]).unwrap();
    let b = explicitly_placed(b.intent().clone(), lock);
    assert_eq!(
        independent_components(&empty(), &[group(&a), group(&b)]).unwrap(),
        vec![vec![0], vec![1]]
    );
    let mut intent = b.intent().clone();
    let mut lock = b.lock().clone();
    let key = intent.roots.keys().next().unwrap().clone();
    let path = PortableRelPath::parse("pack/shared.zip", PathSyntax::ProjectContent).unwrap();
    intent.roots.get_mut(&key).unwrap().source = SourceIntent::Local(path.clone());
    let dependency = lock.dependencies.get_mut(&key).unwrap();
    dependency.identity = ResolvedIdentity::Local(key);
    let mut file = dependency.files.as_slice()[0].clone();
    file.acquisition = AcquisitionSpec::Local(path);
    dependency.files = NonEmpty::new(vec![file]).unwrap();
    let b = explicitly_placed(intent, lock);
    assert_eq!(
        independent_components(&empty(), &[group(&a), group(&b)]).unwrap(),
        vec![vec![0, 1]]
    );
}
