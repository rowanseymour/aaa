//! Things that neither fight nor move but stand in play: the sight-givers
//! and objective markers campaigns place and make.

use std::collections::{HashMap, HashSet};

use tgck::map::Thing;
use tgdata::{Body, Fighter, Id, Shape, Sight};

/// A prop placed in the map.
#[derive(Debug, Clone, PartialEq)]
pub struct Prop {
    pub id: u32,
    pub template: Id,
    /// `None` for one whose template can't be owned.
    pub owner: Option<u32>,
    pub position: [f32; 2],
    pub facing: [f32; 2],
}

/// What a prop's template gives each one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PropTemplate {
    pub ownable: bool,
    /// `None` for one with no health of its own, which leaves nothing
    /// behind when it's destroyed.
    pub health: Option<f32>,
    /// The blocking shape it needs room for where it's made.
    pub shape: Shape,
}

/// The templates that make props: those that never move, fight nothing
/// and can be hurt by nothing, or can't be owned.
pub fn prop_templates(
    bodies: &HashMap<Id, Body>,
    fighters: &HashMap<Id, Fighter>,
    organizations: &HashSet<Id>,
) -> HashMap<Id, PropTemplate> {
    (bodies.iter())
        .filter(|(t, b)| b.fixed && !organizations.contains(*t))
        .filter(|(t, b)| !b.ownable || !fighters.contains_key(*t))
        .map(|(t, b)| {
            let template = PropTemplate {
                ownable: b.ownable,
                health: fighters.get(t).map(|f| f.health),
                shape: b.shape,
            };
            (t.clone(), template)
        })
        .collect()
}

/// The props among the things placed in a map, each with its owner, that
/// aren't `taken` as something else: those whose template is a prop's and
/// sees.
pub(crate) fn props<'a>(
    standing: impl Iterator<Item = (&'a Thing, Option<u32>)>,
    templates: &HashMap<Id, PropTemplate>,
    sights: &HashMap<Id, Sight>,
    taken: &HashSet<u32>,
) -> Vec<Prop> {
    standing
        .filter(|(t, _)| !taken.contains(&t.id))
        .filter_map(|(t, owner)| {
            let template = Id::from(t.template.as_str());
            let kind = templates.get(&template)?;
            sights.get(&template)?;
            let [x, y, _] = t.position;
            let [fx, fy, _] = t.facing;
            let length = libm::hypotf(fx, fy);
            Some(Prop {
                id: t.id,
                template,
                owner: owner.filter(|_| kind.ownable),
                position: [x, y],
                facing: match length > 0.0 {
                    true => [fx / length, fy / length],
                    false => [0.0, -1.0],
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn props_are_what_stays_put_and_fights_nothing_or_belongs_to_nobody() {
        let body = |fixed, ownable| Body {
            fixed,
            ownable,
            ..Body::default()
        };
        let bodies: HashMap<Id, Body> = [
            ("eye", body(true, true)),
            ("compass", body(true, false)),
            ("tower", body(true, true)),
            ("tank", body(false, true)),
            ("regiment", body(true, true)),
        ]
        .into_iter()
        .map(|(t, b)| (Id::from(t), b))
        .collect();
        let fighter = |health| Fighter {
            health,
            ..Fighter::default()
        };
        let fighters: HashMap<Id, Fighter> = [("compass", 500.0), ("tower", 800.0)]
            .into_iter()
            .map(|(t, h)| (Id::from(t), fighter(h)))
            .collect();
        let organizations = [Id::from("regiment")].into();
        let props = prop_templates(&bodies, &fighters, &organizations);
        let mut made: Vec<&str> = props.keys().map(Id::as_str).collect();
        made.sort();
        assert_eq!(made, ["compass", "eye"]);
        assert_eq!(props[&Id::from("compass")].health, Some(500.0));
        assert_eq!(props[&Id::from("eye")].health, None);
    }
}
