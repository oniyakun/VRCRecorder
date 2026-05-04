use spout_rs::{Directory, SpoutReceiver, SenderInfo};

/// Wraps a SpoutReceiver with connection lifecycle management.
pub struct SpoutStream {
    receiver: Option<SpoutReceiver>,
    sender_name: String,
    pub info: Option<SenderInfo>,
    pub connected: bool,
    disconnect_counter: u32,
}

impl SpoutStream {
    pub fn new(sender_name: Option<&str>) -> Self {
        let name = sender_name.unwrap_or("").to_string();
        let mut stream = Self {
            receiver: None,
            sender_name: name.clone(),
            info: None,
            connected: false,
            disconnect_counter: 0,
        };
        stream.connect(&name);
        stream
    }

    fn connect(&mut self, name: &str) {
        let mut receiver = if name.is_empty() {
            SpoutReceiver::new(None)
        } else {
            SpoutReceiver::new(Some(name))
        };

        // SpoutReceiver::new() only sets the name — we must call connect()
        // to actually open the inter-process link to the sender.
        receiver.connect();

        self.connected = receiver.is_connected();
        if self.connected {
            self.info = Some(receiver.sender_info());
            log::info!("Connected to Spout sender: {name}");
        } else {
            log::warn!("Failed to connect to Spout sender: {name}");
        }
        self.receiver = Some(receiver);
    }

    pub fn switch_sender(&mut self, name: &str) {
        self.sender_name = name.to_string();
        self.disconnect_counter = 0;
        self.connect(name);
    }

    /// Call every frame to maintain connection state.
    pub fn update(&mut self) {
        let was_connected = self.connected;

        if let Some(ref mut receiver) = self.receiver {
            self.connected = receiver.is_connected();
            if self.connected {
                self.info = Some(receiver.sender_info());
            }
        } else {
            self.connected = false;
        }

        if !self.connected && was_connected {
            log::warn!("Sender '{}' disconnected", self.sender_name);
        }

        if !self.connected {
            self.disconnect_counter += 1;
            let interval = match self.disconnect_counter {
                n if n < 60 => 60,
                n if n < 300 => 120,
                _ => 300,
            };
            if self.disconnect_counter % interval == 0 {
                log::info!("Attempting reconnect to '{}'...", self.sender_name);
                self.connect(&self.sender_name.clone());
            }
        }
    }

    /// Receive the latest frame into the given OpenGL texture.
    /// Call `connect()` first to establish the link.
    pub fn receive_texture(&mut self, tex_id: u32) -> bool {
        if let Some(ref mut receiver) = self.receiver {
            if self.connected {
                return receiver.receive_texture(tex_id, glow::TEXTURE_2D, false);
            }
        }
        false
    }
}

/// Get the list of currently active Spout sender names.
pub fn list_senders() -> Vec<String> {
    Directory::senders()
}
