//! Models that blend with what's behind them yet still hide their own
//! farther parts, as shapes that write depth do.

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};

pub type SolidMaterial = ExtendedMaterial<StandardMaterial, Solid>;

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone, Default)]
pub struct Solid {}

impl MaterialExtension for Solid {
    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(depth) = &mut descriptor.depth_stencil {
            depth.depth_write_enabled = Some(true);
        }
        Ok(())
    }
}
