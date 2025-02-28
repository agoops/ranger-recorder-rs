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
    audio_stats: Option<(f32, f32, f32, f32, f32)>, // min, q1, median, q3, max
    waveform: Vec<f32>,
}

struct BarkViewer {
    recordings: Vec<Recording>,
    timeline_start: chrono::DateTime<Local>,
    timeline_end: chrono::DateTime<Local>,
    current_playback: Option<Sink>,
    scroll_delta: f32,  // Add scroll tracking
    hovered_timestamp: Option<chrono::DateTime<Local>>,  // Add this field
}

impl Recording {
    fn analyze_audio(&self) -> Option<(f32, f32, f32, f32, f32)> { // min, 25%, median, 75%, max
        if let Ok(reader) = hound::WavReader::open(&self.path) {
            let samples: Vec<f32> = reader.into_samples()
                .filter_map(|s| s.ok())
                .map(|s: i16| s as f32 / i16::MAX as f32)
                .map(|s| s.abs())
                .collect();
            
            if !samples.is_empty() {
                let mut sorted = samples.clone();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
                
                let len = sorted.len();
                let min = sorted[0];
                let q1 = sorted[len / 4];
                let median = sorted[len / 2];
                let q3 = sorted[3 * len / 4];
                let max = sorted[len - 1];
                
                return Some((min, q1, median, q3, max));
            }
        }
        None
    }
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
                        
                        // Analyze audio data during loading
                        let audio_stats = {
                            let samples: Vec<f32> = reader.into_samples()
                                .filter_map(|s| s.ok())
                                .map(|s: i16| s as f32 / i16::MAX as f32)
                                .map(|s| s.abs())
                                .collect();
                            
                            if !samples.is_empty() {
                                let mut sorted = samples;
                                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
                                
                                let len = sorted.len();
                                let min = sorted[0];
                                let q1 = sorted[len / 4];
                                let median = sorted[len / 2];
                                let q3 = sorted[3 * len / 4];
                                let max = sorted[len - 1];
                                
                                Some((min, q1, median, q3, max))
                            } else {
                                None
                            }
                        };
                        
                        if let Ok(timestamp) = NaiveDateTime::parse_from_str(
                            filename.strip_prefix("bark_").unwrap().strip_suffix(".wav").unwrap(),
                            "%Y%m%d_%I_%M_%S_%P"
                        ) {
                            recordings.push(Recording {
                                timestamp: Local.from_local_datetime(&timestamp).unwrap(),
                                path: entry.path().to_owned(),
                                duration,
                                audio_stats,
                                waveform: Vec::new(),
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
        let today_start = Local.from_local_datetime(
            &now.date_naive().and_hms_opt(0, 0, 0).unwrap()
        ).unwrap();
        
        // Find first recording of today
        let timeline_start = recordings.iter()
            .find(|r| r.timestamp.date_naive() == now.date_naive())
            .map(|r| r.timestamp - chrono::Duration::minutes(20))
            .unwrap_or(today_start);
        let timeline_end = now;

        Self {
            recordings,
            timeline_start,
            timeline_end,
            current_playback: None,
            scroll_delta: 0.0,
            hovered_timestamp: None,  // Initialize new field
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

                    // Draw timeline background and y-axis
                    painter.rect_filled(rect, 0.0, egui::Color32::from_gray(32));

                    // Find the maximum value among visible recordings
                    let max_visible_value = self.recordings.iter()
                        .filter(|r| r.timestamp >= self.timeline_start && r.timestamp <= self.timeline_end)
                        .filter_map(|r| r.audio_stats)
                        .map(|(_, _, _, _, max)| max)
                        .fold(0.0f32, f32::max);

                    // Scale to make the largest value take up 80% of the height
                    let scale_factor = if max_visible_value > 0.0 {
                        0.8 / max_visible_value
                    } else {
                        1.0
                    };

                    // Draw y-axis with percentage markers
                    let y_axis_width = 40.0;
                    let plot_rect = rect.shrink2(egui::vec2(y_axis_width, 0.0));
                    
                    // Draw y-axis line
                    painter.line_segment(
                        [
                            egui::pos2(rect.left() + y_axis_width, rect.top()),
                            egui::pos2(rect.left() + y_axis_width, rect.bottom())
                        ],
                        egui::Stroke::new(1.0, egui::Color32::from_gray(128)),
                    );

                    // Draw percentage markers with adjusted scale
                    for i in 0..=10 {
                        let percentage = i as f32 * 10.0;
                        let y = rect.bottom() - (percentage / 100.0) * rect.height();
                        
                        // Draw horizontal grid line
                        painter.line_segment(
                            [
                                egui::pos2(rect.left() + y_axis_width, y),
                                egui::pos2(rect.right(), y)
                            ],
                            egui::Stroke::new(0.5, egui::Color32::from_gray(64)),
                        );
                        
                        // Draw value label (actual amplitude percentage)
                        let actual_value = (percentage / 100.0 / scale_factor * 100.0).round();
                        painter.text(
                            egui::pos2(rect.left() + y_axis_width - 5.0, y),
                            egui::Align2::RIGHT_CENTER,
                            format!("{}%", actual_value),
                            egui::FontId::default(),
                            egui::Color32::from_gray(200),
                        );
                    }

                    // Calculate appropriate time interval based on duration and available width
                    let duration_mins = (self.timeline_end.timestamp() - self.timeline_start.timestamp()) as f64 / 60.0;
                    let available_width = rect.width();
                    
                    // Assume each timestamp needs about 80 pixels of space to be readable
                    let min_pixels_per_label = 80.0;
                    let max_labels = (available_width / min_pixels_per_label).floor() as i64;
                    
                    // Calculate interval to show at most max_labels timestamps
                    let interval_mins = {
                        let raw_interval = ((duration_mins / max_labels as f64).ceil() as i64).max(15);
                        // Round up to next multiple of 15
                        ((raw_interval + 14) / 15) * 15
                    };
                    
                    // Round start time down to the interval
                    let start_mins = (self.timeline_start.timestamp() / 60 / interval_mins) * interval_mins;
                    let end_mins = self.timeline_end.timestamp() / 60;
                    
                    // Draw time markers with calculated interval
                    for mins in (start_mins..=end_mins).step_by(interval_mins as usize) {
                        let progress = (mins * 60 - self.timeline_start.timestamp()) as f32
                            / (self.timeline_end.timestamp() - self.timeline_start.timestamp()) as f32;
                        let x = rect.left() + progress * rect.width();
                        
                        // Only draw if within bounds
                        if x >= rect.left() && x <= rect.right() {
                            painter.line_segment(
                                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                                egui::Stroke::new(1.0, egui::Color32::from_gray(64)),
                            );
                            
                            let time = Local.timestamp_opt(mins * 60, 0).unwrap();
                            // Show date if interval is 24 hours or more
                            let time_str = if interval_mins >= 24 * 60 {
                                time.format("%b %d\n%I:%M %p").to_string()
                            } else {
                                time.format("%I:%M %p").to_string()
                            };
                            
                            painter.text(
                                egui::pos2(x, rect.bottom() - 15.0),
                                egui::Align2::CENTER_CENTER,
                                time_str,
                                egui::FontId::default(),
                                egui::Color32::from_gray(200),
                            );
                        }
                    }

                    // Draw recordings as box plots using cached data
                    for recording in &self.recordings {
                        if recording.timestamp >= self.timeline_start && recording.timestamp <= self.timeline_end {
                            let progress = (recording.timestamp.timestamp() - self.timeline_start.timestamp()) as f32
                                / (self.timeline_end.timestamp() - self.timeline_start.timestamp()) as f32;
                            let x = plot_rect.left() + progress * plot_rect.width();
                            
                            if let Some((min, q1, median, q3, max)) = recording.audio_stats {
                                let box_width = 15.0;
                                let whisker_width = box_width / 2.0;
                                let y_base = plot_rect.bottom();
                                
                                // Choose color based on hover state only
                                let color = if Some(recording.timestamp) == self.hovered_timestamp {
                                    egui::Color32::from_rgb(255, 200, 0)  // Brighter orange when hovered
                                } else {
                                    egui::Color32::from_rgb(255, 128, 0)  // Normal orange
                                };
                                
                                // Draw vertical whisker lines
                                painter.line_segment(
                                    [
                                        egui::pos2(x, y_base - plot_rect.height() * min),
                                        egui::pos2(x, y_base - plot_rect.height() * q1)
                                    ],
                                    egui::Stroke::new(1.0, color),
                                );
                                painter.line_segment(
                                    [
                                        egui::pos2(x, y_base - plot_rect.height() * q3),
                                        egui::pos2(x, y_base - plot_rect.height() * max)
                                    ],
                                    egui::Stroke::new(1.0, color),
                                );
                                
                                // Draw horizontal whisker caps
                                painter.line_segment(
                                    [
                                        egui::pos2(x - whisker_width/2.0, y_base - plot_rect.height() * min),
                                        egui::pos2(x + whisker_width/2.0, y_base - plot_rect.height() * min)
                                    ],
                                    egui::Stroke::new(1.0, color),
                                );
                                painter.line_segment(
                                    [
                                        egui::pos2(x - whisker_width/2.0, y_base - plot_rect.height() * max),
                                        egui::pos2(x + whisker_width/2.0, y_base - plot_rect.height() * max)
                                    ],
                                    egui::Stroke::new(1.0, color),
                                );
                                
                                // Draw box (IQR)
                                painter.rect_filled(
                                    egui::Rect::from_min_max(
                                        egui::pos2(x - box_width/2.0, y_base - plot_rect.height() * q3),
                                        egui::pos2(x + box_width/2.0, y_base - plot_rect.height() * q1),
                                    ),
                                    0.0,
                                    color,
                                );
                                
                                // Draw median line
                                painter.line_segment(
                                    [
                                        egui::pos2(x - box_width/2.0, y_base - plot_rect.height() * median),
                                        egui::pos2(x + box_width/2.0, y_base - plot_rect.height() * median)
                                    ],
                                    egui::Stroke::new(2.0, egui::Color32::WHITE),
                                );
                            }
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
            let mut current_day: Option<chrono::NaiveDate> = None;
            for recording in &recordings_ui {
                let recording_day = recording.timestamp.date_naive();
                
                // Add day header when we encounter a new day
                if current_day != Some(recording_day) {
                    current_day = Some(recording_day);
                    ui.heading(recording_day.format("%A, %B %d, %Y").to_string());
                }

                let path = recording.path.clone();
                let timestamp = recording.timestamp;  // Clone timestamp for hover state
                ui.horizontal(|ui| {
                    ui.label(format!("{} ({:.1}s)", 
                        recording.timestamp.format("%I:%M:%S %p"),
                        recording.duration
                    ));
                    let play_button = ui.button("Play");
                    if play_button.hovered() {
                        self.hovered_timestamp = Some(timestamp);
                    }
                    if play_button.clicked() {
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

            // Reset hover state on each frame
            if !ctx.input(|i| i.pointer.has_pointer()) {
                self.hovered_timestamp = None;
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