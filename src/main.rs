mod geometry;
mod protocol;
mod renderer;
mod video;
mod window;

use std::io::BufRead;
use window::App;
use winit::event_loop::EventLoop;

fn main() {
    let event_loop = EventLoop::with_user_event()
        .build()
        .expect("event loop");
    let proxy = event_loop.create_proxy();

    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() { continue; }
            match serde_json::from_str(&line) {
                Ok(cmd) => {
                    if proxy.send_event(cmd).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    eprintln!("nvim-gfx: bad command: {e}: {line}");
                }
            }
        }
    });

    let mut app = App::new();
    event_loop.run_app(&mut app).expect("event loop run");
}
