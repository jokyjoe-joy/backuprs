// Disabling the terminal window on Windows.
#![cfg_attr(
	target_os = "windows",
	windows_subsystem = "windows"
)]

use chrono::Local;
mod error;

/// Current version of backup.rs, read from Cargo.toml.
/// It is an `Option<&str>`, because if the lib is not compiled
/// with cargo, it will be `None`.
const VERSION: Option<&str> = option_env!("CARGO_PKG_VERSION");
const PKG_NAME: Option<&str> = option_env!("CARGO_PKG_NAME");

fn setup_logger() -> Result<(), fern::InitError> {
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{}v{} - {} {} {}] {}",
                PKG_NAME.unwrap_or("LIB-NO-CARGO"),
                VERSION.unwrap_or("V?.?.?"),
                Local::now().format("%Y-%m-%d -- %H:%M:%S"),
                record.level(),
                record.target(),
                message
            ))
        })
        .level(log::LevelFilter::Debug)
        .chain(std::io::stdout())
        .chain(fern::log_file("output.log")?)
        .apply()?;
    Ok(())
}

fn main() {
    setup_logger().unwrap();
    match backuprs::run() {
        Ok(()) => (),
        Err(e) => {
            // Panic if unknown error has been found, since this
            // can only happen if there is a bug in the application.
            log::error!("{:?}", e);
            panic!("{:?}", e);
        }
    };
}
