use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use std::error::Error;
use std::sync::mpsc;

#[derive(Debug, Clone)]
pub enum MidiEvent {
    NoteOn {
        channel: u8,
        note: u8,
        velocity: u8,
    },
    ControlChange {
        channel: u8,
        controller: u8,
        value: u8,
    },
}

pub struct MidiManager {
    // We hold the connection to keep it alive
    _in_conn: Option<MidiInputConnection<()>>,
    out_conn: Option<MidiOutputConnection>,
    receiver: mpsc::Receiver<MidiEvent>,
}

impl MidiManager {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let (tx, rx) = mpsc::channel();

        // --- INPUT ---
        let mut midi_in = MidiInput::new("Rust BPM Analyzer Input")?;
        midi_in.ignore(Ignore::None);

        let in_ports = midi_in.ports();
        let _in_conn = if let Some(in_port) = in_ports.first() {
            println!(
                "Opening connection to MIDI Input port: {}",
                midi_in.port_name(in_port)?
            );

            let conn = midi_in.connect(
                in_port,
                "midir-read-input",
                move |_stamp, message, _| {
                    if message.len() >= 3 {
                        let status = message[0];
                        let data1 = message[1];
                        let data2 = message[2];

                        let channel = status & 0x0F;
                        let msg_type = status & 0xF0;

                        let event = match msg_type {
                            0x90 if data2 > 0 => Some(MidiEvent::NoteOn {
                                channel,
                                note: data1,
                                velocity: data2,
                            }),
                            0xB0 => Some(MidiEvent::ControlChange {
                                channel,
                                controller: data1,
                                value: data2,
                            }),
                            _ => None,
                        };

                        if let Some(e) = event {
                            let _ = tx.send(e);
                        }
                    }
                },
                (),
            )?;

            Some(conn)
        } else {
            None
        };

        // --- OUTPUT ---
        let midi_out = MidiOutput::new("Rust BPM Analyzer Output")?;
        let out_ports = midi_out.ports();
        let out_conn = if let Some(out_port) = out_ports.first() {
            println!(
                "Opening connection to MIDI Output port: {}",
                midi_out.port_name(out_port)?
            );
            match midi_out.connect(out_port, "midir-write-output") {
                Ok(c) => Some(c),
                Err(e) => {
                    eprintln!("Failed to connect MIDI output: {}", e);
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            _in_conn,
            out_conn,
            receiver: rx,
        })
    }

    pub fn try_recv(&self) -> Result<MidiEvent, mpsc::TryRecvError> {
        self.receiver.try_recv()
    }

    pub fn send_note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        if let Some(conn) = &mut self.out_conn {
            let status = 0x90 | (channel & 0x0F);
            let _ = conn.send(&[status, note, velocity]);
        }
    }

    pub fn send_control_change(&mut self, channel: u8, controller: u8, value: u8) {
        if let Some(conn) = &mut self.out_conn {
            let status = 0xB0 | (channel & 0x0F);
            let _ = conn.send(&[status, controller, value]);
        }
    }
}
