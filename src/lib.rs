//! **B**asic **A**utomated **C**loud **K**eeper for **U**ltimate **P**ersistence
//! aka. BACKUP.rs

use std::path::Path;
use flate2::Compression;
use flate2::write::GzEncoder;
use chrono;
use mega::Node;
use tokio_util::compat::TokioAsyncReadCompatExt;
use utils::SettingsEnv;
use log::{info, error, debug};

mod utils;
mod error;

const SETTINGS_FILE: &str = "./settings.json";

/// TODO! Document this
/// 
/// Note: `mega_client.logout()` is called when the struct is dropped.
struct BackupClient {
    mega_client: mega::Client,
    dropped: bool
}

impl BackupClient {
    pub fn default() -> Self {
        let http_client = reqwest::Client::new();
        let client = mega::Client::builder().build(http_client).unwrap();
        BackupClient {
            mega_client: client,
            dropped: false
        }
    }

    pub async fn login(&mut self, email: &str, password: &str, mfa: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
        info!("Logging in with email: {email}...");
        self.mega_client.login(email, password, mfa).await?;
        Ok(())
    }

    pub async fn logout(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Logging out...");
        // TODO: For some reason `Drop` is not calling (or waiting for) this function to finish.
        self.mega_client.logout().await?;
        Ok(())
    }

    // For some reason if you make `try_logout` a public function, `Drop` will not be able
    // to call this function, therefore it won't be able to log out when client goes out of scope.
    // tokio::spawn will just go nuts. But why?
    async fn try_logout(&mut self) {
        debug!("Trying to log out...");
        match self.logout().await {
            Ok(()) => info!("Successfully log out."),
            Err(e) => error!("Logout error: {:?}", e)
        }
    }

    /// Uploads file to an already created folder in MEGA drive.
    /// 
    /// # Arguments
    /// 
    /// * `file_name` - A string slice with the name of the file to upload
    /// * `dest_folder` - A string slice with the name of the destination directory, which must be already created in the drive
    pub async fn upload_file(&self, file_name: &str, dest_folder: &str) -> Result<(), Box<dyn std::error::Error>> {
        let nodes = self.mega_client.fetch_own_nodes().await?;
        let file_name = Path::new(file_name).file_name().unwrap().to_str().unwrap();
        let dest_folder_node: &Node;

        // Only check folder nodes if not trying to upload to root folder.
        // TODO: Upload by path? Therefore can upload files if there are 
        // multiple folders with same name.
        if dest_folder != "/" {
            // Filters nodes to only contain folders with the name of `dest_folder`.
            let folder_nodes: Vec<&Node> = nodes.iter().filter(|&node| {
                node.name() == dest_folder && node.kind() == mega::NodeKind::Folder
            }).collect();

            if folder_nodes.len() > 1 { return Err(error::UploadError::MultipleFoldersError.into()); }
            else if folder_nodes.len() < 1 { return Err(error::UploadError::NoFolderError.into()); }
            
            // The node of `dest_folder` must be the only one in the vector.
            dest_folder_node = folder_nodes.first().unwrap();
        } else {
            dest_folder_node = nodes.cloud_drive().unwrap();
        }

        // Check if a file with the same name is already uploaded in the same folder.
        let file_nodes : Vec<_> = nodes.iter().filter(|&node| { 
            node.name() == file_name && 
            node.kind() == mega::NodeKind::File && 
            node.parent() == Some(dest_folder_node.handle())
        }).collect();

        // If there is a file with the same name in the same folder, return an error.
        if file_nodes.len() > 0 { 
            return Err(error::MEGAFileExistsError{ file_name: String::from(file_name) }.into()); 
        }

        // Open file and read size to specify the length of the progress bar.
        let file = tokio::fs::File::open(file_name).await?;
        let size = file.metadata().await?.len();

        self.mega_client.upload_node(
            &dest_folder_node,
            file_name,
            size,
            file.compat(),
            mega::LastModified::Now,
        ).await?;

        Ok(())
    }
}

// When the client goes out of scope, user is gracefully logout first.
// First thought would be to call std::mem::take, which leaves a default
// in its place, but this runs into a problem; you'll end up with a stack 
// overflow calling drop. So, we have to use a flag to indicate it's been dropped.
// For more info, see: https://stackoverflow.com/questions/71541765/rust-async-drop
// It is necessary to drop `client` and initiate a logout, because if we stay logged in,
// there will be a lot of open sessions to the MEGA account (You can see it in
// MEGA --> Settings --> Session history).
// TODO! Please check whether async drop is already implemented in Rust:
// https://rust-lang.github.io/async-fundamentals-initiative/index.html
impl Drop for BackupClient {
    fn drop(&mut self) {
        if !self.dropped {
            debug!("Found `BackupClient` out of scope not dropped, dropping it...");
            let mut this = BackupClient::default();
            // `self` would escape the method body, therefore it is necessary to
            // swap the values.
            std::mem::swap(&mut this, self);
            this.dropped = true;
            debug!("Spawning logout task...");
            tokio::spawn(async move { 
                debug!("Spawned thread logging out!");
                this.try_logout().await
            });
        }
    }
}

/// Creates a tar.gz archive from all the folder paths that are given in `dirs`.
/// 
/// # Arguments
/// 
/// * `dirs` - A vector of string slices that holds the path of directories
/// * `file_name` - A string slice that holds the name of the archive with file extension
/// 
/// # Errors
///
/// * Returns `io::ErrorKind::AlreadyExists` if there is already a file with the name `file_name` 
fn create_tarball_from_dirs(dirs: Vec<String>, file_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Check if file already exists.
    match Path::new(file_name).try_exists() {
        Ok(true) => return Err(error::TarballExistsError{file_name: String::from(file_name)}.into()),
        Ok(false) => (),
        Err(e) => return Err(e.into())
    };

    // Create the archive file.
    let tar_gz = std::fs::File::create(file_name)?;
    let enc = GzEncoder::new(tar_gz, Compression::best());
    let mut tar = tar::Builder::new(enc);

    // Loop through each folder and append them to the archive.

    for dir in dirs.iter() {
        // Get folder's name and path separately.
        let dir_path = Path::new(&dir);
        let dir_name = dir_path.file_name().unwrap();

        // Appends the directory with all its file.
        tar.append_dir_all(dir_name, &dir).unwrap();
    }

    Ok(())
}

#[tokio::main]
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let SettingsEnv { 
        email: email_decoded, password: pass_decoded, dirs_to_backup
    } = utils::read_auth_info(SETTINGS_FILE).unwrap();

    // Set archive's file name related to current date.
    let today_date = format!("{}", chrono::offset::Local::now().format("%Y-%m-%d"));
    let file_name = format!("backup{}.tar.gz", today_date);

    info!("Creating tarball from dirs:");
    dirs_to_backup.iter().for_each(|x| { info!("\t{}", x) });

    create_tarball_from_dirs(dirs_to_backup, &file_name)?;
    info!("Created tarball successfully.");
    info!("Uploading file to MEGA.");

    let mfa: Option<&str> = None;

    let mut client = BackupClient::default();

    client.login(&email_decoded, &pass_decoded, mfa).await?;

    match client.upload_file(&file_name, "Backups").await {
        Ok(()) => {
            // As long as Drop is not properly implemented, 
            // log out after successfully uploading a file.
            client.try_logout().await
        },
        Err(e) => {
            // Cleanup before returning error to main.
            error!("Error encountered in `upload_file`, starting cleanup...");
            error!("Trying to log out...");
            client.try_logout().await;
            error!("Removing archive file...");
            std::fs::remove_file(&file_name)?;
            error!("Successfully removed archive file...");
            return Err(e);
        }
    };

    info!("Uploaded file successfully.");

    info!("Removing archive file...");
    std::fs::remove_file(file_name)?;
    info!("Successfully removed archive file...");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_tarball() {
        // Create an archive of the source folder, therefore
        // this test can be run anytime, since `src` must exist
        // to build this binary.
        let dirs = vec![String::from("./src")];
        let file_name = "testarchive.tar.gz";
        create_tarball_from_dirs(dirs, file_name).unwrap();

        let file_path = Path::new(file_name);

        assert!(file_path.exists());

        // Remove file so that the test doesn't have side effects.
        std::fs::remove_file(file_name).unwrap();
        assert!(!file_path.exists())
    }

    #[tokio::test]
    async fn authentication() {
        let SettingsEnv { 
            email: email_decoded, password: pass_decoded , ..
        } = utils::read_auth_info(SETTINGS_FILE).unwrap();

        let mut client = BackupClient::default();
        client.login(&email_decoded, &pass_decoded, None).await
            .expect("Failure while logging in...");

        client.logout().await.expect("Failure while logging out...");
    }

    #[tokio::test]
    async fn upload_remove_file() {
        let SettingsEnv { 
            email: email_decoded, password: pass_decoded , ..
        } = utils::read_auth_info(SETTINGS_FILE).unwrap();

        let mut client = BackupClient::default();
        client.login(&email_decoded, &pass_decoded, None).await
            .expect("Failure while logging in...");


        // Uploading README.md because that's a file that must exist.
        // TODO: `client.upload_file` should return uploaded file's Node,
        // so it can be easier to delete it later (or do anything else with it).
        // TODO: Create a randomly generated file (with random filename) to upload.
        client.upload_file("README.md", "/").await
            .expect("Uploading file has failed...");

        let nodes = client.mega_client.fetch_own_nodes().await
            .expect("Couldn't fetch own nodes.");

        let node = nodes.get_node_by_path("/Root/README.md")
            .expect("Couldn't get node by path...");

        client.mega_client.delete_node(node).await
            .expect("Couldn't delete node...");

        let nonexistent_node = nodes.get_node_by_path("/README.md");
        assert_eq!(nonexistent_node, None);

        // FIXME: Explicit logouts are only necessary, until `Drop` is properly implemented.
        client.logout().await.expect("Failure while logging out...");
    }
    
}