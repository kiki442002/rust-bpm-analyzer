use iced::alignment::Horizontal;
use iced::widget::{button, column, container, pick_list, progress_bar, row, text};
use iced::{Color, Element, Length, Subscription, Task, Theme};
use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::core_bpm::{AudioCapture, AudioMessage, BpmAnalyzer};
use crate::midi::{MidiEvent, MidiManager};
use crate::network_sync::{LinkManager, NetworkManager, NetworkMessage};
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
        size: iced::Size::new(370.0, 480.0),
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
    available_midi_devices: Vec<String>,
    selected_midi_device: Option<String>,

    // Update
    update_available: Option<String>,
    show_update_modal: bool,
    is_updating: bool,

    // Network
    network_manager: Option<NetworkManager>,
    network_energy: f32,
    remote_auto_gain: bool,
    remote_peers: HashMap<String, String>,
}

#[derive(Debug, Clone)]
enum Message {
    Tick,
    ToggleDetection,
    DeviceSelected(String),
    Tap,
    ToggleMidiLearn,
    MidiDeviceSelected(String),
    CheckUpdate,
    UpdateFound(Option<String>),
    StartUpdate,
    CloseUpdateModal,
    UpdateCompleted(bool),
    SetRemoteAutoGain(bool),
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
        let (inputs, outputs) = MidiManager::list_ports().unwrap_or_default();
        let mut available_midi_devices = inputs.clone();
        for out in outputs {
            if !available_midi_devices.contains(&out) {
                available_midi_devices.push(out);
            }
        }
        available_midi_devices.sort();
        let selected_midi_device = available_midi_devices.first().cloned();

        let midi_manager = MidiManager::new()
            .ok()
            .map(|m| std::sync::Arc::new(std::sync::Mutex::new(m)));

        if let Some(manager_mutex) = &midi_manager {
            if let Ok(mut manager) = manager_mutex.lock() {
                if let Some(port) = &selected_midi_device {
                    let _ = manager.select_input(port);
                    let _ = manager.select_output(port);
                }
            }
        }

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
                available_midi_devices,
                selected_midi_device,
                update_available: None,
                show_update_modal: false,
                is_updating: false,
                network_manager: NetworkManager::new(
                    "desktop_gui".to_string(),
                    "Desktop".to_string(),
                )
                .map_err(|e| eprintln!("Net error: {}", e))
                .ok(),
                network_energy: 0.0,
                remote_auto_gain: false,
                remote_peers: HashMap::new(),
            },
            Task::perform(async {}, |_| Message::CheckUpdate),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::MidiDeviceSelected(port_name) => {
                self.selected_midi_device = Some(port_name.clone());
                if let Some(manager) = &self.midi_manager {
                    if let Ok(mut m) = manager.lock() {
                        let _ = m.select_input(&port_name);
                        let _ = m.select_output(&port_name);
                    }
                }
                return Task::none();
            }
            Message::CheckUpdate => {
                return Task::perform(
                    async {
                        let status = self_update::backends::github::ReleaseList::configure()
                            .repo_owner("kiki442002")
                            .repo_name("rust-bpm-analyzer")
                            .build()
                            .unwrap()
                            .fetch();

                        if let Ok(releases) = status {
                            if let Some(release) = releases.first() {
                                let current_version = env!("CARGO_PKG_VERSION");
                                if release.version != current_version {
                                    return Some(release.version.clone());
                                }
                            }
                        }
                        None
                    },
                    Message::UpdateFound,
                );
            }
            Message::UpdateFound(version) => {
                if let Some(v) = version {
                    self.update_available = Some(v);
                    self.show_update_modal = true;
                }
                return Task::none();
            }
            Message::StartUpdate => {
                self.is_updating = true;
                return Task::perform(
                    async {
                        let status = self_update::backends::github::Update::configure()
                            .repo_owner("kiki442002")
                            .repo_name("rust-bpm-analyzer")
                            .bin_name("rust-bpm-analyzer")
                            .show_download_progress(true)
                            .no_confirm(true)
                            .current_version(env!("CARGO_PKG_VERSION"))
                            .build()
                            .unwrap()
                            .update();

                        status.is_ok()
                    },
                    Message::UpdateCompleted,
                );
            }
            Message::UpdateCompleted(success) => {
                self.is_updating = false;
                self.show_update_modal = false;
                if success {
                    println!("Update successful, restarting...");
                    if let Ok(exe) = std::env::current_exe() {
                        let _ = std::process::Command::new(exe).spawn();
                        std::process::exit(0);
                    }
                } else {
                    println!("Update failed.");
                }
                return Task::none();
            }
            Message::CloseUpdateModal => {
                self.show_update_modal = false;
                return Task::none();
            }
            Message::Tick => {
                // Poll network messages
                if let Some(manager) = &self.network_manager {
                    while let Ok(msg) = manager.try_recv() {
                        // Avoid spamming logs for high frequency messages like EnergyLevel
                        if !matches!(&msg, NetworkMessage::EnergyLevel(_)) {
                            println!("Received Network Message: {:?}", msg);
                        }

                        match msg {
                            NetworkMessage::Presence { id, name, online } => {
                                if online {
                                    self.remote_peers.insert(id, name);
                                } else {
                                    self.remote_peers.remove(&id);
                                }
                            }
                            NetworkMessage::EnergyLevel(level) => {
                                self.network_energy = level;
                            }
                            NetworkMessage::AutoGainState(state) => {
                                self.remote_auto_gain = state;
                            }
                            _ => {}
                        }
                    }
                }

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
            Message::SetRemoteAutoGain(val) => {
                if let Some(manager) = &self.network_manager {
                    println!("Sending: SetAutoGain({})", val);
                    let _ = manager.send(NetworkMessage::SetAutoGain(val));
                    self.remote_auto_gain = val;
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        if self.show_update_modal {
            return container(
                column![
                    text("New Update Available!").size(24),
                    text(format!(
                        "Version {} is available.",
                        self.update_available.as_deref().unwrap_or("?")
                    ))
                    .size(18),
                    if self.is_updating {
                        text("Updating...").size(16)
                    } else {
                        text("Do you want to update now?").size(16)
                    },
                    if !self.is_updating {
                        row![
                            button(text("Update Now").align_x(Horizontal::Center))
                                .on_press(Message::StartUpdate)
                                .padding(10)
                                .style(|theme: &'_ Theme, status| {
                                    let palette = theme.palette();
                                    let base = Color {
                                        a: 0.9,
                                        ..palette.success
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
                                }),
                            button(text("Cancel").align_x(Horizontal::Center))
                                .on_press(Message::CloseUpdateModal)
                                .padding(10)
                                .style(|theme: &'_ Theme, status| {
                                    let palette = theme.palette();
                                    let base = Color {
                                        a: 0.9,
                                        ..palette.danger
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
                                })
                        ]
                        .spacing(20)
                    } else {
                        row![].into()
                    }
                ]
                .spacing(20)
                .align_x(Horizontal::Center),
            )
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into();
        }

        let peers_text = if self.is_enabled {
            text(format!(
                "Ableton Link Peers: {} | Remote Devices: {}",
                self.num_peers,
                self.remote_peers.len()
            ))
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

        let midi_picker = pick_list(
            self.available_midi_devices.clone(),
            self.selected_midi_device.clone(),
            Message::MidiDeviceSelected,
        )
        .placeholder("Select MIDI Device")
        .text_size(10)
        .width(Length::Fill);

        let tap_row = row![tap_btn, learn_btn, midi_picker]
            .spacing(10)
            .align_y(iced::alignment::Vertical::Center);

        let has_peers = !self.remote_peers.is_empty();

        let mut auto_gain_btn = button(
            text(if self.remote_auto_gain {
                "Auto Gain: ON"
            } else {
                "Auto Gain: OFF"
            })
            .size(10)
            .align_x(Horizontal::Center),
        )
        .padding(8)
        .style(move |theme: &'_ Theme, status| {
            let palette = theme.palette();
            let base = if self.remote_auto_gain {
                palette.success
            } else {
                palette.background
            };

            let background = match status {
                button::Status::Active => base,
                button::Status::Hovered => Color { a: 0.8, ..base },
                button::Status::Pressed => Color { a: 0.6, ..base },
                button::Status::Disabled => Color::from_rgb(0.3, 0.3, 0.3),
            };

            button::Style {
                background: Some(background.into()),
                text_color: if self.remote_auto_gain {
                    Color::WHITE
                } else {
                    palette.text
                },
                border: iced::Border {
                    radius: 15.0.into(),
                    width: 1.0,
                    color: if self.remote_auto_gain {
                        Color::TRANSPARENT
                    } else {
                        palette.text
                    },
                    ..iced::Border::default()
                },
                ..button::Style::default()
            }
        });

        if has_peers {
            auto_gain_btn =
                auto_gain_btn.on_press(Message::SetRemoteAutoGain(!self.remote_auto_gain));
        }

        let remote_controls = column![
            text("Remote Energy In").size(12).color([0.7, 0.7, 0.7]),
            row![
                progress_bar(0.0..=0.5, self.network_energy)
                    .height(20)
                    .width(Length::Fill),
                auto_gain_btn
            ]
            .spacing(10)
            .align_y(iced::alignment::Vertical::Center)
        ]
        .spacing(5)
        .align_x(Horizontal::Center);

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
                toggle_btn,
                remote_controls
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
