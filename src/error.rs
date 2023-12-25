// TODO! Refactor this whole file?
// Practically could even be replaced by a macro,
// but there must be an easier way to do this.
// See other crates for more.
use thiserror::Error;


#[derive(Debug)]
pub struct TarballExistsError {
    pub file_name: String
}

impl std::error::Error for TarballExistsError {}

impl std::fmt::Display for TarballExistsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f, 
            "Tried to create a file with filename `{}`, which already exists. \
            Try to specify a different filename or consider using randomly generated \
            designations.",
            self.file_name
        )
    }
}

#[derive(Debug)]
pub struct MEGAFileExistsError {
    pub file_name: String
}

impl std::error::Error for MEGAFileExistsError {}

impl std::fmt::Display for MEGAFileExistsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Tried to upload a file with filename `{}`, but it already exists \
            in the cloud drive. Try to specify a different filename or consider \
            using randomly generated designations.",
            self.file_name
        )
    }
}

#[derive(Debug, Error)]
pub enum UploadError {
    #[error("Multiple folders found in drive with specified name.")]
    MultipleFoldersError,
    #[error("No folder is found in drive with specified name.")]
    NoFolderError
}