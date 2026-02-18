#[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
pub mod usb {
    use std::io;
    use std::os::unix::io::RawFd;
    use tokio::io::unix::AsyncFd;
    use tokio::process::Command;

    // Constantes Netlink pour KOBJECT_UEVENT
    const NETLINK_KOBJECT_UEVENT: i32 = 15; // La valeur est 15 (NETLINK_KOBJECT_UEVENT) dans la plupart des headers kernel, parfois 31
    // Vérifions la valeur standard linux/netlink.h
    // #define NETLINK_KOBJECT_UEVENT 15

    // Structure sockaddr_nl pour bind
    #[repr(C)]
    struct SockAddrNl {
        nl_family: u16,
        nl_pad: u16,
        nl_pid: u32,
        nl_groups: u32,
    }

    pub struct UeventListener {
        fd: AsyncFd<RawFd>,
    }

    impl UeventListener {
        pub fn new() -> io::Result<Self> {
            unsafe {
                let fd = libc::socket(
                    libc::AF_NETLINK,
                    libc::SOCK_RAW | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
                    NETLINK_KOBJECT_UEVENT,
                );

                if fd < 0 {
                    return Err(io::Error::last_os_error());
                }

                let mut addr = SockAddrNl {
                    nl_family: libc::AF_NETLINK as u16,
                    nl_pad: 0,
                    nl_pid: std::process::id(), // Notre PID
                    nl_groups: 1, // Multicast group 1 (kernel broadcast) - bitmask pour le groupe 1
                };

                // Bind socket
                let ret = libc::bind(
                    fd,
                    &mut addr as *mut _ as *mut libc::sockaddr,
                    std::mem::size_of::<SockAddrNl>() as libc::socklen_t,
                );

                if ret < 0 {
                    let err = io::Error::last_os_error();
                    libc::close(fd);
                    return Err(err);
                }

                Ok(Self {
                    fd: AsyncFd::new(fd)?,
                })
            }
        }

        pub async fn next_event(&mut self) -> io::Result<String> {
            loop {
                let mut guard = self.fd.readable().await?;
                let mut buf = [0u8; 8192];
                match guard.try_io(|inner_fd| unsafe {
                    let n = libc::recv(
                        *inner_fd.get_ref(),
                        buf.as_mut_ptr() as *mut libc::c_void,
                        buf.len(),
                        0,
                    );
                    if n < 0 {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(n as usize)
                    }
                }) {
                    Ok(Ok(n)) => {
                        // Parser le buffer en string, remplacer les nulls par des newlines pour debug
                        // Format UEVENT: "add@/devices/...\0ACTION=add\0DEVPATH=...\0..."
                        let s = String::from_utf8_lossy(&buf[..n]);
                        return Ok(s.to_string());
                    }
                    Ok(Err(e)) => return Err(e),
                    Err(_would_block) => continue, // Spurious wakeup
                }
            }
        }
    }

    async fn run_usb_script(action: &str, devpath: &str) {
        println!("USB Event detected: Action={} DevPath={}", action, devpath);

        let script = "/mnt/system/usb.sh";

        let child = Command::new("sh").arg(script).spawn();

        match child {
            Ok(mut c) => match c.wait().await {
                Ok(status) => println!("USB plug script finished: {}", status),
                Err(e) => eprintln!("Error waiting for USB plug script: {}", e),
            },
            Err(e) => eprintln!("Failed to spawn USB plug script '{}': {}", script, e),
        }
    }

    fn parse_env(uevent: &str, key: &str) -> Option<String> {
        // uevent contient des KEY=VAL séparés par \0.
        // String::from_utf8_lossy remplace \0 par \u{FFFD} ou conserve si c'est printable?
        // Ah, from_utf8_lossy va garder les \0 s'ils sont dans les bytes.
        // Mais attention, &str en Rust ne peut pas contenir de null bytes intermédiaires facilement manipulables comme en C.
        // Actually, Rust strings CAN contain null bytes.

        for line in uevent.split('\0') {
            if line.starts_with(key) && line.chars().nth(key.len()) == Some('=') {
                return Some(line[key.len() + 1..].to_string());
            }
        }
        None
    }

    pub async fn listen_usb_events() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut listener = match UeventListener::new() {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Impossible d'ouvrir le socket Netlink Uevent: {}", e);
                return Ok(());
            }
        };

        println!("Écoute des événements USB matériels (Netlink KOBJECT_UEVENT)...");

        loop {
            match listener.next_event().await {
                Ok(event_str) => {
                    // Vérifier si c'est un événement USB
                    // On cherche SUBSYSTEM=usb et DEVTYPE=usb_device
                    let subsystem = parse_env(&event_str, "SUBSYSTEM");
                    let devtype = parse_env(&event_str, "DEVTYPE");
                    let action = parse_env(&event_str, "ACTION");
                    let devpath = parse_env(&event_str, "DEVPATH");

                    // println!("DEBUG UEVENT: {:?}", event_str); // Très verbeux

                    if let (Some(sub), Some(dtype), Some(act)) = (subsystem, devtype, action) {
                        if sub == "usb" && dtype == "usb_device" && act == "add" {
                            // C'est un branchement de périphérique USB ! (Hub ou Device)
                            if let Some(path) = devpath {
                                run_usb_script("add", &path).await;
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Erreur lecture Uevent: {}", e);
                    // Petit délai pour éviter boucle infinie en cas d'erreur persistante
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
            }
        }
    }
}
