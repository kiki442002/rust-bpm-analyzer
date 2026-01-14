#[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
pub mod dhcp {
    use futures::stream::TryStreamExt;
    use rtnetlink::new_connection;

    /// Écoute les changements d'état des interfaces réseau et affiche UP/DOWN
    pub async fn listen_interface_events() -> Result<(), Box<dyn std::error::Error>> {
        let (connection, handle, mut messages) = new_connection()?;
        tokio::spawn(connection);

        println!("En attente d'événements Netlink...");
        while let Some((message, _)) = messages.try_next().await? {
            if let rtnetlink::packet::NetlinkMessage::NewLink(link_msg) = message {
                let ifname = link_msg.nlas.iter().find_map(|nla| {
                    if let rtnetlink::packet::rtnl::link::nlas::Nla::IfName(name) = nla {
                        Some(name.clone())
                    } else {
                        None
                    }
                });
                let is_up = link_msg.header.flags & libc::IFF_UP as u32 != 0;
                if let Some(name) = ifname {
                    println!(
                        "Interface {} is {}",
                        name,
                        if is_up { "UP" } else { "DOWN" }
                    );
                }
            }
        }
        Ok(())
    }
}
