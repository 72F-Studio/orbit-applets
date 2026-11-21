// SPDX-License-Identifier: GPL-3.0-only

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> cosmic::iced::Result {
    tracing_subscriber::fmt::init();
    let _ = tracing_log::LogTracer::init();

    tracing::info!("Starting Orbit dock corners applet with version {VERSION}");

    orbit_applet_corners::run()
}
