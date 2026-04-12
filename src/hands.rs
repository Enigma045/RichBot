use serde::Deserialize;
use serde_json;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};


#[derive(Deserialize)]
pub struct FileWriteRequest {
    pub path: String,
    pub content: String,
}

#[derive(Debug)]
pub enum FileError {
    IoError(std::io::Error),
    ParseError(String),
}

impl From<std::io::Error> for FileError {
    fn from(e: std::io::Error) -> Self {
        FileError::IoError(e)
    }
}

fn safe_path(base: &Path, untrusted: &str) -> Result<PathBuf, FileError> {
    let untrusted_path = Path::new(untrusted);
    
    // If the path is absolute, respect it directly.
    if untrusted_path.is_absolute() {
        return Ok(untrusted_path.to_path_buf());
    }

    // Otherwise, join it with the base.
    // We don't strictly enforce 'starts_with' anymore as per user request to allow writing outside sandbox.
    let joined = base.join(untrusted);
    Ok(joined)
}

pub fn write_file(base: &Path, path: &str, content: &str) -> Result<(), FileError> {
    let safe = safe_path(base, path)?;

    // Ensure parent directory exists
    if let Some(parent) = safe.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(safe)?;

    file.write_all(content.as_bytes())?;
    Ok(())
}

pub fn write_files_from_json(base: &Path, json: &str) -> Result<(), FileError> {
    let start = json.find('[').ok_or_else(|| FileError::ParseError("No JSON array found".into()))?;
    let end = json.rfind(']').ok_or_else(|| FileError::ParseError("No closing bracket found".into()))?;
    let json_str = &json[start..=end];

    let reqs: Vec<FileWriteRequest> = serde_json::from_str(json_str)
        .map_err(|e| FileError::ParseError(e.to_string()))?;

    let mut failed = vec![];

    for req in reqs {
        match write_file(base, &req.path, &req.content) {
            Ok(()) => println!("✓ Written: {}", req.path),
            Err(e) => {
                eprintln!("✗ Failed: {} — {:?}", req.path, e);
                failed.push(req.path);
            }
        }
    }

    if !failed.is_empty() {
        eprintln!("Failed files: {:?}", failed);
    }

    Ok(())
}