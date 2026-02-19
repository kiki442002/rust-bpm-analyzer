use super::protocol::{MULTICAST_ADDR, MULTICAST_PORT, NetworkMessage};
use serde_json;
use std::error::Error;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

pub struct NetworkManager {
    socket: UdpSocket,
    receiver: Receiver<NetworkMessage>,
    device_id: String,
    device_name: String,
}

impl NetworkManager {
    /// Creates a new NetworkManager and starts the listening thread.
    ///
    /// # Arguments
    /// * `device_id` - A unique identifier for this device.
    /// * `device_name` - A human-readable name for this device.
    pub fn new(device_id: String, device_name: String) -> Result<Self, Box<dyn Error>> {
        // Create a UDP socketbound to the multicast port
        // Note: On some OS (Windows), you might need SO_REUSEADDR to allow multiple apps to bind same port.
        // Rust std lib doesn't expose it directly easily, but for now we bind 0.0.0.0:44200
        let socket = UdpSocket::bind(format!("0.0.0.0:{}", MULTICAST_PORT))?;

        // Join the multicast group
        let multi_addr: Ipv4Addr = MULTICAST_ADDR.parse()?;
        // Join on all interfaces (0.0.0.0) or specific if needed.
        // Some OS require specific interface IP. 0.0.0.0 usually works for "any".
        socket.join_multicast_v4(&multi_addr, &Ipv4Addr::new(0, 0, 0, 0))?;

        // Loopback is often useful for testing on the same machine
        socket.set_multicast_loop_v4(true)?;

        let socket_clone = socket.try_clone()?;
        let (tx_in, rx_in) = mpsc::channel();

        // Spawn listener thread
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match socket_clone.recv_from(&mut buf) {
                    Ok((size, _src)) => {
                        let data = &buf[..size];
                        if let Ok(msg) = serde_json::from_slice::<NetworkMessage>(data) {
                            let _ = tx_in.send(msg);
                        }
                    }
                    Err(e) => {
                        eprintln!("Network receive error: {}", e);
                    }
                }
            }
        });

        let manager = Self {
            socket,
            receiver: rx_in,
            device_id,
            device_name,
        };

        // Announce presence immediately
        if let Err(e) = manager.announce_presence(true) {
            eprintln!("Failed to announce presence: {}", e);
        }

        Ok(manager)
    }

    /// Sends a message to the multicast group.
    pub fn send(&self, msg: NetworkMessage) -> Result<(), Box<dyn Error>> {
        let json = serde_json::to_vec(&msg)?;
        let addr = format!("{}:{}", MULTICAST_ADDR, MULTICAST_PORT);
        self.socket.send_to(&json, addr)?;
        Ok(())
    }

    /// Non-blocking receive of the next message from the network.
    pub fn try_recv(&self) -> Result<NetworkMessage, TryRecvError> {
        self.receiver.try_recv()
    }

    /// Helper to announce presence
    pub fn announce_presence(&self, online: bool) -> Result<(), Box<dyn Error>> {
        self.send(NetworkMessage::Presence {
            id: self.device_id.clone(),
            name: self.device_name.clone(),
            online,
        })
    }
}
