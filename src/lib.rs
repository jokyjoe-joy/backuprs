//! **B**asic **A**utomated **C**loud **K**eeper for **U**ltimate **P**ersistence
//! aka. BACKUP.rs

use std::path::Path;
use std::{fs, io};
use flate2::Compression;
use flate2::write::GzEncoder;
use chrono;
use utils::AuthEnv;
use std::sync::Arc;
use async_read_progress::TokioAsyncReadProgressExt;
use std::time::Duration;
use tokio_util::compat::TokioAsyncReadCompatExt;

mod utils;

// TODO: Documentation, refactoring...

#[allow(dead_code)]
fn read_path(path: &str) -> Result<Vec<String>, io::Error> {
    // Read path into an iterator.
    let paths_in_dir = fs::read_dir(path);

    // Check if path exists.
    // If it doesn't exist return error to caller.
    let paths_in_dir = match paths_in_dir {
        Ok(path) => path,
        Err(e) => match e.kind() {
            io::ErrorKind::NotFound => return Err(e),
            _ => panic!("{}", e)
        },
    };

    // Create a vector for storing the path names of the files.
    let mut files: Vec<String> = vec![];

    // Loop through each file/folder we've found in the directory. 
    for path in paths_in_dir {
        let path = path.unwrap();

        // If it is a directory, then read that directory as well.
        if path.metadata().unwrap().is_dir() {
            let path_name = path.path().display().to_string();
            let mut found_files = read_path(&path_name).unwrap();
            files.append(&mut found_files);
        } else {
            files.push(path.path().display().to_string());
        }
    }

    Ok(files)
}

fn create_tarball_from_dirs(dirs: Vec<&str>, file_name: &str) -> Result<String, io::Error> {
    // Check if file already exists.
    match Path::new(file_name).try_exists() {
        Ok(true) => return Err(io::Error::new(io::ErrorKind::AlreadyExists, "File already exists.")),
        Ok(false) => (),
        Err(e) => return Err(e)
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

    Ok(String::from(file_name))
}

#[tokio::main(flavor = "current_thread")]
pub async fn run() -> Result<(), io::Error> {
    const DIRS_TO_BACKUP: [&str; 4] = [
        r"C:\Users\hollo\Documents\Bioinfo\blood_immuno",
        r"C:\Users\hollo\Documents\Obsidian_notes", 
        r"C:\Users\hollo\Documents\Finance",
        r"C:\Users\hollo\Documents\Personal"
    ];

    // Set archive's file name related to current date.
    let today_date = format!("{}", chrono::offset::Local::now().format("%Y-%m-%d"));
    let file_name = format!("backup{}.tar.gz", today_date);

    let tarball_name = create_tarball_from_dirs(DIRS_TO_BACKUP.to_vec(), &file_name)?;

    upload_file(&tarball_name, "Backups").await.unwrap();

    Ok(())
}

async fn upload_file(file_name: &str, dest_folder: &str) -> Result<(), Box<dyn std::error::Error>> {
    let settings_file = "./auth_env.json";
    let AuthEnv { email: email_decoded, password: pass_decoded } = utils::read_auth_info(settings_file).unwrap();
    let mfa: Option<&str> = None;

    let http_client = reqwest::Client::new();
    let mut mega = mega::Client::builder().build(http_client).unwrap();

    mega.login(&email_decoded, &pass_decoded, mfa).await
        .expect("Login has failed.");

    let file_name = Path::new(file_name).file_name().unwrap().to_str().unwrap();

    let nodes = mega.fetch_own_nodes().await?;
    let nodes: Vec<_> = nodes.iter().filter(|&node| { node.name() == dest_folder }).collect();

    // TODO: Don't panic?!
    if nodes.len() > 1 { panic!("Multiple folders found with specified name."); }
    else if nodes.len() < 1 { panic!("No folder is found with specified name."); }
    
    let node = *nodes.first().unwrap();

    let file = tokio::fs::File::open(file_name).await?;
    let size = file.metadata().await?.len();

    // Setting up progressbar
    let bar = indicatif::ProgressBar::new(size);
    bar.set_style(utils::progress_bar_style());
    bar.set_message(format!("Uploading {} to {}...", file_name, node.name()));
    let bar = Arc::new(bar);

    let reader = {
        let bar = bar.clone();
        file.report_progress(Duration::from_millis(100), move |bytes_read| {
            bar.set_position(bytes_read as u64);
        })
    };

    // Uploading file to MEGA
    mega.upload_node(
        &node,
        file_name,
        size,
        reader.compat(),
        mega::LastModified::Now,
    )
    .await?;

    bar.finish_with_message(format!("{} uploaded to {} !", file_name, node.name()));
    
    mega.logout().await.unwrap();

    Ok(())
}