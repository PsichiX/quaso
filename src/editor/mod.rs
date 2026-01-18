pub mod features;
pub mod ui;

use crate::{
    context::GameContext,
    editor::features::viewport::{EditorGameViewport, editor_viewport_game_world_and_ui},
    game::GameGlobals,
    third_party::windowing::event::{
        ElementState, Event, ModifiersState, MouseScrollDelta, WindowEvent,
    },
};
use fontdue::Font;
use raui_core::{
    interactive::default_interactions_engine::{Interaction, PointerButton},
    layout::CoordsMapping,
    widget::{
        component::interactive::navigation::{NavJump, NavScroll, NavSignal, NavTextChange},
        node::WidgetNode,
        unit::text::{TextBoxFont, TextBoxHorizontalAlign, TextBoxVerticalAlign},
        utils::{Rect, Vec2},
    },
};
use raui_immediate::{ImSharedProps, apply, begin, end};
use raui_immediate_widgets::material::containers::nav_paper;
use raui_material::theme::{ThemeProps, ThemedTextMaterial, new_dark_theme};
use spitfire_draw::{canvas::Canvas, context::DrawContext, utils::Vertex};
use spitfire_glow::{graphics::Graphics, renderer::GlowTextureFormat};
use spitfire_gui::context::GuiContext;
use spitfire_input::{InputContext, InputMapping, InputMappingRef, MouseButton, VirtualKeyCode};
use std::{
    any::{Any, TypeId},
    borrow::Cow,
    collections::HashMap,
    sync::mpsc::{Receiver, Sender},
};
use typid::ID;

const ROBOTO_FONT_DATA: &[u8] = include_bytes!("./roboto.ttf");
pub const EDITOR_FONT_NAME: &str = "~~editor-roboto-font~~";

pub struct Editor {
    pub(crate) subsystems: Vec<Box<dyn EditorSubsystem>>,
    game_canvas: Option<Canvas>,
    game_widgets: Vec<WidgetNode>,
    #[allow(clippy::type_complexity)]
    gui_drawer: Box<dyn FnMut(&mut GameContext, &mut EditorSubsystems)>,
    edit_mode_switch_key: VirtualKeyCode,
    show_editor_while_running: bool,
    coords_mapping: CoordsMapping,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            subsystems: Default::default(),
            game_canvas: None,
            game_widgets: Default::default(),
            gui_drawer: Box::new(editor_viewport_game_world_and_ui),
            edit_mode_switch_key: VirtualKeyCode::F5,
            show_editor_while_running: true,
            coords_mapping: Default::default(),
        }
    }
}

impl Editor {
    pub fn with_gui_drawer(mut self, f: fn(&mut GameContext, &mut EditorSubsystems)) -> Self {
        self.gui_drawer = Box::new(f);
        self
    }

    pub fn show_editor_while_running(mut self, show: bool) -> Self {
        self.show_editor_while_running = show;
        self
    }

    pub(crate) fn initialize(&mut self, context: GameContext) {
        context.draw.fonts.insert(
            EDITOR_FONT_NAME,
            Font::from_bytes(ROBOTO_FONT_DATA, Default::default()).unwrap(),
        );
    }

    pub(crate) fn begin_frame_capture(
        &mut self,
        graphics: &mut Graphics<Vertex>,
        draw: &mut DrawContext,
    ) {
        if let Some(canvas) = &mut self.game_canvas {
            canvas.surface_mut().set_color(graphics.state.color);
            canvas.activate(draw, graphics, true);
        } else {
            self.game_canvas = Canvas::from_screen(vec![GlowTextureFormat::Rgb], graphics).ok();
            if let Some(canvas) = &self.game_canvas {
                canvas.activate(draw, graphics, true);
            }
        }
    }

    pub(crate) fn end_frame_capture(
        &mut self,
        graphics: &mut Graphics<Vertex>,
        draw: &mut DrawContext,
    ) {
        if let Some(canvas) = &self.game_canvas {
            Canvas::deactivate(draw, graphics);
            draw.textures.insert(
                EditorGameViewport::ID.into(),
                canvas.surface().attachments()[0].texture.clone(),
            );
        }
    }

    pub(crate) fn begin_gui_capture(&mut self) {
        begin();
    }

    pub(crate) fn end_gui_capture(&mut self) {
        self.game_widgets = end();
    }

    pub(crate) fn update(
        &mut self,
        graphics: &mut Graphics<Vertex>,
        gui: &GuiContext,
        globals: &mut GameGlobals,
    ) {
        globals.editor.input.maintain();
        globals.editor.viewport_rectangle = Default::default();
        if let Some(canvas) = &mut self.game_canvas
            && let Some((_, layout)) = gui
                .application
                .layout_data()
                .items
                .iter()
                .find(|(id, _)| id.key() == EditorGameViewport::ID)
        {
            self.coords_mapping = CoordsMapping::new_scaling(
                Rect {
                    left: 0.0,
                    right: graphics.state.main_camera.screen_size.x,
                    top: 0.0,
                    bottom: graphics.state.main_camera.screen_size.y,
                },
                gui.coords_map_scaling,
            );
            let layout = layout.virtual_to_real(&self.coords_mapping);
            globals.editor.viewport_rectangle.x = layout.ui_space.left;
            globals.editor.viewport_rectangle.y = layout.ui_space.top;
            let width = layout.ui_space.width() as u32;
            let height = layout.ui_space.height() as u32;
            let _ = canvas.match_to_size(graphics, width.max(1), height.max(1));
            globals.editor.viewport_rectangle.w = layout.ui_space.width();
            globals.editor.viewport_rectangle.h = layout.ui_space.height();
        }
    }

    pub(crate) fn draw_gui(&mut self, mut context: GameContext) {
        let mut viewport = context.globals.ensure::<EditorGameViewport>();
        viewport.write().widgets.clear();
        viewport.write().widgets.append(&mut self.game_widgets);
        if self.show_editor_while_running || context.globals.editor.is_editing() {
            apply(ImSharedProps(make_theme()), || {
                nav_paper((), || {
                    (self.gui_drawer)(
                        &mut context,
                        &mut EditorSubsystems {
                            subsystems: &mut self.subsystems,
                        },
                    );
                });
            });
        } else {
            viewport.write().world_and_ui(Default::default());
        }
    }

    pub fn event(&mut self, event: &WindowEvent, gui: &mut GuiContext, globals: &mut GameGlobals) {
        if let WindowEvent::KeyboardInput { input, .. } = event
            && input.virtual_keycode == Some(self.edit_mode_switch_key)
            && input.state == ElementState::Pressed
        {
            globals.editor.is_editing = !globals.editor.is_editing;
        }

        match event {
            WindowEvent::ModifiersChanged(modifiers) => {
                globals.editor.input.modifiers = *modifiers;
            }
            WindowEvent::ReceivedCharacter(character) => {
                gui.interactions
                    .engine
                    .interact(Interaction::Navigate(NavSignal::TextChange(
                        NavTextChange::InsertCharacter(*character),
                    )));
            }
            WindowEvent::CursorMoved { position, .. } => {
                globals.editor.input.pointer_position = self.coords_mapping.real_to_virtual_vec2(
                    Vec2 {
                        x: position.x as _,
                        y: position.y as _,
                    },
                    false,
                );
                gui.interactions.engine.interact(Interaction::PointerMove(
                    globals.editor.input.pointer_position,
                ));
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let value = match delta {
                    MouseScrollDelta::LineDelta(x, y) => Vec2 {
                        x: -globals.editor.input.single_scroll_units.x * *x,
                        y: -globals.editor.input.single_scroll_units.y * *y,
                    },
                    MouseScrollDelta::PixelDelta(delta) => Vec2 {
                        x: -delta.x as _,
                        y: -delta.y as _,
                    },
                };
                gui.interactions
                    .engine
                    .interact(Interaction::Navigate(NavSignal::Jump(NavJump::Scroll(
                        NavScroll::Units(value, true),
                    ))));
            }
            WindowEvent::MouseInput { state, button, .. } => match state {
                ElementState::Pressed => match button {
                    MouseButton::Left => {
                        gui.interactions.engine.interact(Interaction::PointerDown(
                            PointerButton::Trigger,
                            globals.editor.input.pointer_position,
                        ));
                    }
                    MouseButton::Right => {
                        gui.interactions.engine.interact(Interaction::PointerDown(
                            PointerButton::Context,
                            globals.editor.input.pointer_position,
                        ));
                    }
                    _ => {}
                },
                ElementState::Released => match button {
                    MouseButton::Left => {
                        gui.interactions.engine.interact(Interaction::PointerUp(
                            PointerButton::Trigger,
                            globals.editor.input.pointer_position,
                        ));
                    }
                    MouseButton::Right => {
                        gui.interactions.engine.interact(Interaction::PointerUp(
                            PointerButton::Context,
                            globals.editor.input.pointer_position,
                        ));
                    }
                    _ => {}
                },
            },
            WindowEvent::KeyboardInput { input, .. } => {
                if input.state == ElementState::Pressed {
                    if let Some(key) = input.virtual_keycode {
                        if gui.interactions.engine.focused_text_input().is_some() {
                            match key {
                                VirtualKeyCode::Left => {
                                    gui.interactions.engine.interact(Interaction::Navigate(
                                        NavSignal::TextChange(NavTextChange::MoveCursorLeft),
                                    ))
                                }
                                VirtualKeyCode::Right => {
                                    gui.interactions.engine.interact(Interaction::Navigate(
                                        NavSignal::TextChange(NavTextChange::MoveCursorRight),
                                    ))
                                }
                                VirtualKeyCode::Home => {
                                    gui.interactions.engine.interact(Interaction::Navigate(
                                        NavSignal::TextChange(NavTextChange::MoveCursorStart),
                                    ))
                                }
                                VirtualKeyCode::End => {
                                    gui.interactions.engine.interact(Interaction::Navigate(
                                        NavSignal::TextChange(NavTextChange::MoveCursorEnd),
                                    ))
                                }
                                VirtualKeyCode::Back => {
                                    gui.interactions.engine.interact(Interaction::Navigate(
                                        NavSignal::TextChange(NavTextChange::DeleteLeft),
                                    ))
                                }
                                VirtualKeyCode::Delete => {
                                    gui.interactions.engine.interact(Interaction::Navigate(
                                        NavSignal::TextChange(NavTextChange::DeleteRight),
                                    ))
                                }
                                VirtualKeyCode::Return | VirtualKeyCode::NumpadEnter => {
                                    gui.interactions.engine.interact(Interaction::Navigate(
                                        NavSignal::TextChange(NavTextChange::NewLine),
                                    ))
                                }
                                VirtualKeyCode::Escape => {
                                    gui.interactions.engine.interact(Interaction::Navigate(
                                        NavSignal::FocusTextInput(().into()),
                                    ));
                                }
                                _ => {}
                            }
                        } else {
                            match key {
                                VirtualKeyCode::Up => gui
                                    .interactions
                                    .engine
                                    .interact(Interaction::Navigate(NavSignal::Up)),
                                VirtualKeyCode::Down => gui
                                    .interactions
                                    .engine
                                    .interact(Interaction::Navigate(NavSignal::Down)),
                                VirtualKeyCode::Left => {
                                    if globals.editor.input.modifiers.shift() {
                                        gui.interactions
                                            .engine
                                            .interact(Interaction::Navigate(NavSignal::Prev));
                                    } else {
                                        gui.interactions
                                            .engine
                                            .interact(Interaction::Navigate(NavSignal::Left));
                                    }
                                }
                                VirtualKeyCode::Right => {
                                    if globals.editor.input.modifiers.shift() {
                                        gui.interactions
                                            .engine
                                            .interact(Interaction::Navigate(NavSignal::Next));
                                    } else {
                                        gui.interactions
                                            .engine
                                            .interact(Interaction::Navigate(NavSignal::Right));
                                    }
                                }
                                VirtualKeyCode::Return
                                | VirtualKeyCode::NumpadEnter
                                | VirtualKeyCode::Space => {
                                    gui.interactions
                                        .engine
                                        .interact(Interaction::Navigate(NavSignal::Accept(true)));
                                }
                                VirtualKeyCode::Escape | VirtualKeyCode::Back => {
                                    gui.interactions
                                        .engine
                                        .interact(Interaction::Navigate(NavSignal::Cancel(true)));
                                }
                                _ => {}
                            }
                        }
                    }
                } else if input.state == ElementState::Released
                    && let Some(key) = input.virtual_keycode
                    && gui.interactions.engine.focused_text_input().is_none()
                {
                    match key {
                        VirtualKeyCode::Return
                        | VirtualKeyCode::NumpadEnter
                        | VirtualKeyCode::Space => {
                            gui.interactions
                                .engine
                                .interact(Interaction::Navigate(NavSignal::Accept(false)));
                        }
                        VirtualKeyCode::Escape | VirtualKeyCode::Back => {
                            gui.interactions
                                .engine
                                .interact(Interaction::Navigate(NavSignal::Cancel(false)));
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

fn make_theme() -> ThemeProps {
    new_dark_theme()
        .text_variant(
            "XXL",
            ThemedTextMaterial {
                font: TextBoxFont {
                    name: EDITOR_FONT_NAME.to_string(),
                    size: 48.0,
                },
                ..Default::default()
            },
        )
        .text_variant(
            "XL",
            ThemedTextMaterial {
                font: TextBoxFont {
                    name: EDITOR_FONT_NAME.to_string(),
                    size: 32.0,
                },
                ..Default::default()
            },
        )
        .text_variant(
            "L",
            ThemedTextMaterial {
                font: TextBoxFont {
                    name: EDITOR_FONT_NAME.to_string(),
                    size: 24.0,
                },
                ..Default::default()
            },
        )
        .text_variant(
            "",
            ThemedTextMaterial {
                font: TextBoxFont {
                    name: EDITOR_FONT_NAME.to_string(),
                    size: 18.0,
                },
                ..Default::default()
            },
        )
        .text_variant(
            "S",
            ThemedTextMaterial {
                font: TextBoxFont {
                    name: EDITOR_FONT_NAME.to_string(),
                    size: 14.0,
                },
                ..Default::default()
            },
        )
        .text_variant(
            "XS",
            ThemedTextMaterial {
                font: TextBoxFont {
                    name: EDITOR_FONT_NAME.to_string(),
                    size: 10.0,
                },
                ..Default::default()
            },
        )
        .text_variant(
            "XXS",
            ThemedTextMaterial {
                font: TextBoxFont {
                    name: EDITOR_FONT_NAME.to_string(),
                    size: 6.0,
                },
                ..Default::default()
            },
        )
        .text_variant(
            "button",
            ThemedTextMaterial {
                font: TextBoxFont {
                    name: EDITOR_FONT_NAME.to_string(),
                    size: 18.0,
                },
                horizontal_align: TextBoxHorizontalAlign::Center,
                vertical_align: TextBoxVerticalAlign::Middle,
                ..Default::default()
            },
        )
}

pub enum EditorInputCommand {
    AddMapping {
        name: Cow<'static, str>,
        mapping: InputMappingRef,
    },
    RemoveMapping {
        name: Cow<'static, str>,
    },
}

pub struct EditorInput {
    pub(crate) single_scroll_units: Vec2,
    pub(crate) pointer_position: Vec2,
    pub(crate) modifiers: ModifiersState,
    pub(crate) context: InputContext,
    table: HashMap<Cow<'static, str>, ID<InputMapping>>,
    sender: Sender<EditorInputCommand>,
    receiver: Receiver<EditorInputCommand>,
}

impl Default for EditorInput {
    fn default() -> Self {
        let (sender, receiver) = std::sync::mpsc::channel();
        Self {
            single_scroll_units: Vec2 { x: 10.0, y: 0.0 },
            pointer_position: Default::default(),
            modifiers: Default::default(),
            context: Default::default(),
            table: Default::default(),
            sender,
            receiver,
        }
    }
}

impl EditorInput {
    pub fn commands(&self) -> &Sender<EditorInputCommand> {
        &self.sender
    }

    pub(crate) fn maintain(&mut self) {
        while let Ok(command) = self.receiver.try_recv() {
            match command {
                EditorInputCommand::AddMapping { name, mapping } => {
                    let id = self.context.push_mapping(mapping.clone());
                    self.table.insert(name, id);
                }
                EditorInputCommand::RemoveMapping { name } => {
                    if let Some(id) = self.table.remove(&name) {
                        self.context.remove_mapping(id);
                    }
                }
            }
        }
        self.context.maintain();
    }
}

pub trait EditorSubsystem {
    #[allow(unused_variables)]
    fn update(&mut self, context: GameContext, delta_time: f32) {}

    #[allow(unused_variables)]
    fn fixed_update(&mut self, context: GameContext, delta_time: f32) {}

    #[allow(unused_variables)]
    fn draw(&mut self, context: GameContext) {}

    #[allow(unused_variables)]
    fn draw_gui(&mut self, context: GameContext) {}

    #[allow(unused_variables)]
    fn event(&mut self, globals: &mut GameGlobals, event: &Event<()>) {}

    fn as_any(&self) -> &dyn Any;

    fn as_any_mut(&mut self) -> &mut dyn Any;
}

pub struct EditorSubsystems<'a> {
    subsystems: &'a mut Vec<Box<dyn EditorSubsystem>>,
}

impl<'a> EditorSubsystems<'a> {
    pub fn add<T: EditorSubsystem + 'static>(&mut self, subsystem: T) {
        self.remove::<T>();
        self.subsystems.push(Box::new(subsystem));
    }

    pub fn remove<T: EditorSubsystem + 'static>(&mut self) {
        self.subsystems
            .retain(|s| s.as_any().type_id() != TypeId::of::<T>());
    }

    pub fn ensure<T: EditorSubsystem + Default + 'static>(&mut self) -> &mut T {
        if self
            .subsystems
            .iter()
            .all(|s| s.as_any().type_id() != TypeId::of::<T>())
        {
            self.subsystems.push(Box::new(T::default()));
        }
        self.get_mut::<T>().unwrap()
    }

    pub fn get<T: EditorSubsystem + 'static>(&self) -> Option<&T> {
        for subsystem in self.subsystems.iter() {
            if let Some(specific) = subsystem.as_any().downcast_ref::<T>() {
                return Some(specific);
            }
        }
        None
    }

    pub fn get_mut<T: EditorSubsystem + 'static>(&mut self) -> Option<&mut T> {
        for subsystem in self.subsystems.iter_mut() {
            if let Some(specific) = subsystem.as_any_mut().downcast_mut::<T>() {
                return Some(specific);
            }
        }
        None
    }
}
