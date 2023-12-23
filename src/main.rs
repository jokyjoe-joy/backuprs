// Disabling the terminal window on Windows.
#![cfg_attr(
	target_os = "windows",
	windows_subsystem = "windows"
)]

use chrono::Local;

fn setup_logger() -> Result<(), fern::InitError> {
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{} {} {}] {}",
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
            // TODO: There must be an easier and simpler way to do this.
            if e.is::<backuprs::TarballExistsError>() || e.is::<backuprs::MEGAFileExistsError>() {
                log::error!("{}", e);
            }
            else {
                // Panic if unknown error has been found, since this
                // can only happen if there is a bug in the application.
                panic!("{:?}", e);
            }
        }
    };
}
