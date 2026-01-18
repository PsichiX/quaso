use crate::{context::GameContext, game::GameObject};
use gltf::animation::Interpolation;
use keket::database::{AssetDatabase, handle::AssetHandle};
use nodio::{
    AnyIndex,
    graph::Graph,
    query::{Node, QueryFetch, Related},
};
use send_wrapper::SendWrapper;
use serde_json::Value;
use spitfire_core::Triangle;
use spitfire_draw::{
    context::DrawContext,
    sprite::SpriteTexture,
    utils::{Drawable, ShaderRef, Vertex, transform_to_matrix},
};
use spitfire_glow::{
    graphics::{GraphicsBatch, GraphicsTarget},
    renderer::{GlowBlending, GlowUniformValue},
};
use std::{
    borrow::Cow,
    cmp::Ordering,
    collections::{HashMap, HashSet},
    error::Error,
    f32,
    hash::Hash,
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard, atomic::AtomicU32},
};
use vek::{Mat4, Quaternion, Rgba, Transform, Vec2, Vec3};

#[derive(Debug, Default, Clone)]
pub struct GltfMesh {
    pub primitives: Vec<GltfPrimitive>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct GltfVertex {
    pub position: Vec3<f32>,
    pub uv: Vec2<f32>,
    pub color: Rgba<f32>,
    pub joints: Option<[u16; 4]>,
    pub weights: Option<[f32; 4]>,
}

#[derive(Debug, Default, Clone)]
pub struct GltfPrimitive {
    pub main_texture: Option<SendWrapper<SpriteTexture>>,
    pub blending: GlowBlending,
    pub triangles: Vec<Triangle>,
    pub vertices: Vec<GltfVertex>,
}

#[derive(Debug, Clone)]
pub struct GltfSkin {
    pub inverse_bind_matrices: Vec<Mat4<f32>>,
    pub bones: Vec<GltfSkeletonBone>,
}

#[derive(Debug, Clone)]
pub struct GltfAnimation {
    pub channels: Vec<GltfAnimationChannel>,
    pub duration: f32,
}

#[derive(Debug, Clone)]
pub struct GltfAnimationChannel {
    pub target_node: GltfNodeId,
    pub times: Vec<f32>,
    pub duration: f32,
    pub values: GltfAnimationValues,
    pub interpolation: Interpolation,
}

#[derive(Debug, Clone)]
pub enum GltfAnimationValues {
    Translation(Vec<Vec3<f32>>),
    Rotation(Vec<Quaternion<f32>>),
    Scale(Vec<Vec3<f32>>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GltfNodeId {
    pub container_handle: AssetHandle,
    pub node_index: usize,
}

impl std::fmt::Display for GltfNodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}", self.container_handle, self.node_index)
    }
}

#[derive(Debug, Clone)]
pub struct GltfSkeletonBone {
    pub id: GltfNodeId,
    pub ibm_index: usize,
}

pub struct GltfSceneRoot;
pub struct GltfSceneParent;
pub struct GltfSceneChild;
pub struct GltfSceneAttribute;
pub struct GltfSceneBone;

pub struct GltfSceneMesh(AssetHandle);

impl GltfSceneMesh {
    pub fn handle(&self) -> AssetHandle {
        self.0
    }
}

pub struct GltfSceneSkin(AssetHandle);

impl GltfSceneSkin {
    pub fn handle(&self) -> AssetHandle {
        self.0
    }
}

#[derive(Debug, Default, Clone)]
pub struct GltfSceneTransform {
    pub transform: Transform<f32, f32, f32>,
    bind_transform: Transform<f32, f32, f32>,
    local_matrix: Mat4<f32>,
    local_inverse_matrix: Mat4<f32>,
    global_matrix: Mat4<f32>,
    global_inverse_matrix: Mat4<f32>,
}

impl GltfSceneTransform {
    pub fn bind_transform(&self) -> Transform<f32, f32, f32> {
        self.bind_transform
    }

    pub fn local_matrix(&self) -> Mat4<f32> {
        self.local_matrix
    }

    pub fn local_inverse_matrix(&self) -> Mat4<f32> {
        self.local_inverse_matrix
    }

    pub fn world_matrix(&self) -> Mat4<f32> {
        self.global_matrix
    }

    pub fn world_inverse_matrix(&self) -> Mat4<f32> {
        self.global_inverse_matrix
    }
}

#[derive(Debug, Clone)]
pub struct GltfNode {
    pub id: GltfNodeId,
    pub name: String,
    pub transform: Transform<f32, f32, f32>,
    pub mesh_handle: Option<AssetHandle>,
    pub skin_handle: Option<AssetHandle>,
    pub children: Vec<Self>,
}

#[derive(Default)]
pub struct GltfSceneInstantiateOptions {
    #[allow(clippy::type_complexity)]
    pub remap_node_names: Option<Box<dyn Fn(&str) -> String + Send + Sync>>,
}

impl GltfSceneInstantiateOptions {
    pub fn remap_node_names(mut self, f: impl Fn(&str) -> String + Send + Sync + 'static) -> Self {
        self.remap_node_names = Some(Box::new(f));
        self
    }
}

#[derive(Debug, Clone)]
pub struct GltfSceneTemplate {
    pub name: String,
    pub container_handle: AssetHandle,
    pub root_nodes: Vec<GltfNode>,
}

impl GltfSceneTemplate {
    pub fn instantiate(&self, database: &AssetDatabase) -> GltfSceneInstance {
        self.instantiate_with_options(database, &Default::default())
    }

    pub fn instantiate_with_options(
        &self,
        database: &AssetDatabase,
        options: &GltfSceneInstantiateOptions,
    ) -> GltfSceneInstance {
        let mut graph = Graph::default();
        let mut bone_nodes = HashSet::new();

        let roots = self
            .root_nodes
            .iter()
            .map(|root_node| {
                Self::instantiate_node(
                    &mut graph,
                    root_node,
                    None,
                    database,
                    &mut bone_nodes,
                    options,
                )
            })
            .collect();

        for bone_id in &bone_nodes {
            let Some(index) = graph
                .iter::<GltfNodeId>()
                .find(|(_, id)| **id == *bone_id)
                .map(|(index, _)| index)
            else {
                continue;
            };
            let bone = graph.insert(GltfSceneBone);
            graph.relate::<GltfSceneAttribute>(index, bone);
        }

        let result = GltfSceneInstance {
            blend_only_affected_animations: false,
            container_handle: self.container_handle,
            graph,
            roots,
            animations: Default::default(),
            parameters: Default::default(),
            animation_node: None,
        };
        result.recompute_matrices();
        result
    }

    fn instantiate_node(
        graph: &mut Graph,
        node: &GltfNode,
        parent_index: Option<AnyIndex>,
        database: &AssetDatabase,
        bone_nodes: &mut HashSet<GltfNodeId>,
        options: &GltfSceneInstantiateOptions,
    ) -> AnyIndex {
        let index = graph.insert(node.id);

        if let Some(parent_index) = parent_index {
            graph.relate_pair::<GltfSceneParent, GltfSceneChild>(parent_index, index);
        } else {
            let root = graph.insert(GltfSceneRoot);
            graph.relate::<GltfSceneAttribute>(index, root);
        }

        let name = if let Some(remap) = &options.remap_node_names {
            remap(&node.name)
        } else {
            node.name.clone()
        };
        let name = graph.insert(name);
        graph.relate::<GltfSceneAttribute>(index, name);

        let transform = graph.insert(GltfSceneTransform {
            transform: node.transform,
            bind_transform: node.transform,
            local_matrix: Default::default(),
            local_inverse_matrix: Default::default(),
            global_matrix: Default::default(),
            global_inverse_matrix: Default::default(),
        });
        graph.relate::<GltfSceneAttribute>(index, transform);

        if let Some(mesh_handle) = node.mesh_handle {
            let mesh = graph.insert(GltfSceneMesh(mesh_handle));
            graph.relate::<GltfSceneAttribute>(index, mesh);
        }

        if let Some(skin_handle) = node.skin_handle {
            let skin = graph.insert(GltfSceneSkin(skin_handle));
            graph.relate::<GltfSceneAttribute>(index, skin);

            if let Some(asset) = skin_handle.access_checked::<&GltfSkin>(database) {
                for bone in asset.bones.iter() {
                    bone_nodes.insert(bone.id);
                }
            }
        }

        for child in &node.children {
            Self::instantiate_node(graph, child, Some(index), database, bone_nodes, options);
        }

        index
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GltfAnimationEvent {
    pub time: f32,
    pub duration: f32,
    pub id: Cow<'static, str>,
    pub value: Value,
}

impl GltfAnimationEvent {
    pub fn new(id: impl Into<Cow<'static, str>>) -> Self {
        Self {
            id: id.into(),
            value: Value::Null,
            time: 0.0,
            duration: 0.0,
        }
    }

    pub fn value(mut self, value: Value) -> Self {
        self.value = value;
        self
    }

    pub fn time(mut self, time: f32) -> Self {
        self.time = time;
        self
    }

    pub fn duration(mut self, duration: f32) -> Self {
        self.duration = duration;
        self
    }

    pub fn time_range(mut self, start: f32, end: f32) -> Self {
        self.time = start;
        self.duration = end - start;
        self
    }
}

#[derive(Debug, Clone)]
pub struct GltfSceneAnimation {
    pub animation_handle: AssetHandle,
    pub time: f32,
    duration: f32,
    cycle_completed: bool,
    pub weight: f32,
    pub looped: bool,
    pub playing: bool,
    pub speed: f32,
    pub affected_nodes: HashSet<AnyIndex>,
    pub nodes_weight_override: HashMap<AnyIndex, f32>,
    pub events_timeline: Vec<GltfAnimationEvent>,
    events_passed: HashSet<usize>,
}

impl GltfSceneAnimation {
    pub fn new(animation_handle: AssetHandle, database: &AssetDatabase) -> Option<Self> {
        let duration = animation_handle
            .access_checked::<&GltfAnimation>(database)?
            .duration;
        Some(Self {
            animation_handle,
            time: 0.0,
            duration,
            cycle_completed: false,
            weight: 1.0,
            looped: false,
            playing: false,
            speed: 1.0,
            affected_nodes: Default::default(),
            nodes_weight_override: Default::default(),
            events_timeline: Default::default(),
            events_passed: Default::default(),
        })
    }

    pub fn setup(self, mut f: impl FnMut(Self) -> Self) -> Self {
        f(self)
    }

    pub fn time(mut self, time: f32) -> Self {
        self.time = time;
        self
    }

    pub fn duration(&self) -> f32 {
        self.duration
    }

    pub fn weight(mut self, weight: f32) -> Self {
        self.weight = weight;
        self
    }

    pub fn looped(mut self, looped: bool) -> Self {
        self.looped = looped;
        self
    }

    pub fn playing(mut self, playing: bool) -> Self {
        self.playing = playing;
        self
    }

    pub fn start(&mut self) {
        self.playing = true;
        self.time = 0.0;
        self.weight = 1.0;
    }

    pub fn stop(&mut self) {
        self.playing = false;
        self.time = 0.0;
        self.weight = 0.0;
    }

    pub fn has_completed(&self) -> bool {
        !self.looped
            && if self.speed >= 0.0 {
                self.time >= self.duration
            } else {
                self.time <= 0.0
            }
    }

    pub fn has_cycle_completed(&self) -> bool {
        self.cycle_completed
    }

    pub fn speed(mut self, speed: f32) -> Self {
        self.speed = speed;
        self
    }

    pub fn affected_nodes(mut self, affected_nodes: impl IntoIterator<Item = AnyIndex>) -> Self {
        self.affected_nodes.extend(affected_nodes);
        self
    }

    pub fn affected_node(mut self, affected_node: AnyIndex) -> Self {
        self.affected_nodes.insert(affected_node);
        self
    }

    pub fn node_weight_overrides(
        mut self,
        nodes_weight_override: impl IntoIterator<Item = (AnyIndex, f32)>,
    ) -> Self {
        self.nodes_weight_override.extend(nodes_weight_override);
        self
    }

    pub fn node_weight_override(mut self, node: AnyIndex, weight: f32) -> Self {
        self.nodes_weight_override.insert(node, weight);
        self
    }

    pub fn event(mut self, event: GltfAnimationEvent) -> Self {
        self.events_timeline.push(event);
        self
    }

    pub fn events(mut self, events: impl IntoIterator<Item = GltfAnimationEvent>) -> Self {
        self.events_timeline.extend(events);
        self
    }

    pub fn passed_events(&self) -> impl Iterator<Item = &GltfAnimationEvent> + '_ {
        self.events_passed
            .iter()
            .filter_map(move |&index| self.events_timeline.get(index))
    }
}

#[derive(Debug, Clone)]
pub struct GltfSceneAnimationHandle(Arc<RwLock<GltfSceneAnimation>>);

impl GltfSceneAnimationHandle {
    pub fn read(&self) -> Option<RwLockReadGuard<'_, GltfSceneAnimation>> {
        self.0.try_read().ok()
    }

    pub fn write(&self) -> Option<RwLockWriteGuard<'_, GltfSceneAnimation>> {
        self.0.try_write().ok()
    }

    pub fn get(&self) -> Option<GltfSceneAnimation> {
        self.read().map(|guard| guard.clone())
    }

    pub fn set(&self, animation: GltfSceneAnimation) {
        if let Ok(mut guard) = self.0.write() {
            *guard = animation;
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct GltfAnimationParameter(Arc<AtomicU32>);

impl GltfAnimationParameter {
    pub fn new(value: f32) -> Self {
        Self(Arc::new(AtomicU32::new(value.to_bits())))
    }

    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(std::sync::atomic::Ordering::SeqCst))
    }

    pub fn set(&self, value: f32) -> f32 {
        f32::from_bits(
            self.0
                .swap(value.to_bits(), std::sync::atomic::Ordering::SeqCst),
        )
    }
}

#[derive(Debug, Default)]
pub struct GltfAnimationBlender {
    animation_weights: HashMap<String, f32>,
}

impl GltfAnimationBlender {
    pub fn animation(&mut self, name: impl ToString, weight: f32) {
        *self.animation_weights.entry(name.to_string()).or_default() += weight;
    }
}

pub trait GltfAnimationNode: Send + Sync {
    fn produce_weights(
        &self,
        instance: &GltfSceneInstance,
        master_weight: f32,
        output: &mut GltfAnimationBlender,
    );

    #[allow(unused_variables)]
    fn update(&mut self, delta_time: f32);
}

impl GltfAnimationNode for () {
    fn produce_weights(&self, _: &GltfSceneInstance, _: f32, _: &mut GltfAnimationBlender) {}
    fn update(&mut self, _: f32) {}
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GltfAnimationTarget(pub String);

impl GltfAnimationTarget {
    pub fn new(name: impl ToString) -> Self {
        Self(name.to_string())
    }
}

impl GltfAnimationNode for GltfAnimationTarget {
    fn produce_weights(
        &self,
        _: &GltfSceneInstance,
        master_weight: f32,
        output: &mut GltfAnimationBlender,
    ) {
        output.animation(self.0.clone(), master_weight);
    }

    fn update(&mut self, _delta_time: f32) {}
}

#[derive(Debug, Clone, PartialEq)]
pub enum GltfAnimationWeight {
    Fixed(f32),
    Parameter(String),
}

impl Default for GltfAnimationWeight {
    fn default() -> Self {
        Self::Fixed(1.0)
    }
}

impl GltfAnimationWeight {
    pub fn fixed(weight: f32) -> Self {
        Self::Fixed(weight)
    }

    pub fn parameter(name: impl ToString) -> Self {
        Self::Parameter(name.to_string())
    }

    fn get(&self, instance: &GltfSceneInstance) -> f32 {
        match self {
            GltfAnimationWeight::Fixed(weight) => *weight,
            GltfAnimationWeight::Parameter(name) => instance
                .parameter(name)
                .map(|p| p.get())
                .unwrap_or_default(),
        }
    }
}

pub struct GltfAnimationTransition {
    pub controller: GltfAnimationTransitionController,
    pub layers: Vec<GltfAnimationTransitionLayer>,
    pub default_layer: Option<String>,
    pub change_speed: Option<f32>,
}

impl GltfAnimationTransition {
    pub fn new(controller: GltfAnimationTransitionController) -> Self {
        Self {
            controller,
            layers: Default::default(),
            default_layer: None,
            change_speed: None,
        }
    }

    pub fn layer(mut self, layer: GltfAnimationTransitionLayer) -> Self {
        self.layers.push(layer);
        self
    }

    pub fn layers(
        mut self,
        layers: impl IntoIterator<Item = GltfAnimationTransitionLayer>,
    ) -> Self {
        self.layers.extend(layers);
        self
    }

    pub fn default_layer(mut self, name: impl ToString) -> Self {
        self.default_layer = Some(name.to_string());
        self
    }

    pub fn change_speed(mut self, speed: f32) -> Self {
        self.change_speed = Some(speed);
        self
    }
}

impl GltfAnimationNode for GltfAnimationTransition {
    fn produce_weights(
        &self,
        instance: &GltfSceneInstance,
        master_weight: f32,
        output: &mut GltfAnimationBlender,
    ) {
        for layer in &self.layers {
            layer
                .node
                .produce_weights(instance, master_weight * layer.current_weight, output);
        }
    }

    fn update(&mut self, delta_time: f32) {
        if let Ok(controller) = self.controller.active.read() {
            for layer in &mut self.layers {
                let target_weight = if (controller.is_empty()
                    && self
                        .default_layer
                        .as_ref()
                        .map(|name| name == &layer.name)
                        .unwrap_or_default())
                    || controller.contains(&layer.name)
                {
                    1.0
                } else {
                    0.0
                };
                let change_speed = layer
                    .change_speed
                    .unwrap_or(self.change_speed.unwrap_or(f32::MAX));
                if (layer.current_weight - target_weight).abs() < f32::EPSILON {
                    layer.current_weight = target_weight;
                } else if layer.current_weight < target_weight {
                    layer.current_weight =
                        (layer.current_weight + change_speed * delta_time).clamp(0.0, 1.0);
                } else {
                    layer.current_weight =
                        (layer.current_weight - change_speed * delta_time).clamp(0.0, 1.0);
                }
            }
        }
        for layer in &mut self.layers {
            layer.node.update(delta_time);
        }
    }
}

pub struct GltfAnimationTransitionLayer {
    pub node: Box<dyn GltfAnimationNode>,
    pub change_speed: Option<f32>,
    name: String,
    current_weight: f32,
}

impl GltfAnimationTransitionLayer {
    pub fn new(name: impl ToString, node: impl GltfAnimationNode + 'static) -> Self {
        Self {
            node: Box::new(node),
            change_speed: None,
            name: name.to_string(),
            current_weight: 0.0,
        }
    }

    pub fn change_speed(mut self, speed: f32) -> Self {
        self.change_speed = Some(speed);
        self
    }
}

#[derive(Default, Clone)]
pub struct GltfAnimationTransitionController {
    active: Arc<RwLock<HashSet<String>>>,
}

impl GltfAnimationTransitionController {
    pub fn clear(&self) {
        if let Ok(mut guard) = self.active.write() {
            guard.clear();
        }
    }

    pub fn change_to(&self, names: impl IntoIterator<Item = impl ToString>) {
        if let Ok(mut guard) = self.active.write() {
            guard.clear();
            for name in names {
                guard.insert(name.to_string());
            }
        }
    }
}

#[derive(Default)]
pub struct GltfAnimationMixer {
    pub layers: Vec<GltfAnimationMixerLayer>,
}

impl GltfAnimationMixer {
    pub fn layer(mut self, layer: GltfAnimationMixerLayer) -> Self {
        self.layers.push(layer);
        self
    }

    pub fn layers(mut self, layers: impl IntoIterator<Item = GltfAnimationMixerLayer>) -> Self {
        self.layers.extend(layers);
        self
    }
}

impl GltfAnimationNode for GltfAnimationMixer {
    fn produce_weights(
        &self,
        instance: &GltfSceneInstance,
        master_weight: f32,
        output: &mut GltfAnimationBlender,
    ) {
        for layer in &self.layers {
            layer.node.produce_weights(
                instance,
                master_weight * layer.weight.get(instance),
                output,
            );
        }
    }

    fn update(&mut self, delta_time: f32) {
        for layer in &mut self.layers {
            layer.node.update(delta_time);
        }
    }
}

pub struct GltfAnimationMixerLayer {
    pub weight: GltfAnimationWeight,
    pub node: Box<dyn GltfAnimationNode>,
}

impl GltfAnimationMixerLayer {
    pub fn new(weight: GltfAnimationWeight, node: impl GltfAnimationNode + 'static) -> Self {
        Self {
            weight,
            node: Box::new(node),
        }
    }
}

pub struct GltfAnimationBlendSpace<const N: usize> {
    pub parameters: [Cow<'static, str>; N],
    pub points: Vec<GltfAnimationBlendSpacePoint<N>>,
}

impl<const N: usize> GltfAnimationBlendSpace<N> {
    pub fn new(parameters: [Cow<'static, str>; N]) -> Self {
        Self {
            parameters,
            points: Default::default(),
        }
    }

    pub fn points(
        mut self,
        points: impl IntoIterator<Item = GltfAnimationBlendSpacePoint<N>>,
    ) -> Self {
        self.points.extend(points);
        self
    }

    pub fn point(mut self, point: GltfAnimationBlendSpacePoint<N>) -> Self {
        self.points.push(point);
        self
    }
}

impl<const N: usize> GltfAnimationNode for GltfAnimationBlendSpace<N> {
    fn produce_weights(
        &self,
        instance: &GltfSceneInstance,
        master_weight: f32,
        output: &mut GltfAnimationBlender,
    ) {
        fn distance<const N: usize>(a: [f32; N], b: [f32; N]) -> f32 {
            let mut sum = 0.0;
            for i in 0..N {
                let diff = a[i] - b[i];
                sum += diff * diff;
            }
            sum.sqrt()
        }

        let parameters = std::array::from_fn::<_, N, _>(|i| self.parameters[i].as_ref())
            .map(|name| instance.parameter(name).map(|p| p.get()).unwrap_or(0.0));
        let distances = self
            .points
            .iter()
            .map(|point| distance(point.parameters, parameters))
            .collect::<Vec<f32>>();

        let mut weights = distances
            .iter()
            .map(|&d| 1.0 / (d * d))
            .collect::<Vec<f32>>();

        if let Some(found) = distances.iter().position(|&d| d <= 1.0e-6_f32) {
            weights.fill(0.0);
            weights[found] = 1.0;
        } else {
            let total_weight: f32 = weights.iter().sum();
            for weight in &mut weights {
                *weight = (*weight / total_weight).clamp(0.0, 1.0);
            }
        }

        for (point, &weight) in self.points.iter().zip(weights.iter()) {
            point
                .node
                .produce_weights(instance, weight * master_weight, output);
        }
    }

    fn update(&mut self, delta_time: f32) {
        for point in &mut self.points {
            point.node.update(delta_time);
        }
    }
}

pub struct GltfAnimationBlendSpacePoint<const N: usize> {
    pub parameters: [f32; N],
    pub node: Box<dyn GltfAnimationNode>,
}

impl<const N: usize> GltfAnimationBlendSpacePoint<N> {
    pub fn new(parameters: [f32; N], node: impl GltfAnimationNode + 'static) -> Self {
        Self {
            parameters,
            node: Box::new(node),
        }
    }
}

pub struct GltfSceneInstance {
    pub blend_only_affected_animations: bool,
    container_handle: AssetHandle,
    graph: Graph,
    roots: Vec<AnyIndex>,
    animations: HashMap<String, GltfSceneAnimationHandle>,
    parameters: HashMap<String, GltfAnimationParameter>,
    animation_node: Option<Box<dyn GltfAnimationNode>>,
}

impl GltfSceneInstance {
    pub fn blend_only_affected_animations(mut self, value: bool) -> Self {
        self.blend_only_affected_animations = value;
        self
    }

    pub fn with_animation(mut self, name: impl ToString, animation: GltfSceneAnimation) -> Self {
        self.add_animation(name, animation);
        self
    }

    pub fn with_parameter(
        mut self,
        name: impl ToString,
        parameter: GltfAnimationParameter,
    ) -> Self {
        self.add_parameter(name, parameter);
        self
    }

    pub fn with_animation_node(mut self, animation_node: impl GltfAnimationNode + 'static) -> Self {
        self.set_animation_node(animation_node);
        self
    }

    pub fn animation(&self, name: &str) -> Option<&GltfSceneAnimationHandle> {
        self.animations.get(name)
    }

    pub fn animations(&self) -> impl Iterator<Item = (&str, &GltfSceneAnimationHandle)> + '_ {
        self.animations
            .iter()
            .map(|(name, handle)| (name.as_str(), handle))
    }

    pub fn parameter(&self, name: &str) -> Option<&GltfAnimationParameter> {
        self.parameters.get(name)
    }

    pub fn parameters(&self) -> impl Iterator<Item = (&str, &GltfAnimationParameter)> + '_ {
        self.parameters
            .iter()
            .map(|(name, parameter)| (name.as_str(), parameter))
    }

    pub fn container_handle(&self) -> AssetHandle {
        self.container_handle
    }

    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    pub fn roots(&self) -> impl Iterator<Item = AnyIndex> + '_ {
        self.roots.iter().copied()
    }

    pub fn find_bone_by_name(&self, name: &str) -> Option<AnyIndex> {
        self.graph.iter::<GltfNodeId>().find_map(|(index, _)| {
            let (_, bone_name) = self
                .graph
                .query::<(
                    Related<GltfSceneAttribute, &GltfSceneBone>,
                    Related<GltfSceneAttribute, &String>,
                )>(index)
                .next()?;
            if bone_name.as_str() == name {
                Some(index)
            } else {
                None
            }
        })
    }

    pub fn query_bone_by_name<'a, Fetch: QueryFetch<'a>>(
        &'a self,
        name: &str,
    ) -> Option<Fetch::Value> {
        let index = self.find_bone_by_name(name)?;
        self.graph.query::<Fetch>(index).next()
    }

    pub fn add_animation(
        &mut self,
        name: impl ToString,
        animation: GltfSceneAnimation,
    ) -> GltfSceneAnimationHandle {
        let handle = GltfSceneAnimationHandle(Arc::new(RwLock::new(animation)));
        self.animations.insert(name.to_string(), handle.clone());
        handle
    }

    pub fn remove_animation(&mut self, name: &str) {
        self.animations.remove(name);
    }

    pub fn add_parameter(
        &mut self,
        name: impl ToString,
        parameter: GltfAnimationParameter,
    ) -> GltfAnimationParameter {
        self.parameters.insert(name.to_string(), parameter.clone());
        parameter
    }

    pub fn remove_parameter(&mut self, name: &str) {
        self.parameters.remove(name);
    }

    pub fn animation_node(&self) -> Option<&dyn GltfAnimationNode> {
        self.animation_node.as_deref()
    }

    pub fn animation_node_mut(&mut self) -> Option<&mut (dyn GltfAnimationNode + 'static)> {
        self.animation_node.as_deref_mut()
    }

    pub fn set_animation_node(&mut self, animation_node: impl GltfAnimationNode + 'static) {
        self.animation_node = Some(Box::new(animation_node));
    }

    pub fn unset_animation_node(&mut self) {
        self.animation_node = None;
    }

    pub fn update_animations(&mut self, delta_time: f32) {
        for handle in self.animations.values() {
            if let Some(mut animation) = handle.write() {
                animation.cycle_completed = false;
                animation.events_passed.clear();
                if !animation.playing {
                    continue;
                }
                let previous_time = animation.time;
                animation.time += delta_time * animation.speed;
                animation.events_passed = animation
                    .events_timeline
                    .iter()
                    .enumerate()
                    .filter_map(|(index, event)| {
                        let (frame_from, frame_to) = (
                            previous_time.min(animation.time),
                            previous_time.max(animation.time),
                        );
                        let event_from = event.time;
                        let event_end = event.time + event.duration;
                        let (event_from, event_end) =
                            (event_from.min(event_end), event_from.max(event_end));
                        if frame_from <= event_end && frame_to >= event_from {
                            Some(index)
                        } else {
                            None
                        }
                    })
                    .collect();
                if animation.time < 0.0 {
                    animation.cycle_completed = true;
                    if animation.looped {
                        animation.time = animation.duration + (animation.time % animation.duration);
                    } else {
                        animation.time = 0.0;
                        animation.playing = false;
                    }
                } else if animation.time > animation.duration {
                    animation.cycle_completed = true;
                    if animation.looped {
                        animation.time %= animation.duration;
                    } else {
                        animation.time = animation.duration;
                        animation.playing = false;
                    }
                }
            }
        }
        if let Some(mut animation_node) = self.animation_node.take() {
            animation_node.update(delta_time);
            let mut blender = GltfAnimationBlender::default();
            animation_node.produce_weights(self, 1.0, &mut blender);
            self.animation_node = Some(animation_node);
            if self.blend_only_affected_animations {
                for (name, weight) in blender.animation_weights {
                    if let Some(handle) = self.animations.get(&name)
                        && let Some(mut animation) = handle.write()
                    {
                        animation.weight = weight;
                    }
                }
            } else {
                for (name, handle) in self.animations.iter() {
                    let weight = blender
                        .animation_weights
                        .iter()
                        .find(|(n, _)| n.as_str() == name.as_str())
                        .map(|(_, w)| *w)
                        .unwrap_or_default();
                    if let Some(mut animation) = handle.write() {
                        animation.weight = weight;
                    }
                }
            };
        }
    }

    pub fn apply_animations(&self, database: &AssetDatabase) {
        let mut delta_changes = HashMap::<
            AnyIndex,
            (
                Vec<(Vec3<f32>, f32)>,
                Vec<(Quaternion<f32>, f32)>,
                Vec<(Vec3<f32>, f32)>,
            ),
        >::default();
        for handle in self.animations.values() {
            let Some(animation) = handle.read() else {
                continue;
            };
            let Some(asset) = animation
                .animation_handle
                .access_checked::<&GltfAnimation>(database)
            else {
                continue;
            };
            for channel in &asset.channels {
                if channel.times.len() < 2 {
                    continue;
                }
                let Some(node_index) = self
                    .graph
                    .iter::<GltfNodeId>()
                    .find(|(_, id)| **id == channel.target_node)
                    .map(|(index, _)| index)
                else {
                    continue;
                };
                if !animation.affected_nodes.is_empty()
                    && !animation.affected_nodes.contains(&node_index)
                {
                    continue;
                }
                let max_time = asset.duration;
                let time_to_sample = if animation.looped {
                    animation.time % max_time
                } else {
                    animation.time.min(max_time)
                };
                let result_index = channel
                    .times
                    .iter()
                    .position(|&t| t > time_to_sample)
                    .unwrap_or(channel.times.len());
                let (i0, i1) = if animation.looped {
                    let i1 = result_index % channel.times.len();
                    let i0 = if i1 == 0 {
                        channel.times.len() - 1
                    } else {
                        i1 - 1
                    };
                    (i0, i1)
                } else {
                    let i1 = result_index.min(channel.times.len() - 1);
                    let i0 = i1.saturating_sub(1);
                    (i0, i1)
                };
                let Some(transform) = self
                    .graph
                    .query::<Related<GltfSceneAttribute, &GltfSceneTransform>>(node_index)
                    .next()
                else {
                    continue;
                };
                match &channel.values {
                    GltfAnimationValues::Translation(values) => match channel.interpolation {
                        Interpolation::Linear => {
                            let t0 = channel.times[i0];
                            let t1 = channel.times[i1];
                            let v0 = values[i0] - transform.bind_transform.position;
                            let v1 = values[i1] - transform.bind_transform.position;
                            let factor = if (t0 - t1).abs() < f32::EPSILON {
                                0.0
                            } else {
                                (time_to_sample - t0) / (t1 - t0)
                            };
                            let value = v0 + (v1 - v0) * factor;
                            delta_changes.entry(node_index).or_default().0.push((
                                value,
                                animation
                                    .nodes_weight_override
                                    .get(&node_index)
                                    .copied()
                                    .unwrap_or(animation.weight),
                            ));
                        }
                        Interpolation::Step => {
                            let value = if i1 == 0 { values[0] } else { values[i0] }
                                - transform.bind_transform.position;
                            delta_changes.entry(node_index).or_default().0.push((
                                value,
                                animation
                                    .nodes_weight_override
                                    .get(&node_index)
                                    .copied()
                                    .unwrap_or(animation.weight),
                            ));
                        }
                        Interpolation::CubicSpline => {
                            println!("CubicSpline interpolation not implemented yet.");
                        }
                    },
                    GltfAnimationValues::Rotation(values) => match channel.interpolation {
                        Interpolation::Linear => {
                            let t0 = channel.times[i0];
                            let t1 = channel.times[i1];
                            let v0 = (values[i0] * transform.bind_transform.orientation.inverse())
                                .normalized();
                            let v1 = (values[i1] * transform.bind_transform.orientation.inverse())
                                .normalized();
                            let factor = if (t0 - t1).abs() < f32::EPSILON {
                                0.0
                            } else {
                                (time_to_sample - t0) / (t1 - t0)
                            };
                            let value = shortest_slerp(v0, v1, factor);
                            delta_changes.entry(node_index).or_default().1.push((
                                value,
                                animation
                                    .nodes_weight_override
                                    .get(&node_index)
                                    .copied()
                                    .unwrap_or(animation.weight),
                            ));
                        }
                        Interpolation::Step => {
                            let value = if i1 == 0 { values[0] } else { values[i0] };
                            let value = (value * transform.bind_transform.orientation.inverse())
                                .normalized();
                            delta_changes.entry(node_index).or_default().1.push((
                                value,
                                animation
                                    .nodes_weight_override
                                    .get(&node_index)
                                    .copied()
                                    .unwrap_or(animation.weight),
                            ));
                        }
                        Interpolation::CubicSpline => {
                            println!("CubicSpline interpolation not implemented yet.");
                        }
                    },
                    GltfAnimationValues::Scale(values) => match channel.interpolation {
                        Interpolation::Linear => {
                            let t0 = channel.times[i0];
                            let t1 = channel.times[i1];
                            let v0 = values[i0] / transform.bind_transform.scale;
                            let v1 = values[i1] / transform.bind_transform.scale;
                            let factor = if (t0 - t1).abs() < f32::EPSILON {
                                0.0
                            } else {
                                (time_to_sample - t0) / (t1 - t0)
                            };
                            let value = v0 + (v1 - v0) * factor;
                            delta_changes.entry(node_index).or_default().2.push((
                                value,
                                animation
                                    .nodes_weight_override
                                    .get(&node_index)
                                    .copied()
                                    .unwrap_or(animation.weight),
                            ));
                        }
                        Interpolation::Step => {
                            let value = if i1 == 0 { values[0] } else { values[i0] }
                                / transform.bind_transform.scale;
                            delta_changes.entry(node_index).or_default().2.push((
                                value,
                                animation
                                    .nodes_weight_override
                                    .get(&node_index)
                                    .copied()
                                    .unwrap_or(animation.weight),
                            ));
                        }
                        Interpolation::CubicSpline => {
                            println!("CubicSpline interpolation not implemented yet.");
                        }
                    },
                }
            }
        }
        for (node_index, (translations, rotations, scales)) in delta_changes {
            let Some(mut transform) = self
                .graph
                .query::<Related<GltfSceneAttribute, &mut GltfSceneTransform>>(node_index)
                .next()
            else {
                continue;
            };
            if !translations.is_empty() {
                if translations.len() == 1 {
                    transform.transform.position =
                        translations[0].0 + transform.bind_transform.position;
                } else {
                    let total_weight = translations.iter().map(|(_, w)| *w).sum::<f32>();
                    if total_weight > f32::EPSILON {
                        let mut accumulated = Vec3::zero();
                        for (value, weight) in translations {
                            accumulated += value * weight;
                        }
                        transform.transform.position =
                            (accumulated / total_weight) + transform.bind_transform.position;
                    }
                }
            }
            if !rotations.is_empty() {
                if rotations.len() == 1 {
                    transform.transform.orientation =
                        (rotations[0].0 * transform.bind_transform.orientation).normalized();
                } else {
                    let total_weight = rotations.iter().map(|(_, w)| *w).sum::<f32>();
                    if total_weight > f32::EPSILON {
                        let mut accumulated = Quaternion::identity();
                        for (value, weight) in rotations {
                            let factor = weight / total_weight;
                            accumulated = shortest_slerp(accumulated, value, factor);
                        }
                        transform.transform.orientation =
                            (accumulated * transform.bind_transform.orientation).normalized();
                    }
                }
            }
            if !scales.is_empty() {
                if scales.len() == 1 {
                    transform.transform.scale = scales[0].0 * transform.bind_transform.scale;
                } else {
                    let total_weight = scales.iter().map(|(_, w)| *w).sum::<f32>();
                    if total_weight > f32::EPSILON {
                        let mut accumulated = Vec3::zero();
                        for (value, weight) in scales {
                            accumulated += value * weight;
                        }
                        transform.transform.scale =
                            (accumulated / total_weight) * transform.bind_transform.scale;
                    }
                }
            }
        }
        self.recompute_matrices();
    }

    pub fn update_and_apply_animations(&mut self, delta_time: f32, database: &AssetDatabase) {
        self.update_animations(delta_time);
        self.apply_animations(database);
    }

    pub fn visit_tree(
        &self,
        f: &mut impl FnMut(
            usize,
            AnyIndex,
            GltfNodeId,
            Option<&String>,
            Option<&GltfSceneTransform>,
            Option<&GltfSceneMesh>,
            Option<&GltfSceneSkin>,
            Option<&GltfSceneBone>,
        ) -> bool,
    ) {
        for index in self.roots() {
            self.visit_tree_inner(0, index, f);
        }
    }

    fn visit_tree_inner(
        &self,
        level: usize,
        index: AnyIndex,
        f: &mut impl FnMut(
            usize,
            AnyIndex,
            GltfNodeId,
            Option<&String>,
            Option<&GltfSceneTransform>,
            Option<&GltfSceneMesh>,
            Option<&GltfSceneSkin>,
            Option<&GltfSceneBone>,
        ) -> bool,
    ) {
        let Some(id) = self.graph.read::<GltfNodeId>(index).ok() else {
            return;
        };
        let name = self
            .graph
            .query::<Related<GltfSceneAttribute, &String>>(index)
            .next();
        let transform = self
            .graph
            .query::<Related<GltfSceneAttribute, &GltfSceneTransform>>(index)
            .next();
        let mesh = self
            .graph
            .query::<Related<GltfSceneAttribute, &GltfSceneMesh>>(index)
            .next();
        let skin = self
            .graph
            .query::<Related<GltfSceneAttribute, &GltfSceneSkin>>(index)
            .next();
        let bone = self
            .graph
            .query::<Related<GltfSceneAttribute, &GltfSceneBone>>(index)
            .next();
        if !f(
            level,
            index,
            *id,
            name.as_deref(),
            transform.as_deref(),
            mesh.as_deref(),
            skin.as_deref(),
            bone.as_deref(),
        ) {
            return;
        }
        for child_index in self
            .graph
            .query::<Related<GltfSceneChild, Node<GltfNodeId>>>(index)
        {
            self.visit_tree_inner(level + 1, child_index, f);
        }
    }

    pub fn recompute_matrices(&self) {
        for root_index in self.roots() {
            self.recompute_matrix(Default::default(), root_index);
        }
    }

    fn recompute_matrix(&self, parent_matrix: Mat4<f32>, index: AnyIndex) {
        let matrix = if let Some(mut transform) = self
            .graph
            .query::<Related<GltfSceneAttribute, &mut GltfSceneTransform>>(index)
            .next()
        {
            transform.local_matrix = transform_to_matrix(transform.transform);
            transform.local_inverse_matrix = transform.local_matrix.inverted();
            transform.global_matrix = parent_matrix * transform.local_matrix;
            transform.global_inverse_matrix = transform.global_matrix.inverted();
            transform.global_matrix
        } else {
            parent_matrix
        };
        for child_index in self
            .graph
            .query::<Related<GltfSceneChild, Node<GltfNodeId>>>(index)
        {
            self.recompute_matrix(matrix, child_index);
        }
    }

    pub fn build_renderables(
        &self,
        database: &AssetDatabase,
        options: &GltfRenderablesOptions,
    ) -> Result<GltfSceneRenderables, Box<dyn Error>> {
        let mut result = Default::default();
        if options.meshes {
            self.recompute_matrices();
            let mut final_bone_matrices: HashMap<GltfNodeId, Mat4<f32>> = Default::default();
            for root_index in self.roots() {
                self.compute_final_bone_matrix(root_index, database, &mut final_bone_matrices)?;
            }
            for root_index in self.roots() {
                self.collect_renderables(
                    root_index,
                    database,
                    options,
                    &final_bone_matrices,
                    &mut result,
                )?;
            }
        }
        if options.bones {
            for root_index in self.roots() {
                self.collect_bones(root_index, options, None, &mut result);
            }
        }
        Ok(result)
    }

    fn compute_final_bone_matrix(
        &self,
        index: AnyIndex,
        database: &AssetDatabase,
        final_bone_matrices: &mut HashMap<GltfNodeId, Mat4<f32>>,
    ) -> Result<(), Box<dyn Error>> {
        if let Some(skin) = self
            .graph
            .query::<Related<GltfSceneAttribute, &GltfSceneSkin>>(index)
            .next()
        {
            let skin_asset = skin
                .handle()
                .access_checked::<&GltfSkin>(database)
                .ok_or("Skin asset not found")?;

            for index in self.roots() {
                Self::traverse_hierarchy(&self.graph, index, &mut |graph, index| {
                    let id = *self.graph.read::<GltfNodeId>(index).unwrap();

                    if let Some(bone) = skin_asset.bones.iter().find(|bone| bone.id == id)
                        && let Some(transform) = graph
                            .query::<Related<GltfSceneAttribute, &GltfSceneTransform>>(index)
                            .next()
                    {
                        final_bone_matrices.insert(
                            bone.id,
                            transform.global_matrix
                                * skin_asset.inverse_bind_matrices[bone.ibm_index],
                        );
                    }
                    Ok(true)
                })?;
            }
        }

        for child_index in self
            .graph
            .query::<Related<GltfSceneChild, Node<GltfNodeId>>>(index)
        {
            self.compute_final_bone_matrix(child_index, database, final_bone_matrices)?;
        }

        Ok(())
    }

    fn collect_renderables(
        &self,
        index: AnyIndex,
        database: &AssetDatabase,
        options: &GltfRenderablesOptions,
        final_bone_matrices: &HashMap<GltfNodeId, Mat4<f32>>,
        renderables: &mut GltfSceneRenderables,
    ) -> Result<(), Box<dyn Error>> {
        if let Some((transform, mesh)) = self
            .graph
            .query::<(
                Related<GltfSceneAttribute, &GltfSceneTransform>,
                Related<GltfSceneAttribute, &GltfSceneMesh>,
            )>(index)
            .next()
        {
            let mesh_asset = mesh
                .handle()
                .access_checked::<&GltfMesh>(database)
                .ok_or("Mesh asset not found")?;
            let skin = self
                .graph
                .query::<Related<GltfSceneAttribute, &GltfSceneSkin>>(index)
                .next();
            let skin_asset =
                skin.and_then(|skin| skin.handle().access_checked::<&GltfSkin>(database));

            for primitive in &mesh_asset.primitives {
                let mut triangles = primitive.triangles.clone();
                if let Some(sorting_fn) = options.triangle_sorting {
                    triangles.sort_by(|a, b| {
                        let a_vertices = [
                            &primitive.vertices[a.a as usize].position,
                            &primitive.vertices[a.b as usize].position,
                            &primitive.vertices[a.c as usize].position,
                        ];
                        let b_vertices = [
                            &primitive.vertices[b.a as usize].position,
                            &primitive.vertices[b.b as usize].position,
                            &primitive.vertices[b.c as usize].position,
                        ];
                        sorting_fn(a_vertices, b_vertices)
                    });
                }
                let vertices = primitive
                    .vertices
                    .iter()
                    .map(|v| {
                        let position = if let Some(skin_asset) = skin_asset {
                            if let (Some(joints), Some(weights)) = (v.joints, v.weights) {
                                let mut skinned_position = Vec3::zero();
                                for index in 0..4 {
                                    let joint_index = joints[index] as usize;
                                    if joint_index < skin_asset.bones.len() {
                                        let bone = &skin_asset.bones[joint_index];
                                        if let Some(bone_matrix) = final_bone_matrices.get(&bone.id)
                                        {
                                            skinned_position +=
                                                bone_matrix.mul_point(v.position) * weights[index];
                                        }
                                    }
                                }
                                skinned_position
                            } else {
                                v.position
                            }
                        } else {
                            v.position
                        };
                        let mut position = [position[options.axes[0]], position[options.axes[1]]];
                        if options.flip_axes[0] {
                            position[0] = -position[0];
                        }
                        if options.flip_axes[1] {
                            position[1] = -position[1];
                        }
                        let position = transform
                            .global_matrix
                            .mul_point(Vec2::from(position))
                            .into_array();
                        Vertex {
                            position,
                            uv: [v.uv.x, v.uv.y, 0.0],
                            color: v.color.into_array(),
                        }
                    })
                    .collect();

                renderables.renderables.push(GltfSceneRenderable {
                    shader: options.shader.clone(),
                    main_texture: primitive.main_texture.as_deref().cloned(),
                    blending: primitive.blending,
                    triangles,
                    vertices,
                });
            }
        }

        for child_index in self
            .graph
            .query::<Related<GltfSceneChild, Node<GltfNodeId>>>(index)
        {
            self.collect_renderables(
                child_index,
                database,
                options,
                final_bone_matrices,
                renderables,
            )?;
        }

        Ok(())
    }

    fn collect_bones(
        &self,
        index: AnyIndex,
        options: &GltfRenderablesOptions,
        mut parent_position: Option<[f32; 2]>,
        renderables: &mut GltfSceneRenderables,
    ) {
        let color = options.bones_color.into_array();
        if let Some((transform, _)) = self
            .graph
            .query::<(
                Related<GltfSceneAttribute, &GltfSceneTransform>,
                Related<GltfSceneAttribute, &GltfSceneBone>,
            )>(index)
            .next()
        {
            let position = transform.global_matrix.mul_point(Vec2::zero()).into_array();
            let mut position = [position[options.axes[0]], position[options.axes[1]]];
            if options.flip_axes[0] {
                position[0] = -position[0];
            }
            if options.flip_axes[1] {
                position[1] = -position[1];
            }
            if let Some(parent_position) = parent_position {
                let from = Vec2::<f32>::from(parent_position);
                let to = Vec2::<f32>::from(position);
                let tangent = (to - from).normalized();
                let normal = Vec2::new(-tangent.y, tangent.x) * options.bones_thickness * 0.5;
                let from_left = from - normal;
                let from_right = from + normal;
                let offset = renderables.bones_vertices.len();
                renderables.bones_vertices.push(Vertex {
                    position: [from_left.x, from_left.y],
                    uv: [0.0, 0.0, 0.0],
                    color,
                });
                renderables.bones_vertices.push(Vertex {
                    position: [from_right.x, from_right.y],
                    uv: [0.0, 0.0, 0.0],
                    color,
                });
                renderables.bones_vertices.push(Vertex {
                    position: [to.x, to.y],
                    uv: [0.0, 0.0, 0.0],
                    color,
                });
                renderables
                    .bones_triangles
                    .push(Triangle { a: 0, b: 1, c: 2 }.offset(offset));
            }
            parent_position = Some(position);
        }

        for child_index in self
            .graph
            .query::<Related<GltfSceneChild, Node<GltfNodeId>>>(index)
        {
            self.collect_bones(child_index, options, parent_position, renderables);
        }
    }

    fn traverse_hierarchy(
        graph: &Graph,
        index: AnyIndex,
        f: &mut impl FnMut(&Graph, AnyIndex) -> Result<bool, Box<dyn Error>>,
    ) -> Result<(), Box<dyn Error>> {
        if !f(graph, index)? {
            return Ok(());
        }

        for child in graph.query::<Related<GltfSceneChild, Node<GltfNodeId>>>(index) {
            Self::traverse_hierarchy(graph, child, f)?;
        }

        Ok(())
    }
}

impl GameObject for GltfSceneInstance {
    fn draw(&mut self, context: &mut GameContext) {
        if let Ok(renderables) = self.build_renderables(
            context.assets,
            &GltfRenderablesOptions::default().sort_triangles_by_max_positive_z(),
        ) {
            renderables.draw(context.draw, context.graphics);
        }
    }
}

pub type GltfRenderablesSorting = fn([&Vec3<f32>; 3], [&Vec3<f32>; 3]) -> Ordering;

#[derive(Debug, Clone)]
pub struct GltfRenderablesOptions {
    pub meshes: bool,
    pub bones: bool,
    pub shader: Option<ShaderRef>,
    pub axes: [usize; 2],
    pub flip_axes: [bool; 2],
    pub triangle_sorting: Option<GltfRenderablesSorting>,
    pub bones_shader: Option<ShaderRef>,
    pub bones_color: Rgba<f32>,
    pub bones_thickness: f32,
}

impl Default for GltfRenderablesOptions {
    fn default() -> Self {
        Self {
            meshes: true,
            bones: false,
            shader: None,
            axes: [0, 1],
            flip_axes: [false, false],
            triangle_sorting: None,
            bones_shader: None,
            bones_color: Rgba::magenta(),
            bones_thickness: 0.0,
        }
    }
}

impl GltfRenderablesOptions {
    pub fn meshes(mut self, meshes: bool) -> Self {
        self.meshes = meshes;
        self
    }

    pub fn bones(mut self, bones: bool) -> Self {
        self.bones = bones;
        self
    }

    pub fn shader(mut self, shader: ShaderRef) -> Self {
        self.shader = Some(shader);
        self
    }

    pub fn axes(mut self, axes: [usize; 2]) -> Self {
        self.axes = axes;
        self
    }

    pub fn flip_axes(mut self, flip_axes: [bool; 2]) -> Self {
        self.flip_axes = flip_axes;
        self
    }

    pub fn triangle_sorting(mut self, sorting: GltfRenderablesSorting) -> Self {
        self.triangle_sorting = Some(sorting);
        self
    }

    pub fn sort_triangles_by_max_positive_z(mut self) -> Self {
        self.triangle_sorting = Some(|a, b| {
            let a = a[0].z.max(a[1].z).max(a[2].z);
            let b = b[0].z.max(b[1].z).max(b[2].z);
            a.partial_cmp(&b).unwrap_or(Ordering::Equal)
        });
        self
    }

    pub fn bones_shader(mut self, shader: ShaderRef) -> Self {
        self.bones_shader = Some(shader);
        self
    }

    pub fn bones_color(mut self, color: Rgba<f32>) -> Self {
        self.bones_color = color;
        self
    }

    pub fn bones_thickness(mut self, thickness: f32) -> Self {
        self.bones_thickness = thickness;
        self
    }
}

#[derive(Debug)]
pub struct GltfSceneRenderable {
    pub shader: Option<ShaderRef>,
    pub main_texture: Option<SpriteTexture>,
    pub blending: GlowBlending,
    pub triangles: Vec<Triangle>,
    pub vertices: Vec<Vertex>,
}

#[derive(Debug, Default)]
pub struct GltfSceneRenderables {
    pub renderables: Vec<GltfSceneRenderable>,
    pub bones_shader: Option<ShaderRef>,
    pub bones_vertices: Vec<Vertex>,
    pub bones_triangles: Vec<Triangle>,
}

impl Drawable for GltfSceneRenderables {
    fn draw(&self, context: &mut DrawContext, graphics: &mut dyn GraphicsTarget<Vertex>) {
        let matrix = context.top_transform();
        for renderable in &self.renderables {
            let batch = GraphicsBatch {
                shader: context.shader(renderable.shader.as_ref()),
                uniforms: std::iter::once((
                    "u_projection_view".into(),
                    GlowUniformValue::M4(
                        graphics.state().main_camera.world_matrix().into_col_array(),
                    ),
                ))
                .chain(
                    renderable
                        .main_texture
                        .iter()
                        .map(|texture| (texture.sampler.clone(), GlowUniformValue::I1(0))),
                )
                .collect(),
                textures: renderable
                    .main_texture
                    .iter()
                    .filter_map(|texture| {
                        Some((context.texture(Some(&texture.texture))?, texture.filtering))
                    })
                    .collect(),
                blending: renderable.blending,
                scissor: None,
                wireframe: false,
            };
            graphics.state_mut().stream.batch_optimized(batch);
            graphics.state_mut().stream.transformed(
                |stream| {
                    stream.extend(renderable.vertices.clone(), renderable.triangles.clone());
                },
                |vertex| {
                    let point = matrix.mul_point(Vec2::from(vertex.position));
                    vertex.position[0] = point.x;
                    vertex.position[1] = point.y;
                },
            );
        }
        if !self.bones_triangles.is_empty() {
            let batch = GraphicsBatch {
                shader: context.shader(self.bones_shader.as_ref()),
                uniforms: std::iter::once((
                    "u_projection_view".into(),
                    GlowUniformValue::M4(
                        graphics.state().main_camera.world_matrix().into_col_array(),
                    ),
                ))
                .collect(),
                textures: vec![],
                blending: GlowBlending::Alpha,
                scissor: None,
                wireframe: false,
            };
            graphics.state_mut().stream.batch_optimized(batch);
            graphics.state_mut().stream.transformed(
                |stream| {
                    stream.extend(self.bones_vertices.clone(), self.bones_triangles.clone());
                },
                |vertex| {
                    let point = matrix.mul_point(Vec2::from(vertex.position));
                    vertex.position[0] = point.x;
                    vertex.position[1] = point.y;
                },
            );
        }
    }
}

fn shortest_slerp(a: Quaternion<f32>, mut b: Quaternion<f32>, t: f32) -> Quaternion<f32> {
    if a.dot(b) < 0.0 {
        b = -b;
    }
    Quaternion::slerp(a, b, t).normalized()
}
