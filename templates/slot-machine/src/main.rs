#![cfg(not(target_arch = "wasm32"))]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod game;

fn main() {
    game::main();
}
