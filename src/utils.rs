use serde::{Deserialize, Serialize};
use base64::Engine;

#[derive(Serialize, Deserialize, Debug)]
pub struct SettingsEnv {
    pub email: String,
    pub password: String,
    pub dirs_to_backup: Vec<String>
}

// TODO: Make this function's example doc run?!
/// Reads a JSON file of base64 encoded credentials.
/// 
/// # Returns
/// 
/// * An `AuthEnv` struct of base64 decoded credentials with the structure of `{ email, password }`
/// 
/// # Examples
/// ```ignore
/// let auth_info = read_auth_info("./auth_env.json").unwrap();
/// let AuthEnv { email, password } = auth_info;
/// ```
pub fn read_auth_info(file_path: &str) -> Result<SettingsEnv, Box<dyn std::error::Error>> {
    // Read username and password from local settings file.
    let contents = std::fs::read_to_string(file_path)?;

    // Parse JSON
    let auth_info: SettingsEnv = serde_json::from_str(&contents)?;

    // Decode username and password
    let email_bytes = base64::engine::general_purpose::STANDARD
        .decode(auth_info.email)?;

    let password_bytes = base64::engine::general_purpose::STANDARD
        .decode(auth_info.password)?;

    let email = String::from_utf8(email_bytes)?;
    let password = String::from_utf8(password_bytes)?;

    Ok(SettingsEnv {
        email,
        password,
        dirs_to_backup: auth_info.dirs_to_backup
    })
}