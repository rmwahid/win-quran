//! Everything this widget remembers, kept under its own name.
//!
//! Both files live in `%APPDATA%\win-quran`: the reading position and the window
//! settings. The plugin's `~/.quran-turn` directory is deliberately left alone.
//!
//! An earlier version shared its state file with the plugin so the two readers would
//! stay in step. That was the wrong call: it made it ambiguous which program owned
//! the file, and it forced this widget to depend on a program it does not need. This
//! widget is its own thing and keeps its own memory.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Where the reader is.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub surah: u16,
    pub ayah: u16,
    /// ISO 8601 UTC, written for the sake of anyone reading the file by hand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

impl Default for Position {
    fn default() -> Self {
        Self {
            surah: 1,
            ayah: 1,
            updated_at: None,
        }
    }
}

/// The widget's own settings.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Settings {
    /// Window position, or `-1` for both when the widget has not been placed yet.
    pub x: i32,
    pub y: i32,
    pub always_on_top: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            x: UNPLACED,
            y: UNPLACED,
            always_on_top: true,
        }
    }
}

/// The sentinel that means "no saved position".
pub const UNPLACED: i32 = -1;

/// The directory this widget keeps everything in.
pub fn home() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("APPDATA")?).join("win-quran"))
}

/// Where the reading position is remembered.
pub fn position_path() -> Option<PathBuf> {
    Some(home()?.join("state.json"))
}

/// This widget's settings file.
pub fn settings_path() -> Option<PathBuf> {
    Some(home()?.join("settings.json"))
}

/// Reads the saved position, or the start of the mushaf when there is none.
pub fn read_position() -> Position {
    position_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Writes the position to a temporary file and renames it over the real one, so a
/// reader can never catch a half written file.
pub fn write_position(position: &Position) -> io::Result<()> {
    let Some(path) = position_path() else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_string_pretty(position).unwrap_or_default();
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temporary, format!("{body}\n"))?;
    fs::rename(&temporary, &path)
}

/// Reads this widget's settings, or defaults when there are none.
pub fn read_settings() -> Settings {
    settings_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Writes this widget's settings.
pub fn write_settings(settings: &Settings) -> io::Result<()> {
    let Some(path) = settings_path() else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_string_pretty(settings).unwrap_or_default();
    fs::write(path, format!("{body}\n"))
}

/// The current time as ISO 8601 UTC with milliseconds.
pub fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = now.as_secs() as i64;
    let millis = now.subsec_millis();
    let days = seconds.div_euclid(86_400);
    let remaining = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        remaining / 3600,
        remaining % 3600 / 60,
        remaining % 60
    )
}

/// Converts days since 1970-01-01 into a calendar date.
///
/// This is Howard Hinnant's `civil_from_days`, which is exact for the whole range
/// of `i64` days and needs no calendar library.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 { shifted } else { shifted - 146_096 } / 146_097;
    let day_of_era = (shifted - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_known_days_to_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(59), (1970, 3, 1));
        // 2000 is a leap year, so its February has a 29th and March starts a day later.
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(11_017), (2000, 3, 1));
        assert_eq!(civil_from_days(20_000), (2024, 10, 4));
    }

    #[test]
    fn the_timestamp_has_the_shape_a_reader_expects() {
        let stamp = timestamp();
        assert_eq!(stamp.len(), 24, "{stamp}");
        assert!(stamp.ends_with('Z'));
        assert_eq!(&stamp[4..5], "-");
        assert_eq!(&stamp[10..11], "T");
    }

    #[test]
    fn both_files_live_under_the_widgets_own_name() {
        // The plugin's directory must not be touched: this widget is its own program.
        for path in [position_path(), settings_path()] {
            let path = path.expect("APPDATA is set on Windows");
            assert!(path.ends_with("win-quran\\state.json") || path.ends_with("win-quran\\settings.json"));
            assert!(!path.to_string_lossy().contains(".quran-turn"));
        }
    }
}
