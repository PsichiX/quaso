#![cfg(target_arch = "wasm32")]

pub mod game;

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
fn main() {
    #[cfg(debug_assertions)]
    {
        use quaso::third_party::{
            tracing_subscriber::{
                Layer,
                filter::LevelFilter,
                fmt::{
                    format::{FmtSpan, Pretty},
                    layer,
                },
                layer::SubscriberExt,
                registry,
                util::SubscriberInitExt,
            },
            tracing_web::{MakeWebConsoleWriter, performance_layer},
        };

        registry()
            .with(
                layer()
                    .without_time()
                    .with_span_events(FmtSpan::ENTER | FmtSpan::CLOSE)
                    .with_writer(MakeWebConsoleWriter::new())
                    .with_filter(LevelFilter::INFO),
            )
            .with(performance_layer().with_details_from_fields(Pretty::default()))
            .init();
    }

    console_error_panic_hook::set_once();
    game::main();
}
