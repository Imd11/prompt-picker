use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::mpsc::{self, SyncSender},
    time::{SystemTime, UNIX_EPOCH},
};

const LOG_FILE_NAME: &str = "calico-visual-liveness.log";
const MAX_LOG_BYTES: u64 = 512 * 1024;
const RETAINED_LOG_FILES: usize = 3;
const LOG_QUEUE_CAPACITY: usize = 256;

#[derive(Default)]
pub(crate) struct VisualLivenessLogger {
    sender: Option<SyncSender<String>>,
}

impl VisualLivenessLogger {
    pub(crate) fn start(directory: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let path = directory.join(LOG_FILE_NAME);
        let (sender, receiver) = mpsc::sync_channel::<String>(LOG_QUEUE_CAPACITY);
        std::thread::Builder::new()
            .name("calico-visual-log".to_string())
            .spawn(move || {
                while let Ok(line) = receiver.recv() {
                    if let Err(error) = append_bounded(&path, &line) {
                        eprintln!("Calico visual log write failed: {error}");
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            sender: Some(sender),
        })
    }

    /// Append one privacy-safe diagnostic line. Every value is a fixed category
    /// or a number — never a free-form error string, path, URL, or content — so
    /// the log cannot leak what the user was doing.
    pub(crate) fn record(
        &self,
        event: &'static str,
        stage: &'static str,
        level: &'static str,
        renderer_instance_id: u64,
        recovery_generation: u64,
        probe_ms: Option<u128>,
    ) {
        let Some(sender) = &self.sender else {
            return;
        };
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let line = format_line(
            event,
            stage,
            level,
            renderer_instance_id,
            recovery_generation,
            probe_ms,
            timestamp_ms,
        );
        // A full queue (or a disconnected writer) simply drops the line; logging
        // must never block or panic the health path.
        let _ = sender.try_send(line);
    }

    /// True once the logger has been started (has a live writer thread). Used to
    /// verify clean shutdown behavior in tests.
    #[cfg(test)]
    pub(crate) fn is_started(&self) -> bool {
        self.sender.is_some()
    }

    /// Drop the sender so the writer thread observes a disconnect and exits.
    /// After this, `record` is a harmless no-op.
    #[cfg(test)]
    pub(crate) fn shut_down(&mut self) {
        self.sender.take();
    }
}

/// Build one log line from fixed categories and numbers only. Kept pure and free
/// of any I/O so the privacy contract (no free-form error strings, paths, URLs,
/// or content) is trivially auditable and unit-testable.
fn format_line(
    event: &'static str,
    stage: &'static str,
    level: &'static str,
    renderer_instance_id: u64,
    recovery_generation: u64,
    probe_ms: Option<u128>,
    timestamp_ms: u128,
) -> String {
    let probe_ms = match probe_ms {
        Some(ms) => ms.to_string(),
        None => "na".to_string(),
    };
    format!(
        "ts_ms={timestamp_ms} event={event} stage={stage} level={level} probe_ms={probe_ms} instance={renderer_instance_id} generation={recovery_generation}\n"
    )
}

fn append_bounded(path: &Path, line: &str) -> Result<(), String> {
    if path.metadata().map(|metadata| metadata.len()).unwrap_or(0) >= MAX_LOG_BYTES {
        rotate(path)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(line.as_bytes())
        .map_err(|error| error.to_string())
}

fn rotate(path: &Path) -> Result<(), String> {
    for index in (1..RETAINED_LOG_FILES).rev() {
        let source = rotated_path(path, index);
        let destination = rotated_path(path, index + 1);
        if source.exists() {
            let _ = fs::remove_file(&destination);
            fs::rename(source, destination).map_err(|error| error.to_string())?;
        }
    }
    if path.exists() {
        let destination = rotated_path(path, 1);
        let _ = fs::remove_file(&destination);
        fs::rename(path, destination).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn rotated_path(path: &Path, index: usize) -> PathBuf {
    PathBuf::from(format!("{}.{}", path.display(), index))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_log_rotation_keeps_only_configured_files() {
        let root = std::env::temp_dir().join(format!(
            "sleepy-cat-visual-log-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join(LOG_FILE_NAME);
        fs::write(&path, vec![b'x'; MAX_LOG_BYTES as usize]).unwrap();

        for _ in 0..(RETAINED_LOG_FILES + 2) {
            append_bounded(&path, "event=test\n").unwrap();
            fs::write(&path, vec![b'x'; MAX_LOG_BYTES as usize]).unwrap();
        }

        assert!(path.exists());
        for index in 1..=RETAINED_LOG_FILES {
            assert!(rotated_path(&path, index).exists());
        }
        assert!(!rotated_path(&path, RETAINED_LOG_FILES + 1).exists());
        let _ = fs::remove_dir_all(root);
    }

    fn temp_log_dir(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "sleepy-cat-visual-log-{tag}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn format_line_emits_only_categories_and_numbers() {
        let line = format_line("probe_alive", "healthy", "L0", 7, 3, Some(12), 1_000);
        assert_eq!(
            line,
            "ts_ms=1000 event=probe_alive stage=healthy level=L0 probe_ms=12 instance=7 generation=3\n"
        );
        // A missing probe duration is rendered as a fixed token, not an error string.
        let line = format_line("lifecycle_sleep", "healthy", "L0", 7, 3, None, 2_000);
        assert!(line.contains("probe_ms=na"));
        // Privacy: the line carries no path separator, URL scheme, or quote that a
        // free-form error string might smuggle in.
        assert!(!line.contains('/'));
        assert!(!line.contains("://"));
        assert!(!line.contains('"'));
    }

    #[test]
    fn full_queue_drops_lines_without_blocking_or_panicking() {
        let dir = temp_log_dir("queue-full");
        let logger = VisualLivenessLogger::start(dir.clone()).unwrap();
        // Far more records than the bounded channel can hold; the writer thread
        // drains concurrently, so the queue is guaranteed to fill at some point.
        // `record` must tolerate a full queue by dropping, never by panicking.
        for i in 0..(LOG_QUEUE_CAPACITY * 8) {
            logger.record("probe_requested", "healthy", "L0", i as u64, 1, None);
        }
        assert!(logger.is_started());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn record_after_shutdown_is_a_harmless_noop() {
        let dir = temp_log_dir("shutdown");
        let mut logger = VisualLivenessLogger::start(dir.clone()).unwrap();
        assert!(logger.is_started());
        logger.shut_down();
        assert!(!logger.is_started());
        // Must not panic even though there is no writer anymore.
        logger.record("probe_requested", "healthy", "L0", 1, 1, None);
        let _ = fs::remove_dir_all(dir);
    }
}
