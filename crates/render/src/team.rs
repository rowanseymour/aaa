//! Models painted in their owner's colour where the model marks a mesh for
//! it: the texture's see-through texels show the colour instead.

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

use crate::models::Animated;

pub type TeamMaterial = ExtendedMaterial<StandardMaterial, Team>;

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct Team {
    /// Linear rgb; w unused.
    #[uniform(100)]
    pub colour: Vec4,
}

impl Team {
    pub fn new([r, g, b]: [f32; 3]) -> Self {
        let c = Color::srgb(r, g, b).to_linear();
        Team {
            colour: Vec4::new(c.red, c.green, c.blue, 1.0),
        }
    }
}

impl MaterialExtension for Team {
    fn fragment_shader() -> ShaderRef {
        "embedded://aaa_render/team.wesl".into()
    }
}

/// Whether a shape is one the owner's colour shows through.
pub(crate) fn marked(nif: &nif::Nif, shape: &nif::SceneObject) -> bool {
    shape.extra_data.iter().any(|&r| {
        nif.get_as::<nif::ExtraData>(r).is_some_and(|e| {
            e.name == "RepColor" && matches!(e.value, nif::ExtraValue::Integer(n) if n != 0)
        })
    })
}

/// Draws the meshes of the model under the entity this is on in their own
/// colours multiplied by this one, for as long as it's there.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub struct Tint(pub [f32; 3]);

/// On a mesh drawn tinted: the tint, the material it's drawn in without
/// it, and the mesh's own copy, recoloured as the tint changes.
#[derive(Component)]
pub(crate) struct Tinted<M: Material>(Tint, Handle<M>, Handle<M>);

/// A material whose colour a [`Tint`] can multiply.
pub(crate) trait Tintable: Material + Clone {
    fn colour(&mut self) -> &mut Color;
}

impl Tintable for StandardMaterial {
    fn colour(&mut self) -> &mut Color {
        &mut self.base_color
    }
}

impl Tintable for TeamMaterial {
    fn colour(&mut self) -> &mut Color {
        &mut self.base.base_color
    }
}

/// A copy of a material with its colour multiplied by a tint.
fn tinted<M: Tintable>(plain: &M, Tint([r, g, b]): Tint) -> M {
    let mut copy = plain.clone();
    let c = copy.colour().to_linear();
    *copy.colour() = LinearRgba::new(c.red * r, c.green * g, c.blue * b, c.alpha).into();
    copy
}

/// Draws the meshes under a [`Tint`] in copies of their materials,
/// recoloured as the tint changes, and in their own again once it's gone.
/// Meshes whose materials are animated keep them, as a copy would stand
/// still.
#[allow(clippy::type_complexity)]
pub(crate) fn tint<M: Tintable>(
    mut commands: Commands,
    roots: Query<(Entity, &Tint)>,
    children: Query<&Children>,
    plain: Query<&MeshMaterial3d<M>, (Without<Tinted<M>>, Without<Animated>)>,
    mut drawn: Query<(Entity, &mut Tinted<M>, &MeshMaterial3d<M>)>,
    parents: Query<&ChildOf>,
    mut materials: ResMut<Assets<M>>,
) {
    for (root, &tint) in &roots {
        for e in children.iter_descendants(root) {
            let Ok(material) = plain.get(e) else {
                continue;
            };
            let Some(copy) = materials.get(&material.0).map(|m| tinted(m, tint)) else {
                continue;
            };
            let copy = materials.add(copy);
            let marker = Tinted(tint, material.0.clone(), copy.clone());
            commands.entity(e).insert((MeshMaterial3d(copy), marker));
        }
    }
    for (e, mut marker, now) in &mut drawn {
        // Given another material since, it keeps that one, to be tinted
        // afresh if it's still to be.
        if now.0 != marker.2 {
            commands.entity(e).remove::<Tinted<M>>();
            continue;
        }
        let under = parents.iter_ancestors(e).find_map(|a| roots.get(a).ok());
        let recoloured = match under {
            Some((_, &t)) if t == marker.0 => continue,
            Some((_, &t)) => materials.get(&marker.1).map(|m| (t, tinted(m, t))),
            None => None,
        };
        match recoloured {
            Some((t, copy)) => {
                if let Some(mut drawn) = materials.get_mut(&marker.2) {
                    *drawn = copy;
                }
                marker.0 = t;
            }
            None => {
                let plain = MeshMaterial3d(marker.1.clone());
                commands.entity(e).insert(plain).remove::<Tinted<M>>();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn tinted_meshes_are_recoloured_in_place_and_untinted_at_once() {
        let mut world = World::new();
        let mut materials = Assets::<StandardMaterial>::default();
        let plain = materials.add(StandardMaterial::from_color(LinearRgba::WHITE));
        world.insert_resource(materials);
        let root = world.spawn(Tint([0.5, 0.5, 0.5])).id();
        let mesh = world
            .spawn((MeshMaterial3d(plain.clone()), ChildOf(root)))
            .id();
        let run = |world: &mut World| {
            world
                .run_system_once(tint::<StandardMaterial>)
                .expect("runs");
        };
        let drawn = |world: &World| {
            let handle = &world
                .get::<MeshMaterial3d<StandardMaterial>>(mesh)
                .unwrap()
                .0;
            let materials = world.resource::<Assets<StandardMaterial>>();
            (
                handle.clone(),
                materials.get(handle).unwrap().base_color.to_linear().red,
            )
        };

        run(&mut world);
        let (copy, red) = drawn(&world);
        assert_ne!(copy, plain);
        assert_eq!(red, 0.5);

        // A new strength recolours the same copy, never showing the plain
        // material between.
        world.entity_mut(root).insert(Tint([0.25, 0.25, 0.25]));
        run(&mut world);
        assert_eq!(drawn(&world), (copy.clone(), 0.25));

        world.entity_mut(root).remove::<Tint>();
        run(&mut world);
        assert_eq!(drawn(&world), (plain, 1.0));
        assert!(world.get::<Tinted<StandardMaterial>>(mesh).is_none());
    }
}
