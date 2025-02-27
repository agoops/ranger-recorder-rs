use eframe::egui;
use chrono::{NaiveDateTime, Local, TimeZone};
use std::path::PathBuf;
use walkdir::WalkDir;
use rodio::{Decoder, OutputStream, Sink};
use std::fs::File;
use std::io::BufReader;
use hound;

#[derive(Clone)]
struct Recording {
    timestamp: chrono::DateTime<Local>,
    path: PathBuf,
    duration: f32,  // duration in seconds
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
                    if let Ok(reader) = hound::WavReader::open(entry.path()) {
                        let spec = reader.spec();
                        let duration = reader.duration() as f32 / spec.sample_rate as f32;
                        
                        if let Ok(timestamp) = NaiveDateTime::parse_from_str(
                            filename.strip_prefix("bark_").unwrap().strip_suffix(".wav").unwrap(),
                            "%Y%m%d_%I_%M_%S_%P"
                        ) {
                            recordings.push(Recording {
                                timestamp: Local.from_local_datetime(&timestamp).unwrap(),
                                path: entry.path().to_owned(),
                                duration,
                            });
                        }
                    }
                }
            }
        }

        // Sort recordings by timestamp
        recordings.sort_by_key(|r| r.timestamp);

        // Set timeline range to start at beginning of current day
        let now = Local::now();
        let today_start = now.date().and_hms_opt(0, 0, 0).unwrap();
        
        // Find first recording of today
        let timeline_start = recordings.iter()
            .find(|r| r.timestamp.date() == now.date())
            .map(|r| r.timestamp - chrono::Duration::minutes(20))
            .unwrap_or(today_start);
        let timeline_end = now;

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

            // Show recording list grouped by day
            ui.heading("Recordings");
            let mut recordings_ui = self.recordings.clone();
            recordings_ui.reverse(); // Reverse the order to show newest first
            
            // Group recordings by day
            let mut current_day: Option<chrono::Date<Local>> = None;
            for recording in &recordings_ui {
                let recording_day = recording.timestamp.date();
                
                // Add day header when we encounter a new day
                if current_day != Some(recording_day) {
                    current_day = Some(recording_day);
                    ui.heading(recording_day.format("%A, %B %d, %Y").to_string());
                }

                let path = recording.path.clone();
                ui.horizontal(|ui| {
                    ui.label(format!("{} ({:.1}s)", 
                        recording.timestamp.format("%I:%M:%S %p"),
                        recording.duration
                    ));
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