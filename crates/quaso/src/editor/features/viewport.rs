use crate::{context::GameContext, editor::EditorSubsystems};
use raui_core::widget::{
    component::image_box::ImageBoxProps,
    node::WidgetNode,
    unit::image::{ImageBoxImage, ImageBoxMaterial},
    utils::{Color, Rect},
};
use raui_immediate::{ImKey, apply, extend};
use raui_immediate_widgets::core::{containers::content_box, image_box};

#[derive(Default)]
pub struct EditorGameViewport {
    pub(crate) widgets: Vec<WidgetNode>,
}

impl EditorGameViewport {
    pub(crate) const ID: &str = "~~editor-game-viewport~~";

    pub fn world_and_ui(&mut self, tint: Color) {
        Self::world(tint);
        self.ui();
    }

    pub fn world(tint: Color) {
        apply(ImKey(Self::ID), || {
            image_box(ImageBoxProps {
                material: ImageBoxMaterial::Image(ImageBoxImage {
                    id: Self::ID.to_owned(),
                    source_rect: Some(Rect {
                        left: 0.0,
                        right: 1.0,
                        top: 1.0,
                        bottom: 0.0,
                    }),
                    tint,
                    ..Default::default()
                }),
                ..Default::default()
            });
        });
    }

    pub fn ui(&mut self) {
        extend(self.widgets.drain(..));
    }
}

pub fn editor_viewport_game_world_and_ui(context: &mut GameContext, _: &mut EditorSubsystems) {
    content_box((), || {
        EditorGameViewport::world(Default::default());
        context.globals.ensure::<EditorGameViewport>().write().ui();
    });
}

pub fn editor_viewport_game_world(_: &mut GameContext, _: &mut EditorSubsystems) {
    content_box((), || {
        EditorGameViewport::world(Default::default());
    });
}

pub fn editor_viewport_game_ui(context: &mut GameContext, _: &mut EditorSubsystems) {
    content_box((), || {
        context.globals.ensure::<EditorGameViewport>().write().ui();
    });
}
