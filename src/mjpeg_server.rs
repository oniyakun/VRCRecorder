use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

/// Shared state between the render thread and the MJPEG server.
pub struct MjpegState {
    pub jpeg_data: Arc<Mutex<Vec<u8>>>,
    /// Monotonically increasing frame sequence number.
    /// The render thread increments this each time a new JPEG is written.
    /// The MJPEG server uses it to detect new frames (instead of jpeg.len(),
    /// which can collide when consecutive frames compress to the same size).
    pub frame_seq: Arc<AtomicU64>,
}

/// Start a minimal MJPEG-over-HTTP server on a random available port.
/// Returns the port number. Spawns a background thread that stays alive
/// for the process lifetime.
pub fn start(state: MjpegState) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind MJPEG server");
    let port = listener.local_addr().unwrap().port();

    thread::Builder::new()
        .name("mjpeg-server".into())
        .spawn(move || {
            serve(listener, state);
        })
        .expect("Failed to spawn MJPEG server thread");

    log::info!("MJPEG server listening on port {port}");
    port
}

fn serve(listener: TcpListener, state: MjpegState) {
    for stream in listener.incoming() {
        match stream {
            Ok(client) => {
                let state = MjpegState {
                    jpeg_data: state.jpeg_data.clone(),
                    frame_seq: state.frame_seq.clone(),
                };
                thread::Builder::new()
                    .name("mjpeg-client".into())
                    .spawn(move || {
                        handle_client(client, &state);
                    })
                    .ok();
            }
            Err(e) => {
                log::error!("MJPEG accept error: {e}");
            }
        }
    }
}

fn handle_client(mut client: TcpStream, state: &MjpegState) {
    // Write HTTP header with multipart MJPEG
    let header = b"HTTP/1.1 200 OK\r\n\
        Content-Type: multipart/x-mixed-replace; boundary=FRAME\r\n\
        Cache-Control: no-cache, no-store, must-revalidate\r\n\
        Connection: close\r\n\
        \r\n";

    if client.write_all(header).is_err() {
        return;
    }

    // Stream frames at source framerate.
    // Use a frame sequence counter instead of jpeg.len() to detect new
    // frames — consecutive frames can compress to identical sizes, which
    // would cause the preview to stall on unchanged scene regions.
    let mut last_seq = 0u64;
    loop {
        let seq = state.frame_seq.load(Ordering::Relaxed);
        if seq == last_seq {
            thread::sleep(std::time::Duration::from_millis(1));
            continue;
        }
        last_seq = seq;

        let jpeg = {
            let buf = state.jpeg_data.lock().unwrap();
            buf.clone()
        };

        if jpeg.is_empty() {
            thread::sleep(std::time::Duration::from_millis(10));
            continue;
        }

        let part = format!(
            "--FRAME\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
            jpeg.len()
        );

        if client.write_all(part.as_bytes()).is_err() {
            return;
        }
        if client.write_all(&jpeg).is_err() {
            return;
        }
        if client.write_all(b"\r\n").is_err() {
            return;
        }
    }
}
