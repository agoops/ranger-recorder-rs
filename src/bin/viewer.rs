use eframe::egui;
use chrono::{NaiveDateTime, Local, TimeZone};
use std::path::PathBuf;
use walkdir::WalkDir;
use rodio::{Decoder, OutputStream, Sink};
use std::fs::File;
use std::io::BufReader;

#[derive(Clone)]
struct Recording {
    timestamp: chrono::DateTime<Local>,
    path: PathBuf,
}

struct BarkViewer {
    recordings: Vec<Recording>,
    timeline_start: chrono::DateTime<Local>,
    timeline_end: chrono::DateTime<Local>,
    current_playback: Option<Sink>,
}

impl BarkViewer {
    fn new() -> Self {
        let mut recordings = Vec::new();
        
        // Scan the barks directory
        for entry in WalkDir::new("barks")
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "wav"))
        {
            if let Some(filename) = entry.path().file_name().and_then(|f| f.to_str()) {
                if filename.starts_with("bark_") {
                    // Parse timestamp from filename (format: bark_YYYYMMDD_H_MM_SS_pm.wav)
                    let timestamp_str = filename.strip_prefix("bark_").unwrap()
                        .strip_suffix(".wav").unwrap();
                    if let Ok(timestamp) = NaiveDateTime::parse_from_str(
                        timestamp_str,
                        "%Y%m%d_%I_%M_%S_%P"
                    ) {
                        recordings.push(Recording {
                            timestamp: Local.from_local_datetime(&timestamp).unwrap(),
                            path: entry.path().to_owned(),
                        });
                    }
                }
            }
        }

        // Sort recordings by timestamp
        recordings.sort_by_key(|r| r.timestamp);

        // Set timeline range
        let timeline_start = recordings.first()
            .map(|r| r.timestamp - chrono::Duration::minutes(5))
            .unwrap_or_else(|| Local::now() - chrono::Duration::hours(24));
        let timeline_end = Local::now();

        Self {
            recordings,
            timeline_start,
            timeline_end,
            current_playback: None,
        }
    }

    fn play_audio(&mut self, path: &PathBuf) {
        // Stop any existing playback
        if let Some(sink) = &self.current_playback {
            sink.stop();
        }

        // Set up audio playback
        if let Ok((stream, stream_handle)) = OutputStream::try_default() {
            if let Ok(file) = File::open(path) {
                let buf_reader = BufReader::new(file);
                if let Ok(source) = Decoder::new(buf_reader) {
                    let sink = Sink::try_new(&stream_handle).unwrap();
                    sink.append(source);
                    self.current_playback = Some(sink);
                    
                    // Keep stream alive
                    std::mem::forget(stream);
                }
            }
        }
    }
}

impl eframe::App for BarkViewer {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Bark Timeline");
            
            // Timeline controls
            ui.horizontal(|ui| {
                if ui.button("Reset View").clicked() {
                    self.timeline_start = self.recordings.first()
                        .map(|r| r.timestamp - chrono::Duration::minutes(5))
                        .unwrap_or_else(|| Local::now() - chrono::Duration::hours(24));
                    self.timeline_end = Local::now();
                }
                if ui.button("Last Hour").clicked() {
                    let now = Local::now();
                    self.timeline_end = now;
                    self.timeline_start = now - chrono::Duration::hours(1);
                }
                if ui.button("Last 24 Hours").clicked() {
                    let now = Local::now();
                    self.timeline_end = now;
                    self.timeline_start = now - chrono::Duration::hours(24);
                }
                if ui.button("Last Week").clicked() {
                    let now = Local::now();
                    self.timeline_end = now;
                    self.timeline_start = now - chrono::Duration::days(7);
                }
            });

            // Timeline visualization
            let timeline_height = 100.0;
            let available_width = ui.available_width();
            
            let (response, painter) = ui.allocate_painter(
                egui::vec2(available_width, timeline_height),
                egui::Sense::hover(),
            );

            let rect = response.rect;
            
            // Draw timeline background
            painter.rect_filled(rect, 0.0, egui::Color32::from_gray(32));

            // Draw time markers every 15 minutes
            let duration = self.timeline_end.signed_duration_since(self.timeline_start);
            let minutes = duration.num_minutes();
            let intervals = minutes / 15;
            
            for interval in 0..=intervals {
                let x = rect.left() + (interval as f32 / intervals as f32) * rect.width();
                painter.line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    egui::Stroke::new(1.0, egui::Color32::from_gray(64)),
                );
                
                // Add time label
                let time = self.timeline_start + chrono::Duration::minutes(interval * 15);
                let time_str = time.format("%I:%M %p").to_string();
                painter.text(
                    egui::pos2(x, rect.bottom() - 15.0),
                    egui::Align2::CENTER_CENTER,
                    time_str,
                    egui::FontId::default(),
                    egui::Color32::from_gray(200),
                );
            }

            // Draw recordings as markers
            for recording in &self.recordings {
                if recording.timestamp >= self.timeline_start && recording.timestamp <= self.timeline_end {
                    let progress = recording.timestamp.signed_duration_since(self.timeline_start).num_seconds() as f32
                        / duration.num_seconds() as f32;
                    let x = rect.left() + progress * rect.width();
                    
                    // Draw bark marker
                    painter.circle_filled(
                        egui::pos2(x, rect.center().y),
                        5.0,
                        egui::Color32::from_rgb(255, 128, 0),
                    );
                }
            }

            // Show recording list
            ui.heading("Recordings");
            let recordings_ui = self.recordings.clone();
            for recording in &recordings_ui {
                let path = recording.path.clone();
                ui.horizontal(|ui| {
                    ui.label(recording.timestamp.format("%Y-%m-%d %I:%M:%S %p").to_string());
                    if ui.button("Play").clicked() {
                        self.play_audio(&path);
                    }
                    if let Some(sink) = &self.current_playback {
                        if ui.button("Stop").clicked() {
                            sink.stop();
                            self.current_playback = None;
                        }
                    }
                });
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0]),
        ..Default::default()
    };
    
    eframe::run_native(
        "Bark Viewer",
        native_options,
        Box::new(|_cc| Box::new(BarkViewer::new())),
    )
} 