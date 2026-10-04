pub mod split;

use crate::{
    context::GameContext,
    editor::{
        EditorSubsystems,
        features::{editable::ui::editable_panel, viewport::editor_viewport_game_world},
        ui::split::{SplitSideSize, split_horizontal},
    },
};

pub fn editor_game_edit(context: &mut GameContext, subsystems: &mut EditorSubsystems) {
    split_horizontal(
        SplitSideSize::Fixed(250.0),
        SplitSideSize::default(),
        || {
            editable_panel(context, subsystems);
            editor_viewport_game_world(context, subsystems);
        },
    );
}
