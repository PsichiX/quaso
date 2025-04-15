use crate::assets::spine::SpineAsset;
use rusty_spine::{
    AnimationEvent, AnimationStateData, BlendMode, Physics,
    controller::{SkeletonCombinedRenderable, SkeletonController},
};
use spitfire_core::Triangle;
use spitfire_draw::{
    context::DrawContext,
    sprite::SpriteTexture,
    utils::{Drawable, ShaderRef, TextureRef, Vertex},
};
use spitfire_glow::{
    graphics::Graphics,
    graphics::GraphicsBatch,
    renderer::{GlowBlending, GlowUniformValue},
};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    error::Error,
    sync::{
        Arc, RwLock, RwLockReadGuard, RwLockWriteGuard,
        mpsc::{Receiver, channel},
    },
};
use vek::{Quaternion, Transform, Vec3};

pub enum SpineEvent {
    Start,
    Interrupt,
    End,
    Complete,
    Dispose,
    Event {
        /// The name of the event, which is unique across all events in the skeleton.
        name: String,
        /// The animation time this event was keyed.
        time: f32,
        /// The event's int value.
        int: i32,
        /// The event's float value.
        float: f32,
        /// The event's string value or an empty string.
        string: String,
        /// The event's audio path or an empty string.
        audio_path: String,
        /// The event's audio volume.
        volume: f32,
        /// The event's audio balance.
        balance: f32,
    },
}

#[derive(Debug)]
pub struct SpineSkeleton {
    pub shader: Option<ShaderRef>,
    pub uniforms: HashMap<Cow<'static, str>, GlowUniformValue>,
    textures: HashMap<String, SpriteTexture>,
    controller: RwLock<SkeletonController>,
    animation_events: Receiver<SpineEvent>,
}

impl SpineSkeleton {
    pub fn new(asset: &SpineAsset) -> Self {
        let (sender, receiver) = channel::<SpineEvent>();
        let mut controller = SkeletonController::new(
            asset.skeleton_data.clone(),
            Arc::new(AnimationStateData::new(asset.skeleton_data.clone())),
        );
        controller.animation_state.set_listener(move |_, event| {
            let _ = sender.send(match event {
                AnimationEvent::Start { .. } => SpineEvent::Start,
                AnimationEvent::Interrupt { .. } => SpineEvent::Interrupt,
                AnimationEvent::End { .. } => SpineEvent::End,
                AnimationEvent::Complete { .. } => SpineEvent::Complete,
                AnimationEvent::Dispose { .. } => SpineEvent::Dispose,
                AnimationEvent::Event {
                    name,
                    time,
                    int,
                    float,
                    string,
                    audio_path,
                    volume,
                    balance,
                    ..
                } => SpineEvent::Event {
                    name: name.to_owned(),
                    time,
                    int,
                    float,
                    string: string.to_owned(),
                    audio_path: audio_path.to_owned(),
                    volume,
                    balance,
                },
            });
        });
        let textures = asset
            .atlas
            .pages()
            .filter_map(|page| {
                let name = page.name().to_owned();
                let sampler = name
                    .strip_suffix(".png")
                    .unwrap_or(name.as_str())
                    .replace(['-', '.', '/', '\\'], "_")
                    .to_lowercase();
                let sampler = format!("u_{sampler}");
                let path = asset.textures.get(&name)?.path().to_owned();
                let texture = SpriteTexture::new(sampler.into(), TextureRef::name(path));
                Some((name, texture))
            })
            .collect::<HashMap<_, _>>();
        Self {
            shader: None,
            uniforms: Default::default(),
            textures,
            controller: RwLock::new(controller),
            animation_events: receiver,
        }
    }

    pub fn shader(mut self, value: ShaderRef) -> Self {
        self.shader = Some(value);
        self
    }

    pub fn uniform(mut self, key: Cow<'static, str>, value: GlowUniformValue) -> Self {
        self.uniforms.insert(key, value);
        self
    }

    pub fn read(&self) -> Option<RwLockReadGuard<SkeletonController>> {
        self.controller.try_read().ok()
    }

    pub fn write(&self) -> Option<RwLockWriteGuard<SkeletonController>> {
        self.controller.try_write().ok()
    }

    pub fn poll_event(&self) -> Option<SpineEvent> {
        self.animation_events.try_recv().ok()
    }

    pub fn play_animation(
        &self,
        name: &str,
        track_index: usize,
        timescale: f32,
        looping: bool,
    ) -> Result<(), Box<dyn Error>> {
        if let Ok(mut controller) = self.controller.try_write() {
            let mut track =
                controller
                    .animation_state
                    .set_animation_by_name(track_index, name, looping)?;
            track.set_timescale(timescale);
        }
        Ok(())
    }

    pub fn stop_animation(&self, track_index: usize) {
        if let Ok(mut controller) = self.controller.try_write() {
            controller.animation_state.clear_track(track_index);
        }
    }

    pub fn update(&self, delta_time: f32) {
        if let Ok(mut controller) = self.controller.try_write() {
            controller.update(delta_time, Physics::Update);
        }
    }

    pub fn bone_names(&self) -> HashSet<String> {
        if let Ok(controller) = self.controller.try_read() {
            controller
                .skeleton
                .bones()
                .map(|bone| bone.data().name().to_owned())
                .collect()
        } else {
            Default::default()
        }
    }

    pub fn transform(&self, bone: Option<&str>) -> Option<Transform<f32, f32, f32>> {
        let controller = self.controller.try_read().ok()?;
        let bone = if let Some(name) = bone {
            controller.skeleton.find_bone(name)?
        } else {
            controller.skeleton.bone_root()
        };
        let (x, y) = bone.local_to_world(bone.x(), bone.y());
        let (scale_x, scale_y) = bone.local_to_world(bone.scale_x(), bone.scale_y());
        let rotation = bone.local_to_world_rotation(bone.rotation());
        Some(Transform {
            position: Vec3::new(x, y, 0.0),
            orientation: Quaternion::rotation_z(rotation.to_radians()),
            scale: Vec3::new(scale_x, scale_y, 1.0),
        })
    }

    pub fn set_transform(
        &self,
        transform: Transform<f32, f32, f32>,
        bone: Option<&str>,
        update_physics: bool,
    ) {
        if let Ok(mut controller) = self.controller.try_write() {
            let mut bone = if let Some(name) = bone {
                if let Some(bone) = controller.skeleton.find_bone_mut(name) {
                    bone
                } else {
                    return;
                }
            } else {
                controller.skeleton.bone_root_mut()
            };
            let (x, y) = bone.world_to_local(transform.position.x, transform.position.y);
            let (scale_x, scale_y) = bone.world_to_local(transform.scale.x, transform.scale.y);
            let rotation = transform.orientation.into_angle_axis();
            let rotation = bone.world_to_local_rotation((rotation.1.z * rotation.0).to_degrees());
            bone.set_x(x);
            bone.set_y(y);
            bone.set_scale_x(scale_x);
            bone.set_scale_y(scale_y);
            bone.set_rotation(rotation);
            controller
                .skeleton
                .update_world_transform(if update_physics {
                    Physics::Update
                } else {
                    Physics::None
                });
        }
    }

    pub fn update_transform(
        &self,
        bone: Option<&str>,
        update_physics: bool,
        f: impl FnOnce(&mut Transform<f32, f32, f32>),
    ) {
        if let Some(mut transform) = self.transform(bone) {
            f(&mut transform);
            self.set_transform(transform, bone, update_physics);
        }
    }

    fn draw_renderables(
        &self,
        renderables: &[SkeletonCombinedRenderable],
        context: &mut DrawContext,
        graphics: &mut Graphics<Vertex>,
    ) {
        for renderable in renderables {
            let batch = GraphicsBatch {
                shader: context.shader(self.shader.as_ref()),
                uniforms: self
                    .uniforms
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_owned()))
                    .chain(std::iter::once((
                        "u_projection_view".into(),
                        GlowUniformValue::M4(graphics.main_camera.world_matrix().into_col_array()),
                    )))
                    .chain(
                        self.textures
                            .iter()
                            .enumerate()
                            .map(|(index, (_, texture))| {
                                (texture.sampler.clone(), GlowUniformValue::I1(index as _))
                            }),
                    )
                    .collect(),
                textures: self
                    .textures
                    .iter()
                    .filter_map(|(_, texture)| {
                        Some((context.texture(Some(&texture.texture))?, texture.filtering))
                    })
                    .collect(),
                blending: match renderable.blend_mode {
                    BlendMode::Normal => GlowBlending::Alpha,
                    BlendMode::Additive => GlowBlending::Additive,
                    BlendMode::Multiply => GlowBlending::Multiply,
                    BlendMode::Screen => GlowBlending::Additive,
                },
                scissor: None,
            };
            graphics.stream.batch_optimized(batch);
            graphics.stream.extend(
                renderable
                    .vertices
                    .iter()
                    .copied()
                    .zip(renderable.uvs.iter().copied())
                    .zip(renderable.colors.iter().copied())
                    .map(|((position, uv), color)| Vertex {
                        position: [position[0], -position[1]],
                        uv: [uv[0], uv[1], 0.0],
                        color,
                    }),
                renderable.indices.chunks(3).map(|chunk| Triangle {
                    a: chunk[0] as _,
                    b: chunk[1] as _,
                    c: chunk[2] as _,
                }),
            );
        }
    }
}

impl Drawable for SpineSkeleton {
    fn draw(&self, context: &mut DrawContext, graphics: &mut Graphics<Vertex>) {
        if let Ok(mut controller) = self.controller.try_write() {
            let renderables = controller.combined_renderables();
            self.draw_renderables(&renderables, context, graphics);
        }
    }
}

#[derive(Debug)]
pub struct LodSpineSkeleton {
    pub skeleton: SpineSkeleton,
    pub refresh_delay: f32,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BudgetedSpineSkeletonLodSwitchStrategy {
    pub reset_to_pose: bool,
    pub transfer_root_bone_transform: bool,
    pub transfer_all_bones_transforms: bool,
    pub synchronize_animations: bool,
}

#[derive(Debug, Default)]
pub struct BudgetedSpineSkeleton {
    pub lod_switch_strategy: BudgetedSpineSkeletonLodSwitchStrategy,
    refresh_timer: f32,
    lod: usize,
    lods: Vec<LodSpineSkeleton>,
    cached_renderables: Vec<SkeletonCombinedRenderable>,
}

impl BudgetedSpineSkeleton {
    pub fn new(lods: impl IntoIterator<Item = LodSpineSkeleton>) -> Self {
        Self {
            lod_switch_strategy: Default::default(),
            lod: 0,
            lods: lods.into_iter().collect(),
            refresh_timer: 0.0,
            cached_renderables: Default::default(),
        }
    }

    pub fn lod_switch_strategy(mut self, value: BudgetedSpineSkeletonLodSwitchStrategy) -> Self {
        self.lod_switch_strategy = value;
        self
    }

    pub fn with_lod(mut self, lod: LodSpineSkeleton) -> Self {
        self.lods.push(lod);
        self
    }

    pub fn lods_count(&self) -> usize {
        self.lods.len()
    }

    pub fn lod(&self) -> usize {
        self.lod
    }

    pub fn set_lod(&mut self, lod: usize) {
        if self.lod == lod || lod > self.lods.len() {
            return;
        }
        if self.lod_switch_strategy.synchronize_animations {
            let prev = &self.lods[self.lod];
            let next = &self.lods[lod];
            if let (Ok(prev_controller), Ok(mut next_controller)) = (
                prev.skeleton.controller.try_read(),
                next.skeleton.controller.try_write(),
            ) {
                next_controller.animation_state.clear_tracks();
                for (track_index, prev_track) in prev_controller
                    .animation_state
                    .tracks()
                    .flatten()
                    .enumerate()
                {
                    if let Ok(mut next_track) =
                        next_controller.animation_state.set_animation_by_name(
                            track_index,
                            prev_track.animation().name(),
                            prev_track.looping(),
                        )
                    {
                        next_track.set_timescale(prev_track.timescale());
                        next_track.set_looping(prev_track.looping());
                        let track_time = next_track.animation().duration()
                            * prev_track.track_time()
                            / prev_track.animation().duration();
                        next_track.set_track_time(track_time);
                    }
                }
            }
        }
        if self.lod_switch_strategy.reset_to_pose {
            let next = &self.lods[lod];
            if let Ok(mut next_controller) = next.skeleton.controller.try_write() {
                next_controller
                    .skeleton
                    .update_world_transform(Physics::Pose);
            }
        }
        if self.lod_switch_strategy.transfer_all_bones_transforms {
            let prev = &self.lods[self.lod];
            let next = &self.lods[lod];
            let prev_bone_names = prev.skeleton.bone_names();
            let next_bone_names = next.skeleton.bone_names();
            if let (Ok(prev_controller), Ok(mut next_controller)) = (
                prev.skeleton.controller.try_read(),
                next.skeleton.controller.try_write(),
            ) {
                for bone_name in prev_bone_names.intersection(&next_bone_names) {
                    if let (Some(prev_bone), Some(mut next_bone)) = (
                        prev_controller.skeleton.find_bone(bone_name),
                        next_controller.skeleton.find_bone_mut(bone_name),
                    ) {
                        next_bone.set_x(prev_bone.x());
                        next_bone.set_y(prev_bone.y());
                        next_bone.set_scale_x(prev_bone.scale_x());
                        next_bone.set_scale_y(prev_bone.scale_y());
                        next_bone.set_rotation(prev_bone.rotation());
                    }
                }
                next_controller
                    .skeleton
                    .update_world_transform(Physics::None);
            }
        } else if self.lod_switch_strategy.transfer_root_bone_transform {
            let prev = &self.lods[self.lod];
            let next = &self.lods[lod];
            if let (Ok(prev_controller), Ok(mut next_controller)) = (
                prev.skeleton.controller.try_read(),
                next.skeleton.controller.try_write(),
            ) {
                let prev_bone = prev_controller.skeleton.bone_root();
                let mut next_bone = next_controller.skeleton.bone_root_mut();
                next_bone.set_x(prev_bone.x());
                next_bone.set_y(prev_bone.y());
                next_bone.set_scale_x(prev_bone.scale_x());
                next_bone.set_scale_y(prev_bone.scale_y());
                next_bone.set_rotation(prev_bone.rotation());
                next_controller
                    .skeleton
                    .update_world_transform(Physics::None);
            }
        }
        self.lod = lod;
        self.refresh_timer = 0.0;
    }

    pub fn lod_skeleton(&self) -> Option<&LodSpineSkeleton> {
        self.lods.get(self.lod)
    }

    pub fn lod_skeleton_mut(&mut self) -> Option<&mut LodSpineSkeleton> {
        self.lods.get_mut(self.lod)
    }

    pub fn try_refresh(&mut self, delta_time: f32) -> bool {
        let Some(lod) = self.lods.get_mut(self.lod) else {
            return false;
        };
        self.refresh_timer += delta_time;
        if self.refresh_timer >= lod.refresh_delay {
            if let Some(lod) = self.lod_skeleton() {
                lod.skeleton.update(self.refresh_timer);
                let renderables = lod
                    .skeleton
                    .write()
                    .map(|mut controller| controller.combined_renderables());
                if let Some(renderables) = renderables {
                    self.cached_renderables = renderables;
                }
            }
            self.refresh_timer = 0.0;
            return true;
        }
        false
    }
}

impl Drawable for BudgetedSpineSkeleton {
    fn draw(&self, context: &mut DrawContext, graphics: &mut Graphics<Vertex>) {
        if let Some(lod) = self.lod_skeleton() {
            lod.skeleton
                .draw_renderables(&self.cached_renderables, context, graphics);
        }
    }
}
