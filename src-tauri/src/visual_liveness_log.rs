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

    pub(crate) fn record(
        &self,
        event: &'static str,
        stage: &'static str,
        renderer_instance_id: u64,
        recovery_generation: u64,
    ) {
        let Some(sender) = &self.sender else {
            return;
        };
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let line = format!(
            "ts_ms={timestamp_ms} event={event} stage={stage} instance={renderer_instance_id} generation={recovery_generation}\n"
        );
        let _ = sender.try_send(line);
    }
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
}
