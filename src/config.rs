use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static TEMPORARY_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug)]
pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|dir| dir.join("codex-quota-ball/config.json"))
    }

    pub fn load(&self) -> Option<Position> {
        serde_json::from_slice(&fs::read(&self.path).ok()?).ok()
    }

    pub fn save(&self, position: Position) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(&position).map_err(io::Error::other)?;

        let (temporary, mut file) = loop {
            let id = TEMPORARY_ID.fetch_add(1, Ordering::Relaxed);
            let mut name =
                self.path.file_name().map(OsString::from).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "missing file name")
                })?;
            name.push(format!(".{}.{id}.tmp", std::process::id()));
            let temporary = self.path.with_file_name(name);
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
            {
                Ok(file) => break (temporary, file),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        };

        let write_result = file.write_all(&bytes);
        drop(file);
        if let Err(error) = write_result {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        if let Err(error) = fs::rename(&temporary, &self.path) {
            let _ = fs::remove_file(temporary);
            return Err(error);
        }
        Ok(())
    }
}

pub fn default_position(monitor_width: i32, ball_width: i32) -> Position {
    Position {
        x: (monitor_width - ball_width - 24).max(0),
        y: 24,
    }
}

pub fn clamp_position(
    position: Position,
    monitor_width: i32,
    monitor_height: i32,
    window_width: i32,
    window_height: i32,
) -> Position {
    Position {
        x: position.x.clamp(0, (monitor_width - window_width).max(0)),
        y: position.y.clamp(0, (monitor_height - window_height).max(0)),
    }
}
