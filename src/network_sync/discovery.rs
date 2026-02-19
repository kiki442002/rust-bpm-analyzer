use super::protocol::{MULTICAST_ADDR, MULTICAST_PORT, NetworkMessage};
use if_addrs::get_if_addrs;
use serde_json;
use socket2::{Domain, Protocol, Socket, Type};
use std::error::Error;
use std::net::{IpAddr, Ipv4Addr, SocketAddrV4, UdpSocket};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

pub struct NetworkManager {
    #[allow(dead_code)]
    socket: UdpSocket,
    receiver: Receiver<NetworkMessage>,
    device_id: String,
    device_name: String,
    // Keep a list of sockets for sending messages to all interfaces
    send_sockets: Vec<UdpSocket>,
}

impl NetworkManager {
    /// Creates a new NetworkManager and starts the listening thread.
    ///
    /// # Arguments
    /// * `device_id` - A unique identifier for this device.
    /// * `device_name` - A human-readable name for this device.
    pub fn new(device_id: String, device_name: String) -> Result<Self, Box<dyn Error>> {
        let multi_addr: Ipv4Addr = MULTICAST_ADDR.parse()?;
        let socket_addr = SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), MULTICAST_PORT);

        // Using socket2 for better control (SO_REUSEADDR, SO_REUSEPORT)
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;

        // Allow multiple sockets to bind to the same port
        socket.set_reuse_address(true)?;
        #[cfg(not(target_os = "windows"))]
        socket.set_reuse_port(true)?;

        // This is critical for Linux: bind to the address, then join multicast
        socket.bind(&socket_addr.into())?;

        // Iterate over all interfaces to join multicast group on each
        if let Ok(interfaces) = get_if_addrs() {
            for iface in interfaces {
                if !iface.is_loopback() {
                    if let IpAddr::V4(ipv4) = iface.addr.ip() {
                        let _ = socket.join_multicast_v4(&multi_addr, &ipv4);
                    }
                }
            }
        }
        // Fallback or specific join (0.0.0.0 often default interface)
        let _ = socket.join_multicast_v4(&multi_addr, &Ipv4Addr::new(0, 0, 0, 0));
        socket.set_multicast_loop_v4(true)?;

        // Convert back to std::net::UdpSocket
        let socket: UdpSocket = socket.into();

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

        // Create sockets for sending on each interface
        let mut send_sockets = Vec::new();

        // One standard socket for default route
        if let Ok(s) = UdpSocket::bind("0.0.0.0:0") {
            if let Err(e) = s.set_multicast_loop_v4(true) {
                eprintln!("Failed to set multicast loop v4: {}", e);
            }
            send_sockets.push(s);
        }

        // Try to create specific sockets bound to each interface IP to force sending from there
        if let Ok(interfaces) = get_if_addrs() {
            for iface in interfaces {
                if !iface.is_loopback() {
                    if let IpAddr::V4(ipv4) = iface.addr.ip() {
                        if let Ok(s) = UdpSocket::bind(SocketAddrV4::new(ipv4, 0)) {
                            if let Err(e) = s.set_multicast_loop_v4(true) {
                                eprintln!("Failed to set multicast loop v4 on {:?}: {}", ipv4, e);
                            }
                            // Optionally set outgoing interface if supported/needed
                            // s.set_multicast_if_v4(&ipv4).ok();
                            send_sockets.push(s);
                            println!("Bound send socket to interface: {}", ipv4);
                        }
                    }
                }
            }
        }

        let manager = Self {
            socket,
            receiver: rx_in,
            device_id,
            device_name,
            send_sockets,
        };

        // Announce presence immediately
        if let Err(e) = manager.announce_presence(true) {
            eprintln!("Failed to announce presence: {}", e);
        }

        Ok(manager)
    }

    /// Sends a message to the multicast group via ALL interfaces.
    pub fn send(&self, msg: NetworkMessage) -> Result<(), Box<dyn Error>> {
        let json = serde_json::to_vec(&msg)?;
        let addr = format!("{}:{}", MULTICAST_ADDR, MULTICAST_PORT);

        // Broadcast on all sockets
        for s in &self.send_sockets {
            let _ = s.send_to(&json, &addr);
        }

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
