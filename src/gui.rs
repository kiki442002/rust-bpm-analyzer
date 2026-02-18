use iced::alignment::Horizontal;
use iced::widget::{button, column, container, pick_list, row, text};
use iced::{Color, Element, Length, Subscription, Task, Theme};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::core_bpm::{AudioCapture, AudioMessage, BpmAnalyzer};
use crate::midi::{MidiEvent, MidiManager};
use crate::network_sync::LinkManager;
use crate::platform::TARGET_SAMPLE_RATE;

#[derive(Debug, Clone)]
pub struct GuiUpdate {
    pub bpm: Option<f32>,
    pub num_peers: usize,
}

#[derive(Debug, Clone)]
struct MidiMapping {
    channel: u8,
    note_or_cc: u8,
    is_note: bool,
}

#[derive(Debug, Clone)]
pub enum GuiCommand {
    SetDetection(bool),
    SetDevice(Option<String>),
    SetBpm(f64),
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let window_settings = iced::window::Settings {
        size: iced::Size::new(350.0, 400.0),
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

    // TAP system
    tap_times: Vec<Instant>,

    // MIDI
    midi_manager: Option<std::sync::Arc<std::sync::Mutex<MidiManager>>>,
    midi_learn: bool,
    tap_midi_mapping: Option<MidiMapping>,
}

#[derive(Debug, Clone)]
enum Message {
    Tick,
    ToggleDetection,
    DeviceSelected(String),
    Tap,
    ToggleMidiLearn,
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

        // Initialize MIDI Manager
        let midi_manager = MidiManager::new()
            .ok()
            .map(|m| std::sync::Arc::new(std::sync::Mutex::new(m)));

        (
            Self {
                bpm: None,
                num_peers: 0,
                is_enabled: false,
                receiver: std::sync::Arc::new(std::sync::Mutex::new(rx_results)),
                sender: tx_commands,
                input_device: default_device,
                available_devices,
                tap_times: Vec::new(),
                midi_manager,
                midi_learn: false,
                tap_midi_mapping: None,
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

                let mut should_tap = false;

                // Poll MIDI events
                if let Some(midi_mutex) = &self.midi_manager {
                    if let Ok(mut midi) = midi_mutex.lock() {
                        while let Ok(event) = midi.try_recv() {
                            if self.midi_learn {
                                match event {
                                    MidiEvent::NoteOn {
                                        channel,
                                        note,
                                        velocity: _,
                                    } => {
                                        self.tap_midi_mapping = Some(MidiMapping {
                                            channel,
                                            note_or_cc: note,
                                            is_note: true,
                                        });
                                        self.midi_learn = false;
                                        println!(
                                            "MIDI Learn: Note {} on Channel {}",
                                            note, channel
                                        );
                                        // APC Mini Feedback: Channel 6 (which is index 6 on APC, typically mapped as channel 6 in DAW, here it's 0-indexed in code usually)
                                        // Actually midi channels in code are 0-15. So channel 1 in MIDI is 0.
                                        // User asked for "channel 6 brightness 100% and velocity 3 for white".
                                        // Assuming 0-indexed, channel 6 is 6. If user means MIDI Channel 7 (labelled 1-16), it's 6.
                                        // For APC Mini Mk2 often Note On Ch 6 with Velocity determines color/brightness.
                                        midi.send_note_on(6, note, 3);
                                    }
                                    MidiEvent::ControlChange {
                                        channel,
                                        controller,
                                        value: _,
                                    } => {
                                        self.tap_midi_mapping = Some(MidiMapping {
                                            channel,
                                            note_or_cc: controller,
                                            is_note: false,
                                        });
                                        self.midi_learn = false;
                                        println!(
                                            "MIDI Learn: CC {} on Channel {}",
                                            controller, channel
                                        );
                                        // APC feedback for CC or buttons mapped via CC:
                                        // Use channel 6 (index) and value 3
                                        midi.send_control_change(6, controller, 3);
                                    }
                                }
                            } else if let Some(mapping) = &self.tap_midi_mapping {
                                let is_match = match event {
                                    MidiEvent::NoteOn {
                                        channel,
                                        note,
                                        velocity: _,
                                    } => {
                                        mapping.is_note
                                            && mapping.channel == channel
                                            && mapping.note_or_cc == note
                                    }
                                    MidiEvent::ControlChange {
                                        channel,
                                        controller,
                                        value: _,
                                    } => {
                                        !mapping.is_note
                                            && mapping.channel == channel
                                            && mapping.note_or_cc == controller
                                    }
                                };

                                if is_match {
                                    should_tap = true;
                                }
                            }
                        }
                    }
                }

                if should_tap {
                    return self.update(Message::Tap);
                }
            }
            Message::ToggleMidiLearn => {
                self.midi_learn = !self.midi_learn;
            }
            Message::Tap => {
                let now = Instant::now();
                // Reset if last tap was too long ago (corresponding to < 100 BPM -> > 0.6s)
                if let Some(last) = self.tap_times.last() {
                    if now.duration_since(*last).as_secs_f32() > 0.6 {
                        self.tap_times.clear();
                    }
                }
                self.tap_times.push(now);

                // Keep only last 5 taps for average
                if self.tap_times.len() > 5 {
                    self.tap_times.remove(0);
                }

                if self.tap_times.len() >= 5 {
                    let mut sum_intervals = 0.0;
                    for i in 0..self.tap_times.len() - 1 {
                        sum_intervals += self.tap_times[i + 1]
                            .duration_since(self.tap_times[i])
                            .as_secs_f64();
                    }
                    let avg_interval = sum_intervals / (self.tap_times.len() - 1) as f64;
                    if avg_interval > 0.0 {
                        let new_bpm = 60.0 / avg_interval;
                        // Avoid extreme values (Min 100 BPM)
                        if new_bpm >= 100.0 && new_bpm <= 400.0 {
                            self.bpm = Some(new_bpm as f32);
                            let _ = self.sender.send(GuiCommand::SetBpm(new_bpm));
                        }
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

        let bpm_display = if !self.is_enabled {
            text("***.*").size(80).color([0.5, 0.5, 0.5])
        } else if let Some(bpm) = self.bpm {
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

        let tap_btn = button(text("TAP").size(16).align_x(Horizontal::Center))
            .on_press(Message::Tap)
            .padding(10)
            .width(iced::Length::Fixed(80.0))
            .style(|theme: &'_ Theme, status| {
                let palette = theme.palette();
                let base = Color {
                    a: 0.9,
                    ..palette.success // Use success color (usually green/cyan) for TAP
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

        // MIDI Learn Button
        let learn_btn_text = if self.midi_learn {
            "Listening..."
        } else {
            "MIDI Learn"
        };
        let learn_btn = button(text(learn_btn_text).size(12).align_x(Horizontal::Center))
            .on_press(Message::ToggleMidiLearn)
            .padding(10)
            .width(iced::Length::Fixed(100.0))
            .style(move |theme: &'_ Theme, status| {
                let palette = theme.palette();
                // If learning, use warning/danger color (orange/red), else neutral
                let base = if self.midi_learn {
                    palette.danger
                } else {
                    Color {
                        a: 0.6,
                        ..palette.background
                    } // Subtle when inactive
                };

                let background = match status {
                    button::Status::Active => base,
                    button::Status::Hovered => Color { a: 0.8, ..base },
                    button::Status::Pressed => Color { a: 0.5, ..base },
                    button::Status::Disabled => Color::from_rgb(0.4, 0.4, 0.4),
                };

                button::Style {
                    background: Some(background.into()),
                    text_color: Color::WHITE,
                    border: iced::Border {
                        radius: 15.0.into(),
                        width: if self.midi_learn { 2.0 } else { 1.0 },
                        color: if self.midi_learn {
                            palette.primary
                        } else {
                            Color::TRANSPARENT
                        },
                        ..iced::Border::default()
                    },
                    ..button::Style::default()
                }
            });

        let tap_row = row![tap_btn, learn_btn]
            .spacing(10)
            .align_y(iced::alignment::Vertical::Center);

        container(
            column![
                row![peers_text]
                    .width(Length::Fill)
                    .align_y(iced::alignment::Vertical::Top),
                column![label_text, bpm_display]
                    .align_x(Horizontal::Center)
                    .spacing(5),
                tap_row,
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
    let mut bpm_history: std::collections::VecDeque<f32> =
        std::collections::VecDeque::with_capacity(5);

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
                        bpm_history.clear();
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
                GuiCommand::SetBpm(new_bpm) => {
                    link_manager.update_tempo(new_bpm, false, None);
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
                            // Update history for moving average
                            if bpm_history.len() >= 5 {
                                bpm_history.pop_front();
                            }
                            bpm_history.push_back(result.bpm);

                            // Calculate average
                            let avg_bpm: f32 =
                                bpm_history.iter().sum::<f32>() / bpm_history.len() as f32;

                            let bpm_to_send = Some(avg_bpm);
                            // Send update to GUI
                            let _ = tx.send(GuiUpdate {
                                bpm: bpm_to_send,
                                num_peers: link_manager.num_peers(),
                            });

                            // Sync Ableton Link
                            // Use the averaged BPM for sync
                            link_manager.update_tempo(
                                avg_bpm as f64,
                                result.is_drop,
                                result.beat_offset,
                            );
                            println!(
                                "Avg BPM: {:.1} | Raw BPM: {:.1} | Conf: {:.2}",
                                avg_bpm, result.bpm, result.confidence
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
