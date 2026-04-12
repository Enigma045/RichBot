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

const PLANS_DIR: &str = "plans";
const PLAN_FILE: &str = "plans/plan.txt";

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

/// Calls the AI once to break the user's request into steps.
/// Each step carries the category (AI function) and estimated_calls.
pub fn decompose(prompt: &str) -> Vec<SubTask> {
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
7. Limit to MAX 8 steps. Simple tasks = 1 step.
8. Return ONLY a valid JSON object, nothing else:
{{
  "tasks": [
    {{ "step": 1, "intent": "Short label", "prompt": "Focused sub-prompt here", "category": 2, "estimated_calls": 2 }},
    ...
  ]
}}

User Request: "{}"
"#,
        prompt
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

/// Writes a human-readable plan to plans/plan.txt and returns the path.
pub fn write_plan(original_prompt: &str, tasks: &[SubTask]) -> String {
    // Ensure the plans/ directory exists
    if let Err(e) = fs::create_dir_all(PLANS_DIR) {
        eprintln!("⚠️  Brain: Could not create plans/ dir: {}", e);
    }

    let total_calls: u32 = tasks.iter().map(|t| t.estimated_calls).sum();

    let mut lines = vec![
        "═══════════════════════════════════════════════════".to_string(),
        " 🧠 ENIGMA BRAIN — EXECUTION PLAN".to_string(),
        "═══════════════════════════════════════════════════".to_string(),
        format!(" Request : {}", original_prompt),
        format!(" Steps   : {}", tasks.len()),
        format!(" Est. AI calls: {}", total_calls),
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

    match fs::write(PLAN_FILE, &content) {
        Ok(()) => eprintln!("📋 Brain: Plan written to {}", PLAN_FILE),
        Err(e) => eprintln!("⚠️  Brain: Could not write plan.txt: {}", e),
    }

    PLAN_FILE.to_string()
}

/// Reads plan.txt and returns its content (used by --send-plan CLI flag).
pub fn read_plan() -> String {
    match fs::read_to_string(PLAN_FILE) {
        Ok(content) => content,
        Err(_) => "No plan found. Run a Brain prompt first.".to_string(),
    }
}

// ─── Step 2: Execute each sub-task with context chaining ────────────────────

/// Runs every sub-task through the appropriate AI function.
/// Each step receives the accumulated outputs of all previous steps as context.
pub fn execute_plan(
    tasks: &[SubTask],
    persona: &str,
    base_url: &str,
) -> Vec<(SubTask, String)> {
    let mut results: Vec<(SubTask, String)> = Vec::new();

    for sub in tasks {
        eprintln!(
            "\n🧠 Brain Step {}/{}: [{}] → {} — {}",
            sub.step,
            tasks.len(),
            sub.intent,
            category_label(sub.category),
            sub.prompt
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
        let output = dispatch(sub.category, &enriched_prompt, persona, base_url);
        results.push((sub.clone(), output));
    }

    results
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

    // Step 1: Decompose into steps with categories
    let tasks = decompose(prompt);
    eprintln!("🧠 Plan ready: {} step(s)", tasks.len());

    // Step 2: Write plan.txt NOW — before any execution
    let plan_path = write_plan(prompt, &tasks);

    // Step 3: Execute each step with context chaining
    let results = execute_plan(&tasks, persona, base_url);

    // Step 4: Synthesise final answer
    let final_response = synthesise(prompt, &results);

    (final_response, plan_path)
}

// ─── Dispatcher ─────────────────────────────────────────────────────────────
// Routes a step to the correct AI function based on its assigned category.
// System prompts of each function are NOT modified.

fn dispatch(category: u64, prompt: &str, persona: &str, base_url: &str) -> String {
    match category {
        2 => {
            // Analyze Project / Knowledge
            let see = crate::operations::unified_search(prompt, base_url);
            let ctx = serde_json::to_string(&see).unwrap_or_default();
            let p = format!(
                "Help me with this request: '{}'. Use the following context if helpful:\n{}\n\nCRITICAL INSTRUCTION: If you found file paths in the context, you MUST explicitly write out the absolute JSON paths in your response so the next AI step knows exactly where the file is located! Do not guess paths.",
                prompt, ctx
            );
            crate::model::set_control_with_persona(&p, persona)
        }
        3 => {
            // Execute Tasks
            // Context is already injected via execute_plan prior chain context.
            let p = "See prior chain context in prompt.".to_string();
            crate::cmd_executor::execute_task(prompt, &p);
            format!("✅ System command dispatched for: {}", prompt)
        }
        4 => {
            // Generate Content / Files
            let p = format!(
                "Return ONLY a valid JSON array of objects with 'path' and 'content'. Task: {}.",
                prompt
            );
            let resp = crate::model::set_control_with_persona(&p, persona);
            let base_str = "./sandbox";
            let base = std::path::Path::new(base_str);
            if !base.exists() {
                let _ = fs::create_dir_all(base).unwrap_or(());
            }
            match crate::hands::write_files_from_json(base, &resp) {
                Ok(()) => format!("✅ Content creation complete for: {}", prompt),
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
            let p = format!("Assist with this request: '{}'.", prompt);
            crate::model::set_control_with_persona(&p, persona)
        }
    }
}
