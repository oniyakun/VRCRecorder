/// Tracks the sender list scan timer and selection state.
pub struct SenderList {
    scan_timer: f64,
    selected: Option<usize>,
}

impl SenderList {
    pub fn new() -> Self {
        Self {
            scan_timer: 0.0,
            selected: None,
        }
    }

    /// Call every frame; tracks time for periodic rescans.
    /// The actual sender enumeration happens in app.rs via `spout::list_senders()`.
    pub fn update(&mut self, dt: f64) {
        self.scan_timer += dt;
    }

    /// Time since last scan, for rate-limiting sender discovery.
    pub fn should_scan(&self) -> bool {
        self.scan_timer > 2.0
    }

    /// Reset the scan timer after a full rescan.
    pub fn reset_scan(&mut self) {
        self.scan_timer = 0.0;
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    pub fn select(&mut self, index: usize) {
        self.selected = Some(index);
    }
}
