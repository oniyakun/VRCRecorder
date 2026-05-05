use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

/// Shared state between the render thread and the MJPEG server.
pub struct MjpegState {
    pub jpeg_data: Arc<Mutex<Vec<u8>>>,
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

    // Stream frames at source framerate (track changes to avoid duplicates)
    let mut last_len = 0usize;
    loop {
        let jpeg = {
            let buf = state.jpeg_data.lock().unwrap();
            buf.clone()
        };

        if jpeg.is_empty() {
            thread::sleep(std::time::Duration::from_millis(10));
            continue;
        }

        // Only send if frame data changed (avoids redundant sends)
        if jpeg.len() == last_len {
            thread::sleep(std::time::Duration::from_millis(1));
            continue;
        }
        last_len = jpeg.len();

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
