use iced::alignment::Horizontal;
use iced::widget::{button, column, container, row, text};
use iced::{Color, Element, Length, Subscription, Task, Theme};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::core_bpm::{AnalysisResult, AudioCapture, BpmAnalyzer, audio::AudioMessage};
use crate::network_sync::LinkManager;

const SAMPLE_RATE: u32 = 44100; // Desktop is always 44100 in this project
const HOP_SIZE: usize = SAMPLE_RATE as usize;

#[derive(Debug, Clone)]
pub struct GuiUpdate {
    pub bpm: Option<f32>,
    pub is_drop: bool,
    pub num_peers: usize,
}

#[derive(Debug, Clone)]
pub enum GuiCommand {
    SetDetection(bool),
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    iced::application("Rust BPM Analyzer", BpmApp::update, BpmApp::view)
        .theme(|_| Theme::Dark)
        .window_size((350.0, 250.0))
        .subscription(BpmApp::subscription)
        .run_with(BpmApp::new)?;
    Ok(())
}

struct BpmApp {
    bpm: Option<f32>,
    is_drop: bool,
    num_peers: usize,
    is_enabled: bool,
    input_device: Option<String>,

    // Receiver to get updates from the analysis thread
    receiver: std::sync::Arc<std::sync::Mutex<mpsc::Receiver<GuiUpdate>>>,
    // Sender to send commands to the analysis thread
    sender: mpsc::Sender<GuiCommand>,
}

#[derive(Debug, Clone)]
enum Message {
    Tick,
    ToggleDetection,
}

impl BpmApp {
    fn new() -> (Self, Task<Message>) {
        let (tx_results, rx_results) = mpsc::channel();
        let (tx_commands, rx_commands) = mpsc::channel();

        // Spawn the analysis thread
        thread::spawn(move || {
            if let Err(e) = run_analysis_loop(tx_results, rx_commands) {
                eprintln!("Analysis loop error: {}", e);
            }
        });

        (
            Self {
                bpm: None,
                is_drop: false,
                num_peers: 0,
                is_enabled: false,
                receiver: std::sync::Arc::new(std::sync::Mutex::new(rx_results)),
                sender: tx_commands,
                input_device: None,
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
                        if let Some(bpm) = result.bpm {
                            self.bpm = Some(bpm);
                        }
                        self.is_drop = result.is_drop;
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
        }
        Task::none()
    }

    fn view(&self) -> Element<Message> {
        let peers_text = text(format!("Link Peers: {}", self.num_peers))
            .size(14)
            .color([0.7, 0.7, 0.7]);

        let bpm_display = if let Some(bpm) = self.bpm {
            text(format!("{:.1}", bpm)).size(80)
        } else {
            text("---.-").size(80).color([0.5, 0.5, 0.5])
        };

        let drop_indicator = if self.is_drop {
            text("DROP!").size(30).color([1.0, 0.0, 0.0])
        } else {
            text("").size(30)
        };

        let label_text = text("BPM").size(20).color([0.6, 0.6, 0.6]);

        let toggle_btn = button(
            container(
                text(if self.is_enabled {
                    "Disable Detection"
                } else {
                    "Enable Detection"
                })
                .size(18)
                .color([1.0, 1.0, 1.0]),
            )
            .width(Length::Fill)
            .align_x(Horizontal::Center),
        )
        .on_press(Message::ToggleDetection)
        .padding(15)
        .width(Length::Fill);

        container(
            column![
                row![peers_text]
                    .width(Length::Fill)
                    .align_y(iced::alignment::Vertical::Top),
                column![label_text, bpm_display, drop_indicator]
                    .align_x(Horizontal::Center)
                    .spacing(5),
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

    let mut new_samples_accumulator: Vec<f32> = Vec::with_capacity(HOP_SIZE);
    let mut analyzer = BpmAnalyzer::new(SAMPLE_RATE, None)?;

    let mut link_manager = LinkManager::new();

    let mut audio_capture = None;

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
                                None,
                                SAMPLE_RATE,
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
            }
        }

        // Use recv_timeout to allow checking commands and updating UI even if no audio comes in
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(AudioMessage::Samples(packet)) => {
                if is_enabled {
                    new_samples_accumulator.extend(packet);

                    if new_samples_accumulator.len() >= HOP_SIZE {
                        let mut bpm_to_send: Option<f32> = None;
                        let mut is_drop_to_send = false;

                        if let Ok(Some(result)) = analyzer.process(&new_samples_accumulator) {
                            bpm_to_send = Some(result.bpm);
                            is_drop_to_send = result.is_drop;

                            // Sync Ableton Link
                            link_manager.update_tempo(
                                result.bpm as f64,
                                result.is_drop,
                                result.beat_offset,
                            );
                            println!(
                                "BPM: {:.1} | Drop: {} | Conf: {:.2} | CoarseConf: {:.2} | Energy: {:.4} | Avg: {:.4}",
                                result.bpm,
                                result.is_drop,
                                result.confidence,
                                result.coarse_confidence,
                                result.energy,
                                result.average_energy,
                            );
                        }

                        // Send update to GUI
                        let _ = tx.send(GuiUpdate {
                            bpm: bpm_to_send,
                            is_drop: is_drop_to_send,
                            num_peers: link_manager.num_peers(),
                        });
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
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // No audio received (expected if disabled)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        // Periodic UI update (for peer count) if we haven't sent one recently
        if last_ui_update.elapsed() > Duration::from_millis(200) {
            let _ = tx.send(GuiUpdate {
                bpm: None, // Reset BPM display if no analysis
                is_drop: false,
                num_peers: link_manager.num_peers(),
            });
            last_ui_update = Instant::now();
        }
    }
    Ok(())
}
