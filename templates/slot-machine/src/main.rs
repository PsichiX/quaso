#![cfg(not(target_arch = "wasm32"))]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod game;

fn main() {
    #[cfg(debug_assertions)]
    {
        use quaso::third_party::tracing_subscriber::{
            Layer,
            filter::LevelFilter,
            fmt::{format::FmtSpan, layer},
            layer::SubscriberExt,
            registry,
            util::SubscriberInitExt,
        };

        registry()
            .with(
                layer()
                    .with_span_events(FmtSpan::ENTER | FmtSpan::CLOSE)
                    .with_writer(std::io::stdout)
                    .with_filter(LevelFilter::INFO),
            )
            .init();
    }

    game::main();
}
