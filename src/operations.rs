use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use walkdir::WalkDir;
use serde::{Deserialize, Serialize};


// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    #[serde(default)]
    pub depth: usize,
    #[serde(rename = "type")]
    pub entry_type: EntryType,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryType {
    File,
    Directory,
}

// ── Errors ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum FileError {
    Io(std::io::Error),
    InvalidPath(String),
}

// impl From<std::io::Error> for FileError {
//     fn from(e: std::io::Error) -> Self {
//         FileError::IoError(e)
//     }
// }

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            FileError::Io(e) => write!(f, "IO error: {}", e),
            FileError::InvalidPath(p) => write!(f, "Invalid path: {}", p),
        }
    }
}

impl From<std::io::Error> for FileError {
    fn from(e: std::io::Error) -> Self {
        FileError::Io(e)
    }
}

// ── Operations ────────────────────────────────────────────────────────────────

pub fn see() -> Vec<FileEntry> {
    WalkDir::new(".")
        .max_depth(3)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|entry| {
            let path_str = entry.path().to_string_lossy();
            !path_str.contains(".git") 
                && !path_str.contains("target") 
                && !path_str.contains("cmd_outputs.txt")
        })
        .map(|entry| FileEntry {
            path: entry.path().display().to_string(),
            depth: entry.depth(),
            entry_type: if entry.file_type().is_dir() {
                EntryType::Directory
            } else {
                EntryType::File
            },
        })
        .collect()
}

pub fn write_file(path: &str, content: &str) -> Result<(), FileError> {
    fs::write(path, content)?;
    Ok(())
}

pub fn read_file(path: &str) -> Result<String, FileError> {
    if !Path::new(path).exists() {
        return Err(FileError::InvalidPath(path.to_string()));
    }
    Ok(fs::read_to_string(path)?)
}

pub fn read_files(entries: &[FileEntry]) -> Vec<Result<String, FileError>> {
    entries
        .iter()
        .filter(|e| matches!(e.entry_type, EntryType::File))
        .map(|e| read_file(&e.path))
        .collect()
}

pub fn append_file(path: &str, content: &str) -> Result<(), FileError> {
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)?;
    file.write_all(content.as_bytes())?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileContent {
    pub name: String,
    pub content: String,
}

pub fn read_files_from_json(ai_response: &str) -> Result<Vec<FileContent>, FileError> {
    // extract only the JSON array from the response
    let start = ai_response.find('[').ok_or_else(|| {
        FileError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "No JSON array found in response",
        ))
    })?;

    let end = ai_response.rfind(']').ok_or_else(|| {
        FileError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "No closing bracket found in response",
        ))
    })?;

    let json_str = &ai_response[start..=end];

    // ADD THIS — remove once fixed
    // eprintln!("DEBUG JSON:\n{}", json_str);

    let entries: Vec<FileEntry> = serde_json::from_str(json_str).map_err(|e| {
        FileError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    })?;

    entries
        .iter()
        .filter(|e| matches!(e.entry_type, EntryType::File))
        .map(|e| {
            let normalized = e.path.replace("\\", "/");
            let content = read_file(&normalized)?;
            Ok(FileContent {
                name: Path::new(&normalized)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                content,
            })
        })
        .collect()
}