use crate::game::utils::events::{Event, Events};
use quaso::third_party::{
    raui_core::{
        props::Props,
        widget::{
            component::interactive::navigation::NavItemActive, unit::content::ContentBoxItemLayout,
        },
    },
    raui_immediate_widgets::{
        core::interactive::ImmediateButton,
        material::{interactive::button_paper, text_paper},
    },
    raui_material::component::text_paper::TextPaperProps,
};

pub fn text_button(props: impl Into<Props>, message: impl ToString) -> ImmediateButton {
    let result = button_paper(props.into().with(NavItemActive), |_| {
        text_paper((
            ContentBoxItemLayout {
                margin: 10.0.into(),
                ..Default::default()
            },
            TextPaperProps {
                text: message.to_string(),
                variant: "button".to_owned(),
                color_override: Some(Default::default()),
                ..Default::default()
            },
        ));
    });
    if result.select_start() {
        Events::write(Event::PlaySound("button/select".into()));
    }
    if result.trigger_start() {
        Events::write(Event::PlaySound("button/click".into()));
    }
    result
}
