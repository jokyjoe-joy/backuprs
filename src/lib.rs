//! **B**asic **A**utomated **C**loud **K**eeper for **U**ltimate **P**ersistence
//! aka. BACKUP.rs

use std::path::Path;
use std::{fs, io};
use std::fs::File;
use flate2::Compression;
use flate2::write::GzEncoder;
use chrono;

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

    // Create the archive file.
    let tar_gz = File::create(file_name)?;
    let enc = GzEncoder::new(tar_gz, Compression::best());
    let mut tar = tar::Builder::new(enc);

    // Loop through each folder and append them to the archive.
    dirs.iter().for_each(|&dir| {
        // Get folder's name and path separately.
        let dir_path = Path::new(dir);
        let dir_name = dir_path.file_name().unwrap();

        // Appends the directory with all its file.
        tar.append_dir_all(dir_name, dir).unwrap();

    });

    Ok(String::from(file_name))
}

pub fn run() -> Result<(), io::Error> {
    const DIRS_TO_BACKUP: [&str; 4] = [
        r"C:\Users\hollo\Documents\Bioinfo\blood_immuno",
        r"C:\Users\hollo\Documents\Obsidian_notes", 
        r"C:\Users\hollo\Documents\Finance",
        r"C:\Users\hollo\Documents\Personal"
    ];

    // Set archive's file name related to current date.
    let today_date = format!("{}", chrono::offset::Local::now().format("%Y-%m-%d"));
    let file_name = format!("backup{}.tar.gz", today_date);

    let tarball = create_tarball_from_dirs(DIRS_TO_BACKUP.to_vec(), &file_name)?;

    // TODO: Communicate with pCloud API and upload tarball.

    Ok(())
}