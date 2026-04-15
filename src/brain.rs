// ═══════════════════════════════════════════════════════════════════════════
// brain.rs — Enigma Mandatory Orchestrator
//
// EVERY prompt runs through Brain first.
// Brain decomposes the request, assigns an AI function category per step,
// chains prior-step context into each subsequent step, writes a plan to
// plans/plan.txt, and synthesises a final response.
// ═══════════════════════════════════════════════════════════════════════════

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

/// Reads max review retries from tracker.json.
/// Falls back to 1 if not set (tracker.max_retries will be added when needed).
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

// ─── SubTask ────────────────────────────────────────────────────────────────

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
/// `max_steps` caps the number of steps the AI may plan (read from tracker).
pub fn decompose(prompt: &str) -> Vec<SubTask> {
    let max_steps = load_max_steps();
    let system = format!(
        r#"You are 'Enigma Brain', an expert AI task planner and router.
Your job: decompose the user's request into a series of clear, focused sub-tasks.

AVAILABLE AI FUNCTIONS (categories):
1 = General Chat      — greetings, simple Q&A, jokes, quick questions
2 = Analyze/Research  — finding local files, finding movies/shows, finding information, gathering context
3 = Execute Tasks     — running commands, opening files, system automation
4 = Create Content    — writing files, generating code, building scripts. NOTE: Use absolute paths if specified by the user, otherwise default to relative paths.
5 = Spotify           — controlling Spotify playback only

RULES:
1. Each sub-task must have ONE specific goal.
2. Assign the best-fit category (1-5) to each step.
3. Set estimated_calls to the number of AI calls you expect (1-3 typical).
4. Keep "prompt" concise and self-contained. Include filenames/keywords from the original.
5. Order steps logically: gather context → act → confirm/summarise.
6. CRITICAL: For ANY task that requires reading, modifying, opening, executing, or interacting with specific local files, you MUST create a Category 2 step FIRST to search for the exact file path, followed by the execution/content step.
7. Limit to MAX {} steps. Simple tasks = 1 step.
8. SPATIAL AWARENESS: If a step involves creating a project or folder (e.g., 'cargo new'), all subsequent steps for that project MUST use the correct subdirectory in their prompt or set the context accordingly.
9. Return ONLY a valid JSON object, nothing else:
{{
  "tasks": [
    {{ "step": 1, "intent": "Short label", "prompt": "Focused sub-prompt here", "category": 2, "estimated_calls": 2 }},
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
/// Each step receives the accumulated outputs of all previous steps as context.
pub fn execute_plan(
    tasks: &[SubTask],
    persona: &str,
    base_url: &str,
    initial_cwd: &str,
) -> (Vec<(SubTask, String)>, String) {
    let mut results: Vec<(SubTask, String)> = Vec::new();
    let mut current_cwd = initial_cwd.to_string();

    for sub in tasks {
        notify_step(sub.step, tasks.len(), &sub.intent);
        eprintln!(
            "   [{}] → {} — {} (CWD: {})",
            category_label(sub.category),
            sub.prompt,
            current_cwd,
            current_cwd
        );

        // Build prior-step context string
        let prior_ctx = if results.is_empty() {
            String::new()
        } else {
            let parts: Vec<String> = results
                .iter()
                .map(|(s, out)| {
                    format!(
                        "Step {} [{}] result:\n{}",
                        s.step, s.intent, out
                    )
                })
                .collect();
            format!("\n\n[Prior step context — use this to inform your response]:\n{}", parts.join("\n---\n"))
        };

        let enriched_prompt = format!("{}{}", sub.prompt, prior_ctx);
        let output = dispatch(sub.category, &enriched_prompt, persona, base_url, &current_cwd);
        
        // Automatic CD tracking: Check if the executor returned a SET_CWD sentinel
        if let Some(pos) = output.find("SET_CWD: ") {
            let rest = &output[pos + 9..];
            if let Some(line_end) = rest.find('\n') {
                let new_cwd = rest[..line_end].trim().to_string();
                if !new_cwd.is_empty() {
                    eprintln!("🧠 Brain: Base directory updated to {}", new_cwd);
                    current_cwd = new_cwd;
                }
            }
        }

        results.push((sub.clone(), output));
        notify_complete(&format!("Step {} finished.", sub.step));
    }

    (results, current_cwd)
}

// ─── Step 3: Synthesise ─────────────────────────────────────────────────────

/// Merges all step outputs into one final, coherent answer.
pub fn synthesise(original_prompt: &str, results: &[(SubTask, String)]) -> String {
    if results.len() == 1 {
        return results[0].1.clone();
    }

    let steps_summary: String = results
        .iter()
        .map(|(sub, out)| {
            format!(
                "Step {}: [{}] ({})\nPrompt: {}\nResult:\n{}\n",
                sub.step,
                sub.intent,
                category_label(sub.category),
                sub.prompt,
                out
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

/// Entry point: decompose → write plan → execute → synthesise.
/// Returns (final_response, plan_file_path).
pub fn run(prompt: &str, persona: &str, base_url: &str) -> (String, String) {
    eprintln!("🧠 Enigma Brain: Analysing request…");

    // Read runtime settings from tracker.json
    let max_retries = load_max_retries();
    let current_max_steps = load_max_steps();
    eprintln!("🔢 Brain config: max_steps={}, max_retries={}", current_max_steps, max_retries);

    // Step 1: Decompose into steps with categories
    let tasks = decompose(prompt);
    eprintln!("🧠 Plan ready: {} step(s)", tasks.len());

    // Step 2: Write plan.txt NOW — before any execution
    let plan_path = write_plan(prompt, &tasks);

    // Step 3: Execute each step with context chaining
    notify_stage("Starting Core Execution");
    let (results, final_cwd) = execute_plan(&tasks, persona, base_url, "./sandbox");

    // Step 4: Synthesise final answer
    notify_stage("Synthesising Final Response");
    let final_response = synthesise(prompt, &results);

    // ── Step 5: Review & Retry Loop ──────────────────────────────────────────
    // After each Brain run, the AI reviews its own output.
    // If it detects failures it generates a corrective plan and re-executes.
    // Controlled by load_max_retries() which reads from tracker.json.
    let mut final_response = final_response;
    let mut results = results;
    let mut current_cwd = final_cwd;

    for attempt in 0..max_retries {
        notify_stage(&format!("Reviewing Results (Attempt {}/{})", attempt + 1, max_retries));

        // Bug Fix: Validate the current CWD is still reachable before each review round.
        // If it was lost (e.g., only category-4 steps ran and SET_CWD was not re-emitted)
        // fall back to the known last SET_CWD embedded in prior outputs.
        if !std::path::Path::new(&current_cwd).exists() {
            // Scan all results for the most recent SET_CWD sentinel
            for (_, out) in results.iter().rev() {
                if let Some(pos) = out.find("SET_CWD: ") {
                    let rest = &out[pos + 9..];
                    let end = rest.find('\n').unwrap_or(rest.len());
                    let recovered = rest[..end].trim().to_string();
                    if std::path::Path::new(&recovered).exists() {
                        eprintln!("🧠 Brain: Recovered CWD from results history: {}", recovered);
                        current_cwd = recovered;
                        break;
                    }
                }
            }
        }

        let review_summary: String = results
            .iter()
            .map(|(s, out)| format!("Step {} [{}]: {}\nOutput: {}", s.step, s.intent, s.prompt, out))
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

        // Something needs fixing — re-plan and re-execute
        notify_stage("Issues detected. Generating corrective plan…");
        let fix_tasks = decompose(verdict);
        notify_stage(&format!("Corrective plan: {} step(s) ready", fix_tasks.len()));
        let _ = write_plan(&format!("[RETRY {}] {}", attempt + 1, verdict), &fix_tasks);

        notify_stage("Executing Corrective Steps");
        let (fix_results, fix_cwd) = execute_plan(&fix_tasks, persona, base_url, &current_cwd);
        
        notify_stage("Updating Final Response");
        let fix_response = synthesise(prompt, &fix_results);

        // Merge fix results into the running results for the next review
        results.extend(fix_results);
        final_response = fix_response;
        current_cwd = fix_cwd;
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
            let see = crate::operations::unified_search(prompt, base_url);
            let ctx = serde_json::to_string(&see).unwrap_or_default();
            let p = format!(
                "Help me with this request: '{}'. Use the following context if helpful:\n{}\n\nCRITICAL INSTRUCTION:\n1. If you found file paths in the context, you MUST explicitly write out the absolute JSON paths in your response!\n2. IGNORE project-specific build artifacts (target/, dist/, etc.), internal cache/temp files, or dependency lockfiles unless the task is specifically about troubleshooting them.\n3. SPATIAL AWARENESS: The current default directory is {}. Do not guess paths.",
                prompt, ctx, cwd
            );
            crate::model::set_control_with_persona(&p, persona)
        }
        3 => {
            // Execute Tasks
            // Context is already injected via execute_plan prior chain context.
            let p = format!("Current Working Directory is: {}. See prior chain context in prompt.", cwd);
            crate::cmd_executor::execute_task(prompt, &p, cwd)
        }
        4 => {
            // Generate Content / Files
            let p = format!(
                "You are a file generator. Output ONLY a valid JSON array — no explanation, no markdown fences.\n\
                 Each element must be an object with exactly two keys: \"path\" (relative file path) and \"content\" (full file text).\n\
                 Example output:\n\
                 [{{\"path\": \"src/main.rs\", \"content\": \"fn main() {{\\n    println!(\\\"Hello\\\");\\n}}\"}}]\n\n\
                 RULES:\n\
                 - Current base directory: {}. Use paths relative to this (e.g., 'src/main.rs', NOT 'file_transfer/src/main.rs').\n\
                 - Do NOT wrap in markdown fences. Do NOT add any text before or after the JSON array.\n\n\
                 Task: {}",
                cwd, prompt
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
                    let canonical = fs::canonicalize(base)
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|_| cwd.to_string());
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
