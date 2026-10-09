use empack_core::requirements::*;

fn optional() -> Requirement {
    Requirement::Optional(OptionalChoice {
        key: ChoiceKey::parse("renderer").unwrap(),
        default_enabled: false,
        description: Some("Choose the renderer".into()),
    })
}

#[test]
fn every_environment_combination_preserves_or_rejects_meaning() {
    use Environments::*;
    use Requirement::*;
    let cases = [
        (Required, Required, Some((Both, false))),
        (Required, Unsupported, Some((Client, false))),
        (Unsupported, Required, Some((Server, false))),
        (optional(), Unsupported, Some((Client, true))),
        (Unsupported, optional(), Some((Server, true))),
        (optional(), optional(), Some((Both, true))),
        (Required, optional(), None),
        (optional(), Required, None),
        (Unsupported, Unsupported, None),
    ];
    for (client, server, expected) in cases {
        let requirements = Requirements { client, server };
        let actual = requirements.uniform();
        assert_eq!(
            actual
                .as_ref()
                .ok()
                .map(|v| (v.environments, v.choice.is_some())),
            expected
        );
        if let Ok(UniformRequirements {
            choice: Some(choice),
            ..
        }) = actual
        {
            assert_eq!(choice.key.as_str(), "renderer");
            assert!(!choice.default_enabled);
            assert_eq!(choice.description.as_deref(), Some("Choose the renderer"));
        }
    }
}

#[test]
fn independent_choices_must_not_collapse() {
    let client = optional();
    for difference in 0..3 {
        let mut server = optional();
        let Requirement::Optional(choice) = &mut server else {
            unreachable!()
        };
        match difference {
            0 => choice.key = ChoiceKey::parse("different").unwrap(),
            1 => choice.default_enabled = true,
            _ => choice.description = None,
        }
        assert_eq!(
            Requirements {
                client: client.clone(),
                server
            }
            .uniform(),
            Err(RequirementError::DifferentChoices)
        );
    }
    assert!(ChoiceKey::parse(" ").is_err());
    assert!(ChoiceKey::parse("x\ny").is_err());
    assert_eq!(
        ChoiceKey::parse("a/b choice").unwrap().as_str(),
        "a/b choice"
    );
}
