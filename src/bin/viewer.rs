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
    scroll_delta: f32,  // Add scroll tracking
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
            scroll_delta: 0.0,
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

            // Add side-by-side layout for timeline and zoom slider
            ui.horizontal(|ui| {
                // Timeline area (taking most of the space)
                ui.vertical(|ui| {
                    let timeline_height = 100.0;
                    let available_width = ui.available_width() - 30.0; // Reserve space for slider
                    
                    let (response, painter) = ui.allocate_painter(
                        egui::vec2(available_width, timeline_height),
                        egui::Sense::click_and_drag(),
                    );

                    let rect = response.rect;
                    
                    // Handle scrolling and zooming
                    if response.hovered() {
                        // Zoom with Ctrl + Scroll
                        if ctx.input(|i| i.modifiers.ctrl) {
                            let scroll_delta = ctx.input(|i| i.raw_scroll_delta.y);
                            if scroll_delta != 0.0 {
                                let zoom_center = response.hover_pos().unwrap().x / rect.width();
                                let zoom_factor = if scroll_delta > 0.0 { 1.25 } else { 0.8 };
                                
                                let center_time = self.timeline_start + chrono::Duration::seconds(
                                    (self.timeline_end.timestamp() - self.timeline_start.timestamp()) as i64 * zoom_center as i64 / 100
                                );
                                
                                let new_duration = chrono::Duration::seconds(
                                    ((self.timeline_end.timestamp() - self.timeline_start.timestamp()) as f32 * zoom_factor) as i64
                                );
                                
                                self.timeline_start = center_time - (new_duration / 2);
                                self.timeline_end = center_time + (new_duration / 2);
                            }
                        } else {
                            // Pan with scroll or drag
                            let scroll_delta = ctx.input(|i| i.raw_scroll_delta.x);
                            let drag_delta = response.drag_delta().x;
                            let total_delta = scroll_delta + drag_delta;
                            
                            if total_delta != 0.0 {
                                let time_width = self.timeline_end.timestamp() - self.timeline_start.timestamp();
                                let delta_time = (total_delta / rect.width()) * time_width as f32;
                                let duration = chrono::Duration::seconds(-delta_time as i64);
                                self.timeline_start += duration;
                                self.timeline_end += duration;
                            }
                        }
                    }

                    // Draw timeline background
                    painter.rect_filled(rect, 0.0, egui::Color32::from_gray(32));

                    // Calculate appropriate time interval based on duration
                    let duration_mins = (self.timeline_end.timestamp() - self.timeline_start.timestamp()) as f64 / 60.0;
                    let interval_mins = if duration_mins <= 15.0 { 1 }
                        else if duration_mins <= 60.0 { 5 }
                        else if duration_mins <= 180.0 { 15 }
                        else if duration_mins <= 720.0 { 30 }
                        else { 60 };

                    // Draw time markers with adaptive intervals
                    let start_mins = self.timeline_start.timestamp() / 60;
                    let end_mins = self.timeline_end.timestamp() / 60;
                    let total_mins = end_mins - start_mins;
                    
                    for mins in (start_mins..=end_mins).step_by(interval_mins as usize) {
                        let progress = (mins - start_mins) as f32 / total_mins as f32;
                        let x = rect.left() + progress * rect.width();
                        
                        painter.line_segment(
                            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                            egui::Stroke::new(1.0, egui::Color32::from_gray(64)),
                        );
                        
                        let time = Local.timestamp_opt(mins * 60, 0).unwrap();
                        let time_str = time.format("%I:%M %p").to_string();
                        painter.text(
                            egui::pos2(x, rect.bottom() - 15.0),
                            egui::Align2::CENTER_CENTER,
                            time_str,
                            egui::FontId::default(),
                            egui::Color32::from_gray(200),
                        );
                    }

                    // Draw recordings
                    for recording in &self.recordings {
                        if recording.timestamp >= self.timeline_start && recording.timestamp <= self.timeline_end {
                            let progress = (recording.timestamp.timestamp() - self.timeline_start.timestamp()) as f32
                                / (self.timeline_end.timestamp() - self.timeline_start.timestamp()) as f32;
                            let x = rect.left() + progress * rect.width();
                            
                            painter.circle_filled(
                                egui::pos2(x, rect.center().y),
                                5.0,
                                egui::Color32::from_rgb(255, 128, 0),
                            );
                        }
                    }
                });

                // Vertical zoom slider
                ui.vertical(|ui| {
                    let duration = self.timeline_end.timestamp() - self.timeline_start.timestamp();
                    let mut zoom_value = (duration as f32 / 3600.0).log10(); // Convert to log scale
                    
                    if ui.add(egui::Slider::new(&mut zoom_value, -1.0..=2.0)
                        .orientation(egui::SliderOrientation::Vertical)
                        .text("Zoom"))
                        .changed() 
                    {
                        let new_duration = (10.0f32.powf(zoom_value) * 3600.0) as i64;
                        let center_time = self.timeline_start.timestamp() + (duration / 2);
                        let half_new_duration = new_duration / 2;
                        
                        self.timeline_start = Local.timestamp_opt(center_time - half_new_duration, 0).unwrap();
                        self.timeline_end = Local.timestamp_opt(center_time + half_new_duration, 0).unwrap();
                    }
                });
            });

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