use std::io::{self, Write};
use std::process::{Command, Stdio};
use std::fs::{self, OpenOptions};
use std::path::Path;
use std::time::Duration;
use std::sync::{Arc, Mutex};

use crate::model;
use crate::operations;

/// Per-command timeout: kill any single command that runs longer than this.
const CMD_TIMEOUT_SECS: u64 = 300;


#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

/// Runs a single shell command in the given working directory, prints and logs output.
/// Kills the child process if it runs longer than CMD_TIMEOUT_SECS.
fn run_command(cmd_str: &str, workdir: &Path, log_file: &mut fs::File) -> String {
    println!("⏳ Running: {}", cmd_str);
    let mut combined_output = String::new();

    // Build the child process (stdout + stderr piped so we can capture them)
    let child_result = if cfg!(target_os = "windows") {
        #[cfg(target_os = "windows")]
        {
            Command::new("cmd")
                .arg("/C")
                .raw_arg(cmd_str)
                .current_dir(workdir)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        }
        #[cfg(not(target_os = "windows"))]
        {
            Command::new("cmd")
                .args(["/C", cmd_str])
                .current_dir(workdir)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        }
    } else {
        Command::new("sh")
            .args(["-c", cmd_str])
            .current_dir(workdir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    };

    let child = match child_result {
        Ok(c) => c,
        Err(e) => {
            let err_msg = format!("❌ Failed to spawn '{}': {}", cmd_str, e);
            eprintln!("{}", err_msg);
            writeln!(log_file, "Failed to spawn '{}': {}", cmd_str, e).unwrap();
            combined_output.push_str(&format!("{}\n", err_msg));
            return combined_output;
        }
    };

    // ── Watchdog thread: kill child if it outlives CMD_TIMEOUT_SECS ─────────
    let child_id = child.id();
    let timed_out = Arc::new(Mutex::new(false));
    let timed_out_clone = Arc::clone(&timed_out);
    let timeout = Duration::from_secs(CMD_TIMEOUT_SECS);
    let watchdog = std::thread::spawn(move || {
        std::thread::sleep(timeout);
        *timed_out_clone.lock().unwrap() = true;
        // Kill by pid; best-effort — ignore errors
        #[cfg(target_os = "windows")]
        { let _ = Command::new("taskkill").args(["/F", "/PID", &child_id.to_string()]).output(); }
        #[cfg(not(target_os = "windows"))]
        { let _ = Command::new("kill").args(["-9", &child_id.to_string()]).output(); }
    });

    // Collect stdout / stderr AFTER child finishes
    let out = child.wait_with_output();
    drop(watchdog); // watchdog thread detaches; it's daemon-like

    match out {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let was_killed = *timed_out.lock().unwrap();

            if was_killed {
                let msg = format!("⏱️ Command '{}' killed after {}s timeout", cmd_str, CMD_TIMEOUT_SECS);
                eprintln!("{}", msg);
                combined_output.push_str(&format!("{}\n", msg));
                writeln!(log_file, "{}", msg).unwrap();
            }

            if !stdout.is_empty() {
                println!("--- STDOUT ---\n{}", stdout);
                combined_output.push_str(&format!("STDOUT:\n{}\n", stdout));
            } else {
                println!("(no stdout)");
            }
            if !stderr.is_empty() {
                // Print AND include in summary so the reviewer AI can read it
                println!("--- STDERR ---\n{}", stderr);
                combined_output.push_str(&format!("STDERR:\n{}\n", stderr));
            }

            writeln!(log_file, "Command: {}", cmd_str).unwrap();
            if !stdout.is_empty() {
                writeln!(log_file, "STDOUT:\n{}", stdout).unwrap();
            }
            if !stderr.is_empty() {
                writeln!(log_file, "STDERR:\n{}", stderr).unwrap();
            }
            writeln!(log_file, "Exit code: {}", output.status).unwrap();
            writeln!(log_file, "----------------------------------------").unwrap();

            combined_output.push_str(&format!("Exit code: {}\n", output.status));
        }
        Err(e) => {
            let err_msg = format!("❌ Failed to collect output for '{}': {}", cmd_str, e);
            eprintln!("{}", err_msg);
            writeln!(log_file, "Failed to collect output for '{}': {}", cmd_str, e).unwrap();
            combined_output.push_str(&format!("{}\n", err_msg));
        }
    }
    combined_output
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

        execute_task(&task, "", "./sandbox");
    }
}

pub fn execute_task(task: &str, context: &str, default_workdir: &str) -> String {
    // Scan project file tree for local fallback natively without AI search penalties
    let local_tree = operations::see();
    let local_tree_json = serde_json::to_string(&local_tree).unwrap_or_else(|_| "[]".to_string());
    let mut execution_summary = String::new();

    let prompt = format!(
        "You are 'Enigma Command Expert', a broad and extremely capable Windows PowerUser and DevOps Administrator.\n\
         Your goal is to fulfill the user's request using Windows CMD or PowerShell.\n\n\
         RESOURCES AT YOUR DISPOSAL:\n\
         - Semantic Search Context (indexed workspace paths): {context}\n\
         - Local Project Tree (ephemeral sandbox): {local_tree}\n\n\
         CONTEXT SELECTION RULES:\n\
         - You can work in either the ephemeral './sandbox' or within absolute workspace paths found in the Search Context.\n\
         - Use absolute paths for existing project files, and './sandbox' for new experiments or isolated tasks.\n\n\
         USER REQUEST: {task}\n\n\
         Respond with ONLY a valid JSON object in this EXACT format:\n\
         {{\n\
           \"workdir\": \"./sandbox\",\n\
           \"files\": [\n\
             {{ \"path\": \"script.ps1\", \"content\": \"...\" }}\n\
           ],\n\
           \"commands\": [\n\
             \"powershell -ExecutionPolicy Bypass -File script.ps1\"\n\
           ]\n\
         }}\n\n\
         CRITICAL RULES:\n\
         1. DO NOT recreate files, folders, or projects that you can already see in the Search Context.\n\
         2. Use the 'files' array ONLY for temporary scripts or small configuration edits required for the command.\n\
         3. NEVER use 'cd' as a command. Set the 'workdir' field instead.\n\
         4. Use relative paths for files ONLY if they are intended for the 'workdir' you specify.\n\
         5. Ignore large build artifacts (e.g., 'target/', 'dist/', 'bin/'), internal caches, or dependency lockfiles unless explicitly troubleshooting a build failure.
         6. SPATIAL AWARENESS: Default to working in '{default_workdir}'. If you create a project (e.g., 'cargo new X'), you MUST specify 'X' as your 'workdir' for any related tasks in this step. Our system will automatically track this for future steps.
         7. Return ONLY the JSON object, nothing else.",
        context = context,
        local_tree = local_tree_json,
        task = task,
        default_workdir = default_workdir
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
            let err_msg = format!("❌ Could not parse AI response as JSON: {}", e);
            eprintln!("{}", err_msg);
            return err_msg;
        }
    };

    // --- Read workdir (default to provided default_workdir) ---
    let workdir_str = parsed["workdir"].as_str().unwrap_or(default_workdir).to_string();
    let mut workdir = Path::new(&workdir_str).to_path_buf();

    // Force relative paths into the sandbox
    if workdir.is_relative() && !workdir_str.starts_with("./sandbox") && !workdir_str.starts_with("sandbox") {
        workdir = Path::new("./sandbox").join(workdir);
    }

    if !workdir.exists() {
        let _ = fs::create_dir_all(&workdir);
    }

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
            let mut path = Path::new(path_str).to_path_buf();
            if path.is_relative() && !path_str.starts_with("./sandbox") && !path_str.starts_with("sandbox") {
                path = workdir.join(path);
            }

            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    if let Err(e) = fs::create_dir_all(parent) {
                        eprintln!("❌ Failed to create directory {:?}: {}", parent, e);
                        continue;
                    }
                }
            }

            match fs::write(&path, content) {
                Ok(()) => println!("  ✅ Created: {}", path.display()),
                Err(e) => eprintln!("  ❌ Failed to write {}: {}", path.display(), e),
            }
        }
    }

    println!("\n📂 Working directory: {}", workdir.display());
    let effective_workdir = &workdir;

    // --- Step 3: Run commands ---
    let commands: Vec<String> = match parsed["commands"].as_array() {
        Some(arr) => arr.iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        None => {
            return "⚠️ No 'commands' array found in AI response.".to_string();
        }
    };

    if commands.is_empty() {
        println!("ℹ️  No commands to run.");
        return "ℹ️ No commands to run.".to_string();
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
        let output = run_command(cmd_str, effective_workdir, &mut log_file);
        execution_summary.push_str(&format!("Command: {}\n{}\n", cmd_str, output));
    }

    println!("\n✅ Done. Full output saved to cmd_outputs.txt");
    
    // --- Auto-Discovery: Check if a new project was created ---
    let mut final_cwd = effective_workdir.to_path_buf();
    if let Ok(entries) = fs::read_dir(effective_workdir) {
        for entry in entries.flatten() {
            if let Ok(ft) = entry.file_type() {
                if ft.is_dir() {
                    let path = entry.path();
                    // Look for project markers in the subdirectory
                    if path.join("Cargo.toml").exists() || path.join("package.json").exists() || 
                       path.join("go.mod").exists() || path.join("requirements.txt").exists() ||
                       path.join(".git").exists() {
                        println!("✨ Auto-Discovery: Detected new project at {}", path.display());
                        final_cwd = path;
                        break;
                    }
                }
            }
        }
    }

    execution_summary.push_str(&format!("\nSET_CWD: {}\n", final_cwd.display()));
    execution_summary
}
