mod app;
mod config;
mod error;
mod osc;
mod record;
mod render;
mod spout;
mod ui;

use app::App;
use winit::event_loop::EventLoop;

fn main() {
    env_logger::init();

    let config = config::AppConfig::load();

    let event_loop = EventLoop::new().expect("Failed to create event loop");

    let mut app = App::new(config);

    let _ = event_loop.run_app(&mut app);
}
