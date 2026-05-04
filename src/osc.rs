use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use rosc::{OscMessage, OscPacket, OscType};

/// Send an OSC message via UDP.
pub fn send_osc(port: u16, address: &str, value: &OscType) -> Result<(), String> {
    let msg = OscPacket::Message(OscMessage {
        addr: address.to_string(),
        args: vec![value.clone()],
    });

    let data =
        rosc::encoder::encode(&msg).map_err(|e| format!("OSC encode failed: {e}"))?;

    let addr = format!("127.0.0.1:{port}");
    log::info!("OSC send → {addr}  [{address}]  {value:?}");

    let socket =
        UdpSocket::bind("127.0.0.1:0").map_err(|e| format!("OSC send bind failed: {e}"))?;
    socket
        .send_to(&data, &addr)
        .map_err(|e| format!("OSC send failed: {e}"))?;
    Ok(())
}

#[derive(Debug)]
pub struct OscMessageParsed {
    pub address: String,
    pub value: OscValue,
}

#[derive(Debug)]
pub enum OscValue {
    Int(i32),
    Float(f32),
    Bool(bool),
}

impl OscValue {
    fn from_osc_type(t: &OscType) -> Option<Self> {
        match t {
            OscType::Int(v) => Some(OscValue::Int(*v)),
            OscType::Float(v) => Some(OscValue::Float(*v)),
            OscType::Bool(v) => Some(OscValue::Bool(*v)),
            _ => None,
        }
    }
}

/// OSC receiver that listens for VRChat avatar parameter messages.
pub struct OscReceiver {
    running: Arc<AtomicBool>,
}

impl OscReceiver {
    pub fn start<F>(port: u16, on_message: F) -> Result<Self, String>
    where
        F: Fn(OscMessageParsed) + Send + 'static,
    {
        let addr = format!("127.0.0.1:{port}");
        let socket =
            UdpSocket::bind(&addr).map_err(|e| format!("Failed to bind OSC port {port}: {e}"))?;

        socket
            .set_read_timeout(Some(std::time::Duration::from_millis(500)))
            .map_err(|e| format!("Failed to set timeout: {e}"))?;

        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while running_clone.load(Ordering::Relaxed) {
                match socket.recv_from(&mut buf) {
                    Ok((len, _src)) => {
                        if let Ok((_rem, packet)) = rosc::decoder::decode_udp(&buf[..len]) {
                            if let OscPacket::Message(msg) = packet {
                                if let Some(arg) = msg.args.first() {
                                    if let Some(val) = OscValue::from_osc_type(arg) {
                                        on_message(OscMessageParsed {
                                            address: msg.addr,
                                            value: val,
                                        });
                                    }
                                }
                            }
                        }
                    }
                    Err(ref e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut =>
                    {
                        continue;
                    }
                    Err(e) => {
                        log::error!("OSC recv error: {e}");
                    }
                }
            }
        });

        log::info!("OSC receiver listening on 127.0.0.1:{port}");

        Ok(Self { running })
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}
