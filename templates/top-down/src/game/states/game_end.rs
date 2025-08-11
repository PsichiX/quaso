use crate::game::{
    states::{gameplay::Gameplay, main_menu::MainMenu},
    ui::{make_theme, text_button::text_button},
    utils::events::{Event, Events},
};
use quaso::{
    context::GameContext,
    game::{GameState, GameStateChange},
    third_party::{
        raui_core::widget::{
            component::image_box::ImageBoxProps,
            unit::{
                flex::FlexBoxItemLayout,
                image::{ImageBoxAspectRatio, ImageBoxImage, ImageBoxMaterial},
                text::TextBoxVerticalAlign,
            },
        },
        raui_immediate::apply_shared_props,
        raui_immediate_widgets::{
            core::{
                containers::{horizontal_box, nav_vertical_box},
                image_box,
            },
            material::text_paper,
        },
        raui_material::component::text_paper::TextPaperProps,
    },
};
use std::fmt::Display;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameEndReason {
    Lost,
    Won,
}

impl Display for GameEndReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lost => write!(f, "YOU LOST"),
            Self::Won => write!(f, "YOU WON"),
        }
    }
}

pub struct GameEnd {
    reason: GameEndReason,
}

impl GameEnd {
    pub fn new(reason: GameEndReason) -> Self {
        Self { reason }
    }
}

impl GameState for GameEnd {
    fn enter(&mut self, context: GameContext) {
        context.graphics.state.color = [0.2, 0.2, 0.2, 1.0];
        context.gui.coords_map_scaling = Default::default();
    }

    fn draw_gui(&mut self, context: GameContext) {
        apply_shared_props(make_theme(), || {
            image_box(ImageBoxProps {
                content_keep_aspect_ratio: Some(ImageBoxAspectRatio {
                    horizontal_alignment: 0.5,
                    vertical_alignment: 0.0,
                    outside: true,
                }),
                material: ImageBoxMaterial::Image(ImageBoxImage {
                    id: match self.reason {
                        GameEndReason::Lost => "ui/lost".to_owned(),
                        GameEndReason::Won => "ui/won".to_owned(),
                    },
                    ..Default::default()
                }),
                ..Default::default()
            });

            nav_vertical_box((), || {
                text_paper(TextPaperProps {
                    text: self.reason.to_string(),
                    variant: "title".to_owned(),
                    vertical_align_override: Some(TextBoxVerticalAlign::Top),
                    color_override: Some(Default::default()),
                    ..Default::default()
                });

                horizontal_box(
                    FlexBoxItemLayout {
                        basis: Some(100.0),
                        grow: 0.0,
                        shrink: 0.0,
                        ..Default::default()
                    },
                    || {
                        let restart = text_button(
                            FlexBoxItemLayout {
                                margin: 20.0.into(),
                                ..Default::default()
                            },
                            "Restart",
                        );

                        let exit = text_button(
                            FlexBoxItemLayout {
                                margin: 20.0.into(),
                                ..Default::default()
                            },
                            "Exit",
                        );

                        if exit.trigger_stop() {
                            *context.state_change = GameStateChange::Swap(Box::new(MainMenu));
                        } else if restart.trigger_stop() {
                            *context.state_change =
                                GameStateChange::Swap(Box::<Gameplay>::default());
                        }
                    },
                );
            });
        });
    }

    fn fixed_update(&mut self, context: GameContext, delta_time: f32) {
        Events::maintain(delta_time);

        Events::read(|events| {
            for event in events {
                if let Event::PlaySound(id) = event {
                    context.audio.play(id.as_ref());
                }
            }
        });
    }
}
