use crate::{
    lighting::{
        Lighting,
        material::{
            LightMaterial, LightMode, LitTarget, ShadowMaterial, ShadowTarget, WaterMaterial,
            WaterTarget,
        },
    },
    third_party::time::Instant,
};
use intuicio_data::type_hash::TypeHash;
use nodio::{AnyIndex, ValueReadAccess, ValueWriteAccess, graph::Graph};
use spitfire_draw::{
    context::DrawContext,
    particles::{ParticleEmitter, ParticleInstance},
    primitives::PrimitivesEmitter,
    sprite::{Sprite, SpriteTexture},
    text::Text,
    utils::{Drawable, ResourceRef, Vertex, transform_to_matrix},
};
use spitfire_glow::graphics::Graphics;
use std::{
    collections::{HashMap, HashSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
};
use vek::{Aabr, Mat4, Rgb, Vec2, Vec3};

const UNBOUNDED: Aabr<f32> = Aabr {
    min: Vec2::new(f32::NEG_INFINITY, f32::NEG_INFINITY),
    max: Vec2::new(f32::INFINITY, f32::INFINITY),
};
const EMPTY: Aabr<f32> = Aabr {
    min: Vec2::new(f32::INFINITY, f32::INFINITY),
    max: Vec2::new(f32::NEG_INFINITY, f32::NEG_INFINITY),
};
const SOLID_THRESHOLD: f32 = 1.0e-4;
const NO_PARENT: usize = usize::MAX;

const PASS_GBUFFER: u8 = 1;
const PASS_OCCUPANCY: u8 = 2;
const PASS_SHADOW: u8 = 4;
const PASS_WATER: u8 = 8;
const PASS_FORWARD: u8 = 16;

struct ChildOf;
struct AttrOf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(AnyIndex);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AttrId(AnyIndex);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    GBuffer,
    Occupancy,
    Shadows,
    Water,
    Forward,
}

#[derive(Default)]
pub enum Primitive {
    #[default]
    Group,
    Sprite(Box<Sprite>),
    Text(Text),
    Particles(Box<Particles>),
    Triangles(Box<Triangles>),
}

/// The vertices are in the local space of the node. All triangles draw as one batch.
#[derive(Debug, Default, Clone)]
pub struct Triangles {
    pub emitter: PrimitivesEmitter,
    pub triangles: Vec<[Vertex; 3]>,
}

/// The instances are in the local space of the node. All instances draw as one batch.
#[derive(Debug, Default, Clone)]
pub struct Particles {
    pub emitter: ParticleEmitter,
    pub instances: Vec<ParticleInstance>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform2d {
    pub position: Vec2<f32>,
    pub rotation: f32,
    pub scale: Vec2<f32>,
}

impl Default for Transform2d {
    fn default() -> Self {
        Self {
            position: Vec2::zero(),
            rotation: 0.0,
            scale: Vec2::one(),
        }
    }
}

impl Transform2d {
    pub fn matrix(&self) -> Mat4<f32> {
        Mat4::<f32>::translation_2d(self.position)
            * Mat4::<f32>::rotation_z(self.rotation)
            * Mat4::<f32>::scaling_3d(Vec3::new(self.scale.x, self.scale.y, 1.0))
    }
}

/// `World` ignores the parent transform, but keeps the parent visibility and draw order.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Space {
    #[default]
    Inherit,
    World,
}

/// The sort is stable, so equal keys keep the insertion order.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ChildOrder {
    #[default]
    Insertion,
    Y,
    Z,
}

pub struct SceneNode {
    pub primitive: Primitive,
    pub transform: Transform2d,
    pub space: Space,
    pub visible: bool,
    pub child_order: ChildOrder,
    /// The sort key under a parent with `ChildOrder::Z`. `ChildOrder::Y` adds it to the world y.
    pub sort_key: f32,
}

impl Default for SceneNode {
    fn default() -> Self {
        Self {
            primitive: Primitive::Group,
            transform: Transform2d::default(),
            space: Space::Inherit,
            visible: true,
            child_order: ChildOrder::Insertion,
            sort_key: 0.0,
        }
    }
}

impl SceneNode {
    pub fn group() -> Self {
        Self::default()
    }

    pub fn sprite(sprite: Sprite) -> Self {
        Self {
            primitive: Primitive::Sprite(Box::new(sprite)),
            ..Default::default()
        }
    }

    pub fn text(text: Text) -> Self {
        Self {
            primitive: Primitive::Text(text),
            ..Default::default()
        }
    }

    pub fn particles(emitter: ParticleEmitter, instances: Vec<ParticleInstance>) -> Self {
        Self {
            primitive: Primitive::Particles(Box::new(Particles { emitter, instances })),
            ..Default::default()
        }
    }

    pub fn triangles(emitter: PrimitivesEmitter, triangles: Vec<[Vertex; 3]>) -> Self {
        Self {
            primitive: Primitive::Triangles(Box::new(Triangles { emitter, triangles })),
            ..Default::default()
        }
    }

    pub fn position(mut self, position: impl Into<Vec2<f32>>) -> Self {
        self.transform.position = position.into();
        self
    }

    pub fn rotation(mut self, rotation: f32) -> Self {
        self.transform.rotation = rotation;
        self
    }

    pub fn scale(mut self, scale: impl Into<Vec2<f32>>) -> Self {
        self.transform.scale = scale.into();
        self
    }

    pub fn space(mut self, space: Space) -> Self {
        self.space = space;
        self
    }

    pub fn child_order(mut self, child_order: ChildOrder) -> Self {
        self.child_order = child_order;
        self
    }

    pub fn sort_key(mut self, sort_key: f32) -> Self {
        self.sort_key = sort_key;
        self
    }

    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }
}

pub trait Attribute: 'static {}

/// The surface look of the node. A node without `Light` uses `LightMaterial::default()`.
#[derive(Debug, Clone)]
pub struct Light(pub LightMaterial);

/// A change of the color or the intensity rebuilds no light field, so flicker is cheap.
#[derive(Debug, Clone)]
pub struct Emitter {
    pub color: Rgb<f32>,
    pub intensity: f32,
    pub map: Option<SpriteTexture>,
}

impl Emitter {
    pub fn new(color: Rgb<f32>, intensity: f32) -> Self {
        Self {
            color,
            intensity,
            map: None,
        }
    }

    pub fn map(mut self, map: SpriteTexture) -> Self {
        self.map = Some(map);
        self
    }
}

/// `absorption` None takes the absorption of the node `Light`, or opaque when that is zero.
#[derive(Debug, Default, Clone)]
pub struct Occluder {
    pub shape: Silhouette,
    pub absorption: Option<Rgb<f32>>,
}

impl Occluder {
    pub fn shape(mut self, shape: Silhouette) -> Self {
        self.shape = shape;
        self
    }

    pub fn absorption(mut self, absorption: Rgb<f32>) -> Self {
        self.absorption = Some(absorption);
        self
    }
}

/// `erase_self` removes the own shadow from the top of the caster before higher casters draw.
/// Casters sort by `order`, or by the material height when `order` is None. Casters with the
/// same key all draw before their erasers, so a row of planks does not shadow itself.
#[derive(Debug, Clone)]
pub struct ShadowCaster {
    pub material: ShadowMaterial,
    pub shape: Silhouette,
    pub erase_self: bool,
    pub column: Option<ShadowColumn>,
    pub order: Option<f32>,
}

/// A column caster draws copies of its shape from `base` up to the material height,
/// so the shadow joins the ground to the top (trunks, poles, towers).
/// Without `count`, the copies stay closer than a third of `size` along the shadow, so a long
/// evening shadow does not break into separate dots. `size` None takes the smaller side of
/// the shape sprite. With `count`, the column draws `count + 1` evenly spaced copies.
/// A copy at height `h` has the softness `material.softness + h * softness_per_height`.
/// The column sorts by its top height. A particle shape draws no column.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct ShadowColumn {
    pub softness_per_height: f32,
    pub size: Option<f32>,
    pub base: f32,
    pub count: Option<u32>,
}

impl ShadowCaster {
    pub fn new(material: ShadowMaterial) -> Self {
        Self {
            material,
            shape: Silhouette::Own,
            erase_self: false,
            column: None,
            order: None,
        }
    }

    pub fn shape(mut self, shape: Silhouette) -> Self {
        self.shape = shape;
        self
    }

    pub fn erase_self(mut self) -> Self {
        self.erase_self = true;
        self
    }

    pub fn column(mut self, softness_per_height: f32) -> Self {
        self.column = Some(ShadowColumn {
            softness_per_height,
            ..Default::default()
        });
        self
    }

    /// Sets the copy spacing size of the column. Does nothing without a column.
    pub fn column_size(mut self, size: f32) -> Self {
        if let Some(column) = &mut self.column {
            column.size = Some(size);
        }
        self
    }

    /// Sets a fixed copy count and the height of the lowest copy. Does nothing without a column.
    pub fn column_steps(mut self, base: f32, count: u32) -> Self {
        if let Some(column) = &mut self.column {
            column.base = base;
            column.count = Some(count);
        }
        self
    }

    pub fn order(mut self, order: f32) -> Self {
        self.order = Some(order);
        self
    }

    fn sort_key(&self) -> f32 {
        self.order.unwrap_or(self.material.height)
    }
}

/// A node with `Water` draws only in the water stage.
#[derive(Debug, Clone)]
pub struct Water(pub WaterMaterial);

/// A node with `Forward` draws only in the forward stage.
#[derive(Debug, Default, Clone, Copy)]
pub struct Forward;

/// A node with `Unlit` draws only in the forward stage, with its own shader and blending.
/// No light material applies to it.
#[derive(Debug, Default, Clone, Copy)]
pub struct Unlit;

/// `Sprite` and `Particles` are in the local space of the node. A group node with a shape
/// draws only that shape, e.g. a shadow of falling petals.
#[derive(Debug, Default, Clone)]
pub enum Silhouette {
    #[default]
    Own,
    Sprite(Box<Sprite>),
    Particles(Box<Particles>),
}

impl Silhouette {
    pub fn sprite(sprite: Sprite) -> Self {
        Self::Sprite(Box::new(sprite))
    }

    pub fn particles(emitter: ParticleEmitter, instances: Vec<ParticleInstance>) -> Self {
        Self::Particles(Box::new(Particles { emitter, instances }))
    }

    /// `own` is the bounds of the node primitive.
    fn bounds(&self, world: Mat4<f32>, own: Aabr<f32>) -> Aabr<f32> {
        match self {
            Self::Own => own,
            Self::Sprite(sprite) => sprite_bounds(sprite, world),
            Self::Particles(particles) => particles_bounds(particles, world),
        }
    }
}

impl Attribute for Light {}
impl Attribute for Emitter {}
impl Attribute for Occluder {}
impl Attribute for ShadowCaster {}
impl Attribute for Water {}
impl Attribute for Forward {}
impl Attribute for Unlit {}

#[derive(Debug, Default, Clone, Copy)]
pub struct SceneStats {
    pub nodes: u32,
    pub rebuilt: bool,
    pub refreshed: u32,
    pub marks: u32,
    pub gbuffer: u32,
    pub occupancy: u32,
    pub shadows: u32,
    pub water: u32,
    pub forward: u32,
    pub update_ms: f32,
    pub render_ms: f32,
}

#[derive(Debug, Default, Clone, Copy)]
struct Attrs {
    light: Option<AnyIndex>,
    emitter: Option<AnyIndex>,
    occluder: Option<AnyIndex>,
    shadow: Option<AnyIndex>,
    water: Option<AnyIndex>,
    forward: bool,
    unlit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct OccupantKey {
    bounds: Aabr<f32>,
    shape: u64,
    solid: bool,
}

#[derive(Debug, Clone)]
struct DrawItem {
    node: NodeId,
    parent: usize,
    subtree_end: usize,
    world: Mat4<f32>,
    visible: bool,
    passes: u8,
    attrs: Attrs,
    bounds: Aabr<f32>,
    shape_bounds: Aabr<f32>,
    subtree_bounds: Aabr<f32>,
    child_order: ChildOrder,
    sort_key: f32,
}

/// A retained scene of lit nodes. The game builds the nodes once and changes them in `update`.
/// `render` draws every pass of `Lighting` and marks the occupancy changes by itself.
pub struct Scene {
    graph: Graph,
    root: NodeId,
    occupancy_key: u64,
    dirty: HashSet<NodeId>,
    structure_dirty: bool,
    items: Vec<DrawItem>,
    item_index: HashMap<NodeId, usize>,
    changed: Vec<bool>,
    occupants: HashMap<NodeId, OccupantKey>,
    occupancy_marks: Vec<Aabr<f32>>,
    stats: SceneStats,
}

impl Default for Scene {
    fn default() -> Self {
        let mut graph = Graph::default();
        let root = NodeId(graph.insert(SceneNode::group()));
        Self {
            graph,
            root,
            occupancy_key: 0,
            occupancy_marks: Default::default(),
            dirty: Default::default(),
            structure_dirty: true,
            items: Default::default(),
            item_index: Default::default(),
            changed: Default::default(),
            occupants: Default::default(),
            stats: Default::default(),
        }
    }
}

impl Scene {
    pub fn root(&self) -> NodeId {
        self.root
    }

    pub fn stats(&self) -> SceneStats {
        self.stats
    }

    pub fn contains(&self, node: NodeId) -> bool {
        self.graph.is::<SceneNode>(node.0)
    }

    pub fn spawn(&mut self, parent: NodeId, node: SceneNode) -> NodeId {
        self.spawn_at(parent, usize::MAX, node)
    }

    /// An index past the end puts the node last.
    pub fn spawn_at(&mut self, parent: NodeId, index: usize, node: SceneNode) -> NodeId {
        let id = NodeId(self.graph.insert(node));
        let parent = if self.contains(parent) {
            parent
        } else {
            self.root
        };
        self.graph.relate_at::<ChildOf>(parent.0, id.0, index);
        self.structure_dirty = true;
        id
    }

    /// Removes the node, its subtree and their attributes. The root only loses its children.
    pub fn remove(&mut self, node: NodeId) {
        if !self.contains(node) {
            return;
        }
        let mut stack = self.children(node).collect::<Vec<_>>();
        while let Some(current) = stack.pop() {
            stack.extend(self.children(current));
            self.remove_single(current);
        }
        if node != self.root {
            self.remove_single(node);
        }
        self.structure_dirty = true;
    }

    fn remove_single(&mut self, node: NodeId) {
        let attrs = self
            .graph
            .relations_outgoing::<AttrOf>(node.0)
            .collect::<Vec<_>>();
        for attr in attrs {
            let _ = self.graph.remove(attr);
        }
        let _ = self.graph.remove(node.0);
        self.dirty.remove(&node);
    }

    /// Does nothing when `parent` is inside the subtree of `node`.
    pub fn reparent(&mut self, node: NodeId, parent: NodeId, index: Option<usize>) {
        if node == self.root || !self.contains(node) || !self.contains(parent) {
            return;
        }
        let mut current = Some(parent);
        while let Some(ancestor) = current {
            if ancestor == node {
                return;
            }
            current = self.parent(ancestor);
        }
        if let Some(old) = self.parent(node) {
            self.graph.unrelate::<ChildOf>(old.0, node.0);
        }
        self.graph
            .relate_at::<ChildOf>(parent.0, node.0, index.unwrap_or(usize::MAX));
        self.structure_dirty = true;
    }

    pub fn move_to(&mut self, node: NodeId, index: usize) {
        if let Some(parent) = self.parent(node) {
            self.graph.move_relation::<ChildOf>(parent.0, node.0, index);
            self.structure_dirty = true;
        }
    }

    pub fn node(&self, node: NodeId) -> Option<ValueReadAccess<'_, SceneNode>> {
        self.graph.read::<SceneNode>(node.0).ok()
    }

    pub fn node_mut(&mut self, node: NodeId) -> Option<ValueWriteAccess<'_, SceneNode>> {
        let access = self.graph.write::<SceneNode>(node.0).ok()?;
        self.dirty.insert(node);
        Some(access)
    }

    /// Replaces the attribute of the same type.
    pub fn attach<A: Attribute>(&mut self, node: NodeId, attribute: A) -> Option<AttrId> {
        if !self.contains(node) {
            return None;
        }
        if let Some(old) = self.attr_index::<A>(node) {
            let _ = self.graph.remove(old);
        }
        let attr = self.graph.insert(attribute);
        self.graph.relate::<AttrOf>(node.0, attr);
        self.dirty.insert(node);
        Some(AttrId(attr))
    }

    pub fn detach<A: Attribute>(&mut self, node: NodeId) -> bool {
        let Some(attr) = self.attr_index::<A>(node) else {
            return false;
        };
        let _ = self.graph.remove(attr);
        self.dirty.insert(node);
        true
    }

    pub fn attr<A: Attribute>(&self, node: NodeId) -> Option<ValueReadAccess<'_, A>> {
        self.graph.read::<A>(self.attr_index::<A>(node)?).ok()
    }

    pub fn attr_mut<A: Attribute>(&mut self, node: NodeId) -> Option<ValueWriteAccess<'_, A>> {
        let attr = self.attr_index::<A>(node)?;
        let access = self.graph.write::<A>(attr).ok()?;
        self.dirty.insert(node);
        Some(access)
    }

    pub fn children(&self, node: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.graph.relations_outgoing::<ChildOf>(node.0).map(NodeId)
    }

    pub fn parent(&self, node: NodeId) -> Option<NodeId> {
        self.graph
            .relations_incomming::<ChildOf>(node.0)
            .next()
            .map(NodeId)
    }

    /// The position after the last `render`.
    pub fn world_position(&self, node: NodeId) -> Option<Vec2<f32>> {
        let item = &self.items[*self.item_index.get(&node)?];
        Some(item.world.mul_point(Vec3::zero()).xy())
    }

    /// The next `render` rebuilds the light fields inside these world bounds. The scene marks
    /// its own node changes, so only occupants drawn in a `render_with` hook need this.
    pub fn mark_occupancy_changed(&mut self, min: Vec2<f32>, max: Vec2<f32>) {
        self.occupancy_marks.push(Aabr { min, max });
    }

    /// Removes all nodes. The next `render` rebuilds the light fields in full.
    pub fn clear(&mut self) {
        self.graph.clear();
        self.root = NodeId(self.graph.insert(SceneNode::group()));
        self.occupancy_key = self.occupancy_key.wrapping_add(1);
        self.dirty.clear();
        self.items.clear();
        self.item_index.clear();
        self.occupants.clear();
        self.structure_dirty = true;
    }

    pub fn render(
        &mut self,
        lighting: &mut Lighting,
        draw: &mut DrawContext,
        graphics: &mut Graphics<Vertex>,
    ) {
        self.render_with(lighting, draw, graphics, |_, _, _| {});
    }

    /// `hook` runs at the end of each stage, so immediate draws can mix with the scene.
    pub fn render_with(
        &mut self,
        lighting: &mut Lighting,
        draw: &mut DrawContext,
        graphics: &mut Graphics<Vertex>,
        mut hook: impl FnMut(Stage, &mut DrawContext, &mut Graphics<Vertex>),
    ) {
        let start = Instant::now();
        self.update(lighting);
        self.stats.update_ms = start.elapsed().as_secs_f32() * 1000.0;

        lighting.begin(draw, graphics);
        let view = lighting.view_bounds();
        self.stats.gbuffer = self.draw_walk(
            view,
            PASS_GBUFFER,
            draw,
            graphics,
            |scene, item, draw, graphics| {
                scene.draw_lit(item, false, LightMode::Deferred, draw, graphics);
            },
        );
        hook(Stage::GBuffer, draw, graphics);

        lighting.begin_occupancy(draw, graphics);
        let occupancy = lighting.occupancy_bounds();
        self.stats.occupancy = self.draw_walk(
            occupancy,
            PASS_OCCUPANCY,
            draw,
            graphics,
            |scene, item, draw, graphics| {
                scene.draw_lit(item, true, LightMode::Deferred, draw, graphics);
            },
        );
        hook(Stage::Occupancy, draw, graphics);
        lighting.end_occupancy(draw, graphics);

        let casters = self.shadow_casters(lighting, view);
        self.stats.shadows = casters.len() as u32;
        lighting.begin_shadows(draw, graphics);
        self.draw_shadows(&casters, lighting.config.shadow_length, draw, graphics);
        hook(Stage::Shadows, draw, graphics);
        lighting.end_shadows(draw, graphics);

        lighting.resolve(draw, graphics);
        self.stats.water = self.draw_walk(
            view,
            PASS_WATER,
            draw,
            graphics,
            |scene, item, draw, graphics| {
                scene.draw_water(item, draw, graphics);
            },
        );
        hook(Stage::Water, draw, graphics);
        self.stats.forward = self.draw_walk(
            view,
            PASS_FORWARD,
            draw,
            graphics,
            |scene, item, draw, graphics| {
                if item.attrs.unlit {
                    scene.draw_unlit(item, draw, graphics);
                } else {
                    scene.draw_lit(item, false, LightMode::Forward, draw, graphics);
                }
            },
        );
        hook(Stage::Forward, draw, graphics);
        lighting.end(draw, graphics);
        self.stats.render_ms = start.elapsed().as_secs_f32() * 1000.0;
    }

    fn attr_index<A: Attribute>(&self, node: NodeId) -> Option<AnyIndex> {
        let type_hash = TypeHash::of::<A>();
        self.graph
            .relations_outgoing::<AttrOf>(node.0)
            .find(|attr| attr.type_hash() == type_hash)
    }

    fn update(&mut self, lighting: &mut Lighting) {
        self.stats = SceneStats::default();
        lighting.set_occupancy_key(Some(self.occupancy_key));
        if !self.structure_dirty {
            for node in &self.dirty {
                let Some(&index) = self.item_index.get(node) else {
                    continue;
                };
                let item = &self.items[index];
                let parent_sorts_y = item.parent != NO_PARENT
                    && self.items[item.parent].child_order == ChildOrder::Y;
                let order_changed = self.graph.read::<SceneNode>(node.0).is_ok_and(|node| {
                    node.child_order != item.child_order || node.sort_key != item.sort_key
                });
                if parent_sorts_y || order_changed {
                    self.structure_dirty = true;
                    break;
                }
            }
        }
        let mut marks = if self.structure_dirty {
            self.rebuild()
        } else if !self.dirty.is_empty() {
            self.refresh()
        } else {
            Vec::new()
        };
        self.dirty.clear();
        marks.append(&mut self.occupancy_marks);
        self.stats.nodes = self.items.len() as u32;
        self.stats.marks = marks.len() as u32;
        for bounds in marks {
            lighting.mark_occupancy_changed(bounds.min, bounds.max);
        }
    }

    fn rebuild(&mut self) -> Vec<Aabr<f32>> {
        self.structure_dirty = false;
        self.stats.rebuilt = true;
        let old_items = std::mem::take(&mut self.items);
        let old_index = std::mem::take(&mut self.item_index);
        self.push_subtree(self.root, NO_PARENT);
        self.update_subtree_bounds();
        self.changed.clear();
        self.changed.extend(self.items.iter().map(|item| {
            self.dirty.contains(&item.node)
                || old_index.get(&item.node).is_none_or(|index| {
                    let old = &old_items[*index];
                    old.world != item.world
                        || old.visible != item.visible
                        || old.passes != item.passes
                })
        }));
        let mut marks = self.update_occupants();
        let item_index = &self.item_index;
        self.occupants.retain(|node, key| {
            let keep = item_index.contains_key(node);
            if !keep {
                marks.push(key.bounds);
            }
            keep
        });
        marks
    }

    fn push_subtree(&mut self, node: NodeId, parent: usize) {
        let index = self.items.len();
        let Some(item) = self.make_item(node, parent) else {
            return;
        };
        let world = item.world;
        let child_order = item.child_order;
        self.items.push(item);
        self.item_index.insert(node, index);
        let mut children = self.children(node).collect::<Vec<_>>();
        match child_order {
            ChildOrder::Insertion => {}
            ChildOrder::Y => {
                let mut keyed = children
                    .iter()
                    .map(|child| {
                        let key = self.node(*child).map_or(0.0, |child| {
                            let local = child.transform.matrix();
                            let world = match child.space {
                                Space::Inherit => world * local,
                                Space::World => local,
                            };
                            world.mul_point(Vec3::zero()).y + child.sort_key
                        });
                        (key, *child)
                    })
                    .collect::<Vec<_>>();
                keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
                children = keyed.into_iter().map(|(_, child)| child).collect();
            }
            ChildOrder::Z => {
                let mut keyed = children
                    .iter()
                    .map(|child| {
                        let key = self.node(*child).map_or(0.0, |child| child.sort_key);
                        (key, *child)
                    })
                    .collect::<Vec<_>>();
                keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
                children = keyed.into_iter().map(|(_, child)| child).collect();
            }
        }
        for child in children {
            self.push_subtree(child, index);
        }
        self.items[index].subtree_end = self.items.len();
    }

    fn refresh(&mut self) -> Vec<Aabr<f32>> {
        self.changed.clear();
        self.changed.resize(self.items.len(), false);
        for node in &self.dirty {
            if let Some(&index) = self.item_index.get(node) {
                self.changed[index] = true;
            }
        }
        for index in 0..self.items.len() {
            let parent = self.items[index].parent;
            if !self.changed[index] && (parent == NO_PARENT || !self.changed[parent]) {
                continue;
            }
            self.changed[index] = true;
            let node = self.items[index].node;
            if let Some(mut item) = self.make_item(node, parent) {
                item.subtree_end = self.items[index].subtree_end;
                self.items[index] = item;
                self.stats.refreshed += 1;
            }
        }
        self.update_subtree_bounds();
        self.update_occupants()
    }

    fn make_item(&self, node: NodeId, parent: usize) -> Option<DrawItem> {
        let scene_node = self.graph.read::<SceneNode>(node.0).ok()?;
        let local = scene_node.transform.matrix();
        let (parent_world, parent_visible) = match self.items.get(parent) {
            Some(parent) => (parent.world, parent.visible),
            None => (Mat4::identity(), true),
        };
        let world = match scene_node.space {
            Space::Inherit => parent_world * local,
            Space::World => local,
        };
        let mut attrs = Attrs::default();
        for attr in self.graph.relations_outgoing::<AttrOf>(node.0) {
            let type_hash = attr.type_hash();
            if type_hash == TypeHash::of::<Light>() {
                attrs.light = Some(attr);
            } else if type_hash == TypeHash::of::<Emitter>() {
                attrs.emitter = Some(attr);
            } else if type_hash == TypeHash::of::<Occluder>() {
                attrs.occluder = Some(attr);
            } else if type_hash == TypeHash::of::<ShadowCaster>() {
                attrs.shadow = Some(attr);
            } else if type_hash == TypeHash::of::<Water>() {
                attrs.water = Some(attr);
            } else if type_hash == TypeHash::of::<Forward>() {
                attrs.forward = true;
            } else if type_hash == TypeHash::of::<Unlit>() {
                attrs.unlit = true;
            }
        }
        let has_primitive = !matches!(scene_node.primitive, Primitive::Group);
        let light_absorbs = attrs.light.is_some_and(|light| {
            self.graph
                .read::<Light>(light)
                .is_ok_and(|light| light.0.absorption.reduce_partial_max() > 0.0)
        });
        let mut passes = 0;
        if attrs.water.is_some() {
            passes |= PASS_WATER;
        } else if attrs.forward || attrs.unlit {
            passes |= PASS_FORWARD;
        } else if has_primitive {
            passes |= PASS_GBUFFER;
        }
        if attrs.occluder.is_some() || attrs.emitter.is_some() || light_absorbs {
            passes |= PASS_OCCUPANCY;
        }
        if attrs.shadow.is_some() {
            passes |= PASS_SHADOW;
        }
        let bounds = primitive_bounds(&scene_node.primitive, world);
        let shape_bounds = attrs
            .occluder
            .and_then(|occluder| self.graph.read::<Occluder>(occluder).ok())
            .map_or(bounds, |occluder| occluder.shape.bounds(world, bounds));
        Some(DrawItem {
            node,
            parent,
            subtree_end: 0,
            world,
            visible: parent_visible && scene_node.visible,
            passes,
            attrs,
            bounds,
            shape_bounds,
            subtree_bounds: EMPTY,
            child_order: scene_node.child_order,
            sort_key: scene_node.sort_key,
        })
    }

    /// Children follow their parent in the list, so a reverse pass unions each subtree.
    fn update_subtree_bounds(&mut self) {
        for item in &mut self.items {
            item.subtree_bounds = union(item.bounds, item.shape_bounds);
        }
        for index in (1..self.items.len()).rev() {
            let parent = self.items[index].parent;
            if parent != NO_PARENT {
                let bounds = self.items[index].subtree_bounds;
                let parent = &mut self.items[parent];
                parent.subtree_bounds = union(parent.subtree_bounds, bounds);
            }
        }
    }

    fn update_occupants(&mut self) -> Vec<Aabr<f32>> {
        let mut marks = Vec::new();
        for index in 0..self.items.len() {
            if !self.changed[index] {
                continue;
            }
            let node = self.items[index].node;
            let key = self.occupant_key(&self.items[index]);
            let old = match key {
                Some(key) => self.occupants.insert(node, key),
                None => self.occupants.remove(&node),
            };
            if old == key {
                continue;
            }
            marks.extend(old.map(|old| old.bounds));
            marks.extend(key.map(|key| key.bounds));
        }
        marks
    }

    fn occupant_key(&self, item: &DrawItem) -> Option<OccupantKey> {
        if !item.visible || item.passes & PASS_OCCUPANCY == 0 {
            return None;
        }
        let mut hasher = DefaultHasher::new();
        for value in item.world.into_col_array() {
            value.to_bits().hash(&mut hasher);
        }
        let occluder = item
            .attrs
            .occluder
            .and_then(|occluder| self.graph.read::<Occluder>(occluder).ok());
        match occluder.as_deref() {
            Some(Occluder {
                shape: Silhouette::Sprite(sprite),
                ..
            }) => hash_sprite(sprite, &mut hasher),
            Some(Occluder {
                shape: Silhouette::Particles(particles),
                ..
            }) => hash_particles(particles, &mut hasher),
            _ => {
                let node = self.graph.read::<SceneNode>(item.node.0).ok()?;
                match &node.primitive {
                    Primitive::Group => {}
                    Primitive::Sprite(sprite) => hash_sprite(sprite, &mut hasher),
                    Primitive::Text(text) => {
                        text.text.hash(&mut hasher);
                        text.font.hash(&mut hasher);
                        text.size.to_bits().hash(&mut hasher);
                        hash_floats(text.tint.into_array(), &mut hasher);
                        hash_transform(text.transform, &mut hasher);
                    }
                    Primitive::Particles(particles) => hash_particles(particles, &mut hasher),
                    Primitive::Triangles(triangles) => hash_triangles(triangles, &mut hasher),
                }
            }
        }
        let material = self.material(item, true);
        let solid = material.emission.reduce_partial_max() > SOLID_THRESHOLD
            || material.absorption.reduce_partial_max() > SOLID_THRESHOLD;
        Some(OccupantKey {
            bounds: item.shape_bounds,
            shape: hasher.finish(),
            solid,
        })
    }

    fn material(&self, item: &DrawItem, occupancy: bool) -> LightMaterial {
        let mut material = item
            .attrs
            .light
            .and_then(|light| self.graph.read::<Light>(light).ok())
            .map(|light| light.0.clone())
            .unwrap_or_default();
        if let Some(emitter) = item
            .attrs
            .emitter
            .and_then(|emitter| self.graph.read::<Emitter>(emitter).ok())
        {
            material.emission += emitter.color * emitter.intensity;
            if emitter.map.is_some() {
                material.emission_map = emitter.map.clone();
            }
        }
        if let Some(occluder) = item
            .attrs
            .occluder
            .and_then(|occluder| self.graph.read::<Occluder>(occluder).ok())
            && (occupancy || matches!(occluder.shape, Silhouette::Own))
        {
            material.absorption = occluder.absorption.unwrap_or_else(|| {
                if material.absorption.reduce_partial_max() > 0.0 {
                    material.absorption
                } else {
                    Rgb::broadcast(LightMaterial::OPAQUE_ABSORPTION)
                }
            });
        }
        material
    }

    /// Skips a whole subtree when the subtree bounds miss `area`. Returns the drawn count.
    fn draw_walk(
        &self,
        area: Aabr<f32>,
        pass: u8,
        draw: &mut DrawContext,
        graphics: &mut Graphics<Vertex>,
        mut f: impl FnMut(&Self, &DrawItem, &mut DrawContext, &mut Graphics<Vertex>),
    ) -> u32 {
        let mut drawn = 0;
        let mut index = 0;
        while index < self.items.len() {
            let item = &self.items[index];
            if !item.visible || !intersects(item.subtree_bounds, area) {
                index = item.subtree_end.max(index + 1);
                continue;
            }
            let bounds = if pass == PASS_OCCUPANCY {
                item.shape_bounds
            } else {
                item.bounds
            };
            if item.passes & pass != 0 && intersects(bounds, area) {
                f(self, item, draw, graphics);
                drawn += 1;
            }
            index += 1;
        }
        drawn
    }

    fn draw_lit(
        &self,
        item: &DrawItem,
        occupancy: bool,
        mode: LightMode,
        draw: &mut DrawContext,
        graphics: &mut Graphics<Vertex>,
    ) {
        let material = self.material(item, occupancy);
        let silhouette = occupancy
            .then(|| {
                item.attrs
                    .occluder
                    .and_then(|occluder| self.graph.read::<Occluder>(occluder).ok())
            })
            .flatten();
        draw.push_transform_relative(item.world);
        let shape = silhouette.as_deref().map(|occluder| &occluder.shape);
        if let Some(Silhouette::Sprite(sprite)) = shape {
            let mut sprite = sprite.clone();
            sprite.apply_light_material(&material, mode);
            sprite.draw(draw, graphics);
        } else if let Some(Silhouette::Particles(particles)) = shape {
            let mut emitter = particles.emitter.clone();
            emitter.apply_light_material(&material, mode);
            emitter
                .emit(particles.instances.iter().cloned())
                .draw(draw, graphics);
        } else if let Ok(mut node) = self.graph.write::<SceneNode>(item.node.0) {
            match &mut node.primitive {
                Primitive::Group => {}
                Primitive::Sprite(sprite) => {
                    let mut sprite = sprite.clone();
                    sprite.apply_light_material(&material, mode);
                    sprite.draw(draw, graphics);
                }
                // Text has no Clone. The material sets every field it reads, so an in-place
                // apply is safe across passes.
                Primitive::Text(text) => {
                    text.apply_light_material(&material, mode);
                    text.draw(draw, graphics);
                }
                Primitive::Particles(particles) => {
                    let mut emitter = particles.emitter.clone();
                    emitter.apply_light_material(&material, mode);
                    emitter
                        .emit(particles.instances.iter().cloned())
                        .draw(draw, graphics);
                }
                Primitive::Triangles(triangles) => {
                    let mut emitter = triangles.emitter.clone();
                    emitter.apply_light_material(&material, mode);
                    emitter
                        .emit_triangles(triangles.triangles.iter().copied())
                        .draw(draw, graphics);
                }
            }
        }
        draw.pop_transform();
    }

    fn draw_unlit(&self, item: &DrawItem, draw: &mut DrawContext, graphics: &mut Graphics<Vertex>) {
        let Ok(node) = self.graph.read::<SceneNode>(item.node.0) else {
            return;
        };
        draw.push_transform_relative(item.world);
        match &node.primitive {
            Primitive::Group => {}
            Primitive::Sprite(sprite) => sprite.draw(draw, graphics),
            Primitive::Text(text) => text.draw(draw, graphics),
            Primitive::Particles(particles) => particles
                .emitter
                .emit(particles.instances.iter().cloned())
                .draw(draw, graphics),
            Primitive::Triangles(triangles) => triangles
                .emitter
                .emit_triangles(triangles.triangles.iter().copied())
                .draw(draw, graphics),
        }
        draw.pop_transform();
    }

    fn draw_water(&self, item: &DrawItem, draw: &mut DrawContext, graphics: &mut Graphics<Vertex>) {
        let (Some(water), Ok(node)) = (
            item.attrs
                .water
                .and_then(|water| self.graph.read::<Water>(water).ok()),
            self.graph.read::<SceneNode>(item.node.0),
        ) else {
            return;
        };
        if let Primitive::Sprite(sprite) = &node.primitive {
            let mut sprite = sprite.clone();
            sprite.apply_water_material(&water.0);
            draw.push_transform_relative(item.world);
            sprite.draw(draw, graphics);
            draw.pop_transform();
        }
    }

    /// Casters sort from low to high. A caster with `erase_self` draws as an eraser right after
    /// itself, so its own shadow leaves its top before the higher casters draw.
    fn shadow_casters(&self, lighting: &Lighting, view: Aabr<f32>) -> Vec<usize> {
        let offset = -lighting
            .config
            .sun_direction
            .try_normalized()
            .unwrap_or_default()
            * lighting.config.shadow_length;
        let mut casters = Vec::new();
        let mut index = 0;
        while index < self.items.len() {
            let item = &self.items[index];
            if !item.visible {
                index = item.subtree_end.max(index + 1);
                continue;
            }
            let current = index;
            index += 1;
            let Some(caster) = item
                .attrs
                .shadow
                .and_then(|shadow| self.graph.read::<ShadowCaster>(shadow).ok())
            else {
                continue;
            };
            let bounds = caster.shape.bounds(item.world, item.bounds);
            let shift = offset * caster.material.height;
            let reach = Aabr {
                min: bounds.min + Vec2::partial_min(shift, Vec2::zero()),
                max: bounds.max + Vec2::partial_max(shift, Vec2::zero()),
            };
            if intersects(reach, view) {
                casters.push((caster.sort_key(), current));
            }
        }
        casters.sort_by(|a, b| a.0.total_cmp(&b.0));
        casters.into_iter().map(|(_, index)| index).collect()
    }

    fn draw_shadows(
        &self,
        casters: &[usize],
        shadow_length: f32,
        draw: &mut DrawContext,
        graphics: &mut Graphics<Vertex>,
    ) {
        let casters = casters
            .iter()
            .filter_map(|index| {
                let item = &self.items[*index];
                let caster = item
                    .attrs
                    .shadow
                    .and_then(|shadow| self.graph.read::<ShadowCaster>(shadow).ok())?;
                let shape = match &caster.shape {
                    Silhouette::Sprite(sprite) => {
                        Some(ShadowShape::Sprite(sprite.as_ref().clone()))
                    }
                    Silhouette::Particles(particles) => {
                        Some(ShadowShape::Particles(particles.as_ref().clone()))
                    }
                    Silhouette::Own => {
                        self.node(item.node).and_then(|node| match &node.primitive {
                            Primitive::Sprite(sprite) => {
                                Some(ShadowShape::Sprite(sprite.as_ref().clone()))
                            }
                            Primitive::Particles(particles) => {
                                Some(ShadowShape::Particles(particles.as_ref().clone()))
                            }
                            _ => None,
                        })
                    }
                }?;
                Some((item.world, caster.clone(), shape))
            })
            .collect::<Vec<_>>();
        for level in casters.chunk_by(|a, b| a.1.sort_key() == b.1.sort_key()) {
            for (world, caster, shape) in level {
                draw.push_transform_relative(*world);
                match (caster.column, shape) {
                    (Some(column), ShadowShape::Sprite(sprite)) => {
                        let count = column.count.unwrap_or_else(|| {
                            let size = column.size.unwrap_or_else(|| {
                                sprite.size.unwrap_or_default().reduce_partial_min()
                            });
                            column_count(caster.material.height - column.base, size, shadow_length)
                        });
                        for height in column_heights(column.base, caster.material.height, count) {
                            let mut material = caster.material;
                            material.height = height;
                            material.softness += height * column.softness_per_height;
                            shape.draw(&material, draw, graphics);
                        }
                    }
                    _ => shape.draw(&caster.material, draw, graphics),
                }
                draw.pop_transform();
            }
            for (world, _, shape) in level.iter().filter(|(_, caster, _)| caster.erase_self) {
                draw.push_transform_relative(*world);
                shape.draw(&ShadowMaterial::eraser(), draw, graphics);
                draw.pop_transform();
            }
        }
    }
}

enum ShadowShape {
    Sprite(Sprite),
    Particles(Particles),
}

impl ShadowShape {
    fn draw(
        &self,
        material: &ShadowMaterial,
        draw: &mut DrawContext,
        graphics: &mut Graphics<Vertex>,
    ) {
        match self {
            Self::Sprite(sprite) => {
                let mut sprite = sprite.clone();
                sprite.apply_shadow_material(material);
                sprite.draw(draw, graphics);
            }
            Self::Particles(particles) => {
                let mut emitter = particles.emitter.clone();
                emitter.apply_shadow_material(material);
                emitter
                    .emit(particles.instances.iter().cloned())
                    .draw(draw, graphics);
            }
        }
    }
}

fn column_count(span: f32, size: f32, shadow_length: f32) -> u32 {
    let step = size.max(1.0) * 0.3 / shadow_length.max(0.1);
    (span / step).ceil().max(1.0) as u32
}

fn column_heights(base: f32, top: f32, count: u32) -> impl Iterator<Item = f32> {
    let count = count.max(1);
    (0..=count).map(move |index| base + (top - base) * index as f32 / count as f32)
}

fn union(a: Aabr<f32>, b: Aabr<f32>) -> Aabr<f32> {
    Aabr {
        min: Vec2::partial_min(a.min, b.min),
        max: Vec2::partial_max(a.max, b.max),
    }
}

fn intersects(a: Aabr<f32>, b: Aabr<f32>) -> bool {
    a.min.x < b.max.x && a.max.x > b.min.x && a.min.y < b.max.y && a.max.y > b.min.y
}

/// Text has no size before the layout, so text is never culled.
fn primitive_bounds(primitive: &Primitive, world: Mat4<f32>) -> Aabr<f32> {
    match primitive {
        Primitive::Group => EMPTY,
        Primitive::Sprite(sprite) => sprite_bounds(sprite, world),
        Primitive::Text(_) => UNBOUNDED,
        Primitive::Particles(particles) => particles_bounds(particles, world),
        Primitive::Triangles(triangles) => triangles
            .triangles
            .iter()
            .flatten()
            .map(|vertex| {
                let point = world
                    .mul_point(Vec3::new(vertex.position[0], vertex.position[1], 0.0))
                    .xy();
                Aabr {
                    min: point,
                    max: point,
                }
            })
            .fold(EMPTY, union),
    }
}

fn particles_bounds(particles: &Particles, world: Mat4<f32>) -> Aabr<f32> {
    particles
        .instances
        .iter()
        .map(|instance| {
            quad_bounds(
                world * transform_to_matrix(instance.transform),
                instance.size,
                instance.pivot,
            )
        })
        .fold(EMPTY, union)
}

/// A sprite without a size takes the texture size at draw time, so it is never culled.
fn sprite_bounds(sprite: &Sprite, world: Mat4<f32>) -> Aabr<f32> {
    let Some(size) = sprite.size else {
        return UNBOUNDED;
    };
    quad_bounds(
        world * transform_to_matrix(sprite.transform),
        size,
        sprite.pivot,
    )
}

fn quad_bounds(matrix: Mat4<f32>, size: Vec2<f32>, pivot: Vec2<f32>) -> Aabr<f32> {
    let min = -size * pivot;
    let max = size - size * pivot;
    [min, Vec2::new(max.x, min.y), max, Vec2::new(min.x, max.y)]
        .into_iter()
        .map(|corner| matrix.mul_point(Vec3::from(corner)).xy())
        .fold(EMPTY, |bounds, point| {
            union(
                bounds,
                Aabr {
                    min: point,
                    max: point,
                },
            )
        })
}

fn hash_floats<const N: usize>(values: [f32; N], hasher: &mut DefaultHasher) {
    for value in values {
        value.to_bits().hash(hasher);
    }
}

fn hash_transform(transform: vek::Transform<f32, f32, f32>, hasher: &mut DefaultHasher) {
    hash_floats(transform.position.into_array(), hasher);
    hash_floats(transform.orientation.into_vec4().into_array(), hasher);
    hash_floats(transform.scale.into_array(), hasher);
}

fn hash_textures(textures: &[SpriteTexture], hasher: &mut DefaultHasher) {
    for texture in textures {
        match &texture.texture {
            ResourceRef::Name(name) => name.hash(hasher),
            ResourceRef::Object(_) => 1u8.hash(hasher),
        }
    }
}

fn hash_particles(particles: &Particles, hasher: &mut DefaultHasher) {
    hash_textures(&particles.emitter.textures, hasher);
    for instance in &particles.instances {
        hash_floats(
            [
                instance.region.x,
                instance.region.y,
                instance.region.w,
                instance.region.h,
                instance.page,
            ],
            hasher,
        );
        hash_floats(instance.tint.into_array(), hasher);
        hash_floats(instance.size.into_array(), hasher);
        hash_floats(instance.pivot.into_array(), hasher);
        hash_transform(instance.transform, hasher);
    }
}

fn hash_triangles(triangles: &Triangles, hasher: &mut DefaultHasher) {
    hash_textures(&triangles.emitter.textures, hasher);
    for vertex in triangles.triangles.iter().flatten() {
        hash_floats(vertex.position, hasher);
        hash_floats(vertex.uv, hasher);
        hash_floats(vertex.color, hasher);
    }
}

fn hash_sprite(sprite: &Sprite, hasher: &mut DefaultHasher) {
    hash_textures(&sprite.textures, hasher);
    hash_floats(
        [
            sprite.region.x,
            sprite.region.y,
            sprite.region.w,
            sprite.region.h,
            sprite.page,
        ],
        hasher,
    );
    hash_floats(sprite.tint.into_array(), hasher);
    hash_floats(sprite.size.unwrap_or_default().into_array(), hasher);
    hash_floats(sprite.pivot.into_array(), hasher);
    hash_transform(sprite.transform, hasher);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(size: f32) -> SceneNode {
        SceneNode::sprite(Sprite::default().size(Vec2::broadcast(size)))
    }

    fn order(scene: &Scene) -> Vec<NodeId> {
        scene.items.iter().map(|item| item.node).collect()
    }

    fn update(scene: &mut Scene) -> u32 {
        let mut lighting = Lighting::default();
        scene.update(&mut lighting);
        scene.stats.marks
    }

    #[test]
    fn test_draw_order_follows_insertion_and_sort_modes() {
        let mut scene = Scene::default();
        let root = scene.root();
        let layer = scene.spawn(root, SceneNode::group().child_order(ChildOrder::Z));
        let high = scene.spawn(layer, quad(1.0).sort_key(2.0));
        let low = scene.spawn(layer, quad(1.0).sort_key(1.0));
        let actors = scene.spawn(root, SceneNode::group().child_order(ChildOrder::Y));
        let south = scene.spawn(actors, quad(1.0).position(Vec2::new(0.0, 50.0)));
        let north = scene.spawn(actors, quad(1.0).position(Vec2::new(0.0, 10.0)));
        let first = scene.spawn_at(root, 0, SceneNode::group());
        update(&mut scene);
        assert_eq!(
            order(&scene),
            vec![root, first, layer, low, high, actors, north, south]
        );

        scene.node_mut(north).unwrap().transform.position.y = 90.0;
        scene.move_to(first, 5);
        update(&mut scene);
        assert_eq!(
            order(&scene),
            vec![root, layer, low, high, actors, south, north, first]
        );
    }

    #[test]
    fn test_transforms_inherit_unless_world_space() {
        let mut scene = Scene::default();
        let parent = scene.spawn(
            scene.root(),
            SceneNode::group().position(Vec2::new(10.0, 0.0)).scale(2.0),
        );
        let child = scene.spawn(parent, quad(1.0).position(Vec2::new(1.0, 1.0)));
        let world = scene.spawn(
            parent,
            quad(1.0).position(Vec2::new(1.0, 1.0)).space(Space::World),
        );
        update(&mut scene);
        assert_eq!(scene.world_position(child), Some(Vec2::new(12.0, 2.0)));
        assert_eq!(scene.world_position(world), Some(Vec2::new(1.0, 1.0)));

        scene.node_mut(parent).unwrap().transform.position.x = 20.0;
        update(&mut scene);
        assert_eq!(scene.world_position(child), Some(Vec2::new(22.0, 2.0)));
        assert!(!scene.stats.rebuilt);
    }

    #[test]
    fn test_emission_change_marks_nothing_and_move_marks_both_places() {
        let mut scene = Scene::default();
        let lamp = scene.spawn(scene.root(), quad(4.0));
        scene.attach(lamp, Emitter::new(Rgb::one(), 1.0));
        assert_eq!(update(&mut scene), 1);

        scene.attr_mut::<Emitter>(lamp).unwrap().intensity = 3.0;
        assert_eq!(update(&mut scene), 0);

        scene.node_mut(lamp).unwrap().transform.position.x = 100.0;
        assert_eq!(update(&mut scene), 2);

        scene.node_mut(lamp).unwrap().visible = false;
        assert_eq!(update(&mut scene), 1);
    }

    #[test]
    fn test_manual_mark_reaches_one_update() {
        let mut scene = Scene::default();
        assert_eq!(update(&mut scene), 0);

        scene.mark_occupancy_changed(Vec2::zero(), Vec2::one());
        assert_eq!(update(&mut scene), 1);
        assert_eq!(update(&mut scene), 0);
    }

    #[test]
    fn test_remove_marks_old_bounds_and_drops_subtree() {
        let mut scene = Scene::default();
        let chunk = scene.spawn(scene.root(), SceneNode::group());
        let wall = scene.spawn(chunk, quad(4.0));
        scene.attach(wall, Occluder::default());
        let other = scene.spawn(scene.root(), quad(4.0));
        scene.attach(other, Occluder::default());
        assert_eq!(update(&mut scene), 2);

        scene.remove(chunk);
        assert_eq!(update(&mut scene), 1);
        assert!(!scene.contains(wall));
        assert!(scene.attr::<Occluder>(wall).is_none());
        assert_eq!(order(&scene), vec![scene.root(), other]);
        assert_eq!(scene.occupants.len(), 1);
    }

    #[test]
    fn test_particles_bound_their_instances_and_mark_on_change() {
        let mut scene = Scene::default();
        let smoke = scene.spawn(
            scene.root(),
            SceneNode::particles(
                ParticleEmitter::default(),
                vec![
                    ParticleInstance::default()
                        .position(Vec2::new(10.0, 0.0))
                        .size(Vec2::broadcast(4.0))
                        .pivot(Vec2::broadcast(0.5)),
                ],
            )
            .position(Vec2::new(100.0, 0.0)),
        );
        scene.attach(smoke, Light(LightMaterial::filter(Rgb::gray(0.5), 1.0)));
        assert_eq!(update(&mut scene), 1);
        let bounds = scene.items[scene.item_index[&smoke]].bounds;
        assert_eq!(bounds.min, Vec2::new(108.0, -2.0));
        assert_eq!(bounds.max, Vec2::new(112.0, 2.0));

        assert_eq!(update(&mut scene), 0);
        if let Some(mut node) = scene.node_mut(smoke)
            && let Primitive::Particles(particles) = &mut node.primitive
        {
            particles.instances.clear();
        }
        assert_eq!(update(&mut scene), 2);
    }

    #[test]
    fn test_unlit_draws_only_forward() {
        let mut scene = Scene::default();
        let tower = scene.spawn(scene.root(), quad(4.0));
        scene.attach(tower, Unlit);
        scene.attach(
            tower,
            ShadowCaster::new(ShadowMaterial::new(10.0)).column(0.1),
        );
        update(&mut scene);
        assert_eq!(
            scene.items[scene.item_index[&tower]].passes,
            PASS_FORWARD | PASS_SHADOW
        );
    }

    #[test]
    fn test_column_heights_span_ground_to_top_and_densify_with_length() {
        let short = column_heights(0.0, 120.0, column_count(120.0, 18.0, 0.5)).collect::<Vec<_>>();
        let long = column_heights(0.0, 120.0, column_count(120.0, 18.0, 3.0)).collect::<Vec<_>>();
        let fixed = column_heights(14.0, 140.0, 9).collect::<Vec<_>>();
        assert_eq!(fixed.len(), 10);
        assert_eq!(fixed.first(), Some(&14.0));
        assert_eq!(fixed.last(), Some(&140.0));
        assert_eq!(short.first(), Some(&0.0));
        assert_eq!(short.last(), Some(&120.0));
        assert!(long.len() > short.len());
        assert!(
            long.windows(2)
                .all(|pair| pair[1] - pair[0] <= 18.0 * 0.3 / 3.0 + 1.0e-4)
        );
    }

    #[test]
    fn test_shadow_order_overrides_height_and_particle_shape_bounds() {
        let mut scene = Scene::default();
        let petals = scene.spawn(scene.root(), SceneNode::group());
        scene.attach(
            petals,
            ShadowCaster::new(ShadowMaterial::new(1.0))
                .shape(Silhouette::particles(
                    ParticleEmitter::default(),
                    vec![
                        ParticleInstance::default()
                            .position(Vec2::new(5.0, 0.0))
                            .size(Vec2::broadcast(2.0)),
                    ],
                ))
                .order(2.0),
        );
        let trunk = scene.spawn(scene.root(), quad(4.0));
        scene.attach(
            trunk,
            ShadowCaster::new(ShadowMaterial::new(50.0)).order(0.0),
        );
        update(&mut scene);
        assert_eq!(scene.items[scene.item_index[&petals]].passes, PASS_SHADOW);
        let view = Aabr {
            min: Vec2::broadcast(-100.0),
            max: Vec2::broadcast(100.0),
        };
        let casters = scene.shadow_casters(&Lighting::default(), view);
        assert_eq!(
            casters,
            vec![scene.item_index[&trunk], scene.item_index[&petals]]
        );
    }

    #[test]
    fn test_subtree_culling_skips_far_groups() {
        let mut scene = Scene::default();
        let near = scene.spawn(scene.root(), SceneNode::group());
        scene.spawn(near, quad(4.0));
        let far = scene.spawn(
            scene.root(),
            SceneNode::group().position(Vec2::new(1000.0, 0.0)),
        );
        scene.spawn(far, quad(4.0));
        scene.spawn(far, quad(4.0));
        update(&mut scene);
        let mut visited = 0;
        let area = Aabr {
            min: Vec2::new(-10.0, -10.0),
            max: Vec2::new(10.0, 10.0),
        };
        let mut index = 0;
        while index < scene.items.len() {
            let item = &scene.items[index];
            if !intersects(item.subtree_bounds, area) {
                index = item.subtree_end;
                continue;
            }
            visited += 1;
            index += 1;
        }
        assert_eq!(visited, 3);
    }
}
