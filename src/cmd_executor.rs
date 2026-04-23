use std::io::{self, Write};
use std::process::{Command, Stdio};
use std::fs::{self, OpenOptions};
use std::path::Path;
use std::time::Duration;
use std::sync::{Arc, Mutex};

use crate::model;

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
    // Uses a channel so the watchdog exits IMMEDIATELY when the command finishes
    // instead of sleeping for 300s and leaking an OS thread per command.
    let child_id = child.id();
    let timed_out = Arc::new(Mutex::new(false));
    let timed_out_clone = Arc::clone(&timed_out);
    let timeout = Duration::from_secs(CMD_TIMEOUT_SECS);
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let watchdog = std::thread::spawn(move || {
        if done_rx.recv_timeout(timeout).is_err() {
            // Timed out — the command is still running, kill it
            *timed_out_clone.lock().unwrap() = true;
            #[cfg(target_os = "windows")]
            { let _ = Command::new("taskkill").args(["/F", "/PID", &child_id.to_string()]).output(); }
            #[cfg(not(target_os = "windows"))]
            { let _ = Command::new("kill").args(["-9", &child_id.to_string()]).output(); }
        }
        // If recv_timeout returns Ok(()), the command finished — watchdog exits cleanly.
    });

    // Collect stdout / stderr AFTER child finishes
    let out = child.wait_with_output();
    let _ = done_tx.send(()); // Signal watchdog to exit immediately
    let _ = watchdog.join();  // Reap the thread — no leaks

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
    let local_tree = if default_workdir != "./sandbox" && default_workdir != "." {
        crate::operations::list_cwd(default_workdir)
    } else {
        crate::operations::see()
    };
    let local_tree_json = serde_json::to_string(&local_tree).unwrap_or_else(|_| "[]".to_string());
    let mut execution_summary = String::new();

    // FIX 1: Use the actual default_workdir in the JSON format example instead of the
    // hardcoded literal "./sandbox". The old example caused the AI to always copy
    // "./sandbox" verbatim regardless of what default_workdir was, which then triggered
    // the path-resolution branch that treated any "./sandbox"-prefixed value as
    // root-anchored — regressing CWD back to sandbox on every step.
    let prompt = format!(
        "You are 'Enigma Command Expert', a broad and extremely capable Windows PowerUser and DevOps Administrator.\n\
         Your goal is to fulfill the user's request using Windows CMD or PowerShell.\n\n\
         RESOURCES AT YOUR DISPOSAL:\n\
         - Semantic Search Context (indexed workspace paths): {context}\n\
         - Local Project Tree (ephemeral sandbox or established workdir): {local_tree}\n\n\
         CONTEXT SELECTION RULES:\n\
         - You can work in either the ephemeral './sandbox' or within absolute workspace paths found in the Search Context.\n\
         - Use absolute paths for existing project files, and './sandbox' for new experiments or isolated tasks.\n\n\
         USER REQUEST: {task}\n\n\
         Respond with ONLY a valid JSON object in this EXACT format:\n\
         {{\n\
           \"workdir\": \"{default_workdir}\",\n\
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
         5. Ignore large build artifacts (e.g., 'target/', 'dist/', 'bin/'), internal caches, or dependency lockfiles unless explicitly troubleshooting a build failure.\n\
         6. SPATIAL AWARENESS: Default to working in '{default_workdir}'. If you create a project (e.g., 'cargo new X'), you MUST specify 'X' as your 'workdir' for any related tasks in this step. Our system will automatically track this for future steps.\n\
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

    // ── Workdir Resolution ───────────────────────────────────────────────────
    //
    // FIX 2: Replaced the old starts_with("./sandbox") root-anchor heuristic with
    // a three-case normalised comparison. The old logic had two failure modes:
    //
    //   A) AI returns "./sandbox" (ancestor of default_workdir) → old code used it
    //      directly, regressing CWD from e.g. "./sandbox/myproject" back to root.
    //
    //   B) AI correctly returns the full path (e.g. "./sandbox/myproject") → joining
    //      it with default_workdir caused double-nesting.
    //
    // New rules (applied after normalising both sides):
    //   1. AI returned an absolute path          → use it directly.
    //   2. AI returned a path that is a prefix   → AI regressed; ignore, keep default.
    //      of (or equal to) default_workdir
    //   3. AI returned a path that starts with   → full path given; use directly to
    //      default_workdir (i.e. already rooted)   avoid double-nesting.
    //   4. Everything else (bare relative subdir) → join with default_workdir (normal).

    let default_norm = crate::operations::normalize_path(Path::new(default_workdir));
    let mut workdir = default_norm.clone();

    if let Some(req_str) = parsed["workdir"].as_str() {
        let req_str = req_str.trim();
        if !req_str.is_empty() && req_str != "." {
            let req_path = Path::new(req_str);

            if req_path.is_absolute() {
                // Case 1: Absolute path — trust it directly.
                workdir = crate::operations::normalize_path(req_path);
            } else {
                let req_norm = crate::operations::normalize_path(req_path);

                if default_norm.starts_with(&req_norm) {
                    // Case 2: AI returned an ancestor of (or equal to) our current
                    // default_workdir — this is a regression. Stay where we are.
                    eprintln!(
                        "⚠️  Workdir guard: AI returned '{}' which is a parent of '{}'. Keeping default.",
                        req_str, default_workdir
                    );
                    workdir = default_norm.clone();
                } else if req_norm.starts_with(&default_norm) {
                    // Case 3: AI returned a full path already rooted under
                    // default_workdir — use directly to avoid double-nesting.
                    workdir = req_norm;
                } else {
                    // Case 4: Bare relative subdirectory — join with default_workdir.
                    workdir = default_norm.join(req_str);
                    workdir = crate::operations::normalize_path(&workdir);
                }
            }
        }
    }

    let workdir_str = workdir.to_string_lossy().to_string();
    println!("📂 Working directory: {}", workdir_str);

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

            // Resolve relative file paths against the resolved workdir, not sandbox root.
            if path.is_relative() {
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

    // --- Step 2: Run commands ---
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
    // GUARD: Only scan for sub-projects when we're still in a container directory
    // (e.g. "./sandbox"). If we're already inside a project, scanning subdirs can
    // incorrectly navigate into nested crates/test workspaces/etc.
    let ew_str = effective_workdir.to_string_lossy();
    let is_container_dir = ew_str == "./sandbox"
        || ew_str == "sandbox"
        || ew_str.ends_with("/sandbox")
        || ew_str.ends_with("\\sandbox");

    let mut final_cwd = effective_workdir.to_path_buf();
    if is_container_dir {
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
    }

    let mut cwd_str = final_cwd.display().to_string();
    if cwd_str.starts_with(r"\\?\") {
        cwd_str = cwd_str[4..].to_string();
    }

    execution_summary.push_str(&format!("\nSET_CWD: {}\n", cwd_str));
    execution_summary
}
