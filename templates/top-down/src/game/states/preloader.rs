use quaso::{
    assets::shader::ShaderAsset,
    context::GameContext,
    game::{GameState, GameStateChange},
    third_party::{
        spitfire_glow::graphics::Shader,
        spitfire_gui::interactions::GuiInteractionsInputs,
        spitfire_input::{
            ArrayInputCombinator, InputActionRef, InputAxisRef, InputConsume, InputMapping,
            VirtualAction, VirtualAxis,
        },
        windowing::event::MouseButton,
    },
};

use crate::game::states::main_menu::MainMenu;

macro_rules! load_texture_series {
    (
        $context:expr,
        $id:literal,
        [$($index:literal),+]
    ) => {
        $(
            $context
                .assets
                .ensure(concat!(
                    "texture://images/",
                    $id,
                    "-",
                    $index,
                    ".png?as=",
                    $id,
                    "/",
                    $index,
                ))
                .unwrap();
        )+
    };
}

pub struct Preloader;

impl GameState for Preloader {
    fn enter(&mut self, mut context: GameContext) {
        Self::load_shaders(&mut context);
        Self::load_fonts(&mut context);
        Self::load_textures(&mut context);
        Self::load_sounds_and_music(&mut context);
        Self::setup_gui_inputs(&mut context);

        *context.state_change = GameStateChange::Swap(Box::new(MainMenu));
    }
}

impl Preloader {
    fn load_shaders(context: &mut GameContext) {
        context
            .assets
            .spawn(
                "shader://color",
                (ShaderAsset::new(
                    Shader::COLORED_VERTEX_2D,
                    Shader::PASS_FRAGMENT,
                ),),
            )
            .unwrap();
        context
            .assets
            .spawn(
                "shader://image",
                (ShaderAsset::new(
                    Shader::TEXTURED_VERTEX_2D,
                    Shader::TEXTURED_FRAGMENT,
                ),),
            )
            .unwrap();
        context
            .assets
            .spawn(
                "shader://text",
                (ShaderAsset::new(Shader::TEXT_VERTEX, Shader::TEXT_FRAGMENT),),
            )
            .unwrap();
        context
            .assets
            .ensure("shader://shaders/character.glsl?as=character")
            .unwrap();
        context
            .assets
            .ensure("shader://shaders/sphere_light.glsl?as=sphere-light")
            .unwrap();
        context
            .assets
            .ensure("shader://shaders/lighting.glsl?as=lighting")
            .unwrap();
    }

    fn load_fonts(context: &mut GameContext) {
        context
            .assets
            .ensure("font://fonts/roboto.ttf?as=roboto")
            .unwrap();
    }

    fn load_textures(context: &mut GameContext) {
        // map
        context
            .assets
            .ensure("texture://maps/world/simplified/Level_0/_composite.png?as=map/level-0")
            .unwrap();

        // // player character
        load_texture_series!(context, "player/idle", [1]);
        load_texture_series!(
            context,
            "player/run",
            [
                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
                24
            ]
        );
        load_texture_series!(context, "player/axe", [1, 2, 3, 4, 5, 6, 7, 8]);
        load_texture_series!(context, "player/sword", [1, 2, 3, 4, 5, 6, 7]);

        // enemy character
        load_texture_series!(context, "enemy/idle", [1, 2, 3, 4, 5]);
        load_texture_series!(context, "enemy/run", [1, 2, 3, 4, 5, 6, 7, 8]);
        load_texture_series!(context, "enemy/attack", [1, 2, 3, 4, 5, 6, 7, 8]);

        // items
        context
            .assets
            .ensure("texture://images/item/apple.png?as=item/apple")
            .unwrap();
        context
            .assets
            .ensure("texture://images/item/banana.png?as=item/banana")
            .unwrap();
        context
            .assets
            .ensure("texture://images/item/orange.png?as=item/orange")
            .unwrap();
        context
            .assets
            .ensure("texture://images/item/torch.png?as=item/torch")
            .unwrap();

        // particles
        context
            .assets
            .ensure("texture://images/particles/fire.png?as=particle/fire")
            .unwrap();

        // ui
        context
            .assets
            .ensure("texture://images/ui/panel.png?as=ui/panel")
            .unwrap();
        context
            .assets
            .ensure("texture://images/ui/bar.png?as=ui/bar")
            .unwrap();
        context
            .assets
            .ensure("texture://images/ui/button-idle.png?as=ui/button/idle")
            .unwrap();
        context
            .assets
            .ensure("texture://images/ui/button-select.png?as=ui/button/select")
            .unwrap();
        context
            .assets
            .ensure("texture://images/ui/button-trigger.png?as=ui/button/trigger")
            .unwrap();
        context
            .assets
            .ensure("texture://images/ui/cover.png?as=ui/cover")
            .unwrap();
        context
            .assets
            .ensure("texture://images/ui/won.png?as=ui/won")
            .unwrap();
        context
            .assets
            .ensure("texture://images/ui/lost.png?as=ui/lost")
            .unwrap();
    }

    fn load_sounds_and_music(context: &mut GameContext) {
        context
            .assets
            .ensure("sound://sounds/footstep-grass-1.ogg?as=footstep/grass/1")
            .unwrap();
        context
            .assets
            .ensure("sound://sounds/footstep-grass-2.ogg?as=footstep/grass/2")
            .unwrap();
        context
            .assets
            .ensure("sound://sounds/footstep-grass-3.ogg?as=footstep/grass/3")
            .unwrap();
        context
            .assets
            .ensure("sound://sounds/sword.ogg?as=sword")
            .unwrap();
        context
            .assets
            .ensure("sound://sounds/axe.ogg?as=axe")
            .unwrap();
        context
            .assets
            .ensure("sound://sounds/collect.ogg?as=collect")
            .unwrap();
        context
            .assets
            .ensure("sound://sounds/button-select.ogg?as=button/select")
            .unwrap();
        context
            .assets
            .ensure("sound://sounds/button-click.ogg?as=button/click")
            .unwrap();

        context
            .assets
            .ensure("sound://music/forest.ogg?as=forest")
            .unwrap();
        context
            .assets
            .ensure("sound://music/battle.ogg?as=battle")
            .unwrap();
    }

    fn setup_gui_inputs(context: &mut GameContext) {
        context
            .gui
            .interactions
            .engine
            .deselect_when_no_button_found = true;

        let pointer_x = InputAxisRef::default();
        let pointer_y = InputAxisRef::default();
        let pointer_trigger = InputActionRef::default();

        context.gui.interactions.inputs = GuiInteractionsInputs {
            pointer_position: ArrayInputCombinator::new([pointer_x.clone(), pointer_y.clone()]),
            pointer_trigger: pointer_trigger.clone(),
            ..Default::default()
        };

        context.input.push_mapping(
            InputMapping::default()
                .consume(InputConsume::Hit)
                .axis(VirtualAxis::MousePositionX, pointer_x)
                .axis(VirtualAxis::MousePositionY, pointer_y)
                .action(
                    VirtualAction::MouseButton(MouseButton::Left),
                    pointer_trigger,
                ),
        );
    }
}
