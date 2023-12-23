use serde::{Deserialize, Serialize};
use base64::Engine;

pub fn progress_bar_style() -> indicatif::ProgressStyle {
    let template = format!(
        "{}{{bar:30.magenta.bold/magenta/bold}}{} {{percent}} % (ETA {{eta}}): {{msg}}",
        console::style("▐").bold().magenta(),
        console::style("▌").bold().magenta(),
    );

    indicatif::ProgressStyle::default_bar()
        .progress_chars("▨▨╌")
        .template(template.as_str())
        .unwrap()
}


#[derive(Serialize, Deserialize, Debug)]
pub struct AuthEnv {
    pub email: String,
    pub password: String
}

// TODO: Make this function's example doc run?!
/// Reads a JSON file of base64 encoded credentials.
/// 
/// # Examples
/// ```no_run
/// let auth_info = read_auth_info("./auth_env.json").unwrap();
/// let AuthEnv { email, password } = auth_info;
/// ```
pub fn read_auth_info(file_path: &str) -> Result<AuthEnv, Box<dyn std::error::Error>> {
    // Read username and password from local settings file.
    let contents = std::fs::read_to_string(file_path)?;

    // Parse JSON
    let auth_info: AuthEnv = serde_json::from_str(&contents)?;

    // auth_env.json example
    // { 
    //     "email": "eW91X3ZlX2JlZW4=",
    //     "password": "cmlja19yb2xsZWQ="
    // }

    // Decode username and password
    let email_bytes = base64::engine::general_purpose::STANDARD
        .decode(auth_info.email)?;

    let password_bytes = base64::engine::general_purpose::STANDARD
        .decode(auth_info.password)?;

    let email = String::from_utf8(email_bytes)?;
    let password = String::from_utf8(password_bytes)?;

    Ok(AuthEnv {
        email,
        password
    })
}