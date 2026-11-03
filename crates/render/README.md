# aaa-render

Draws Axis & Allies (2004) maps, as loaded by [`aaa-map`](../map), with
[Bevy](https://bevy.org): Bevy plugins and spawning functions that the editor
and the game can share.

## What it draws

- The height map, with each tile textured by its terrain variation and soft
  edges between terrain types from the game's transition masks
- Water surfaces, with the bed carved out below them and coloured by depth
  from the biome's water settings
- Placed objects with the game's models: trees, buildings, roads, fences and
  the like, at each object's own size, variation and height; skinned models such as
  tall grass stand in the pose they were modelled in
- Herds of animals, each animal where the map has it standing
- Models' own keyframe and fade animations, such as waves rolling in to shore
- Lighting from the map's sun, combined the way the game's fixed-function
  renderer does
- Simple markers where a model can't be drawn yet: magenta boxes for any
  object that should have one, boxes in their nation's colour for units, red
  pillars for start locations, and red boxes for objects without a model such
  as sound emitters

Not yet: animated water textures, skeletal animation, particle effects.
Some developer test maps refer to terrain types the game no longer
defines; those tiles show as magenta.

Licensed under the [MIT license](../../LICENSE).
