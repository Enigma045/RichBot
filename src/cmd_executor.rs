use std::io::{self, Write};
use std::process::Command;
use std::fs::{self, OpenOptions};
use std::path::Path;

use crate::model;
use crate::operations;



#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

/// Runs a single shell command in the given working directory, prints and logs output.
fn run_command(cmd_str: &str, workdir: &Path, log_file: &mut fs::File) {
    println!("⏳ Running: {}", cmd_str);

    let output = if cfg!(target_os = "windows") {
        #[cfg(target_os = "windows")]
        {
            Command::new("cmd")
                .arg("/C")
                .raw_arg(cmd_str)
                .current_dir(workdir)
                .output()
        }
        #[cfg(not(target_os = "windows"))]
        {
            Command::new("cmd")
                .args(["/C", cmd_str])
                .current_dir(workdir)
                .output()
        }
    } else {
        Command::new("sh")
            .args(["-c", cmd_str])
            .current_dir(workdir)
            .output()
    };

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);

            if !stdout.is_empty() {
                println!("--- STDOUT ---\n{}", stdout);
            } else {
                println!("(no stdout)");
            }
            if !stderr.is_empty() {
                eprintln!("--- STDERR ---\n{}", stderr);
            }

            writeln!(log_file, "Command: {}", cmd_str).unwrap();
            if !stdout.is_empty() {
                writeln!(log_file, "STDOUT:\n{}", stdout).unwrap();
            }
            if !stderr.is_empty() {
                writeln!(log_file, "STDERR:\n{}", stderr).unwrap();
            }
            writeln!(log_file, "Exit code: {}", out.status).unwrap();
            writeln!(log_file, "----------------------------------------").unwrap();
        }
        Err(e) => {
            eprintln!("❌ Failed to execute '{}': {}", cmd_str, e);
            writeln!(log_file, "Failed to execute '{}': {}", cmd_str, e).unwrap();
        }
    }
}

pub fn execute_ai_commands() {
    loop {
        println!("\n💬 What do you want to do? (or type 'quit' to exit)");
        print!("> ");
        io::stdout().flush().unwrap();

        let mut task = String::new();
        io::stdin().read_line(&mut task).unwrap();
        let task = task.trim().to_string();

        if task.is_empty() {
            continue;
        }
        if task == "quit" || task == "exit" {
            println!("👋 Goodbye!");
            break;
        }

        execute_task(&task);
    }
}

pub fn execute_task(task: &str) {
    // Scan project file tree for context
    let file_tree = operations::see();
    let file_tree_json = serde_json::to_string(&file_tree).unwrap_or_else(|_| "[]".to_string());

    let prompt = format!(
        "You are a command generator and project creator for Windows CMD/PowerShell.\n\
         Current project file tree: {file_tree}\n\n\
         The user wants to: {task}\n\n\
         Respond with ONLY a valid JSON object (no markdown, no backticks) in this EXACT format:\n\
         {{\n\
           \"workdir\": \"relative/path/to/run/commands/in\",\n\
           \"files\": [\n\
             {{ \"path\": \"relative/path/to/file\", \"content\": \"file content here\" }}\n\
           ],\n\
           \"commands\": [\n\
             \"command to run\"\n\
           ]\n\
         }}\n\n\
         CRITICAL RULES:\n\
         1. \"workdir\" is the directory where ALL commands will be run. \
            For a project in 'server/', set workdir to \"server\". \
            For the current directory use \".\".\n\
         2. NEVER include 'cd' as a command — use 'workdir' instead.\n\
         3. If you are already creating project files in 'files', do NOT run \
            'cargo new', 'npm init', 'mkdir' or any scaffolding command — the files are created for you.\n\
         4. After writing files for a Rust project, only run: \"cargo build\" and/or \"cargo run\".\n\
         5. After writing files for a Python project, only run: \"python main.py\" (or relevant file).\n\
         6. After writing files for a Node project, only run: \"npm install\" then \"node index.js\".\n\
         7. Always quote filenames/paths with spaces in commands.\n\
         8. For opening media files or launching UI apps, NEVER use 'start'. Use PowerShell: powershell -c \"Invoke-Item 'filename.ext'\" or powershell -c \"Start-Process 'app.exe' -ArgumentList 'args'\"\n\
         9. Keep file content complete and functional, never use placeholders.\n\
         10. Use relative paths only in 'files'.\n\
         11. Escape all double quotes inside 'content' strings as \\\".\n\
         Return ONLY the JSON object, nothing else.",
        file_tree = file_tree_json,
        task = task
    );

    let ai_response = model::set_control(&prompt);

    // Strip any accidental markdown fences
    let cleaned = ai_response.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    // Parse the combined response
    let parsed: serde_json::Value = match serde_json::from_str(cleaned) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("❌ Could not parse AI response as JSON: {}", e);
            eprintln!("Raw response:\n{}", ai_response);
            return;
        }
    };

    // --- Read workdir (default to ".") ---
    let workdir_str = parsed["workdir"].as_str().unwrap_or(".").to_string();
    let workdir = Path::new(&workdir_str);

    // --- Step 1: Create files ---
    if let Some(files) = parsed["files"].as_array() {
        if !files.is_empty() {
            println!("\n📁 Creating {} file(s)...", files.len());
        }
        for file_entry in files {
            let path_str = match file_entry["path"].as_str() {
                Some(p) => p,
                None => { eprintln!("⚠️  Skipping file entry with no path"); continue; }
            };
            let content = file_entry["content"].as_str().unwrap_or("");
            let path = Path::new(path_str);

            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    if let Err(e) = fs::create_dir_all(parent) {
                        eprintln!("❌ Failed to create directory {:?}: {}", parent, e);
                        continue;
                    }
                }
            }

            match fs::write(path, content) {
                Ok(()) => println!("  ✅ Created: {}", path_str),
                Err(e) => eprintln!("  ❌ Failed to write {}: {}", path_str, e),
            }
        }
    }

    // --- Step 2: Validate workdir exists ---
    if !workdir.exists() {
        eprintln!("⚠️  workdir '{}' does not exist, falling back to '.'", workdir_str);
    }
    let effective_workdir = if workdir.exists() { workdir } else { Path::new(".") };
    println!("\n📂 Working directory: {}", effective_workdir.display());

    // --- Step 3: Run commands ---
    let commands: Vec<String> = match parsed["commands"].as_array() {
        Some(arr) => arr.iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        None => {
            eprintln!("⚠️  No 'commands' array found in AI response.");
            return;
        }
    };

    if commands.is_empty() {
        println!("ℹ️  No commands to run.");
        return;
    }

    println!("\n🤖 Running {} command(s):\n", commands.len());
    for cmd in &commands {
        println!("  → {}", cmd);
    }
    println!();

    let mut log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("cmd_outputs.txt")
        .expect("Should be able to open cmd_outputs.txt for logging");

    for cmd_str in &commands {
        run_command(cmd_str, effective_workdir, &mut log_file);
    }

    println!("\n✅ Done. Full output saved to cmd_outputs.txt");
}
