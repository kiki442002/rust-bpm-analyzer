use cpal::Sample;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

pub enum AudioMessage {
    Samples(Vec<f32>),
    Reset,
    SampleRateChanged(u32),
}

#[derive(Clone, Copy)]
pub struct PolicyAudioRestart {
    pub max_restarts: usize,
    pub time_window: Duration,
    pub retry_delay: Duration,
}

impl Default for PolicyAudioRestart {
    fn default() -> Self {
        Self {
            max_restarts: 5,
            time_window: Duration::from_secs(8),
            retry_delay: Duration::from_secs(1),
        }
    }
}

enum ControlMessage {
    Stop,
    Error(String),
}
pub struct AudioCapture {
    control_sender: Sender<ControlMessage>,
    thread_handle: Option<thread::JoinHandle<()>>,
    device_name: Option<String>,
    // Fields needed for restarting
    data_sender: Sender<AudioMessage>,
    sample_rate: u32,
    restart_policy: PolicyAudioRestart,
    buffer_duration: Option<Duration>,
}
struct AudioWorker {
    data_sender: Sender<AudioMessage>,
    control_sender: Sender<ControlMessage>,
    control_receiver: Receiver<ControlMessage>,
    device_name: Option<String>,
    error_count: u32,
    crash_timestamps: VecDeque<Instant>,
    sample_rate: u32,
    restart_policy: PolicyAudioRestart,
    buffer_duration: Option<Duration>,
}

impl AudioWorker {
    fn new(
        data_sender: Sender<AudioMessage>,
        control_sender: Sender<ControlMessage>,
        control_receiver: Receiver<ControlMessage>,
        device_name: Option<String>,
        sample_rate: u32,
        restart_policy: PolicyAudioRestart,
        buffer_duration: Option<Duration>,
    ) -> Self {
        Self {
            data_sender,
            control_sender,
            control_receiver,
            device_name,
            error_count: 0,
            crash_timestamps: VecDeque::with_capacity(restart_policy.max_restarts),
            sample_rate,
            restart_policy,
            buffer_duration,
        }
    }

    fn should_stop_restarting(&mut self) -> bool {
        let now = Instant::now();
        if self.crash_timestamps.len() >= self.restart_policy.max_restarts {
            self.crash_timestamps.pop_front();
        }
        self.crash_timestamps.push_back(now);

        if self.crash_timestamps.len() == self.restart_policy.max_restarts {
            let first = self.crash_timestamps.front().unwrap();
            let last = self.crash_timestamps.back().unwrap();
            if last.duration_since(*first) < self.restart_policy.time_window {
                return true;
            }
        }
        false
    }

    fn run(&mut self) {
        loop {
            match self.initialize_stream() {
                Ok(stream) => {
                    println!("Audio stream started successfully.");

                    match self.control_receiver.recv() {
                        Ok(ControlMessage::Stop) => {
                            println!("Stopping audio capture...");
                            break;
                        }
                        Ok(ControlMessage::Error(e)) => {
                            self.error_count += 1;
                            eprintln!(
                                "Stream error (count: {}): {}. Restarting...",
                                self.error_count, e
                            );
                            if self.should_stop_restarting() {
                                eprintln!(
                                    "Too many errors in short time (5 errors in < 3s). Stopping."
                                );
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                    drop(stream);
                }
                Err(e) => {
                    self.error_count += 1;
                    let delay = self.restart_policy.retry_delay;
                    eprintln!(
                        "Failed to initialize stream (count: {}): {}. Retrying in {:?}...",
                        self.error_count, e, delay
                    );

                    if self.should_stop_restarting() {
                        eprintln!("Too many errors in short time. Stopping.");
                        break;
                    }

                    let step = Duration::from_millis(100);
                    let steps = (delay.as_millis() as u64 + 99) / 100; // Round up

                    for _ in 0..steps {
                        thread::sleep(step);
                        if let Ok(ControlMessage::Stop) = self.control_receiver.try_recv() {
                            return;
                        }
                    }
                }
            }
        }
    }

    fn initialize_stream(&self) -> Result<cpal::Stream, Box<dyn std::error::Error>> {
        let host = cpal::default_host();

        let device = if let Some(name) = &self.device_name {
            host.input_devices()?
                .find(|d| d.name().map(|n| n == *name).unwrap_or(false))
                .ok_or(format!("Device '{}' not found", name))?
        } else {
            host.default_input_device()
                .ok_or("No input device available")?
        };

        println!("Input device: {}", device.name()?);
        let target_sample_rate = cpal::SampleRate(self.sample_rate);
        let supported_configs = device.supported_input_configs()?;
        let configs: Vec<_> = supported_configs.collect();

        let mut best_config = None;
        let mut min_diff = u32::MAX;
        let mut selected_rate = target_sample_rate;

        for config in &configs {
            let min_r = config.min_sample_rate();
            let max_r = config.max_sample_rate();

            if target_sample_rate >= min_r && target_sample_rate <= max_r {
                best_config = Some(config);
                selected_rate = target_sample_rate;
                break;
            }

            // Check distance to min
            let diff_min = if target_sample_rate < min_r {
                min_r.0 - target_sample_rate.0
            } else {
                target_sample_rate.0 - min_r.0
            };
            if diff_min < min_diff {
                min_diff = diff_min;
                best_config = Some(config);
                selected_rate = min_r;
            }

            // Check distance to max
            let diff_max = if target_sample_rate < max_r {
                max_r.0 - target_sample_rate.0
            } else {
                target_sample_rate.0 - max_r.0
            };
            if diff_max < min_diff {
                min_diff = diff_max;
                best_config = Some(config);
                selected_rate = max_r;
            }
        }

        let supported_config = match best_config {
            Some(c) => c.with_sample_rate(selected_rate),
            None => {
                eprintln!("Error: No supported configuration found.");
                return Err("No supported input config found".into());
            }
        };

        if selected_rate != target_sample_rate {
            println!(
                "Requested sample rate {} Hz not supported. Using closest: {} Hz",
                target_sample_rate.0, selected_rate.0
            );
        }

        let sample_format = supported_config.sample_format();

        // Calculate buffer size based on duration if provided
        let buffer_size = if let Some(duration) = self.buffer_duration {
            let requested_frames = (selected_rate.0 as f64 * duration.as_secs_f64()) as u32;
            match supported_config.buffer_size() {
                cpal::SupportedBufferSize::Range { min, max } => {
                    let frames = requested_frames.clamp(*min, *max);
                    if frames != requested_frames {
                        println!(
                            "Buffer size adjusted to match device capabilities: {} -> {}",
                            requested_frames, frames
                        );
                    }
                    cpal::BufferSize::Fixed(frames)
                }
                cpal::SupportedBufferSize::Unknown => cpal::BufferSize::Fixed(requested_frames),
            }
        } else {
            cpal::BufferSize::Default
        };

        let mut config: cpal::StreamConfig = supported_config.into();
        config.buffer_size = buffer_size;

        println!("Selected input config: {:?}", config);

        let control_sender = self.control_sender.clone();
        let err_fn = move |err| {
            eprintln!("an error occurred on stream: {}", err);
            let _ = control_sender.send(ControlMessage::Error(format!("{}", err)));
        };

        let stream = match sample_format {
            cpal::SampleFormat::I8 => {
                self.create_execution_stream::<i8>(&device, &config.into(), err_fn)?
            }
            cpal::SampleFormat::U8 => {
                self.create_execution_stream::<u8>(&device, &config.into(), err_fn)?
            }
            cpal::SampleFormat::I16 => {
                self.create_execution_stream::<i16>(&device, &config.into(), err_fn)?
            }
            cpal::SampleFormat::U16 => {
                self.create_execution_stream::<u16>(&device, &config.into(), err_fn)?
            }
            cpal::SampleFormat::I32 => {
                self.create_execution_stream::<i32>(&device, &config.into(), err_fn)?
            }
            cpal::SampleFormat::U32 => {
                self.create_execution_stream::<u32>(&device, &config.into(), err_fn)?
            }
            cpal::SampleFormat::F32 => {
                self.create_execution_stream::<f32>(&device, &config.into(), err_fn)?
            }
            cpal::SampleFormat::F64 => {
                self.create_execution_stream::<f64>(&device, &config.into(), err_fn)?
            }
            sample_format => {
                return Err(format!("Unsupported sample format: {:?}", sample_format).into());
            }
        };

        Ok(stream)
    }

    fn create_execution_stream<T>(
        &self,
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        err_fn: impl Fn(cpal::StreamError) + Send + 'static,
    ) -> Result<cpal::Stream, Box<dyn std::error::Error>>
    where
        T: cpal::Sample + cpal::SizedSample,
        f32: cpal::FromSample<T>,
    {
        let sender = self.data_sender.clone();

        // Notify main thread that a new stream is starting
        let _ = sender.send(AudioMessage::Reset);
        // Notify about the actual sample rate being used
        let _ = sender.send(AudioMessage::SampleRateChanged(config.sample_rate.0));

        let stream = device.build_input_stream(
            config,
            move |data: &[T], _: &_| {
                let buffer: Vec<f32> = data.iter().map(|&s| f32::from_sample(s)).collect();

                if let Err(_e) = sender.send(AudioMessage::Samples(buffer)) {
                    // Receiver dropped, stop sending
                }
            },
            err_fn,
            None,
        )?;

        stream.play()?;

        Ok(stream)
    }
}

impl AudioCapture {
    pub fn new(
        data_sender: Sender<AudioMessage>,
        device_name: Option<String>,
        sample_rate: u32,
        restart_policy: Option<PolicyAudioRestart>,
        buffer_duration: Option<Duration>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let (control_sender, control_receiver) = channel();
        let policy = restart_policy.unwrap_or_default();

        let mut worker = AudioWorker::new(
            data_sender.clone(),
            control_sender.clone(),
            control_receiver,
            device_name.clone(),
            sample_rate,
            policy,
            buffer_duration,
        );

        let thread_handle = thread::spawn(move || {
            worker.run();
        });

        Ok(AudioCapture {
            control_sender,
            thread_handle: Some(thread_handle),
            device_name,
            data_sender,
            sample_rate,
            restart_policy: policy,
            buffer_duration,
        })
    }

    #[allow(dead_code)]
    pub fn list_devices() -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let host = cpal::default_host();
        let devices = host.input_devices()?;
        let mut names = Vec::new();
        for device in devices {
            if let Ok(name) = device.name() {
                names.push(name);
            }
        }
        Ok(names)
    }

    #[allow(dead_code)]
    pub fn default_device_name() -> Option<String> {
        let host = cpal::default_host();
        host.default_input_device().and_then(|d| d.name().ok())
    }

    #[allow(dead_code)]
    pub fn set_device(
        &mut self,
        device_name: Option<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Stop current worker
        let _ = self.control_sender.send(ControlMessage::Stop);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }

        // Create new worker with new device
        let (control_sender, control_receiver) = channel();

        let mut worker = AudioWorker::new(
            self.data_sender.clone(),
            control_sender.clone(),
            control_receiver,
            device_name.clone(),
            self.sample_rate,
            self.restart_policy,
            self.buffer_duration,
        );

        let thread_handle = thread::spawn(move || {
            worker.run();
        });

        // Update self
        self.control_sender = control_sender;
        self.thread_handle = Some(thread_handle);
        self.device_name = device_name;

        Ok(())
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        let _ = self.control_sender.send(ControlMessage::Stop);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
}
