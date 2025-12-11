pub mod editables;
pub mod ui;

use crate::context::GameContext;
use crate::editor::EditorSubsystem;
use crate::interactible::Interactible;
use raui_core::widget::unit::text::TextBoxSizeValue;
use raui_immediate_widgets::material::text_paper;
use raui_material::component::text_paper::TextPaperProps;
use spitfire_draw::utils::ShaderRef;
use spitfire_input::{InputActionRef, InputAxisRef};
use std::ops::{BitAnd, BitOr, BitXor, Not};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{
    any::{Any, TypeId},
    borrow::Cow,
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};

thread_local! {
    static REGISTRY: RefCell<BTreeMap<EditableEntry, EditableItem>> = Default::default();
    static ACTIVELY_EDITING: RefCell<BTreeSet<EditableEntry>> = Default::default();
    static STACK: RefCell<Vec<Vec<EditableEntry>>> = Default::default();
    static INSTANCE: AtomicUsize = Default::default();
}

const SHADER_NAME: &str = "~~editable-renderables-shader~~";
const DEBUG_VERTEX: &str = r#"#version 300 es
    layout(location = 0) in vec2 a_position;
    out vec4 v_color;
    uniform mat4 u_projection_view;

    void main() {
        gl_Position = u_projection_view * vec4(a_position, 0.0, 1.0);
    }
    "#;

const DEBUG_FRAGMENT: &str = r#"#version 300 es
    precision highp float;
    precision highp int;
    out vec4 o_color;
    uniform float u_time;

    vec3 hsv2rgb(vec3 c) {
        vec4 K = vec4(1.0, 2.0/3.0, 1.0/3.0, 3.0);
        vec3 p = abs(fract(c.xxx + K.xyz) * 6.0 - K.www);
        return c.z * mix(K.xxx, clamp(p - K.xxx, 0.0, 1.0), c.y);
    }

    void main() {
        vec2 pixel = floor(gl_FragCoord.xy);
        float hue = fract((floor(pixel.x) + floor(pixel.y)) * 0.01 + u_time);
        o_color = vec4(hsv2rgb(vec3(hue, 1.0, 1.0)), 1.0);
    }
    "#;

pub(crate) struct EditableItem {
    pub widget_location: EditableWidgetLocation,
    pub entry: EditableEntry,
    pub data: Box<dyn Any>,
    #[allow(clippy::type_complexity)]
    pub update: Box<dyn FnMut(&mut dyn Any, &mut GameContext, f32)>,
    #[allow(clippy::type_complexity)]
    pub draw: Box<dyn FnMut(&mut dyn Any, &mut GameContext)>,
    #[allow(clippy::type_complexity)]
    pub draw_widget: Box<dyn FnMut(&mut dyn Any, &mut GameContext)>,
    #[allow(clippy::type_complexity)]
    pub description: Box<dyn Fn(&dyn Any) -> Cow<'static, str>>,
    pub is_alive: bool,
}

impl EditableItem {
    pub(crate) fn update(&mut self, context: &mut GameContext, delta_time: f32) {
        (self.update)(&mut *self.data, context, delta_time);
    }

    pub(crate) fn draw(&mut self, context: &mut GameContext) {
        (self.draw)(&mut *self.data, context);
    }

    pub(crate) fn draw_widget(&mut self, context: &mut GameContext) {
        text_paper(TextPaperProps {
            text: format!(
                "#{} | {}",
                self.entry.instance(),
                (self.description)(&*self.data)
            ),
            variant: "S".to_owned(),
            height: TextBoxSizeValue::Content,
            ..Default::default()
        });

        (self.draw_widget)(&mut *self.data, context);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EditableEntry {
    instance: usize,
    location: u64,
    wrapper_type_id: TypeId,
    data_type_id: TypeId,
}

impl EditableEntry {
    pub fn maintain() {
        REGISTRY.with_borrow_mut(|registry| {
            registry.retain(|_, record| record.is_alive);
            for record in registry.values_mut() {
                record.is_alive = false;
            }
        });
        STACK.with_borrow_mut(|stack| stack.clear());
        INSTANCE.with(|v| v.store(0, Ordering::SeqCst))
    }

    fn item<'a, T: EditableType>(
        location: u64,
        default_value: impl FnOnce() -> T,
        description: Option<Cow<'static, str>>,
        registry: &'a mut BTreeMap<EditableEntry, EditableItem>,
        stack: &'a mut [Vec<EditableEntry>],
    ) -> &'a mut EditableItem {
        let entry = Self {
            instance: INSTANCE.with(|v| v.fetch_add(1, Ordering::SeqCst)),
            location,
            wrapper_type_id: TypeId::of::<T>(),
            data_type_id: T::data_type_id(),
        };
        if let Some(region) = stack.last_mut() {
            region.push(entry)
        }
        registry.entry(entry).or_insert_with(|| EditableItem {
            widget_location: T::WIDGET_LOCATION,
            entry,
            data: Box::new(default_value()),
            update: Box::new(|data, context, delta_time| {
                data.downcast_mut::<T>()
                    .unwrap()
                    .update(context, delta_time)
            }),
            draw: Box::new(|data, context| data.downcast_mut::<T>().unwrap().draw(context)),
            draw_widget: Box::new(|data, context| {
                data.downcast_mut::<T>().unwrap().widget(context)
            }),
            description: Box::new(move |data| {
                if let Some(description) = description.as_ref() {
                    description.clone()
                } else {
                    data.downcast_ref::<T>().unwrap().description()
                }
            }),
            is_alive: false,
        })
    }

    fn with_registry<R>(f: impl FnOnce(&mut BTreeMap<EditableEntry, EditableItem>) -> R) -> R {
        REGISTRY.with_borrow_mut(|registry| f(registry))
    }

    fn with_registry_and_stack<R>(
        f: impl FnOnce(&mut BTreeMap<EditableEntry, EditableItem>, &mut Vec<Vec<EditableEntry>>) -> R,
    ) -> R {
        REGISTRY.with_borrow_mut(|registry| STACK.with_borrow_mut(|stack| f(registry, stack)))
    }

    pub(crate) fn with_registry_and_actively_editing<R>(
        f: impl FnOnce(&mut BTreeMap<EditableEntry, EditableItem>, &BTreeSet<EditableEntry>) -> R,
    ) -> R {
        REGISTRY.with_borrow_mut(|registry| {
            ACTIVELY_EDITING.with_borrow(|actively_editing| f(registry, actively_editing))
        })
    }

    pub fn entry<T: EditableType>(
        location: u64,
        default_value: impl FnOnce() -> T,
        description: Option<Cow<'static, str>>,
    ) -> Self {
        Self::with_registry_and_stack(|registry, stack| {
            let item = Self::item(location, default_value, description, registry, stack);
            item.is_alive = true;
            item.entry
        })
    }

    pub fn value<T: EditableType>(
        location: u64,
        default_value: impl FnOnce() -> T,
        description: Option<Cow<'static, str>>,
        edit: bool,
    ) -> T::Value {
        Self::with_registry_and_stack(|registry, stack| {
            let item = Self::item(location, default_value, description, registry, stack);
            if edit {
                item.entry.start_editing();
            }
            item.is_alive = true;
            item.data
                .downcast_ref::<T>()
                .expect("EditableEntry type mismatch")
                .unpack()
        })
    }

    pub fn entry_value<T: EditableType>(
        location: u64,
        default_value: impl FnOnce() -> T,
        description: Option<Cow<'static, str>>,
    ) -> (Self, T::Value) {
        Self::with_registry_and_stack(|registry, stack| {
            let item = Self::item(location, default_value, description, registry, stack);
            item.is_alive = true;
            let value = item
                .data
                .downcast_ref::<T>()
                .expect("EditableEntry type mismatch")
                .unpack();
            (item.entry, value)
        })
    }

    pub fn get<T: EditableType>(self) -> Option<T::Value> {
        Self::with_registry(|registry| {
            let item = registry.get_mut(&self)?;
            item.data
                .downcast_ref::<T>()
                .map(|wrapped| wrapped.unpack())
        })
    }

    pub fn with<T: 'static, R>(self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        Self::with_registry(|registry| {
            let item = registry.get_mut(&self)?;
            item.data.downcast_mut::<T>().map(f)
        })
    }

    pub fn location(self) -> u64 {
        self.location
    }

    pub fn instance(self) -> usize {
        self.instance
    }

    pub fn wrapper_type_id(self) -> TypeId {
        self.wrapper_type_id
    }

    pub fn data_type_id(self) -> TypeId {
        self.data_type_id
    }

    pub fn start_editing(self) {
        ACTIVELY_EDITING.with_borrow_mut(|actively_editing| {
            if !actively_editing.contains(&self) {
                actively_editing.insert(self);
            }
        });
    }

    pub fn start_editing_many(entries: impl IntoIterator<Item = Self>) {
        ACTIVELY_EDITING.with_borrow_mut(|actively_editing| {
            actively_editing.extend(entries);
        });
    }

    pub fn stop_editing(self) {
        ACTIVELY_EDITING.with_borrow_mut(|actively_editing| {
            actively_editing.remove(&self);
        });
    }

    pub fn stop_editing_many(entries: impl IntoIterator<Item = Self>) {
        ACTIVELY_EDITING.with_borrow_mut(|actively_editing| {
            for entry in entries {
                actively_editing.remove(&entry);
            }
        });
    }

    pub fn stop_editing_all() {
        ACTIVELY_EDITING.with_borrow_mut(|actively_editing| {
            actively_editing.clear();
        });
    }

    pub fn begin_region() {
        STACK.with_borrow_mut(|stack| {
            stack.push(Default::default());
        });
    }

    pub fn end_region() -> Vec<Self> {
        STACK.with_borrow_mut(|stack| {
            let entries = stack.pop().unwrap_or_default();
            if let Some(region) = stack.last_mut() {
                region.extend(entries.iter().copied());
            }
            entries
        })
    }

    pub fn push_to_region(self) {
        STACK.with_borrow_mut(|stack| {
            if let Some(region) = stack.last_mut() {
                region.push(self);
            }
        });
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditableWidgetLocation(u8);

impl EditableWidgetLocation {
    pub const NOWHERE: Self = Self(0);
    pub const WORLD_SPACE: Self = Self(1 << 0);
    pub const EDITING_PANEL: Self = Self(1 << 1);
}

impl EditableWidgetLocation {
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

impl BitOr for EditableWidgetLocation {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitAnd for EditableWidgetLocation {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl BitXor for EditableWidgetLocation {
    type Output = Self;

    fn bitxor(self, rhs: Self) -> Self::Output {
        Self(self.0 ^ rhs.0)
    }
}

impl Not for EditableWidgetLocation {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self(!self.0)
    }
}

pub trait EditableType: 'static {
    type Value;
    const WIDGET_LOCATION: EditableWidgetLocation;

    fn data_type_id() -> TypeId {
        TypeId::of::<Self::Value>()
    }

    fn unpack(&self) -> Self::Value;

    #[allow(unused_variables)]
    fn update(&mut self, context: &mut GameContext, delta_time: f32) {}

    #[allow(unused_variables)]
    fn draw(&mut self, context: &mut GameContext) {}

    #[allow(unused_variables)]
    fn widget(&mut self, context: &mut GameContext) {}

    fn description(&self) -> Cow<'static, str> {
        std::any::type_name::<Self::Value>().into()
    }
}

#[derive(Default)]
pub struct Editable {
    pub pointer_x: InputAxisRef,
    pub pointer_y: InputAxisRef,
    pub pointer_trigger: InputActionRef,
    pub highlight: Option<(Vec<EditableEntry>, Interactible)>,
}

#[derive(Default)]
pub struct EditableSubsystem;

impl Drop for EditableSubsystem {
    fn drop(&mut self) {
        EditableEntry::stop_editing_all();
    }
}

impl EditorSubsystem for EditableSubsystem {
    fn update(&mut self, mut context: GameContext, delta_time: f32) {
        EditableEntry::with_registry_and_actively_editing(|registry, entries| {
            for entry in entries {
                if let Some(editable) = registry.get_mut(entry)
                    && editable
                        .widget_location
                        .contains(EditableWidgetLocation::WORLD_SPACE)
                {
                    editable.update(&mut context, delta_time);
                }
            }
        });
    }

    fn draw(&mut self, mut context: GameContext) {
        let shader = Self::shader(&mut context);

        {
            let editable = context.globals.ensure::<Editable>();
            let mut editable = editable.write();
            if let Some((entries, interactible)) = editable.highlight.take() {
                interactible.draw_wireframe(
                    &shader,
                    [1.0, 1.0, 0.0, 1.0].into(),
                    context.draw,
                    context.graphics,
                    context.time,
                );
                if editable.pointer_trigger.get().is_pressed() {
                    EditableEntry::stop_editing_all();
                    EditableEntry::start_editing_many(entries);
                }
            }
        }

        EditableEntry::with_registry_and_actively_editing(|registry, entries| {
            for entry in entries {
                if let Some(editable) = registry.get_mut(entry)
                    && editable
                        .widget_location
                        .contains(EditableWidgetLocation::WORLD_SPACE)
                {
                    editable.draw(&mut context);
                }
            }
        });
    }

    fn draw_gui(&mut self, _: GameContext) {
        EditableEntry::maintain();
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl EditableSubsystem {
    pub fn shader(context: &mut GameContext) -> ShaderRef {
        let shader = context
            .draw
            .shaders
            .entry(SHADER_NAME.into())
            .or_insert_with(|| {
                context
                    .graphics
                    .shader(DEBUG_VERTEX, DEBUG_FRAGMENT)
                    .unwrap()
            });
        ShaderRef::object(shader.clone())
    }
}

#[macro_export]
macro_rules! editable {
    (@location) => {{
        use std::{
            collections::hash_map::DefaultHasher,
            hash::{Hash, Hasher},
        };
        let file = file!();
        let line = line!();
        let column = column!();
        let mut hasher = DefaultHasher::new();
        (file, line, column).hash(&mut hasher);
        hasher.finish()
    }};
    (@entry $description:literal => $value:expr) => {{
        $crate::editor::features::editable::EditableEntry::entry(
            editable!(@location),
            move || $value,
            Some($description.into()),
        )
    }};
    (@entry $value:expr) => {{
        $crate::editor::features::editable::EditableEntry::entry(
            editable!(@location),
            move || $value,
            None,
        )
    }};
    (@edit $description:literal => $value:expr) => {{
        $crate::editor::features::editable::EditableEntry::value(
            editable!(@location),
            move || $value,
            Some($description.into()),
            true,
        )
    }};
    (@edit $value:expr) => {{
        $crate::editor::features::editable::EditableEntry::value(
            editable!(@location),
            move || $value,
            None,
            true,
        )
    }};
    ($description:literal => $value:expr) => {{
        $crate::editor::features::editable::EditableEntry::value(
            editable!(@location),
            move || $value,
            Some($description.into()),
            false,
        )
    }};
    ($value:expr) => {{
        $crate::editor::features::editable::EditableEntry::value(
            editable!(@location),
            move || $value,
            None,
            false,
        )
    }};
}

#[macro_export]
macro_rules! editable_renderables {
    ($context:expr, $body:block) => {{
        if !$context.globals.editor.is_editing() {
            { $body }
        } else {
            $crate::editor::features::editable::EditableEntry::begin_region();
            let interactible = $crate::interactible::RenderableInteractible::new($context.graphics);
            {
                $body
            }
            let entries = $crate::editor::features::editable::EditableEntry::end_region();
            let editable = $context
                .globals
                .ensure::<$crate::editor::features::editable::Editable>();
            let x = editable.read().pointer_x.get().0;
            let y = editable.read().pointer_y.get().0;
            let screen = $context
                .globals
                .editor
                .window_screen_to_viewport_screen($context.graphics, Vec2::new(x, y));
            let world = $context
                .graphics
                .state
                .main_camera
                .screen_to_world_point(screen);
            let interactible = interactible.interactible($context.graphics);
            if interactible.contains_point(world) {
                editable.write().highlight = Some((entries, interactible));
            }
        }
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    impl EditableType for i32 {
        type Value = Self;
        const WIDGET_LOCATION: EditableWidgetLocation = EditableWidgetLocation::NOWHERE;

        fn data_type_id() -> TypeId {
            TypeId::of::<Self>()
        }

        fn unpack(&self) -> Self::Value {
            *self
        }
    }

    #[test]
    fn test_editable_hash_uniqueness() {
        let hash1 = editable!(@location);
        let hash2 = editable!(@location);
        assert_ne!(hash1, hash2, "Hashes should be unique for different calls");
    }

    #[test]
    fn test_editable_value_storage() {
        let initial_value = editable!(10_i32);
        assert_eq!(initial_value, 10);

        let updated_value = editable!(20_i32);
        assert_eq!(updated_value, 20);
    }

    #[test]
    fn test_editable_entry_modification() {
        for index in 0..3 {
            let entry = editable!(@entry 5_i32);

            match index {
                0 => {
                    let value = entry.get::<i32>().unwrap();
                    assert_eq!(value, 5);
                    entry.with::<i32, ()>(|value| {
                        *value = 42;
                    });
                }
                2 => {
                    let value = entry.get::<i32>().unwrap();
                    assert_eq!(value, 42);
                }
                _ => break,
            }
        }
    }
}
