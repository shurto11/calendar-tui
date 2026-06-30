use std::path::PathBuf;

pub fn config_dir() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("calendar-tui");
    path
}

pub fn load_excluded_calendars() -> Vec<String> {
    let path = config_dir().join("excluded_calendars.txt");
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && !s.starts_with('#'))
        .collect()
}

pub fn save_excluded_calendars(excluded: &[String]) -> anyhow::Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("excluded_calendars.txt");
    std::fs::write(path, excluded.join("\n"))?;
    Ok(())
}
