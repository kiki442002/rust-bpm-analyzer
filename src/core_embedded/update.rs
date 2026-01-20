#[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
pub mod update {
    use libc;
    use self_update::backends::github::Update;
    use self_update::cargo_crate_version;
    use std::fs;
    use std::os::unix::process::CommandExt;
    use std::path::PathBuf;

    pub struct Updater {
        repo_owner: String,
        repo_name: String,
        bin_name: String,
        backup_path: PathBuf,
    }

    impl Updater {
        /// Vérifie s'il existe une mise à jour disponible sur GitHub sans l'appliquer.
        pub fn check(&self) -> Result<Option<String>, Box<dyn std::error::Error>> {
            let status = self_update::backends::github::Update::configure()
                .repo_owner(&self.repo_owner)
                .repo_name(&self.repo_name)
                .bin_name(&self.bin_name)
                .target("aarch64-unknown-linux-gnu") // Cible explicite
                .show_download_progress(false)
                .current_version(cargo_crate_version!())
                .build()? // construit la config
                .get_latest_release()?;

            if status.version != cargo_crate_version!() {
                Ok(Some(status.version))
            } else {
                Ok(None)
            }
        }
        pub fn new(repo_owner: &str, repo_name: &str, bin_name: &str) -> Self {
            let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from(bin_name));
            let backup_path = exe.with_extension("bak");
            Updater {
                repo_owner: repo_owner.to_string(),
                repo_name: repo_name.to_string(),
                bin_name: bin_name.to_string(),
                backup_path,
            }
        }

        pub fn check_and_update(&self) -> Result<(), Box<dyn std::error::Error>> {
            let exe = std::env::current_exe()?;
            // Sauvegarde l'ancien binaire
            fs::copy(&exe, &self.backup_path)?;

            let status = self_update::backends::github::Update::configure()
                .repo_owner(&self.repo_owner)
                .repo_name(&self.repo_name)
                .bin_name(&self.bin_name)
                .target("aarch64-unknown-linux-gnu") // Cible explicite
                .no_confirm(true) // Ne pas demander confirmation
                .show_download_progress(true)
                .current_version(cargo_crate_version!())
                .build()? // construit la config
                .update(); // lance la mise à jour

            match status {
                Ok(status) if status.updated() => {
                    println!("Mise à jour réussie en version {} !", status.version());
                    self.restart()?;
                }
                Ok(_) => {
                    println!("Aucune mise à jour disponible.");
                }
                Err(e) => {
                    println!(
                        "Erreur lors de la mise à jour : {}. Restauration de l'ancien binaire...",
                        e
                    );
                    self.rollback()?;
                }
            }
            Ok(())
        }

        fn restart(&self) -> Result<(), Box<dyn std::error::Error>> {
            let exe = std::env::current_exe()?;
            unsafe {
                std::process::Command::new(&exe)
                    .before_exec(|| {
                        libc::setsid();
                        Ok(())
                    })
                    .spawn()?;
                std::process::exit(0);
            }
        }

        pub fn rollback(&self) -> Result<(), Box<dyn std::error::Error>> {
            let exe = std::env::current_exe()?;
            if self.backup_path.exists() {
                fs::copy(&self.backup_path, &exe)?;
                println!("Rollback effectué : ancien binaire restauré.");
            } else {
                println!("Aucun backup trouvé pour rollback.");
            }
            Ok(())
        }
    }
}
