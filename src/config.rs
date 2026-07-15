use serde::{Deserialize, Serialize};
use std::{fs, io, path::PathBuf};

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
        let temporary = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(&position).map_err(io::Error::other)?;
        fs::write(&temporary, bytes)?;
        fs::rename(temporary, &self.path)
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
