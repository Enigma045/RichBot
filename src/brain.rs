// ═══════════════════════════════════════════════════════════════════════════
// brain.rs — Enigma Task Decomposer & Orchestrator
//
// Takes a high-level user prompt, breaks it into focused sub-tasks, executes
// each one through the AI router, and synthesises a final response.
// ═══════════════════════════════════════════════════════════════════════════

use serde::{Deserialize, Serialize};
use serde_json::json;

/// A single focused step produced by the decomposer.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SubTask {
    pub step: u32,
    pub intent: String,   // Short label  e.g. "Create file", "Search docs"
    pub prompt: String,   // The focused, trimmed prompt for this step
}

/// The full plan returned by the AI decomposer.
#[derive(Serialize, Deserialize, Debug)]
struct TaskPlan {
    tasks: Vec<SubTask>,
}

// ─── Step 1: Decompose ───────────────────────────────────────────────────────

/// Calls the AI once with a meta-prompt to break the user's request into steps.
/// Returns a list of `SubTask` items ordered by execution.
pub fn decompose(prompt: &str) -> Vec<SubTask> {
    let system = format!(
        r#"You are 'Enigma Brain', an expert task planner.
Your job: decompose the user's request into a series of clear, focused sub-tasks.

RULES:
1. Each sub-task must have a SINGLE, specific goal — never combine two actions.
2. Keep the "prompt" field concise and self-contained (include any filenames/keywords from the original).
3. Order steps logically (e.g. gather info → execute → confirm).
4. Limit to a MAX of 8 steps. If the task is simple (1 step), return only 1.
5. Return ONLY a valid JSON object in this exact format and nothing else:
{{
  "tasks": [
    {{ "step": 1, "intent": "Short label", "prompt": "Focused sub-prompt here" }},
    ...
  ]
}}

User Request: "{}"
"#,
        prompt
    );

    let raw = crate::model::set_control_with_persona(&system, "Quick");
    let clean = raw.trim().trim_matches('`').trim_start_matches("json").trim();

    match serde_json::from_str::<TaskPlan>(clean) {
        Ok(plan) if !plan.tasks.is_empty() => plan.tasks,
        _ => {
            eprintln!("⚠️  Brain: Decomposition failed — falling back to single step.");
            vec![SubTask {
                step: 1,
                intent: "Direct Execution".to_string(),
                prompt: prompt.to_string(),
            }]
        }
    }
}

// ─── Step 2: Execute each sub-task ──────────────────────────────────────────

/// Runs every sub-task through the auto_router and collects each result.
/// Returns a `Vec<(SubTask, String)>` pairing each step with its output.
pub fn execute_plan(
    tasks: &[SubTask],
    persona: &str,
    base_url: &str,
) -> Vec<(SubTask, String)> {
    let mut results: Vec<(SubTask, String)> = Vec::new();

    for sub in tasks {
        eprintln!(
            "\n🧠 Brain Step {}/{}: [{}] — {}",
            sub.step,
            tasks.len(),
            sub.intent,
            sub.prompt
        );

        // Capture the router's stdout by calling route_to_string
        let output = route_to_string(&sub.prompt, persona, base_url);
        results.push((sub.clone(), output));
    }

    results
}

// ─── Step 3: Synthesise ─────────────────────────────────────────────────────

/// Merges all step outputs into one final, coherent answer.
pub fn synthesise(original_prompt: &str, results: &[(SubTask, String)]) -> String {
    if results.len() == 1 {
        // No synthesis needed — just return the single result
        return results[0].1.clone();
    }

    let steps_summary: String = results
        .iter()
        .map(|(sub, out)| {
            format!(
                "Step {}: [{}]\nPrompt: {}\nResult:\n{}\n",
                sub.step, sub.intent, sub.prompt, out
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

/// Entry point: decompose → execute → synthesise. Returns the final response string.
pub fn run(prompt: &str, persona: &str, base_url: &str) -> String {
    eprintln!("🧠 Enigma Brain: Analysing request…");
    let tasks = decompose(prompt);
    eprintln!("🧠 Plan ready: {} step(s)", tasks.len());

    let results = execute_plan(&tasks, persona, base_url);
    let final_response = synthesise(prompt, &results);
    final_response
}

// ─── Internal Router Bridge ──────────────────────────────────────────────────
// Calls auto_router but captures its *stdout* so we can chain results.
// It works by using a shared in-memory string — we reuse the task functions
// directly here rather than shelling out, to stay within the same process.

fn route_to_string(prompt: &str, persona: &str, base_url: &str) -> String {
    use std::io::Write;

    // We call the same underlying task functions as auto_router but collect
    // their output instead of printing it.

    let router_prompt = format!(
        r#"Categorise this into 1 number only (no explanation):
1=Chat  2=Research  3=System/FileCommand  4=CreateContent  5=Spotify
Request: "{}""#,
        prompt
    );

    let res = crate::model::set_control_with_persona(&router_prompt, "Quick");
    let category: u64 = res.chars()
        .find(|c| c.is_ascii_digit())
        .and_then(|c| c.to_digit(10))
        .map(|d| d as u64)
        .unwrap_or(1);

    match category {
        2 => {
            // Research / Analysis
            let see = crate::operations::unified_search(prompt, base_url);
            let ctx = serde_json::to_string(&see).unwrap_or_default();
            let p = format!(
                "Help me with this request: '{}'. Use the following context if helpful:\n{}",
                prompt, ctx
            );
            crate::model::set_control_with_persona(&p, persona)
        }
        3 => {
            // System command execution  — capture via cmd_executor
            let see = crate::operations::unified_search(prompt, base_url);
            let ctx = serde_json::to_string(&see).unwrap_or_default();
            // execute_task prints to stdout; we call it and note that output
            // will appear live. Return a simple confirmation string.
            crate::cmd_executor::execute_task(prompt, &ctx);
            format!("System command dispatched for: {}", prompt)
        }
        4 => {
            // Content creation
            let see = crate::operations::unified_search(prompt, base_url);
            let ctx = serde_json::to_string(&see).unwrap_or_default();
            let p = format!(
                "Return ONLY a valid JSON array of objects with 'path' and 'content'. Task: {}. Context: {}",
                prompt, ctx
            );
            let resp = crate::model::set_control_with_persona(&p, persona);
            let base = std::path::Path::new(".");
            match crate::hands::write_files_from_json(base, &resp) {
                Ok(()) => format!("Content creation complete for: {}", prompt),
                Err(e) => format!("Content creation failed: {:?}", e),
            }
        }
        5 => {
            // Spotify
            if let Some(saved) = crate::sport::load_tokens() {
                let new_token = crate::sport::refresh_access_token(&saved.refresh_token);
                let refresh = new_token.refresh_token.unwrap_or(saved.refresh_token);
                crate::sport::save_tokens(&new_token.access_token, &refresh);
                // process_ai_command prints directly; return a note
                crate::sport::process_ai_command(&new_token.access_token, prompt);
                format!("Spotify command processed: {}", prompt)
            } else {
                "❌ Spotify not authorised.".to_string()
            }
        }
        _ => {
            // Chat / General
            let see = crate::operations::unified_search(prompt, base_url);
            let ctx = serde_json::to_string(&see).unwrap_or_default();
            let p = format!(
                "Assist with this request: '{}'. Use this context if relevant: {}",
                prompt, ctx
            );
            crate::model::set_control_with_persona(&p, persona)
        }
    }
}
