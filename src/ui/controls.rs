pub struct RecordingControls {
    pub recording: bool,
    pub elapsed: std::time::Duration,
    start_time: Option<std::time::Instant>,
}

impl RecordingControls {
    pub fn new() -> Self {
        Self {
            recording: false,
            elapsed: std::time::Duration::ZERO,
            start_time: None,
        }
    }

    pub fn update(&mut self) {
        if let Some(start) = self.start_time {
            self.elapsed = start.elapsed();
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) -> RecordingAction {
        let mut action = RecordingAction::None;

        ui.horizontal(|ui| {
            if self.recording {
                let stop_btn = egui::Button::new(
                    egui::RichText::new("⏹ Stop").color(egui::Color32::WHITE)
                )
                .fill(egui::Color32::DARK_RED)
                .min_size(egui::vec2(80.0, 32.0));

                if ui.add(stop_btn).clicked() {
                    action = RecordingAction::Stop;
                }

                // Red pulsing dot
                let time = ui.input(|i| i.time);
                let alpha = ((time * 3.0).sin() * 0.5 + 0.5) as f32;
                ui.colored_label(
                    egui::Color32::from_rgba_premultiplied(
                        255,
                        0,
                        0,
                        (alpha * 255.0) as u8,
                    ),
                    "●",
                );
                ui.label(format_elapsed(self.elapsed));
            } else {
                let rec_btn = egui::Button::new(
                    egui::RichText::new("● REC").color(egui::Color32::WHITE)
                )
                .fill(egui::Color32::DARK_RED)
                .min_size(egui::vec2(80.0, 32.0));

                if ui.add(rec_btn).clicked() {
                    action = RecordingAction::Start;
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.recording {
                    if let Some(start) = self.start_time {
                        let total = start.elapsed();
                        ui.label(format_elapsed(total));
                    }
                }
            });
        });

        action
    }

    pub fn on_start(&mut self) {
        self.recording = true;
        self.start_time = Some(std::time::Instant::now());
    }

    pub fn on_stop(&mut self) {
        self.recording = false;
        self.start_time = None;
        self.elapsed = std::time::Duration::ZERO;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingAction {
    None,
    Start,
    Stop,
}

fn format_elapsed(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}
