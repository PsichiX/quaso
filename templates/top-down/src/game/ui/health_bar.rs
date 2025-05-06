use quaso::third_party::{
    raui_core::{
        props::Props,
        widget::{
            component::image_box::ImageBoxProps,
            unit::{
                content::ContentBoxItemLayout,
                image::{ImageBoxFrame, ImageBoxImage, ImageBoxImageScaling, ImageBoxMaterial},
            },
            utils::{Color, Rect},
        },
    },
    raui_immediate_widgets::core::{containers::content_box, image_box},
};

pub fn health_bar(props: impl Into<Props>, value: usize) {
    let percentage = (value as f32 / 100.0).clamp(0.0, 1.0);

    content_box(props, || {
        image_box(ImageBoxProps {
            material: ImageBoxMaterial::Image(ImageBoxImage {
                id: "ui/bar".to_owned(),
                scaling: ImageBoxImageScaling::Frame(ImageBoxFrame {
                    source: Rect {
                        left: 3.0 / 24.0,
                        right: 3.0 / 24.0,
                        top: 3.0 / 8.0,
                        bottom: 3.0 / 8.0,
                    },
                    destination: 6.0.into(),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        });

        image_box((
            ContentBoxItemLayout {
                anchors: Rect {
                    left: 0.0,
                    right: percentage,
                    top: 0.0,
                    bottom: 1.0,
                },
                margin: 6.0.into(),
                ..Default::default()
            },
            ImageBoxProps::colored(Color {
                r: 0.7,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            }),
        ));
    });
}
