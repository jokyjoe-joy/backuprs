//! **B**asic **A**utomated **C**loud **K**eeper for **U**ltimate **P**ersistence
//! aka. BACKUP.rs

use std::{fs::File, path::Path};
use flate2::Compression;
use flate2::write::GzEncoder;
use chrono;
use mega::Node;
use tokio_util::compat::TokioAsyncReadCompatExt;
use utils::SettingsEnv;
use log::{info, error, debug, warn};

mod utils;
mod error;

const SETTINGS_FILE: &str = "./settings.json";

struct BackupClient {
    mega_client: mega::Client,
    dropped: bool,
    backup_folder: String,
    backup_node: Option<Node>
}

impl BackupClient {
    pub fn default() -> Self {
        let http_client = reqwest::Client::new();
        let client = mega::Client::builder().build(http_client).unwrap();
        BackupClient {
            mega_client: client,
            dropped: false,
            backup_folder: String::from("/Root/Backups"),
            backup_node: None
        }
    }

    pub fn new(backup_folder: String) -> Self {
        let http_client = reqwest::Client::new();
        let client = mega::Client::builder().build(http_client).unwrap();
        BackupClient {
            mega_client: client,
            dropped: false,
            backup_folder: backup_folder,
            backup_node: None
        }
    }

    /// Logs into the MEGA service using the provided credentials.
    ///
    /// # Arguments
    ///
    /// * `email`: The email address associated with the MEGA account.
    /// * `password`: The password for the MEGA account.
    /// * `mfa`: An optional multi-factor authentication (MFA) code if MFA is enabled for the account.
    ///
    /// # Returns
    ///
    /// Returns a `Result` indicating success or failure. In case of an error during the login process,
    /// it returns an `Err` containing the error information.
    ///
    /// # Errors
    ///
    /// Returns an error if there is an issue during the login process, such as invalid credentials or
    /// network-related problems.
    ///
    /// # Remarks
    ///
    /// After successful login, the function fetches the nodes associated with the MEGA account and
    /// attempts to retrieve the node corresponding to the specified backup folder. The retrieved node
    /// is then stored in the `backup_node` field of the `BackupClient` for later use.
    pub async fn login(&mut self, email: &str, password: &str, mfa: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
        info!("Logging in with email: {email}...");
        self.mega_client.login(email, password, mfa).await?;

        let nodes = self.mega_client.fetch_own_nodes().await?;
        let parent_node = nodes.get_node_by_path(&self.backup_folder);
        self.backup_node = parent_node.cloned();

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

    /// Checks for obsolete backup nodes in the client's `backup_node` based on the specified criteria.
    ///
    /// # Arguments
    ///
    /// * `max_backups`: The maximum number of backups to keep. If the total number of backup nodes
    ///   exceeds this limit, the function considers the oldest nodes as obsolete.
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing either `Some(Vec<Node>)` with the obsolete backup nodes
    /// or `None` if no obsolete nodes are found. In case of an error during the operation,
    /// it returns an `Err` containing the error information.
    ///
    /// # Errors
    ///
    /// * Returns an error if there is an issue fetching the nodes from the MEGA client.
    /// * Returns an error if `self.backup_node` is None.
    pub async fn find_obsolete_nodes(&self, max_backups: usize) -> Result<Option<Vec<Node>>, Box<dyn std::error::Error>> {
        info!("Checking if there are more than {:?} backups.", max_backups);
        let nodes = self.mega_client.fetch_own_nodes().await?;
        
        let mut backup_nodes: Vec<Node> = nodes.into_iter()
        .filter(|node| {
            node.parent() == Some(self.backup_node.as_ref().expect("Backup node must be already defined to find obsolete nodes.").handle())
            && node.name().contains(".tar.gz") 
            && node.name().contains("backup")
        })
        .collect();

        if backup_nodes.len() > max_backups {
            backup_nodes.sort_by_key(|x| { x.created_at() });
    
            let no_of_obsolete_nodes = backup_nodes.len() - max_backups;
            info!("Found {:?} obsolete node(s).", no_of_obsolete_nodes);
    
            Ok(Some(backup_nodes.into_iter().take(no_of_obsolete_nodes).collect()))

        } else {
            info!("Not found any obsolete nodes.");
            Ok(None)
        }
    }

    /// Removes all nodes that are specified as an argument.
    /// 
    /// # Arguments
    /// 
    /// * `obsolete_nodes` - Vector of nodes that must be deleted.
    pub async fn remove_obsolete_nodes(&self, obsolete_nodes: Vec<Node>) -> Result<(), Box<dyn std::error::Error>> {
        for node in obsolete_nodes.iter() {
            info!("Deleting node {:?}...", node.name());
            self.mega_client.delete_node(node).await?;
        }

        Ok(())
    }

    /// Uploads a file to the client's MEGA backup folder node.
    ///
    /// # Arguments
    ///
    /// * `file_name` - The name of the file to be uploaded.
    ///
    /// # Returns
    ///
    /// Returns a `Result` with an empty `Ok(())` on successful upload or an error on failure.
    ///
    /// # Errors
    ///
    /// The function can return errors in the form of a `Box<dyn std::error::Error>`. Possible errors include:
    /// * `MEGAFileExistsError` if a file with the same name already exists in the specified folder.
    /// * I/O errors, file opening errors, or any other errors that may occur during the upload process.
    ///
    /// # Panics
    ///
    /// Panics if the file name cannot be converted to a valid UTF-8 string or if there is an issue with
    /// fetching own nodes or getting file metadata.
    pub async fn upload_file(&self, file_name: &str) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(dest_folder_node) = &self.backup_node {
            let nodes = self.mega_client.fetch_own_nodes().await?;
            let file_name = Path::new(file_name).file_name().unwrap().to_str().unwrap();
    
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
        } else {
            warn!("Tried to upload a file while there was no backup node specified!");
            Ok(())
        }
    }
}

// When the client goes out of scope, user is gracefully logged out first.
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

/// Creates a tarball archive from the specified list of directories, saving it to the
/// given file name. Optionally, you can provide a list of folder names to be ignored.
/// 
/// # Arguments
/// 
/// * `dirs` - A vector of strings representing the absolute paths to the directories to
///            be included in the tarball.
/// * `file_name` - The name of the tarball file to be created.
/// * `ignore_folders` - An optional vector of strings containing folder names to be ignored
///                      during the tarball creation process.
/// 
/// # Errors
///
/// This function returns a `Result<(), Box<dyn std::error::Error>>`. Possible error variants
/// include:
/// * `TarballExistsError` - Returned if the specified tarball file already exists.
/// * Any error that occurs during file operations, such as file creation, reading, or appending
///   to the tarball.
///
fn create_tarball_from_dirs(dirs: Vec<String>, file_name: &str, ignore_folders: Option<Vec<String>>) -> Result<(), Box<dyn std::error::Error>> {
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

    for dir_path in dirs.iter() {
        let dir_contents = get_dir_contents(dir_path, &ignore_folders)?;

        for node_path in dir_contents.iter() {
            debug!("Adding file to tarball: {:?}", node_path);
            // Open file that will be later appended to the tar.
            let mut f = File::open(&node_path)?;
            
            // Convert absolute path to relative path from `dir_path`.
            // E.g.: C:\\Users\\username\\Documents\\My\\Path\\backup_folder\\Makefile"
            // ----> "backup_folder\\Makefile"
            let backup_abs_folder_path = format!("{}\\", dir_path);
            let backup_folder_name = dir_path.split("\\").last().unwrap();
            let relative_path = format!("{}\\{}", backup_folder_name, node_path.trim_start_matches(&backup_abs_folder_path));

            tar.append_file(Path::new(&relative_path), &mut f)?;

        }
    }

    let _ = tar.finish();

    Ok(())
}

/// Recursively retrieves the contents (files and subdirectories' files) of the specified directory,
/// excluding those listed in the optional `ignore_folders` vector.
/// 
/// # Arguments
/// 
/// * `dir` - A string representing the path to the directory whose contents are to be retrieved.
/// * `ignore_folders` - An optional vector of strings containing folder names to be ignored
///                      during the retrieval process.
/// 
/// # Errors
/// 
/// Possible error variants include any errors that may occur during directory traversal or metadata retrieval.
/// 
/// # Returns
/// 
/// Returns a `Result` with a vector of strings holding the absolute path of the found files, or an error on failure.
fn get_dir_contents(dir: &str, ignore_folders: &Option<Vec<String>>) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let dir_contents = Path::new(&dir).read_dir()?;

    let mut nodes_to_save: Vec<String> = Vec::new();

    for node in dir_contents {
        let node = node?;
        if node.metadata()?.is_dir() {
            if let Some(folders) = ignore_folders {
                if folders.contains(&node.file_name().to_string_lossy().to_string()) {
                    continue;
                }
            }

            let mut node_contents = get_dir_contents(node.path().as_os_str().to_str().unwrap(), ignore_folders)?;
            nodes_to_save.append(&mut node_contents);
        } else {
            nodes_to_save.push(node.path().to_string_lossy().to_string())
        }
    }

    Ok(nodes_to_save)
}

#[tokio::main]
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let SettingsEnv { 
        email: email_decoded, password: pass_decoded, dirs_to_backup, dirs_to_ignore
    } = utils::read_auth_info(SETTINGS_FILE)?;

    // Set archive's file name related to current date.
    let today_date = format!("{}", chrono::offset::Local::now().format("%Y-%m-%d"));
    let file_name = format!("backup{}.tar.gz", today_date);

    info!("Creating tarball from dirs:");
    dirs_to_backup.iter().for_each(|x| { info!("\t{}", x) });

    create_tarball_from_dirs(dirs_to_backup, &file_name, Some(dirs_to_ignore))?;
    info!("Created tarball successfully.");
    info!("Uploading file to MEGA.");

    let mfa: Option<&str> = None;

    let mut client = BackupClient::new(String::from("/Root/Backups"));

    client.login(&email_decoded, &pass_decoded, mfa).await?;

    match client.upload_file(&file_name).await {
        Ok(()) => (),
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

    let obsolete_nodes = client.find_obsolete_nodes(10).await?;

    if let Some(nodes) = obsolete_nodes {
        client.remove_obsolete_nodes(nodes).await?;
    }

    client.try_logout().await;

    info!("Removing archive file...");
    std::fs::remove_file(file_name)?;
    info!("Successfully removed archive file...");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retrieve_dir_contents() {
        let expected_contents = vec![
            String::from("src\\lib.rs"), 
            String::from("src\\main.rs")
        ];

        let contents = get_dir_contents("src", &None).unwrap();
        
        assert!(expected_contents.iter().all(|item| contents.contains(item)));
    }

    #[test]
    fn create_tarball() {
        // Create an archive of the source folder, therefore
        // this test can be run anytime, since `src` must exist
        // to build this binary.
        let dirs = vec![String::from("./src")];
        let file_name = "testarchive.tar.gz";
        create_tarball_from_dirs(dirs, file_name, None).unwrap();

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

        let mut client = BackupClient::new(String::from("/Root/Backups"));
        client.login(&email_decoded, &pass_decoded, None).await
            .expect("Failure while logging in...");


        // Uploading README.md because that's a file that must exist.
        // TODO: `client.upload_file` should return uploaded file's Node,
        // so it can be easier to delete it later (or do anything else with it).
        // TODO: Create a randomly generated file (with random filename) to upload.
        client.upload_file("README.md").await
            .expect("Uploading file has failed...");

        let nodes = client.mega_client.fetch_own_nodes().await
            .expect("Couldn't fetch own nodes.");

        let node = nodes.get_node_by_path("/Root/Backups/README.md")
            .expect("Couldn't get node by path...");

        client.mega_client.delete_node(node).await
            .expect("Couldn't delete node...");

        let nonexistent_node = nodes.get_node_by_path("/README.md");
        assert_eq!(nonexistent_node, None);

        // FIXME: Explicit logouts are only necessary, until `Drop` is properly implemented.
        client.logout().await.expect("Failure while logging out...");
    }
}