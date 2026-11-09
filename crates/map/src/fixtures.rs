//! Map records for tests.

use tgck::map::Thing;

/// An object at `id` metres along x owned by `kingdom`, 0 for nobody,
/// towing the object `towing` or standing as `organization`.
pub(crate) fn placed(
    id: u32,
    template: &str,
    kingdom: u32,
    towing: Option<u32>,
    organization: Option<tgck::map::Organization>,
) -> Thing {
    use tgck::map::{Actor, Component, Detail, State, Trailer};
    let trailer = Trailer {
        main_component: -1,
        required_object: None,
        old_stance: None,
        saved_group: None,
        group: None,
        map_string: None,
        towing: towing.map(|t| [t, 0]),
        number: None,
        lists: None,
        num_deploying: None,
    };
    let components = organization
        .map(Component::Organization)
        .into_iter()
        .collect();
    Thing {
        template: template.into(),
        class: tgck::map::ACTOR,
        id,
        name: String::new(),
        position: [id as f32, 0.0, 0.0],
        facing: [0.0, 1.0, 0.0],
        flags: 0,
        scale: 1.0,
        variation: -1,
        seen_by: 0,
        detail: Detail::Actor(Actor {
            version: 14,
            kingdom,
            states: 0,
            template_components: 0,
            components: 0,
            state: State::Decoded {
                components,
                trailer,
            },
        }),
    }
}

pub(crate) fn organization(layout: &str, members: Vec<Thing>) -> tgck::map::Organization {
    use tgck::map::{Organization, Position, Updatable};
    Organization {
        updatable: Updatable { version: 1, id: 0 },
        version: 10,
        layout: layout.into(),
        slots: members.iter().map(|m| m.template.clone()).collect(),
        positions: (0..members.len() as i32)
            .map(|index| Position {
                index,
                old: None,
                template: Some(String::new()),
                offset: [0.0, 0.0],
                replenished: Some(0.0),
            })
            .collect(),
        members,
        leader: -1,
        morale: 1.0,
        formation: 0,
        desired_formation: Some(0),
        old: None,
        fighting: None,
        level: Some(1.0),
        time: Some(0.0),
        position: Some([0.0, 0.0]),
        hq: Some(0),
        transport: Some(0),
    }
}

/// `thing` with its health a fraction `health` of its template's.
pub(crate) fn hurt(mut thing: Thing, health: f32) -> Thing {
    use tgck::map::{Body, Component, Detail, State};
    if let Detail::Actor(actor) = &mut thing.detail
        && let State::Decoded { components, .. } = &mut actor.state
    {
        components.push(Component::Body(Body {
            version: 4,
            updatable: None,
            death_time: Some(-1.0),
            health,
            sink_time: Some(0.0),
            last_damage_time: Some(0.0),
        }));
    }
    thing
}
