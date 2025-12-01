use gltf::animation::Interpolation;
use keket::database::{AssetDatabase, handle::AssetHandle};
use nodio::{
    AnyIndex,
    graph::Graph,
    query::{Node, Related},
};
use send_wrapper::SendWrapper;
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

    pub fn global_matrix(&self) -> Mat4<f32> {
        self.global_matrix
    }

    pub fn global_inverse_matrix(&self) -> Mat4<f32> {
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

#[derive(Debug, Clone)]
pub struct GltfSceneTemplate {
    pub name: String,
    pub container_handle: AssetHandle,
    pub root_nodes: Vec<GltfNode>,
}

impl GltfSceneTemplate {
    pub fn instantiate(&self, transform: Transform<f32, f32, f32>) -> GltfSceneInstance {
        let mut graph = Graph::default();

        let roots = self
            .root_nodes
            .iter()
            .map(|root_node| Self::instantiate_node(&mut graph, root_node, None))
            .collect();

        let result = GltfSceneInstance {
            transform,
            blend_only_affected_animations: false,
            container_handle: self.container_handle,
            graph,
            roots,
            animations: Default::default(),
            parameters: Default::default(),
            animation_node: Box::new(()),
        };
        result.recompute_matrices();
        result
    }

    fn instantiate_node(
        graph: &mut Graph,
        node: &GltfNode,
        parent_index: Option<AnyIndex>,
    ) -> AnyIndex {
        let index = graph.insert(node.id);

        if let Some(parent_index) = parent_index {
            graph.relate_pair::<GltfSceneParent, GltfSceneChild>(parent_index, index);
        } else {
            let root = graph.insert(GltfSceneRoot);
            graph.relate::<GltfSceneAttribute>(index, root);
        }

        let name = graph.insert(node.name.clone());
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
        }

        for child in &node.children {
            Self::instantiate_node(graph, child, Some(index));
        }

        index
    }
}

#[derive(Debug, Clone)]
pub struct GltfSceneAnimation {
    pub animation_handle: AssetHandle,
    pub time: f32,
    pub weight: f32,
    pub looped: bool,
    pub playing: bool,
    pub speed: f32,
    pub affected_nodes: HashSet<AnyIndex>,
    pub nodes_weight_override: HashMap<AnyIndex, f32>,
}

impl GltfSceneAnimation {
    pub fn new(animation_handle: AssetHandle) -> Self {
        Self {
            animation_handle,
            time: 0.0,
            weight: 1.0,
            looped: false,
            playing: false,
            speed: 1.0,
            affected_nodes: Default::default(),
            nodes_weight_override: Default::default(),
        }
    }

    pub fn time(mut self, time: f32) -> Self {
        self.time = time;
        self
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

#[derive(Debug, Clone, PartialEq)]
pub enum GltfAnimationMixerLayerWeight {
    Fixed(f32),
    Parameter(String),
}

impl Default for GltfAnimationMixerLayerWeight {
    fn default() -> Self {
        Self::Fixed(1.0)
    }
}

impl GltfAnimationMixerLayerWeight {
    pub fn fixed(weight: f32) -> Self {
        Self::Fixed(weight)
    }

    pub fn parameter(name: impl ToString) -> Self {
        Self::Parameter(name.to_string())
    }

    fn get(&self, instance: &GltfSceneInstance) -> f32 {
        match self {
            GltfAnimationMixerLayerWeight::Fixed(weight) => *weight,
            GltfAnimationMixerLayerWeight::Parameter(name) => instance
                .parameter(name)
                .map(|p| p.get())
                .unwrap_or_default(),
        }
    }
}

pub struct GltfAnimationMixerLayer {
    pub weight: GltfAnimationMixerLayerWeight,
    pub node: Box<dyn GltfAnimationNode>,
}

impl GltfAnimationMixerLayer {
    pub fn new(
        weight: GltfAnimationMixerLayerWeight,
        node: impl GltfAnimationNode + 'static,
    ) -> Self {
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
    pub transform: Transform<f32, f32, f32>,
    pub blend_only_affected_animations: bool,
    container_handle: AssetHandle,
    graph: Graph,
    roots: Vec<AnyIndex>,
    animations: HashMap<String, GltfSceneAnimationHandle>,
    parameters: HashMap<String, GltfAnimationParameter>,
    animation_node: Box<dyn GltfAnimationNode>,
}

impl GltfSceneInstance {
    pub fn transform(mut self, transform: Transform<f32, f32, f32>) -> Self {
        self.transform = transform;
        self
    }

    pub fn position(mut self, position: Vec3<f32>) -> Self {
        self.transform.position = position;
        self
    }

    pub fn rotation(mut self, rotation: Quaternion<f32>) -> Self {
        self.transform.orientation = rotation;
        self
    }

    pub fn scale(mut self, scale: Vec3<f32>) -> Self {
        self.transform.scale = scale;
        self
    }

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

    pub fn animation_node(&self) -> &dyn GltfAnimationNode {
        &*self.animation_node
    }

    pub fn animation_node_mut(&mut self) -> &mut dyn GltfAnimationNode {
        &mut *self.animation_node
    }

    pub fn set_animation_node(&mut self, animation_node: impl GltfAnimationNode + 'static) {
        self.animation_node = Box::new(animation_node);
    }

    pub fn update_animations(&mut self, delta_time: f32, database: &AssetDatabase) {
        let delta_time = delta_time.max(0.0);
        for handle in self.animations.values() {
            if let Some(mut animation) = handle.write() {
                let Some(asset) = animation
                    .animation_handle
                    .access_checked::<&GltfAnimation>(database)
                else {
                    continue;
                };
                if animation.playing {
                    animation.time = animation.time.max(0.0);
                    animation.time += delta_time * animation.speed;
                    if animation.time > asset.duration {
                        if animation.looped {
                            animation.time %= asset.duration;
                        } else {
                            animation.time = asset.duration;
                            animation.playing = false;
                        }
                    }
                }
            }
        }
        self.animation_node.update(delta_time);
        let mut blender = GltfAnimationBlender::default();
        self.animation_node.produce_weights(self, 1.0, &mut blender);
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
                            let v0 = values[i0] * transform.bind_transform.orientation.inverse();
                            let v1 = values[i1] * transform.bind_transform.orientation.inverse();
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
                            let value = if i1 == 0 { values[0] } else { values[i0] }
                                * transform.bind_transform.orientation.inverse();
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
                    transform.transform.position = translations[0].0;
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
                    transform.transform.orientation = rotations[0].0;
                } else {
                    let total_weight = rotations.iter().map(|(_, w)| *w).sum::<f32>();
                    if total_weight > f32::EPSILON {
                        let mut accumulated = Quaternion::identity();
                        for (value, weight) in rotations {
                            let factor = weight / total_weight;
                            accumulated = shortest_slerp(accumulated, value, factor);
                        }
                        transform.transform.orientation =
                            accumulated * transform.bind_transform.orientation;
                    }
                }
            }
            if !scales.is_empty() {
                if scales.len() == 1 {
                    transform.transform.scale = scales[0].0;
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
        self.update_animations(delta_time, database);
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
        if !f(
            level,
            index,
            *id,
            name.as_deref(),
            transform.as_deref(),
            mesh.as_deref(),
            skin.as_deref(),
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
        let matrix = transform_to_matrix(self.transform);
        for root_index in self.roots() {
            self.recompute_matrix(matrix, root_index);
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
        self.recompute_matrices();
        let mut final_bone_matrices: HashMap<GltfNodeId, Mat4<f32>> = Default::default();
        for root_index in self.roots() {
            self.compute_final_bone_matrix(root_index, database, &mut final_bone_matrices)?;
        }
        let mut result = Default::default();
        for root_index in self.roots() {
            self.collect_renderables(
                root_index,
                database,
                options,
                &final_bone_matrices,
                &mut result,
            )?;
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

pub type GltfRenderablesSorting = fn([&Vec3<f32>; 3], [&Vec3<f32>; 3]) -> Ordering;

#[derive(Debug, Clone)]
pub struct GltfRenderablesOptions {
    pub shader: Option<ShaderRef>,
    pub axes: [usize; 2],
    pub flip_axes: [bool; 2],
    pub triangle_sorting: Option<GltfRenderablesSorting>,
}

impl Default for GltfRenderablesOptions {
    fn default() -> Self {
        Self {
            shader: None,
            axes: [0, 1],
            flip_axes: [false, false],
            triangle_sorting: None,
        }
    }
}

impl GltfRenderablesOptions {
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
}

impl Drawable for GltfSceneRenderables {
    fn draw(&self, context: &mut DrawContext, graphics: &mut dyn GraphicsTarget<Vertex>) {
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
            graphics
                .state_mut()
                .stream
                .extend(renderable.vertices.clone(), renderable.triangles.clone());
        }
    }
}

fn shortest_slerp(a: Quaternion<f32>, mut b: Quaternion<f32>, t: f32) -> Quaternion<f32> {
    if a.dot(b) < 0.0 {
        b = -b;
    }
    Quaternion::slerp(a, b, t)
}
