use serde::Deserialize;
use serde_json;
use std::fs::{File, OpenOptions};
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
    PathTraversal(String),
}

impl From<std::io::Error> for FileError {
    fn from(e: std::io::Error) -> Self {
        FileError::IoError(e)
    }
}

fn safe_path(base: &Path, untrusted: &str) -> Result<PathBuf, FileError> {
    let base = base.canonicalize()
        .map_err(|e| FileError::IoError(e))?;

    let joined = base.join(untrusted);

  let (cannon, filename) = if joined.exists() {
      (joined.canonicalize()?, None)
  } else {
      let parent = joined.parent()
           .ok_or_else(|| FileError::PathTraversal("no parent dir".into()))?;
      let parent_cannon = parent.canonicalize()
           .map_err(|_| FileError::PathTraversal("parent dir does not exist".into()))?;
      let filename = joined.file_name()
           .ok_or_else(|| FileError::PathTraversal("no filename".into()))?;
      (parent_cannon, Some(filename.to_owned()))
  };

  let final_path = match filename {
    Some(f) => cannon.join(f),
    None => cannon,
  };

  if !final_path.starts_with(&base) {
    return Err(FileError::PathTraversal(format!(
        "path '{}' escapes base dir '{}'",
        final_path.display(),
        base.display()
    )));
  }

  Ok(final_path)

}

pub fn write_file(base: &Path, path: &str, content: &str) -> Result<(), FileError> {
    let safe = safe_path(base, path)?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(safe)?;

    file.write_all(b"\n")?;
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