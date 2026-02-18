use iced::alignment::Horizontal;
use iced::widget::{button, column, container, pick_list, row, text};
use iced::{Color, Element, Length, Subscription, Task, Theme};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::core_bpm::{AudioCapture, AudioMessage, BpmAnalyzer};
use crate::network_sync::LinkManager;
use crate::platform::TARGET_SAMPLE_RATE;

#[derive(Debug, Clone)]
pub struct GuiUpdate {
    pub bpm: Option<f32>,
    pub num_peers: usize,
}

#[derive(Debug, Clone)]
pub enum GuiCommand {
    SetDetection(bool),
    SetDevice(Option<String>),
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let window_settings = iced::window::Settings {
        size: iced::Size::new(350.0, 350.0),
        ..Default::default()
    };

    iced::application("Rust BPM Analyzer", BpmApp::update, BpmApp::view)
        .theme(|_| Theme::Dracula)
        .subscription(BpmApp::subscription)
        .window(window_settings)
        .run_with(BpmApp::new)?;
    Ok(())
}

struct BpmApp {
    bpm: Option<f32>,
    num_peers: usize,
    is_enabled: bool,
    input_device: Option<String>,
    available_devices: Vec<String>,

    // Receiver to get updates from the analysis thread
    receiver: std::sync::Arc<std::sync::Mutex<mpsc::Receiver<GuiUpdate>>>,
    // Sender to send commands to the analysis thread
    sender: mpsc::Sender<GuiCommand>,
}

#[derive(Debug, Clone)]
enum Message {
    Tick,
    ToggleDetection,
    DeviceSelected(String),
}

impl BpmApp {
    fn new() -> (Self, Task<Message>) {
        let (tx_results, rx_results) = mpsc::channel();
        let (tx_commands, rx_commands) = mpsc::channel();

        // Fetch available devices
        let available_devices = AudioCapture::list_devices().unwrap_or_default();
        let default_device =
            AudioCapture::default_device_name().or_else(|| available_devices.first().cloned());

        // Spawn the analysis thread
        thread::spawn(move || {
            if let Err(e) = run_analysis_loop(tx_results, rx_commands) {
                eprintln!("Analysis loop error: {}", e);
            }
        });

        (
            Self {
                bpm: None,
                num_peers: 0,
                is_enabled: false,
                receiver: std::sync::Arc::new(std::sync::Mutex::new(rx_results)),
                sender: tx_commands,
                input_device: default_device,
                available_devices,
            },
            Task::none(),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick => {
                // Poll all available messages
                if let Ok(rx) = self.receiver.lock() {
                    while let Ok(result) = rx.try_recv() {
                        self.bpm = result.bpm;
                        self.num_peers = result.num_peers;
                    }
                }
            }
            Message::ToggleDetection => {
                self.is_enabled = !self.is_enabled;
                if !self.is_enabled {
                    self.bpm = None;
                }
                println!(
                    "Detection toggled: {}",
                    if self.is_enabled { "ON" } else { "OFF" }
                );
                let _ = self.sender.send(GuiCommand::SetDetection(self.is_enabled));
            }
            Message::DeviceSelected(device_name) => {
                self.input_device = Some(device_name.clone());
                let _ = self.sender.send(GuiCommand::SetDevice(Some(device_name)));
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let peers_text = if self.is_enabled {
            text(format!("Link Peers: {}", self.num_peers))
                .size(14)
                .color([0.7, 0.7, 0.7])
        } else {
            text("").size(14).color([0.5, 0.5, 0.5])
        };

        let bpm_display = if let Some(bpm) = self.bpm {
            text(format!("{:.1}", bpm)).size(80)
        } else {
            text("---.-").size(80).color([0.5, 0.5, 0.5])
        };

        let label_text = text("BPM").size(20).color([0.6, 0.6, 0.6]);

        let device_picker = pick_list(
            self.available_devices.clone(),
            self.input_device.clone(),
            Message::DeviceSelected,
        )
        .placeholder("Select Audio Device")
        .width(Length::Fill);

        let toggle_btn = button(
            text(if self.is_enabled {
                "Disable Detection"
            } else {
                "Enable Detection"
            })
            .size(18)
            .width(Length::Fill)
            .align_x(Horizontal::Center),
        )
        .on_press(Message::ToggleDetection)
        .padding(15)
        .width(Length::Fill)
        .style(|theme: &'_ Theme, status| {
            let palette = theme.palette();
            let base = Color {
                a: 0.9,
                ..palette.primary
            };

            let background = match status {
                button::Status::Active => base,
                button::Status::Hovered => Color { a: 0.75, ..base },
                button::Status::Pressed => Color { a: 0.6, ..base },
                button::Status::Disabled => Color::from_rgb(0.4, 0.4, 0.4),
            };

            button::Style {
                background: Some(background.into()),
                text_color: Color::WHITE,
                border: iced::Border {
                    radius: 15.0.into(),
                    ..iced::Border::default()
                },
                ..button::Style::default()
            }
        });

        container(
            column![
                row![peers_text]
                    .width(Length::Fill)
                    .align_y(iced::alignment::Vertical::Top),
                column![label_text, bpm_display]
                    .align_x(Horizontal::Center)
                    .spacing(5),
                device_picker,
                toggle_btn
            ]
            .align_x(Horizontal::Center)
            .spacing(20)
            .padding(20),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        iced::window::frames().map(|_| Message::Tick)
    }
}

// This function runs in a background thread and does the heavy lifting
fn run_analysis_loop(
    tx: mpsc::Sender<GuiUpdate>,
    rx_cmd: mpsc::Receiver<GuiCommand>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (sender, receiver) = mpsc::channel();
    let sender_clone = sender.clone(); // Keep a clone to restart audio capture
    let mut last_ui_update = Instant::now();
    let mut is_enabled = false;
    let mut current_device: Option<String> = None;
    let mut current_hop_size = TARGET_SAMPLE_RATE as usize;

    let mut new_samples_accumulator: Vec<f32> = Vec::with_capacity(TARGET_SAMPLE_RATE as usize);
    let mut analyzer = BpmAnalyzer::new(TARGET_SAMPLE_RATE, None)?;

    let mut link_manager = LinkManager::new();

    let mut audio_capture: Option<AudioCapture> = None;

    loop {
        // Check for GUI commands
        while let Ok(cmd) = rx_cmd.try_recv() {
            match cmd {
                GuiCommand::SetDetection(enabled) => {
                    link_manager.link_state(enabled);
                    is_enabled = enabled;
                    if enabled {
                        if audio_capture.is_none() {
                            println!("Starting audio capture...");
                            // Re-create audio capture
                            match AudioCapture::new(
                                sender_clone.clone(),
                                current_device.clone(),
                                TARGET_SAMPLE_RATE,
                                None,
                                Some(Duration::from_millis(500)),
                            ) {
                                Ok(capture) => audio_capture = Some(capture),
                                Err(e) => eprintln!("Failed to restart audio capture: {}", e),
                            }
                        }
                    } else {
                        if audio_capture.is_some() {
                            println!("Stopping audio capture...");
                            audio_capture = None; // Drops the capture and stops the stream
                        }
                        new_samples_accumulator.clear();
                    }
                }
                GuiCommand::SetDevice(device_name) => {
                    println!("Switching device to: {:?}", device_name);
                    current_device = device_name.clone();
                    if let Some(capture) = &mut audio_capture {
                        if let Err(e) = capture.set_device(device_name) {
                            eprintln!("Failed to switch device: {}", e);
                        }
                    }
                }
            }
        }

        // Use recv_timeout to allow checking commands and updating UI even if no audio comes in
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(AudioMessage::Samples(packet)) => {
                if is_enabled {
                    new_samples_accumulator.extend(packet);

                    if new_samples_accumulator.len() >= current_hop_size {
                        if let Ok(Some(result)) = analyzer.process(&new_samples_accumulator) {
                            let bpm_to_send = Some(result.bpm);
                            // Send update to GUI
                            let _ = tx.send(GuiUpdate {
                                bpm: bpm_to_send,
                                num_peers: link_manager.num_peers(),
                            });

                            // Sync Ableton Link
                            link_manager.update_tempo(
                                result.bpm as f64,
                                result.is_drop,
                                result.beat_offset,
                            );
                            println!(
                                "BPM: {:.1} | Drop: {} | Conf: {:.2} | CoarseConf: {:.2}",
                                result.bpm,
                                result.is_drop,
                                result.confidence,
                                result.coarse_confidence
                            );
                        }

                        last_ui_update = Instant::now();

                        new_samples_accumulator.clear();
                    }
                } else {
                    // Drain any remaining samples if disabled but still receiving
                    new_samples_accumulator.clear();
                }
            }
            Ok(AudioMessage::Reset) => {
                new_samples_accumulator.clear();
            }
            Ok(AudioMessage::SampleRateChanged(rate)) => {
                println!("Audio sample rate changed to: {} Hz", rate);
                match BpmAnalyzer::new(rate, None) {
                    Ok(new_analyzer) => {
                        analyzer = new_analyzer;
                        // Update HOP_SIZE to match 1 second of audio at new rate
                        current_hop_size = (rate / 2) as usize;
                        // Resize accumulator
                        if new_samples_accumulator.capacity() < current_hop_size {
                            new_samples_accumulator
                                .reserve(current_hop_size - new_samples_accumulator.len());
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to re-initialize analyzer with rate {}: {}", rate, e)
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // No audio received (expected if disabled)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        // Periodic UI update (for peer count) if we haven't sent one recently
        if last_ui_update.elapsed() > Duration::from_millis(200) {
            let link_bpm = link_manager.get_tempo();
            let _ = tx.send(GuiUpdate {
                bpm: Some(link_bpm as f32), // Send Link BPM instead of None
                num_peers: link_manager.num_peers(),
            });
            last_ui_update = Instant::now();
        }
    }
    Ok(())
}
