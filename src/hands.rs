use serde::Deserialize;
use serde_json;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};


#[derive(Deserialize)]
pub struct FileWriteRequest {
    pub path: String,
    pub content: Option<String>,
    #[serde(default = "default_op")]
    pub op: String, // "overwrite", "append", "patch", "insert_at"
    pub search: Option<String>,
    pub replace: Option<String>,
    pub line: Option<usize>,
}

fn default_op() -> String { "overwrite".into() }

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

    // Join with the base safely and NORMALIZE to prevent super-nesting or redundant dots.
    let joined = base.join(untrusted_path);
    let normalized = crate::operations::normalize_path(&joined);
    
    Ok(normalized)
}

pub fn write_file(base: &Path, req: &FileWriteRequest) -> Result<(), FileError> {
    let safe = safe_path(base, &req.path)?;

    // Ensure parent directory exists
    if let Some(parent) = safe.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let unescaped = if let Some(content) = &req.content {
        // Unescape common double-encoded JSON escape sequences.
        content
            .replace("\\\\", "\x00BACKSLASH\x00")
            .replace("\\n", "\n")
            .replace("\\t", "\t")
            .replace("\\r", "\r")
            .replace("\\\"", "\"")
            .replace("\\'", "'")
            .replace("\x00BACKSLASH\x00", "\\")
    } else {
        String::new()
    };

    match req.op.as_str() {
        "append" => {
            let mut file = OpenOptions::new()
                .append(true)
                .create(true)
                .open(safe)?;
            file.write_all(unescaped.as_bytes())?;
        }
        "patch" => {
            if let (Some(search), Some(replace)) = (&req.search, &req.replace) {
                crate::operations::patch_file(&safe.to_string_lossy(), search, replace)
                    .map_err(|e| FileError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
            } else {
                return Err(FileError::ParseError("Patch operation requires 'search' and 'replace' fields.".into()));
            }
        }
        "insert_at" => {
            if let Some(line_num) = req.line {
                let content = std::fs::read_to_string(&safe)?;
                let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
                if line_num <= lines.len() {
                    lines.insert(line_num.saturating_sub(1), unescaped);
                } else {
                    lines.push(unescaped);
                }
                std::fs::write(&safe, lines.join("\n"))?;
            } else {
                return Err(FileError::ParseError("Insert_at operation requires 'line' field.".into()));
            }
        }
        _ => {
            // "overwrite" or default
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(safe)?;
            file.write_all(unescaped.as_bytes())?;
        }
    }
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
                        match write_file(base, &req) {
                            Ok(()) => println!("✓ Updated: {}", req.path),
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