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
    sender: mpsc::Sender<MidiEvent>,
}

impl MidiManager {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let (tx, rx) = mpsc::channel();
        let mut manager = Self {
            _in_conn: None,
            out_conn: None,
            receiver: rx,
            sender: tx,
        };

        // Try to connect to first available ports
        if let Ok((inputs, outputs)) = Self::list_ports() {
            if let Some(in_name) = inputs.first() {
                let _ = manager.select_input(in_name);
            }
            if let Some(out_name) = outputs.first() {
                let _ = manager.select_output(out_name);
            }
        }

        Ok(manager)
    }

    pub fn list_ports() -> Result<(Vec<String>, Vec<String>), Box<dyn Error>> {
        let midi_in = MidiInput::new("Rust BPM Analyzer List Input")?;
        let midi_out = MidiOutput::new("Rust BPM Analyzer List Output")?;

        let in_ports = midi_in
            .ports()
            .iter()
            .filter_map(|p| midi_in.port_name(p).ok())
            .collect();
        let out_ports = midi_out
            .ports()
            .iter()
            .filter_map(|p| midi_out.port_name(p).ok())
            .collect();

        Ok((in_ports, out_ports))
    }

    pub fn select_input(&mut self, port_name: &str) -> Result<(), Box<dyn Error>> {
        // Disconnect current input
        self._in_conn = None;

        let mut midi_in = MidiInput::new("Rust BPM Analyzer Input")?;
        midi_in.ignore(Ignore::None);

        let ports = midi_in.ports();
        let port = ports
            .iter()
            .find(|p| midi_in.port_name(p).unwrap_or_default() == port_name);

        if let Some(p) = port {
            println!("Opening connection to MIDI Input port: {}", port_name);
            let tx = self.sender.clone();
            let conn = midi_in.connect(
                p,
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
            self._in_conn = Some(conn);
        } else {
            println!("MIDI Input port not found: {}", port_name);
        }
        Ok(())
    }

    pub fn select_output(&mut self, port_name: &str) -> Result<(), Box<dyn Error>> {
        // Disconnect current output
        self.out_conn = None;

        let midi_out = MidiOutput::new("Rust BPM Analyzer Output")?;
        let ports = midi_out.ports();
        let port = ports
            .iter()
            .find(|p| midi_out.port_name(p).unwrap_or_default() == port_name);

        if let Some(p) = port {
            println!("Opening connection to MIDI Output port: {}", port_name);
            match midi_out.connect(p, "midir-write-output") {
                Ok(c) => self.out_conn = Some(c),
                Err(e) => eprintln!("Failed to connect MIDI output: {}", e),
            }
        } else {
            println!("MIDI Output port not found: {}", port_name);
        }
        Ok(())
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
