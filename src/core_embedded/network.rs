#[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
pub mod network {
    use crate::core_embedded::display::display::{BpmDisplay, StatusBarIcon};
    use crate::core_embedded::update::update::Updater;
    use futures::StreamExt;
    use netlink_packet_core::NetlinkPayload;
    use netlink_packet_route::RouteNetlinkMessage;
    use netlink_packet_route::link::LinkAttribute;
    use rtnetlink::new_connection;
    use rtnetlink::sys::AsyncSocket;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use tokio::process::Command;
    use tokio::time::{Duration, timeout};

    // Flag statique pour empêcher l'exécution simultanée multiple
    static IS_CHECKING_UPDATE: AtomicBool = AtomicBool::new(false);

    async fn check_internet_and_update(display: Option<Arc<Mutex<BpmDisplay>>>, updater: Updater) {
        // Si une vérification est déjà en cours, on annule celle-ci
        if IS_CHECKING_UPDATE
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            println!("Une vérification Internet/Update est déjà en cours. Ignoré.");
            return;
        }

        println!("Vérification accès Internet (tentatives pdt 10s)...");
        let start = std::time::Instant::now();
        // On augmente à 10s comme demandé
        let max_duration = Duration::from_secs(10);
        let mut success = false;

        // On vérifie tant que le temps n'est pas écoulé
        while start.elapsed() < max_duration {
            let ping = Command::new("ping")
                .args(&["-c", "1", "-W", "1", "8.8.8.8"])
                .output()
                .await;

            if let Ok(output) = ping {
                if output.status.success() {
                    success = true;
                    break;
                }
            }
            // Petite pause
            tokio::time::sleep(Duration::from_millis(1000)).await;
        }

        if success {
            println!("Ping Internet: SUCCÈS");
            if let Some(disp_arc) = &display {
                if let Ok(mut disp) = disp_arc.lock() {
                    let _ = disp.draw_status_icon(StatusBarIcon::Internet);
                    let _ = disp.flush();
                }
            }

            println!("Vérification des mises à jour...");
            // match updater.check() {
            //     Ok(Some(new_version)) => {
            //         println!("Mise à jour disponible : {}", new_version);
            //         if let Some(disp_arc) = &display {
            //             if let Ok(mut disp) = disp_arc.lock() {
            //                 let _ = disp.draw_status_icon(StatusBarIcon::Update);
            //                 let _ = disp.flush();
            //             }
            //         }
            //     }
            //     Ok(None) => println!("Pas de mise à jour."),
            //     Err(e) => eprintln!("Erreur check update: {}", e),
            // }
            match updater.check_and_update() {
                Ok(_) => {
                    println!("Mise à jour réussie !");
                }
                Err(e) => {
                    eprintln!("Erreur lors de la mise à jour : {}", e);
                }
            }
        } else {
            println!("Ping Internet: ÉCHEC (Timeout 10s)");
        }

        // On libère le flag à la fin
        IS_CHECKING_UPDATE.store(false, Ordering::SeqCst);
    }

    fn update_link_status(
        display: &Option<Arc<Mutex<BpmDisplay>>>,
        name: &str,
        is_up: bool,
        updater: Option<Updater>,
    ) {
        if name != "eth0" && name != "usb0" {
            // On ne gère que eth0 et usb0
            return;
        }
        if !is_up {
            if let Some(disp_arc) = display {
                if let Ok(mut disp) = disp_arc.lock() {
                    if name == "usb0" {
                        let _ = disp.clear_status_icon(StatusBarIcon::Usb);
                    } else {
                        let _ = disp.clear_status_icon(StatusBarIcon::Ethernet);
                    }
                    let _ = disp.flush();
                }
            }
            return;
        } else {
            if let Some(disp_arc) = display {
                if let Ok(mut disp) = disp_arc.lock() {
                    if name == "usb0" {
                        let _ = disp.draw_status_icon(StatusBarIcon::Usb);
                    } else {
                        let _ = disp.draw_status_icon(StatusBarIcon::Ethernet);
                    }
                    let _ = disp.flush();
                }
            }
        }
    }

    fn extract_link_info(
        link_msg: &netlink_packet_route::link::LinkMessage,
    ) -> (Option<String>, bool) {
        let name = link_msg.attributes.iter().find_map(|attr| match attr {
            LinkAttribute::IfName(name) => Some(name.clone()),
            _ => None,
        });

        // Vérification du flag IFF_LOWER_UP (bit 16) pour confirmer que le lien physique est actif (câble branché)
        // IFF_UP (bit 0) indique seulement que l'interface est "administrativement" activée.
        let flags = link_msg.header.flags.bits();
        let is_up = (flags & 1) != 0; // IFF_UP
        let is_lower_up = (flags & 65536) != 0; // IFF_LOWER_UP (0x10000)

        // On considère l'interface "active" si elle est Admin UP ET Physique UP
        (name, is_up && is_lower_up)
    }

    /// Écoute les changements d'état des interfaces réseau et affiche UP/DOWN
    pub async fn listen_interface_events(
        display: Option<Arc<Mutex<BpmDisplay>>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (mut connection, handle, mut messages) = new_connection()?;

        // Souscription au groupe multicast RTNLGRP_LINK (1)
        // Nécessite le trait rtnetlink::sys::AsyncSocket en scope
        connection
            .socket_mut()
            .socket_mut()
            .add_membership(1)
            .map_err(|e| format!("Add membership error: {}", e))?;

        tokio::spawn(connection);

        let updater = Updater::new("kiki442002", "rust-bpm-analyzer", "rust-bpm-analyzer");

        let mut iface_map: HashMap<u32, String> = HashMap::new();
        // 1. Scan initial des interfaces existantes
        println!("Scan initial des interfaces réseau...");
        let mut links = handle.link().get().execute();
        while let Some(msg_result) = links.next().await {
            match msg_result {
                Ok(link_msg) => {
                    let (name_opt, is_up) = extract_link_info(&link_msg);
                    if let Some(name) = name_opt {
                        iface_map.insert(link_msg.header.index, name.clone());
                        println!(
                            "Initial: Interface {} is {}",
                            name,
                            if is_up { "UP" } else { "DOWN" }
                        );
                        if name == "eth0" && is_up {
                            tokio::spawn(check_internet_and_update(
                                display.clone(),
                                updater.clone(),
                            ));
                        }
                        update_link_status(&display, &name, is_up, Some(updater.clone()));
                    }
                }
                Err(e) => eprintln!("Erreur lors du scan initial: {}", e),
            }
        }

        // 2. Boucle d'événements (changements dynamiques)
        println!("En attente d'événements Netlink...");
        while let Some((message, _)) = messages.next().await {
            // Dans les versions récentes avec netlink-packet-route, le payload est du type RouteNetlinkMessage
            // encapsulé dans NetlinkPayload::InnerMessage
            match message.payload {
                NetlinkPayload::InnerMessage(RouteNetlinkMessage::NewLink(link_msg)) => {
                    let (name_opt, is_up) = extract_link_info(&link_msg);

                    // Gestion du cache nom d'interface
                    let name_final = if let Some(n) = name_opt {
                        iface_map.insert(link_msg.header.index, n.clone());
                        Some(n)
                    } else {
                        iface_map.get(&link_msg.header.index).cloned()
                    };

                    if let Some(name) = name_final {
                        println!(
                            "Event: Interface {} is {}",
                            name,
                            if is_up { "UP" } else { "DOWN" }
                        );
                        if name == "eth0" && is_up {
                            tokio::spawn(check_internet_and_update(
                                display.clone(),
                                updater.clone(),
                            ));
                        }
                        update_link_status(&display, &name, is_up, Some(updater.clone()));
                    } else {
                        // println!("DEBUG: Interface index {} changed but name unknown", link_msg.header.index);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}
