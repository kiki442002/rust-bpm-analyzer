#[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
pub mod update {
    use self_update::cargo_crate_version;
    use std::fs;
    use std::os::unix::process::CommandExt;
    use std::path::PathBuf;

    #[derive(Clone)]
    pub struct Updater {
        repo_owner: String,
        repo_name: String,
        bin_name: String,
        backup_path: PathBuf,
    }

    impl Updater {
        /// Vérifie s'il existe une mise à jour disponible sur GitHub via ReleaseList
        pub fn check(&self) -> Result<Option<String>, Box<dyn std::error::Error>> {
            // Configuration inspirée de l'exemple self_update
            let releases = self_update::backends::github::ReleaseList::configure()
                .repo_owner(&self.repo_owner)
                .repo_name(&self.repo_name)
                .build()?
                .fetch()?;

            // println!("found releases:");
            // println!("{:#?}\n", releases);

            if let Some(release) = releases.first() {
                if release.version != cargo_crate_version!() {
                    return Ok(Some(release.version.clone()));
                }
            }
            Ok(None)
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

            // Configuration de l'update selon l'exemple github
            let status = self_update::backends::github::Update::configure()
                .repo_owner(&self.repo_owner)
                .repo_name(&self.repo_name)
                .bin_name(&self.bin_name)
                .show_download_progress(true)
                .no_confirm(true)
                .current_version(cargo_crate_version!())
                .build()?
                .update()?;

            println!("Update status: `{}`!", status.version());

            if status.updated() {
                println!("Mise à jour réussie ! Redémarrage...");
                self.restart()?;
            } else {
                println!("Déjà à jour.");
            }
            Ok(())
        }

        fn restart(&self) -> Result<(), Box<dyn std::error::Error>> {
            let cur_dir = std::env::current_dir()?;
            // On utilise ./bin_name car current_exe() peut être invalide après update
            let exe = cur_dir.join(&self.bin_name);

            println!("Redémarrage de : {:?}", exe);
            let err = std::process::Command::new(&exe).exec();
            Err(Box::new(err))
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
