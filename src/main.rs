use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use hound;
use std::fs::File;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::fs; // Add this import for directory creation
use chrono::Local;

const THRESHOLD: f32 = 0.05; // Adjust sensitivity for bark detection
const MIN_BARK_DURATION: Duration = Duration::from_secs(1);

fn main() {
    let host = cpal::default_host();
    let device = host.default_input_device().expect("Failed to find input device");
    let config = device.default_input_config().expect("Failed to get default input config");

    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    let samples_per_chunk = (sample_rate as f32 * MIN_BARK_DURATION.as_secs_f32()) as usize;
    
    let recording = Arc::new(Mutex::new(false));
    let last_bark_time = Arc::new(Mutex::new(None));
    let mut writer: Option<hound::WavWriter<_>> = None;
    
    let stream = device.build_input_stream(
        &config.into(),
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            let max_amplitude = data.iter().map(|x| x.abs()).fold(0.0, f32::max);
            let mut is_recording = recording.lock().unwrap();
            let mut last_bark = last_bark_time.lock().unwrap();
            let now = Instant::now();

            if max_amplitude > THRESHOLD {
                if !*is_recording {
                    *is_recording = true;
                    *last_bark = Some(now);
                    // Create barks directory if it doesn't exist
                    fs::create_dir_all("barks").expect("Failed to create barks directory");
                    
                    let timestamp = Local::now().format("%Y%m%d_%H%M%S");
                    let filename = format!("barks/bark_{}.wav", timestamp);
                    println!("Started recording: {}", filename);
                    let spec = hound::WavSpec {
                        channels: channels as u16,
                        sample_rate: sample_rate,
                        bits_per_sample: 16,
                        sample_format: hound::SampleFormat::Int,
                    };
                    writer = Some(hound::WavWriter::create(filename, spec).unwrap());
                }
            }
            
            if *is_recording {
                if let Some(ref mut w) = writer {
                    for &sample in data.iter().take(samples_per_chunk) {
                        let scaled_sample = (sample * i16::MAX as f32) as i16;
                        w.write_sample(scaled_sample).unwrap();
                    }
                }
                if last_bark.unwrap().elapsed() > MIN_BARK_DURATION {
                    *is_recording = false;
                    writer = None;
                    println!("Finished recording");
                }
            }
        },
        |err| eprintln!("Error: {}", err),
        None,
    ).expect("Failed to create stream");

    stream.play().expect("Failed to start stream");
    println!("Listening for barks...");
    std::thread::sleep(Duration::from_secs(60 * 60)); // Run for an hour
}
