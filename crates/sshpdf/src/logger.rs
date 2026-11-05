//! A minimal logger writing to a file (stderr would garble the TUI).

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

struct FileLogger {
    file: Mutex<File>,
    start: Instant,
}

impl log::Log for FileLogger {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        if let Ok(mut f) = self.file.lock() {
            let t = self.start.elapsed().as_secs_f64();
            let _ = writeln!(f, "{t:9.3} {:5} {}: {}", record.level(), record.target(), record.args());
        }
    }

    fn flush(&self) {
        if let Ok(mut f) = self.file.lock() {
            let _ = f.flush();
        }
    }
}

/// Log everything (debug level) to `path`.
pub fn init(path: &Path) -> std::io::Result<()> {
    let file = File::create(path)?;
    let logger = FileLogger {
        file: Mutex::new(file),
        start: Instant::now(),
    };
    // Lives for the whole run.
    let logger: &'static FileLogger = Box::leak(Box::new(logger));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(log::LevelFilter::Debug);
    }
    Ok(())
}
