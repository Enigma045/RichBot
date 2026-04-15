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

    // Unescape common double-encoded JSON escape sequences.
    // IMPORTANT: backslash must be first so we don't double-process other sequences.
    let unescaped = content
        .replace("\\\\", "\x00BACKSLASH\x00")  // placeholder for real backslash
        .replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace("\\r", "\r")
        .replace("\\\"", "\"")
        .replace("\\'", "'")
        .replace("\x00BACKSLASH\x00", "\\");   // restore real backslash

    file.write_all(unescaped.as_bytes())?;
    Ok(())
}

pub fn write_files_from_json(base: &Path, json: &str) -> Result<(), FileError> {
    // Find the last ']' once — it is the boundary of the JSON array
    let end = match json.rfind(']') {
        Some(e) => e,
        None => return Err(FileError::ParseError("No closing ']' found in AI response.".into())),
    };

    // Scan forward for each '[' and try to parse from there to `end`
    let mut search_idx = 0;
    let mut last_error: Option<serde_json::Error> = None;

    while search_idx < end {
        // Find next '[' within the bounds
        if let Some(rel_start) = json[search_idx..=end].find('[') {
            let abs_start = search_idx + rel_start;

            // Guard against non-UTF-8 boundaries (char_indices ensures valid slices)
            let json_slice = match json.get(abs_start..=end) {
                Some(s) => s,
                None => { search_idx = abs_start + 1; continue; }
            };

            match serde_json::from_str::<Vec<FileWriteRequest>>(json_slice) {
                Ok(reqs) => {
                    if reqs.is_empty() {
                        return Err(FileError::ParseError("AI returned an empty file list.".into()));
                    }
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
                    return Ok(());
                }
                Err(e) => {
                    last_error = Some(e);
                    search_idx = abs_start + 1;
                }
            }
        } else {
            break; // no more '[' before the last ']'
        }
    }

    let err_msg = match last_error {
        Some(e) => format!("JSON Parse Error: {}. Input snippet: {}", e, &json[..json.len().min(500)]),
        None => "No valid JSON array found in AI response.".into(),
    };
    Err(FileError::ParseError(err_msg))
}