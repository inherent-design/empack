use empack_core::{
    distribution::{Consumer, Delivery, Recipe, RecipeError, UpdateAuthority},
    requirements::Environments,
};

#[test]
fn supported_consumers_default_to_snapshots() {
    for (consumer, environment) in [
        (Consumer::Modrinth, Environments::Both),
        (Consumer::CurseForge, Environments::Client),
        (Consumer::Prism, Environments::Client),
        (Consumer::Server, Environments::Server),
        (Consumer::Empack, Environments::Both),
    ] {
        let recipe = Recipe::new(consumer, Delivery::References, environment).unwrap();
        assert_eq!(recipe.consumer(), consumer);
        assert_eq!(recipe.environments(), environment);
        assert_eq!(recipe.update_authority(), UpdateAuthority::Snapshot);
    }
}

#[test]
fn bundled_content_can_follow_updates_and_references_can_remain_snapshots() {
    for (consumer, environment) in [
        (Consumer::Prism, Environments::Client),
        (Consumer::Server, Environments::Server),
        (Consumer::Empack, Environments::Both),
    ] {
        for delivery in [Delivery::Bundled, Delivery::References] {
            let snapshot = Recipe::new(consumer, delivery, environment).unwrap();
            let subscribed = snapshot
                .with_update_authority(UpdateAuthority::Empack)
                .unwrap();
            assert_eq!(subscribed.delivery(), delivery);
            assert_eq!(snapshot.update_authority(), UpdateAuthority::Snapshot);
            assert_eq!(subscribed.update_authority(), UpdateAuthority::Empack);
        }
    }
}

#[test]
fn platform_recipes_do_not_allow_full_dependency_embedding_or_empack_authority() {
    for consumer in [Consumer::Modrinth, Consumer::CurseForge] {
        assert_eq!(
            Recipe::new(consumer, Delivery::Bundled, Environments::Client),
            Err(RecipeError::Delivery)
        );
        let recipe = Recipe::new(consumer, Delivery::References, Environments::Client).unwrap();
        assert!(
            recipe
                .with_update_authority(UpdateAuthority::Platform)
                .is_ok()
        );
        assert_eq!(
            recipe.with_update_authority(UpdateAuthority::Empack),
            Err(RecipeError::UpdateAuthority)
        );
    }
}

#[test]
fn instance_consumers_reject_platform_authority_and_wrong_sides() {
    for (consumer, environment) in [
        (Consumer::Prism, Environments::Client),
        (Consumer::Server, Environments::Server),
        (Consumer::Empack, Environments::Both),
    ] {
        let recipe = Recipe::new(consumer, Delivery::References, environment).unwrap();
        assert_eq!(
            recipe.with_update_authority(UpdateAuthority::Platform),
            Err(RecipeError::UpdateAuthority)
        );
    }
    for (consumer, environment) in [
        (Consumer::Prism, Environments::Server),
        (Consumer::Prism, Environments::Both),
        (Consumer::CurseForge, Environments::Server),
        (Consumer::Server, Environments::Client),
        (Consumer::Server, Environments::Both),
    ] {
        assert_eq!(
            Recipe::new(consumer, Delivery::References, environment),
            Err(RecipeError::Environment)
        );
    }
}
