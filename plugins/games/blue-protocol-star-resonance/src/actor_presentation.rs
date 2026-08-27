use std::collections::BTreeSet;

use crate::class_localization::{
    class_icon_path, class_role, localized_class_name, localized_specialization_name,
    specialization_accent, specialization_class_id, specialization_icon_path, specialization_role,
};
use crate::specialization_detection::{
    specialization_from_observed_abilities, specialization_identity_from_observed_abilities,
};

/// One game-owned identity decision shared by History, the live overlay, and
/// any later BPSR presentation plug-in. Consumers must not independently pair
/// a captured class with a specialization from another snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorCombatIdentity {
    pub class_id: Option<i32>,
    pub specialization_id: Option<i32>,
}

/// Localized presentation derived from the same reviewed identity decision.
/// The icon path remains relative to the BPSR asset root so the game-neutral
/// host can expose it through its own read-only asset route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorCombatPresentation {
    pub class_id: Option<i32>,
    pub specialization_id: Option<i32>,
    pub class_name: Option<String>,
    pub specialization_name: Option<String>,
    pub icon: Option<String>,
    pub role: Option<String>,
    pub accent: Option<String>,
}

pub fn resolve_actor_combat_identity(
    class_id: Option<i32>,
    specialization_id: Option<i32>,
    ability_ids: impl IntoIterator<Item = i64>,
) -> Result<ActorCombatIdentity, String> {
    let ability_ids = ability_ids.into_iter().collect::<BTreeSet<_>>();
    if let Some(class_id) = class_id
        && let Some(specialization_id) =
            specialization_from_observed_abilities(class_id, ability_ids.iter().copied())?
    {
        return Ok(ActorCombatIdentity {
            class_id: Some(class_id),
            specialization_id: Some(specialization_id),
        });
    }
    if let Some((observed_class_id, observed_specialization_id)) =
        specialization_identity_from_observed_abilities(ability_ids.iter().copied())?
    {
        return Ok(ActorCombatIdentity {
            class_id: Some(observed_class_id),
            specialization_id: Some(observed_specialization_id),
        });
    }
    let Some(specialization_id) = specialization_id else {
        return Ok(ActorCombatIdentity {
            class_id,
            specialization_id: None,
        });
    };
    let specialization_class = specialization_class_id(specialization_id)?;
    let (class_id, specialization_id) = match (class_id, specialization_class) {
        (Some(class_id), Some(owner_class_id)) if class_id == owner_class_id => {
            (Some(class_id), Some(specialization_id))
        }
        (None, Some(owner_class_id)) => (Some(owner_class_id), Some(specialization_id)),
        // Preserve the proven class but never publish a class/spec pairing
        // rejected by the current-build specialization catalog.
        (class_id, _) => (class_id, None),
    };
    Ok(ActorCombatIdentity {
        class_id,
        specialization_id,
    })
}

pub fn resolve_actor_combat_presentation(
    class_id: Option<i32>,
    specialization_id: Option<i32>,
    ability_ids: impl IntoIterator<Item = i64>,
    locale: &str,
) -> Result<ActorCombatPresentation, String> {
    let identity = resolve_actor_combat_identity(class_id, specialization_id, ability_ids)?;
    let class_name = identity
        .class_id
        .map(|class_id| localized_class_name(class_id, locale))
        .transpose()?
        .flatten()
        .map(str::to_owned);
    let specialization_name = identity
        .specialization_id
        .map(|specialization_id| localized_specialization_name(specialization_id, locale))
        .transpose()?
        .flatten()
        .map(|name| name.strip_suffix(" Spec").unwrap_or(name).to_owned());
    let specialization_icon = identity
        .specialization_id
        .map(specialization_icon_path)
        .transpose()?
        .flatten();
    let class_icon = identity
        .class_id
        .map(class_icon_path)
        .transpose()?
        .flatten();
    let role = identity
        .specialization_id
        .map(specialization_role)
        .transpose()?
        .flatten()
        .or(identity.class_id.map(class_role).transpose()?.flatten())
        .map(str::to_owned);
    let accent = identity
        .specialization_id
        .map(specialization_accent)
        .transpose()?
        .flatten()
        .map(str::to_owned);
    Ok(ActorCombatPresentation {
        class_id: identity.class_id,
        specialization_id: identity.specialization_id,
        class_name,
        specialization_name,
        icon: specialization_icon.or(class_icon).map(str::to_owned),
        role,
        accent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_run_abilities_override_an_incompatible_profile_spec() {
        let presentation =
            resolve_actor_combat_presentation(Some(11), Some(119), [2_233], "en-US").unwrap();
        assert_eq!(presentation.class_id, Some(11));
        assert_eq!(presentation.specialization_id, Some(117));
        assert_eq!(presentation.class_name.as_deref(), Some("Marksman"));
        assert_eq!(
            presentation.specialization_name.as_deref(),
            Some("Falconry")
        );
        assert!(
            presentation
                .icon
                .as_deref()
                .is_some_and(|icon| icon.ends_with(".png"))
        );
    }

    #[test]
    fn incompatible_class_and_spec_are_never_published() {
        let identity = resolve_actor_combat_identity(Some(11), Some(119), []).unwrap();
        assert_eq!(identity.class_id, Some(11));
        assert_eq!(identity.specialization_id, None);
    }
}
