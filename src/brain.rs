// ═══════════════════════════════════════════════════════════════════════════
// brain.rs — Enigma Mandatory Orchestrator
//
// EVERY prompt runs through Brain first.
// Brain decomposes the request, assigns an AI function category per step,
// chains prior-step context into each subsequent step, writes a plan to
// plans/plan.txt, and synthesises a final response.
// ═══════════════════════════════════════════════════════════════════════════

use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use chrono::Local;

const PLANS_DIR: &str = "plans";

// ─── Status Notifications ───────────────────────────────────────────────────
fn notify_stage(message: &str) {
    println!("\n🚀 [STAGE] {}", message);
    let _ = std::io::stdout().flush();
}

fn notify_step(step: u32, total: usize, intent: &str) {
    println!("📍 [STEP {}/{}] Starting: {}", step, total, intent);
    let _ = std::io::stdout().flush();
}

fn notify_complete(message: &str) {
    println!("✅ [COMPLETE] {}\n", message);
    let _ = std::io::stdout().flush();
}

// ─── Runtime Configuration (read from tracker.json) ─────────────────────────

/// Reads the current max Brain decomposition steps from tracker.json.
/// Returns 16 (the AI's natural cap) when tracker.max_steps == 0.
pub fn load_max_steps() -> u32 {
    let tracker = crate::model::RequestTracker::new();
    if tracker.max_steps == 0 { 16 } else { tracker.max_steps }
}

/// Reads the current max rollbacks (keyword "rollback N") from tracker.json.
#[allow(dead_code)]
pub fn load_max_rollbacks() -> u32 {
    let tracker = crate::model::RequestTracker::new();
    tracker.max_rollbacks
}

/// Reads max review retries from tracker.json.
#[allow(dead_code)]
pub fn load_max_retries() -> u8 {
    let tracker = crate::model::RequestTracker::new();
    tracker.max_retries
}

/// AI function categories — mirrors the auto_router labels.
/// 1 = General Chat
/// 2 = Analyze Project / Knowledge
/// 3 = Execute Tasks
/// 4 = Generate Content / Files
/// 5 = Spotify Integration
pub fn category_label(cat: u64) -> &'static str {
    match cat {
        2 => "Analyze Project / Knowledge",
        3 => "Execute Tasks",
        4 => "Generate Content / Files",
        5 => "Spotify Integration",
        _ => "General Chat",
    }
}

// ─── Result Types ────────────────────────────────────────────────────────────

/// Typed execution status — replaces fragile string-matching on raw output.
#[derive(Debug, Clone, PartialEq)]
pub enum StepStatus {
    /// Step completed without a detected hard error.
    Success,
    /// Step produced a hard error; inner string is the reason.
    Failed(String),
}

/// The typed result of one executed sub-task.
/// Replaces the stringly-typed `(SubTask, String, String)` tuple.
#[derive(Debug, Clone)]
pub struct StepResult {
    pub task:   SubTask,
    pub output: String,
    pub cwd:    String,
    pub status: StepStatus,
}

impl StepResult {
    /// One-line summary used in context chains and review prompts.
    pub fn summary(&self) -> String {
        format!("Step {} [{}]: {}\nOutput: {}", self.task.step, self.task.intent, self.task.prompt, self.output)
    }

    /// True only on a definitive hard failure — avoids false positives from
    /// STDERR content that is merely informational.
    pub fn is_hard_failure(&self) -> bool {
        matches!(&self.status, StepStatus::Failed(_))
    }

    /// Strip internal sentinels (SET_CWD, exit codes, STDOUT/STDERR labels)
    /// before showing output to the user or to the synthesiser.
    pub fn clean_output(&self) -> String {
        self.output
            .lines()
            .filter(|l| {
                !l.starts_with("SET_CWD:")
                    && !l.starts_with("Exit code:")
                    && !l.starts_with("STDOUT:")
                    && !l.starts_with("STDERR:")
                    && !l.starts_with("Command:")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Maximum characters of prior-step output to inject into each step's context.
/// Prevents token-limit blowout on long multi-step tasks.
const MAX_PRIOR_CTX_CHARS: usize = 8_000;


/// One focused step produced by the Brain decomposer.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SubTask {
    pub step: u32,
    /// Short human-readable label e.g. "Search context"
    pub intent: String,
    /// The focused, self-contained prompt for this step
    pub prompt: String,
    /// Which AI function to invoke (1-5)
    #[serde(default = "default_category")]
    pub category: u64,
    /// Estimated number of AI calls this step will consume
    #[serde(default = "default_calls")]
    pub estimated_calls: u32,
}

fn default_category() -> u64 { 1 }
fn default_calls() -> u32 { 1 }

/// The full plan returned by the AI decomposer.
#[derive(Serialize, Deserialize, Debug)]
struct TaskPlan {
    tasks: Vec<SubTask>,
}

// ─── Step 1: Decompose ───────────────────────────────────────────────────────

/// Calls the AI once to break the user's request into focused steps.
/// `max_steps` is passed in by the caller (already loaded from tracker).
pub fn decompose(prompt: &str, max_steps: u32) -> Vec<SubTask> {
    let system = format!(
        r#"You are 'Enigma Brain', an expert AI task planner and router.
Your job: decompose the user's request into a series of clear, focused sub-tasks.

AVAILABLE AI FUNCTIONS (categories):
1 = General Chat      — greetings, simple Q&A, jokes, quick questions
2 = Analyze/Research  — finding local files, finding movies/shows, finding information, gathering context. FEATURE: You can also request specific file contents by responding ONLY with a JSON array of paths (e.g., ["main.rs"]).
3 = Execute Tasks     — running commands, opening files, system automation.
4 = Create Content    — writing files, generating code, building scripts. FEATURE: Supports partial updates via 'op' field ("patch", "append", "insert_at", "overwrite"). Use absolute paths if specified by the user, otherwise default to relative paths.
5 = Spotify           — controlling Spotify playback only

RULES:
1. Each sub-task must have ONE specific goal.
2. Assign the best-fit category (1-5) to each step.
3. Set estimated_calls to the number of AI calls you expect (1-3 typical).
4. Keep "prompt" concise and self-contained. Include filenames/keywords from the original.
5. Order steps logically: gather context → act → confirm/summarise.
6. CRITICAL: For ANY task that requires reading, modifying, opening, executing, or interacting with specific local files, you MUST create a Category 2 step FIRST to search for the exact file path, followed by the execution/content step.
7. SCOPED READING: If you need to see a file's code before editing it, use Category 2 and specify the file path.
8. RELIABLE EDITING: Use Category 4 with op: "patch" to modify specific blocks in large existing files.
9. Limit to MAX {} steps. Simple tasks = 1 step.
10. SPATIAL AWARENESS: If a step involves creating a project or folder (e.g., 'cargo new X'), you MUST assume that subsequent steps will execute *inside* that subdirectory. Write your prompts relative to that new root.
11. DO NOT generate redundant 'cd' or 'folder setup' steps if the project already exists in the search results.
12. Return ONLY a valid JSON object, nothing else:
{{
  "tasks": [
    {{ "step": 1, "intent": "Search context", "prompt": "Find path for X", "category": 2, "estimated_calls": 2 }},
    ...
  ]
}}

User Request: "{}"
"#,
        max_steps, prompt
    );

    let raw = crate::model::set_control_with_persona(&system, "Quick");
    let start = raw.find('{').unwrap_or(0);
    let end = raw.rfind('}').unwrap_or(raw.len().saturating_sub(1)) + 1;
    
    let clean = if start < end {
        &raw[start..end]
    } else {
        raw.trim()
    };

    match serde_json::from_str::<TaskPlan>(clean) {
        Ok(plan) if !plan.tasks.is_empty() => plan.tasks,
        _ => {
            eprintln!("⚠️  Brain: Decomposition failed — single-step fallback.");
            vec![SubTask {
                step: 1,
                intent: "Direct Execution".to_string(),
                prompt: prompt.to_string(),
                category: 1,
                estimated_calls: 1,
            }]
        }
    }
}

// ─── Plan File ───────────────────────────────────────────────────────────────

/// Writes a human-readable plan to a unique file in plans/ and returns the path.
pub fn write_plan(original_prompt: &str, tasks: &[SubTask]) -> String {
    // Ensure the plans/ directory exists
    if let Err(e) = fs::create_dir_all(PLANS_DIR) {
        eprintln!("⚠️  Brain: Could not create plans/ dir: {}", e);
    }

    // Generate unique timestamped filename: plan_20260412_184810.txt
    let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
    let filename = format!("plans/plan_{}.txt", timestamp);

    let total_calls: u32 = tasks.iter().map(|t| t.estimated_calls).sum();

    let mut lines = vec![
        "═══════════════════════════════════════════════════".to_string(),
        " 🧠 ENIGMA BRAIN — EXECUTION PLAN".to_string(),
        "═══════════════════════════════════════════════════".to_string(),
        format!(" Request : {}", original_prompt),
        format!(" Steps   : {}", tasks.len()),
        format!(" Est. AI calls: {}", total_calls),
        format!(" File    : {}", filename),
        "───────────────────────────────────────────────────".to_string(),
    ];

    for task in tasks {
        lines.push(format!(
            "Step {}: [{}]",
            task.step,
            task.intent
        ));
        lines.push(format!(
            "  Function : {} (cat {})",
            category_label(task.category),
            task.category
        ));
        lines.push(format!("  Est calls: {}", task.estimated_calls));
        lines.push(format!("  Prompt   : {}", task.prompt));
        lines.push("───────────────────────────────────────────────────".to_string());
    }

    lines.push(String::new());
    lines.push("Generated by Enigma Brain. Plan will be updated after execution.".to_string());

    let content = lines.join("\n");

    // Primary unique log
    if let Err(e) = fs::write(&filename, &content) {
        eprintln!("⚠️  Brain: Could not write unique plan file: {}", e);
    }

    // Latest-link for Go-bot/WhatsApp compatibility
    if let Err(e) = fs::write("plans/plan.txt", &content) {
        eprintln!("⚠️  Brain: Could not update plans/plan.txt: {}", e);
    }

    eprintln!("📋 Brain: Plan saved to {}", filename);

    filename
}

/// Reads the LATEST plan file.
pub fn read_plan() -> String {
    let mut plans: Vec<_> = fs::read_dir(PLANS_DIR)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().starts_with("plan_"))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default();
    
    plans.sort();
    
    if let Some(latest) = plans.last() {
        fs::read_to_string(latest).unwrap_or_else(|_| "Error reading plan.".to_string())
    } else {
        "No plans found. Run a Brain prompt first.".to_string()
    }
}

// ─── Step 2: Execute each sub-task with context chaining ────────────────────

/// Runs every sub-task through the appropriate AI function.
/// Each step receives a bounded summary of all previous steps as context.
/// Returns typed StepResults so callers never need to string-match on raw output.
pub fn execute_plan(
    tasks: &[SubTask],
    persona: &str,
    base_url: &str,
    initial_cwd: &str,
    prior_results: &[StepResult],
) -> (Vec<StepResult>, String, bool) {
    let mut results: Vec<StepResult> = Vec::new();
    let mut current_cwd = initial_cwd.to_string();
    let mut interrupted = false;

    for sub in tasks {
        notify_step(sub.step, tasks.len(), &sub.intent);
        eprintln!(
            "   [{}] → {} (CWD: {})",
            category_label(sub.category),
            sub.prompt,
            current_cwd
        );

        // Build bounded prior-step context.
        // Concatenate summaries newest-first, then cap at MAX_PRIOR_CTX_CHARS so
        // long runs don't silently blow up the context window.
        let mut full_results: Vec<&StepResult> = prior_results.iter().collect();
        full_results.extend(results.iter());

        let prior_ctx = if full_results.is_empty() {
            String::new()
        } else {
            let raw: String = full_results
                .iter()
                .map(|r| r.summary())
                .collect::<Vec<_>>()
                .join("\n---\n");

            let capped = if raw.len() > MAX_PRIOR_CTX_CHARS {
                // Keep the tail (most recent context is most valuable)
                let start = raw.len() - MAX_PRIOR_CTX_CHARS;
                format!("[…prior context truncated…]\n{}", &raw[start..])
            } else {
                raw
            };
            format!("\n\n[Prior step context — use this to inform your response]:\n{}", capped)
        };

        let enriched_prompt = format!("{}{}", sub.prompt, prior_ctx);
        let output = dispatch(sub.category, &enriched_prompt, persona, base_url, &current_cwd);

        // CWD tracking: parse SET_CWD sentinel (falls back to full remainder if no trailing newline).
        if let Some(pos) = output.find("SET_CWD: ") {
            let rest = &output[pos + 9..];
            let new_cwd = if let Some(line_end) = rest.find('\n') {
                rest[..line_end].trim().to_string()
            } else {
                rest.trim().to_string()
            };
            if !new_cwd.is_empty() {
                let norm = crate::operations::normalize_path(Path::new(&new_cwd));
                let norm_str = norm.to_string_lossy().to_string();
                eprintln!("🧠 Brain: Base directory updated to {}", norm_str);
                current_cwd = norm_str;
            }
        }

        // Typed error detection — no more fragile string matching on STDERR content.
        let status = detect_step_status(&output);
        let failed = matches!(&status, StepStatus::Failed(_));

        results.push(StepResult {
            task: sub.clone(),
            output,
            cwd: current_cwd.clone(),
            status,
        });

        if failed {
            eprintln!("🛑 Brain: Execution halted at step {}.", sub.step);
            notify_complete(&format!("Step {} failed.", sub.step));
            interrupted = true;
            break;
        }

        notify_complete(&format!("Step {} finished.", sub.step));
    }

    (results, current_cwd, interrupted)
}

/// Determines step status from the raw output string.
/// Separates definitive hard failures (non-zero exit codes, spawn failures,
/// content-write failures) from STDERR output that is merely informational.
fn detect_step_status(output: &str) -> StepStatus {
    // Content/spawn hard failures
    if output.contains("❌ Content creation failed") || output.contains("Failed to spawn") {
        return StepStatus::Failed(output.lines().find(|l| l.contains("❌")).unwrap_or("unknown").to_string());
    }

    // PowerShell structured error records (not just any STDERR line)
    let is_ps_err = output.contains("FullyQualifiedErrorId")
        || (output.contains("At line:") && output.contains("char:") && output.contains("CategoryInfo"));
    if is_ps_err {
        return StepStatus::Failed("PowerShell error record detected".to_string());
    }

    // Non-zero exit code — must be an explicit "Exit code:" line to avoid false positives
    for line in output.lines() {
        if line.starts_with("Exit code:") || line.starts_with("Exit code: exit code:") {
            let code_part = line.trim_start_matches("Exit code:").trim();
            // Accept "0", "exit status: 0", or "exit code: 0"
            let success = code_part == "0"
                || code_part.ends_with(": 0")
                || code_part == "exit status: 0";
            if !success {
                return StepStatus::Failed(format!("Non-zero {}", line));
            }
        }
    }

    StepStatus::Success
}


// ─── Step 3: Synthesise ─────────────────────────────────────────────────────

/// Merges all step outputs into one final, coherent answer.
/// Single-step tasks are still routed through synthesis so sentinels are stripped
/// and the output is always clean before it reaches the user.
pub fn synthesise(original_prompt: &str, results: &[StepResult]) -> String {
    // For a single step, strip internal sentinels and return directly —
    // no need for a second AI call just to reformat one result.
    if results.len() == 1 {
        let clean = results[0].clean_output();
        let trimmed = clean.trim();
        return if trimmed.is_empty() {
            results[0].output.clone()
        } else {
            trimmed.to_string()
        };
    }

    let steps_summary: String = results
        .iter()
        .map(|r| {
            format!(
                "Step {}: [{}] ({})\nPrompt: {}\nResult:\n{}\n",
                r.task.step,
                r.task.intent,
                category_label(r.task.category),
                r.task.prompt,
                r.clean_output()   // sentinels stripped before the synthesiser sees them
            )
        })
        .collect::<Vec<_>>()
        .join("\n---\n");

    let synthesis_prompt = format!(
        r#"You are 'Enigma Brain'. Below are the sequential results of a multi-step task.

Original Request: "{}"

Step Results:
{}

Your job: Write a single, coherent, friendly response that:
1. Summarises what was accomplished across all steps.
2. Highlights any important outputs (file paths, command outputs, data found).
3. If any step failed or had no output, mention it briefly.
4. Keep it concise — no repetition."#,
        original_prompt, steps_summary
    );

    crate::model::set_control_with_persona(&synthesis_prompt, "Helpful")
}


// ─── Full Brain Pipeline ─────────────────────────────────────────────────────

fn execute_with_rollbacks(
    prompt: &str,
    mut current_tasks: Vec<SubTask>,
    persona: &str,
    base_url: &str,
    mut current_cwd: String,
    mut results: Vec<StepResult>,
    rollbacks_left: &mut u32,
) -> (Vec<StepResult>, String) {
    loop {
        notify_stage("Starting Core Execution");
        let (batch_results, final_cwd, halted) = execute_plan(&current_tasks, persona, base_url, &current_cwd, &results);
        results.extend(batch_results);
        current_cwd = final_cwd;

        if !halted { break; }

        if *rollbacks_left == 0 {
            eprintln!("⚠️ Max rollbacks reached or disabled. Proceeding...");
            break;
        }

        notify_stage(&format!("Rollback Triggered ({} left). Assessing failure...", *rollbacks_left));

        let review_summary: String = results
            .iter()
            .map(|r| r.summary())
            .collect::<Vec<_>>()
            .join("\n---\n");

        let original_plan_summary = current_tasks
            .iter()
            .map(|s| format!("Step {} [Cat {}]: {}", s.step, s.category, s.intent))
            .collect::<Vec<_>>()
            .join("\n");

        let rollback_prompt = format!(
            r#"You are 'Enigma Brain — Dynamic Rollback Assessor'.
Original Request: "{}"
Current Working Directory: "{}"

The active execution plan was:
{}

Execution halted because the very last step in this log failed:
{}

Your task:
1. ROOT CAUSE ANALYSIS: Identify exactly why the failure happened. Trace back the logic. Was it a wrong path in Step 2? A missing module from Step 1?
2. OPTIMAL ROLLBACK: Decide the most logical step to jump back to. Do not just pick the previous step if the error has deeper roots. Minimise redundant rework while ensuring a solid fix.
3. REASONING: Briefly explain your chosen rollback point (e.g. "Missing dependency in Step 2 requires re-running setup").
4. COMPLETE RESPONSE: Output exactly 'REASONING: <your_reasoning>' followed by 'ROLLBACK: <step_number>', then the new JSON array of SubTasks to FIX the problem AND COMPLETELY FINISH the plan.
   IMPORTANT: Number your replacement tasks starting from the ROLLBACK step number so they do not collide with retained step results.

Format:
REASONING: Short explanation here.
ROLLBACK: 2
{{
  "tasks": [
    {{ "step": 2, "intent": "Short label", "prompt": "Focused sub-prompt here", "category": 4, "estimated_calls": 1 }},
    ... (all remaining steps to finish original objective)
  ]
}}
"#, prompt, current_cwd, original_plan_summary, review_summary
        );

        let review = crate::model::set_control_with_persona(&rollback_prompt, "Quick");

        if review.trim().is_empty() {
           eprintln!("⚠️ Empty rollback response. Halting.");
           break;
        }

        if let Some(pos) = review.find("REASONING: ") {
            let s = review[pos + 11..].lines().next().unwrap_or("").trim();
            eprintln!("🧐 Brain Rollback Reasoning: {}", s);
        }

        let target_step = if let Some(pos) = review.find("ROLLBACK: ") {
            let s = review[pos + 10..].lines().next().unwrap_or("").trim();
            s.parse::<u32>().unwrap_or_else(|_| results.last().map(|r| r.task.step).unwrap_or(1))
        } else {
            results.last().map(|r| r.task.step).unwrap_or(1)
        };

        eprintln!("⏪ Brain rolling back to Step {}...", target_step);
        results.retain(|r| r.task.step < target_step);

        // Reset CWD to the last retained step, or original sandbox root
        current_cwd = results.last()
            .map(|r| r.cwd.clone())
            .unwrap_or_else(|| "./sandbox".to_string());
        eprintln!("🧠 Brain: CWD reset to {} after rollback", current_cwd);

        let start = review.find('{').unwrap_or(0);
        let end = review.rfind('}').map(|i| i + 1).unwrap_or(review.len());
        let clean = if start < end { &review[start..end] } else { review.trim() };

        match serde_json::from_str::<TaskPlan>(clean) {
            Ok(ref plan) if !plan.tasks.is_empty() => {
                current_tasks = plan.tasks.clone();
                let _ = write_plan(&format!("[ROLLBACK TO {}]", target_step), &current_tasks);
                notify_stage(&format!("Rollback Plan Generated ({} steps)", current_tasks.len()));
            },
            Err(e) => {
                eprintln!("⚠️ Failed to parse rollback plan: {}", e);
                let _ = std::fs::write("plans/rollback_err.json", clean);
                break;
            },
            _ => {
                eprintln!("⚠️ Rollback plan was empty. Halting rollback engine.");
                break;
            }
        }

        *rollbacks_left -= 1;
    } // End rollback loop

    (results, current_cwd)
}

/// Entry point: decompose → write plan → execute → synthesise.
/// Returns (final_response, plan_file_path).
pub fn run(prompt: &str, persona: &str, base_url: &str) -> (String, String) {
    eprintln!("🧠 Enigma Brain: Analysing request…");

    // Read tracker.json ONCE and extract all config values — avoids 3 separate disk reads.
    let tracker = crate::model::RequestTracker::new();
    let current_max_steps = if tracker.max_steps == 0 { 16 } else { tracker.max_steps };
    let max_retries      = tracker.max_retries;
    let max_rollbacks    = tracker.max_rollbacks;
    eprintln!("🔢 Brain config: max_steps={}, max_retries={}, max_rollbacks={}", current_max_steps, max_retries, max_rollbacks);

    // Step 1: Decompose — pass the already-loaded max_steps, no extra disk read.
    let tasks = decompose(prompt, current_max_steps);
    eprintln!("🧠 Plan ready: {} step(s)", tasks.len());

    // Step 2: Write plan.txt NOW — before any execution
    let mut plan_path = write_plan(prompt, &tasks);

    // Step 3: Execute each step with context chaining and dynamic rollback
    let mut current_cwd = "./sandbox".to_string();
    let mut results: Vec<StepResult> = Vec::new();
    let mut rollbacks_left = max_rollbacks;

    let (mut executed_results, mut final_cwd) = execute_with_rollbacks(
        prompt,
        tasks,
        persona,
        base_url,
        current_cwd.clone(),
        results.clone(),
        &mut rollbacks_left,
    );
    results = executed_results;
    current_cwd = final_cwd;

    // Step 4: Synthesise final answer
    notify_stage("Synthesising Final Response");
    let mut final_response = synthesise(prompt, &results);

    // ── Step 5: Review & Retry Loop ──────────────────────────────────────────

    for attempt in 0..max_retries {
        notify_stage(&format!("Reviewing Results (Attempt {}/{})", attempt + 1, max_retries));

        // Validate the current CWD is still reachable before each review round.
        if !std::path::Path::new(&current_cwd).exists() {
            if let Some(r) = results.last() {
                if std::path::Path::new(&r.cwd).exists() {
                    current_cwd = r.cwd.clone();
                    eprintln!("🧠 Brain: Restored CWD from history: {}", current_cwd);
                }
            }
        }

        let review_summary: String = results
            .iter()
            .map(|r| r.summary())
            .collect::<Vec<_>>()
            .join("\n---\n");

        let review_prompt = format!(
            r#"You are 'Enigma Brain — Reviewer'.

Original Request: "{original}"

Current working directory: "{cwd}"

Below are all the step results from the last execution:
{summary}

Your task:
1. Check every step for failures, errors, or incomplete output.
2. Pay special attention to 'Exit code: exit code: 1' or any line starting with 'error:' — these indicate real failures.
3. If ALL steps succeeded and the output is correct, respond with exactly: OK
4. If ANY step failed, respond with a corrective request describing ONLY what needs to be fixed. Include the exact error message and the file/command it came from. Be specific.

Respond with either 'OK' or a corrective request. Nothing else."#,
            original = prompt,
            cwd = current_cwd,
            summary = review_summary
        );

        let review = crate::model::set_control_with_persona(&review_prompt, "Quick");
        let verdict = review.trim();

        if verdict.eq_ignore_ascii_case("ok") || verdict.starts_with("OK") {
            notify_complete("Brain Review: No issues found. Stopping retries.");
            break;
        }

        // Something needs fixing — re-plan and re-execute.
        notify_stage("Issues detected. Generating corrective plan…");
        let fix_tasks = decompose(verdict, current_max_steps);
        notify_stage(&format!("Corrective plan: {} step(s) ready", fix_tasks.len()));
        let _ = write_plan(&format!("[RETRY {}] {}", attempt + 1, verdict), &fix_tasks);

        notify_stage("Executing Corrective Steps");
        let (fix_results, fix_cwd) = execute_with_rollbacks(
            prompt,
            fix_tasks,
            persona,
            base_url,
            current_cwd.clone(),
            results.clone(),
            &mut rollbacks_left,
        );

        results = fix_results;
        current_cwd = fix_cwd;

        // Synthesise over the FULL results so the user sees the complete picture.
        notify_stage("Updating Final Response");
        final_response = synthesise(prompt, &results);
    }
    // ── End Review & Retry Loop ──────────────────────────────────────────────

    notify_complete("Request Fully Handled");
    (final_response, plan_path)
}

// ─── Dispatcher ─────────────────────────────────────────────────────────────
// Routes a step to the correct AI function based on its assigned category.
// System prompts of each function are NOT modified.

fn dispatch(category: u64, prompt: &str, persona: &str, base_url: &str, cwd: &str) -> String {
    match category {
        2 => {
            // Analyze Project / Knowledge
            let query_lower = prompt.to_lowercase();
            let force_global = query_lower.contains("search global") || query_lower.contains("search project");

            // Once CWD is specifically established, use list_cwd unless 'search global' is requested
            let (see, ctx_label) = if cwd != "./sandbox" && cwd != "." && !force_global {
                (crate::operations::list_cwd(cwd), "Local Context (Current Dir)")
            } else {
                (crate::operations::unified_search(prompt, base_url), if force_global { "Global Search Context" } else { "Action+Search Context" })
            };
            let ctx = serde_json::to_string(&see).unwrap_or_default();
            let mut p = format!(
                "Help me with this request: '{}'. Use the following context ({}):\n{}\n\n\
                CRITICAL INSTRUCTION:\n\
                1. If you found file paths in the context, you MUST explicitly write out the absolute JSON paths in your response!\n\
                2. IGNORE project-specific build artifacts, internal cache/temp files, or dependency lockfiles.\n\
                3. SPATIAL AWARENESS: The current default directory is {}.\n\
                4. SCOPED READING: If you need to see the full content of specific files to understand where to edit, respond ONLY with a JSON array of strings containing the file paths, e.g., [\"src/main.rs\", \"Cargo.toml\"].",
                prompt, ctx_label, ctx, cwd
            );
            
            let mut ai_response = crate::model::set_control_with_persona(&p, persona);

            // AUTO-FETCH: If the response is a JSON array of file paths, fetch them and re-run once.
            if ai_response.trim().starts_with('[') && ai_response.trim().ends_with(']') {
                if let Ok(paths) = serde_json::from_str::<Vec<String>>(ai_response.trim()) {
                    let contents = crate::operations::read_file_list(paths);
                    let contents_json = serde_json::to_string(&contents).unwrap_or_default();
                    p.push_str(&format!("\n\nSCOPED FILE CONTENTS:\n{}", contents_json));
                    ai_response = crate::model::set_control_with_persona(&p, persona);
                }
            }
            ai_response
        }
        3 => {
            // Execute Tasks
            let tree = crate::operations::list_cwd(cwd);
            let tree_ctx = serde_json::to_string(&tree).unwrap_or_else(|_| "[]".to_string());
            let p = format!(
                "Current Working Directory: {}\n\
                 Contents of this directory:\n{}\n\n\
                 See prior chain context in prompt. Respond with the requested execution logic.", 
                cwd, tree_ctx
            );
            crate::cmd_executor::execute_task(prompt, &p, cwd)
        }
        4 => {
            // Generate Content / Files
            let tree = crate::operations::list_cwd(cwd);
            let tree_ctx = serde_json::to_string(&tree).unwrap_or_else(|_| "[]".to_string());
            let p = format!(
                "You are a file generator. Output ONLY a valid JSON array — no explanation, no markdown fences.\n\n\
                 ENVIRONMENT:\n\
                 - Current Working Directory: {}\n\
                 - Files already in this folder:\n{}\n\n\
                 OBJECT SCHEMA:\n\
                 - \"path\": (required) relative file path.\n\
                 - \"op\": \"overwrite\" (default), \"append\", \"patch\", \"insert_at\".\n\
                 - \"content\": Full text (for overwrite/append/insert_at).\n\
                 - \"search\": Text to find (for \"patch\").\n\
                 - \"replace\": Text to replace with (for \"patch\").\n\
                 - \"line\": Line number (1-indexed, for \"insert_at\").\n\n\
                 RULES:\n\
                 - Use \"patch\" for modifying large existing files to be more reliable.\n\
                 - Do NOT wrap in markdown fences. Do NOT add any text before or after the JSON array.\n\n\
                 Task: {}",
                cwd, tree_ctx, prompt
            );
            let ai_response = crate::model::set_control_with_persona(&p, persona);
            
            // Strip any accidental markdown fences
            let cleaned = ai_response.trim()
                .trim_start_matches("```json")
                .trim_start_matches("```")
                .trim_end_matches("```")
                .trim();

            let base = std::path::Path::new(cwd);
            if !base.exists() {
                let _ = fs::create_dir_all(base).unwrap_or(());
            }
            match crate::hands::write_files_from_json(base, cleaned) {
                Ok(()) => {
                    // Bug Fix: Emit SET_CWD so execute_plan can track the base dir
                    // even after a category-4 (file-write) step, just like category-3 does.
                    let mut canonical = fs::canonicalize(base)
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|_| cwd.to_string());
                    
                    // Normalize: remove UNC prefix if present
                    if canonical.starts_with(r"\\?\") {
                        canonical = canonical[4..].to_string();
                    }
                    format!("✅ Content creation complete for: {}\nSET_CWD: {}", prompt, canonical)
                }
                Err(e) => format!("❌ Content creation failed: {:?}", e),
            }
        }
        5 => {
            // Spotify Integration
            if let Some(saved) = crate::sport::load_tokens() {
                let new_token = crate::sport::refresh_access_token(&saved.refresh_token);
                let refresh = new_token.refresh_token.unwrap_or(saved.refresh_token);
                crate::sport::save_tokens(&new_token.access_token, &refresh);
                crate::sport::process_ai_command(&new_token.access_token, prompt);
                format!("🎵 Spotify command processed: {}", prompt)
            } else {
                "❌ Spotify not authorised.".to_string()
            }
        }
        _ => {
            // General Chat (category 1 or unknown)
            let p = format!("Assist with this request: '{}'. Current base directory context: {}.", prompt, cwd);
            crate::model::set_control_with_persona(&p, persona)
        }
    }
}
