use crate::{
    context::GameContext,
    editor::{
        EditorInputCommand, EditorSubsystems,
        features::editable::{Editable, EditableEntry, EditableSubsystem, EditableWidgetLocation},
    },
};
use raui_core::{
    props::Props,
    widget::{
        component::{
            containers::{
                horizontal_box::HorizontalBoxProps, size_box::SizeBoxProps,
                vertical_box::VerticalBoxProps,
            },
            interactive::{navigation::NavItemActive, scroll_view::ScrollViewRange},
        },
        unit::{
            flex::FlexBoxItemLayout,
            size::SizeBoxSizeValue,
            text::{TextBoxHorizontalAlign, TextBoxSizeValue, TextBoxVerticalAlign},
        },
    },
};
use raui_immediate::{ImKey, ImmediateOnMount, ImmediateOnUnmount, apply, use_effects};
use raui_immediate_widgets::{
    core::containers::{horizontal_box, size_box, vertical_box},
    material::{
        containers::scroll_paper,
        interactive::{text_button_paper, text_field_paper},
        scroll_paper_side_scrollbars, text_paper,
    },
};
use raui_material::{
    component::{interactive::text_field_paper::TextFieldPaperProps, text_paper::TextPaperProps},
    theme::{ThemeColor, ThemedWidgetProps},
};
use spitfire_input::{InputConsume, InputMapping, MouseButton, VirtualAction, VirtualAxis};
use std::str::FromStr;

pub fn editable_panel(context: &mut GameContext, subsystems: &mut EditorSubsystems) {
    subsystems.ensure::<EditableSubsystem>();
    let editable = context.globals.ensure::<Editable>();
    let command1 = context.globals.editor.input.commands().clone();
    let command2 = command1.clone();
    let props = (
        ImmediateOnMount::new(move || {
            let x = editable.read().pointer_x.clone();
            let y = editable.read().pointer_y.clone();
            let trigger = editable.read().pointer_trigger.clone();
            let _ = command1.send(EditorInputCommand::AddMapping {
                name: "editable".into(),
                mapping: InputMapping::default()
                    .consume(InputConsume::All)
                    .axis(VirtualAxis::MousePositionX, x)
                    .axis(VirtualAxis::MousePositionY, y)
                    .action(VirtualAction::MouseButton(MouseButton::Left), trigger)
                    .into(),
            });
        }),
        ImmediateOnUnmount::new(move || {
            let _ = command2.send(EditorInputCommand::RemoveMapping {
                name: "editable".into(),
            });
        }),
    );

    use_effects(props, || {
        vertical_box((), || {
            let props = (
                FlexBoxItemLayout {
                    basis: Some(30.0),
                    grow: 0.0,
                    shrink: 0.0,
                    margin: 4.0.into(),
                    ..Default::default()
                },
                TextPaperProps {
                    text: "Clear editables".to_owned(),
                    variant: "button".to_owned(),
                    ..Default::default()
                },
                NavItemActive,
            );
            if text_button_paper(props).trigger_start() {
                EditableEntry::stop_editing_all();
            }

            let props = (NavItemActive, ScrollViewRange::default());
            scroll_paper(props, || {
                apply(ImKey("content"), || {
                    EditableEntry::with_registry_and_actively_editing(|registry, entries| {
                        if entries.is_empty() {
                            text_paper(TextPaperProps {
                                text: "Select some object(s) to edit.".to_owned(),
                                horizontal_align_override: Some(TextBoxHorizontalAlign::Center),
                                vertical_align_override: Some(TextBoxVerticalAlign::Middle),
                                ..Default::default()
                            });
                        } else {
                            let props = SizeBoxProps {
                                width: SizeBoxSizeValue::Fill,
                                height: SizeBoxSizeValue::Content,
                                ..Default::default()
                            };
                            size_box(props, || {
                                let props = VerticalBoxProps {
                                    override_slots_layout: Some(FlexBoxItemLayout {
                                        grow: 0.0,
                                        shrink: 0.0,
                                        margin: 2.0.into(),
                                        ..Default::default()
                                    }),
                                    ..Default::default()
                                };
                                vertical_box(props, || {
                                    for entry in entries {
                                        if let Some(editable) = registry.get_mut(entry)
                                            && editable
                                                .widget_location
                                                .contains(EditableWidgetLocation::EDITING_PANEL)
                                        {
                                            editable.draw_widget(context);
                                        }
                                    }
                                });
                            });
                        }
                    });
                });

                apply(ImKey("scrollbars"), || {
                    scroll_paper_side_scrollbars(());
                });
            });
        });
    });
}

pub fn edit_textual_property<T: ToString + FromStr + Send + Sync>(
    label: &str,
    value: &mut T,
    props: impl Into<Props>,
) {
    let size_props = SizeBoxProps {
        width: SizeBoxSizeValue::Fill,
        height: SizeBoxSizeValue::Exact(20.0),
        ..Default::default()
    };
    let text_props = Props::from((
        TextFieldPaperProps {
            hint: "> Type value here...".to_owned(),
            paper_theme: ThemedWidgetProps {
                color: ThemeColor::Primary,
                ..Default::default()
            },
            variant: "S".to_owned(),
            ..Default::default()
        },
        NavItemActive,
    ))
    .merge(props.into());

    size_box(size_props, || {
        let props = HorizontalBoxProps {
            separation: 2.0,
            ..Default::default()
        };
        horizontal_box(props, || {
            if !label.is_empty() {
                let props = (
                    FlexBoxItemLayout::no_growing_and_shrinking(),
                    TextPaperProps {
                        text: label.to_owned(),
                        width: TextBoxSizeValue::Content,
                        ..Default::default()
                    },
                );
                text_paper(props);
            }

            if let Some(v) = text_field_paper(value, text_props.clone()).0 {
                *value = v;
            }
        });
    });
}

pub fn edit_textual_property_convert<T: Clone + ToString + FromStr + Send + Sync>(
    label: &str,
    value: &mut T,
    props: impl Into<Props>,
    mut to_display: impl FnMut(T) -> T,
    mut from_display: impl FnMut(T) -> T,
) {
    let size_props = SizeBoxProps {
        width: SizeBoxSizeValue::Fill,
        height: SizeBoxSizeValue::Exact(20.0),
        ..Default::default()
    };
    let text_props = Props::from((
        TextFieldPaperProps {
            hint: "> Type value here...".to_owned(),
            paper_theme: ThemedWidgetProps {
                color: ThemeColor::Primary,
                ..Default::default()
            },
            variant: "S".to_owned(),
            ..Default::default()
        },
        NavItemActive,
    ))
    .merge(props.into());

    size_box(size_props, || {
        let props = HorizontalBoxProps {
            separation: 2.0,
            ..Default::default()
        };
        horizontal_box(props, || {
            if !label.is_empty() {
                let props = (
                    FlexBoxItemLayout::no_growing_and_shrinking(),
                    TextPaperProps {
                        text: label.to_owned(),
                        width: TextBoxSizeValue::Content,
                        ..Default::default()
                    },
                );
                text_paper(props);
            }

            let v = to_display(value.clone());
            if let Some(v) = text_field_paper(&v, text_props.clone()).0 {
                let v = from_display(v);
                *value = v;
            }
        });
    });
}
