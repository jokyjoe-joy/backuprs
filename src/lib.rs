//! **B**asic **A**utomated **C**loud **K**eeper for **U**ltimate **P**ersistence
//! aka. BACKUP.rs

use std::path::Path;
use flate2::Compression;
use flate2::write::GzEncoder;
use chrono;
use std::sync::Arc;
use async_read_progress::TokioAsyncReadProgressExt;
use std::time::Duration;
use tokio_util::compat::TokioAsyncReadCompatExt;
use utils::AuthEnv;
use log::{info, error};

mod utils;
mod error;

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
        self.mega_client.login(email, password, mfa).await?;
        Ok(())
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
        
        // Filters nodes to only contain folders with the name of `dest_folder`.
        let folder_nodes: Vec<_> = nodes.iter().filter(|&node| {
            node.name() == dest_folder && node.kind() == mega::NodeKind::Folder
        }).collect();

        if folder_nodes.len() > 1 { return Err(error::UploadError::MultipleFoldersError.into()); }
        else if folder_nodes.len() < 1 { return Err(error::UploadError::NoFolderError.into()); }
        
        // The node of `dest_folder` must be the only one in the vector.
        let dest_folder_node = *folder_nodes.first().unwrap();

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

        // TODO! Remove progressbar?
        // Setting up progressbar
        let bar = indicatif::ProgressBar::new(size);
        bar.set_style(utils::progress_bar_style());
        bar.set_message(format!("Uploading {} to {}...", file_name, dest_folder_node.name()));
        let bar = Arc::new(bar);

        let reader = {
            let bar = bar.clone();
            file.report_progress(Duration::from_millis(100), move |bytes_read| {
                bar.set_position(bytes_read as u64);
            })
        };

        self.mega_client.upload_node(
            &dest_folder_node,
            file_name,
            size,
            reader.compat(),
            mega::LastModified::Now,
        ).await?;

        bar.finish_with_message(format!("{} uploaded to {} !", file_name, dest_folder_node.name()));

        Ok(())
    }
}

// When the client goes out of scope, user is gracefully logout first.
// First thought would be to call std::mem::take, which leaves a default
// in its place, but this runs into a problem; you'll end up with a stack 
// overflow calling drop. So, we have to use a flag to indicate it's been dropped.
// For more info, see: https://stackoverflow.com/questions/71541765/rust-async-drop
impl Drop for BackupClient {
    fn drop(&mut self) {
        if !self.dropped {
            let mut this = BackupClient::default();
            // `self` would escape the method body, therefore it is necessary to
            // swap the values.
            std::mem::swap(&mut this, self);
            this.dropped = true;
            tokio::spawn(async move { this.mega_client.logout().await });
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
fn create_tarball_from_dirs(dirs: Vec<&str>, file_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Check if file already exists.
    match Path::new(file_name).try_exists() {
        Ok(true) => return Err(error::TarballExistsError{file_name: String::from(file_name)}.into()),
        Ok(false) => (),
        Err(e) => return Err(e.into())
    };

    // Setting up progressbar
    let bar = indicatif::ProgressBar::new(dirs.len() as u64);
    bar.set_style(utils::progress_bar_style());
    bar.set_message("Creating tarball...");
    let bar = Arc::new(bar);

    // Create the archive file.
    let tar_gz = std::fs::File::create(file_name)?;
    let enc = GzEncoder::new(tar_gz, Compression::best());
    let mut tar = tar::Builder::new(enc);

    // Loop through each folder and append them to the archive.

    for (count, &dir) in dirs.iter().enumerate() {
        // Get folder's name and path separately.
        let dir_path = Path::new(dir);
        let dir_name = dir_path.file_name().unwrap();

        bar.set_position(count as u64);

        // Appends the directory with all its file.
        tar.append_dir_all(dir_name, dir).unwrap();
    }

    bar.finish_with_message(format!("Created a tarball with the name {} !", file_name));

    Ok(())
}

#[tokio::main(flavor = "current_thread")]
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    const DIRS_TO_BACKUP: [&str; 4] = [
        r"C:\Users\hollo\Documents\Bioinfo\blood_immuno",
        r"C:\Users\hollo\Documents\Obsidian_notes", 
        r"C:\Users\hollo\Documents\Finance",
        r"C:\Users\hollo\Documents\Personal"
    ];

    // Set archive's file name related to current date.
    let today_date = format!("{}", chrono::offset::Local::now().format("%Y-%m-%d"));
    let file_name = format!("backup{}.tar.gz", today_date);

    info!("Creating tarball from dirs:");
    DIRS_TO_BACKUP.iter().for_each(|x| { info!("\t{}", x) });

    create_tarball_from_dirs(DIRS_TO_BACKUP.to_vec(), &file_name)?;
    info!("Created tarball successfully.");
    info!("Uploading file to MEGA.");

    let settings_file = "./auth_env.json";
    let AuthEnv { email: email_decoded, password: pass_decoded } = utils::read_auth_info(settings_file).unwrap();
    let mfa: Option<&str> = None;

    let mut client = BackupClient::default();
    client.login(&email_decoded, &pass_decoded, mfa).await?;

    match client.upload_file(&file_name, "Backups").await {
        Ok(()) => (),
        Err(e) => {
            // Cleanup before returning error to main.
            error!("Error encountered in `upload_file`, starting cleanup...");
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
        let dirs = vec!["./src"];
        let file_name = "testarchive.tar.gz";
        create_tarball_from_dirs(dirs, file_name).unwrap();

        let file_path = Path::new(file_name);

        assert!(file_path.exists());

        // Remove file so that the test doesn't have side effects.
        std::fs::remove_file(file_name).unwrap();
        assert!(!file_path.exists())
    }

    // TODO: Write tests for BackupClient.
}