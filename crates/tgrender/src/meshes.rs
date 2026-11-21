//! Building meshes.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

/// A mesh of the triangles `indices` lists, three to each, between
/// vertices with these positions, normals and texture coordinates.
pub(crate) fn triangles(
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
) -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

/// Normals for `n` vertices of something lying flat: all straight up.
pub(crate) fn up(n: usize) -> Vec<[f32; 3]> {
    vec![[0.0, 1.0, 0.0]; n]
}
