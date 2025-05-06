pub mod health_bar;
pub mod text_button;

use super::utils::world_to_screen_anchor;
use quaso::{
    context::GameContext,
    third_party::{
        raui_core::widget::{
            unit::{
                content::ContentBoxItemLayout,
                image::{ImageBoxFrame, ImageBoxImage, ImageBoxImageScaling},
                text::{TextBoxFont, TextBoxHorizontalAlign, TextBoxVerticalAlign},
            },
            utils::Rect,
        },
        raui_material::theme::{
            new_all_white_theme, ThemeProps, ThemedButtonMaterial, ThemedImageMaterial,
            ThemedTextMaterial,
        },
        vek::Vec2,
    },
};

pub fn world_to_screen_content_layout(
    position: Vec2<f32>,
    region: Rect,
    context: &GameContext,
) -> ContentBoxItemLayout {
    let anchor = world_to_screen_anchor(position, context);
    ContentBoxItemLayout {
        anchors: Rect {
            left: anchor.x,
            right: anchor.x,
            top: anchor.y,
            bottom: anchor.y,
        },
        margin: Rect {
            left: region.left,
            right: -region.right,
            top: region.top,
            bottom: -region.bottom,
        },
        ..Default::default()
    }
}

pub fn make_theme() -> ThemeProps {
    new_all_white_theme()
        .content_background(
            "",
            ThemedImageMaterial::Image(ImageBoxImage {
                id: "ui/panel".to_owned(),
                scaling: ImageBoxImageScaling::Frame(ImageBoxFrame {
                    source: 0.5.into(),
                    destination: 48.0.into(),
                    ..Default::default()
                }),
                ..Default::default()
            }),
        )
        .button_background(
            "",
            ThemedButtonMaterial {
                default: ThemedImageMaterial::Image(ImageBoxImage {
                    id: "ui/button/idle".to_owned(),
                    scaling: ImageBoxImageScaling::Frame(ImageBoxFrame {
                        source: 0.5.into(),
                        destination: 16.0.into(),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                selected: ThemedImageMaterial::Image(ImageBoxImage {
                    id: "ui/button/select".to_owned(),
                    scaling: ImageBoxImageScaling::Frame(ImageBoxFrame {
                        source: 0.5.into(),
                        destination: 16.0.into(),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                trigger: ThemedImageMaterial::Image(ImageBoxImage {
                    id: "ui/button/trigger".to_owned(),
                    scaling: ImageBoxImageScaling::Frame(ImageBoxFrame {
                        source: 0.5.into(),
                        destination: 16.0.into(),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
            },
        )
        .text_variant(
            "",
            ThemedTextMaterial {
                font: TextBoxFont {
                    name: "roboto".to_owned(),
                    size: 32.0,
                },
                ..Default::default()
            },
        )
        .text_variant(
            "title",
            ThemedTextMaterial {
                font: TextBoxFont {
                    name: "roboto".to_owned(),
                    size: 100.0,
                },
                horizontal_align: TextBoxHorizontalAlign::Center,
                vertical_align: TextBoxVerticalAlign::Middle,
                ..Default::default()
            },
        )
        .text_variant(
            "button",
            ThemedTextMaterial {
                font: TextBoxFont {
                    name: "roboto".to_owned(),
                    size: 38.0,
                },
                horizontal_align: TextBoxHorizontalAlign::Center,
                vertical_align: TextBoxVerticalAlign::Middle,
                ..Default::default()
            },
        )
}
