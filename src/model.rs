use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::io;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use colored::Colorize;
use chrono::Local;

// ── Shared HTTP client — built once, reused for every AI call. ───────────────
// Rebuilding reqwest::blocking::Client on every request wastes time on TLS
// stack init and connection-pool allocation. OnceLock gives us a zero-cost
// singleton that is safe to share across the single-threaded blocking calls.
static HTTP_CLIENT: OnceLock<Client> = OnceLock::new();

fn shared_client() -> &'static Client {
    HTTP_CLIENT.get_or_init(|| {
        Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .unwrap_or_else(|_| Client::new())
    })
}

use crate::styles;
use crate::api_keys::*;

const TRACKER_FILE: &str = "tracker.json";

fn persona_prefix(persona: &str) -> &'static str {
    match persona {
        "Helpful" => "You are a detailed, conversational AI collaborator. Explain your reasoning and provide thorough answers. ",
        _ =>         "You are a quick AI assistant. Be concise, direct, and efficient. Avoid unnecessary fluff. ",
    }
}

#[derive(Clone)]
struct ProviderSlot {
    id:          &'static str,
    label:       &'static str,
    limit:       u32,
    key:         &'static str,
    backend:     Backend,
    throttle_ms: Option<u64>,
}

#[derive(Clone)]
enum Backend {
    Gemini(&'static str),
    Groq,
    Cerebras,
    Mistral,
    OpenRouterGpt,
    OpenRouterNemotron,
}

fn provider_table() -> Vec<ProviderSlot> {
    vec![
        ProviderSlot { id: "groq_1",    label: "Groq v1",                 limit: 250, key: GROQ_KEY,         backend: Backend::Groq,                 throttle_ms: None },
        ProviderSlot { id: "groq_2",    label: "Groq v2",                 limit: 250, key: GROQ_KEY2,        backend: Backend::Groq,                 throttle_ms: None },
        ProviderSlot { id: "groq_3",    label: "Groq v3",                 limit: 250, key: GROQ_KEY3,        backend: Backend::Groq,                 throttle_ms: None },
        ProviderSlot { id: "groq_4",    label: "Groq v4",                 limit: 250, key: GROQ_KEY4,        backend: Backend::Groq,                 throttle_ms: None },
        ProviderSlot { id: "or_gpt_1",  label: "OpenAI GPT-OSS v1",      limit: 250, key: OPEN_ROUTER_KEY,  backend: Backend::OpenRouterGpt,        throttle_ms: None },
        ProviderSlot { id: "or_gpt_2",  label: "OpenAI GPT-OSS v2",      limit: 250, key: GPT_120_KEY2,     backend: Backend::OpenRouterGpt,        throttle_ms: None },
        ProviderSlot { id: "or_gpt_3",  label: "OpenAI GPT-OSS v3",      limit: 250, key: GPT_120_KEY3,     backend: Backend::OpenRouterGpt,        throttle_ms: None },
        ProviderSlot { id: "or_gpt_4",  label: "OpenAI GPT-OSS v4",      limit: 250, key: GPT_120_KEY4,     backend: Backend::OpenRouterGpt,        throttle_ms: None },
        ProviderSlot { id: "or_nemo_1", label: "Nvidia Nemotron v1",      limit: 250, key: OPEN_ROUTER_KEY,  backend: Backend::OpenRouterNemotron,   throttle_ms: None },
        ProviderSlot { id: "flash_lite_1", label: "Gemini Flash-Lite v1", limit: 1000, key: GEMINI_KEY,  backend: Backend::Gemini("gemini-2.5-flash-lite"), throttle_ms: None },
        ProviderSlot { id: "flash_lite_2", label: "Gemini Flash-Lite v2", limit: 1000, key: GEMINI_KEY2, backend: Backend::Gemini("gemini-2.5-flash-lite"), throttle_ms: None },
        ProviderSlot { id: "flash_lite_3", label: "Gemini Flash-Lite v3", limit: 1000, key: GEMINI_KEY3, backend: Backend::Gemini("gemini-2.5-flash-lite"), throttle_ms: None },
        ProviderSlot { id: "flash_lite_4", label: "Gemini Flash-Lite v4", limit: 1000, key: GEMINI_KEY4, backend: Backend::Gemini("gemini-2.5-flash-lite"), throttle_ms: None },
        ProviderSlot { id: "flash_1", label: "Gemini Flash v1",           limit: 250, key: GEMINI_KEY,  backend: Backend::Gemini("gemini-2.5-flash"), throttle_ms: None },
        ProviderSlot { id: "flash_2", label: "Gemini Flash v2",           limit: 250, key: GEMINI_KEY2, backend: Backend::Gemini("gemini-2.5-flash"), throttle_ms: None },
        ProviderSlot { id: "flash_3", label: "Gemini Flash v3",           limit: 250, key: GEMINI_KEY3, backend: Backend::Gemini("gemini-2.5-flash"), throttle_ms: None },
        ProviderSlot { id: "flash_4", label: "Gemini Flash v4",           limit: 250, key: GEMINI_KEY4, backend: Backend::Gemini("gemini-2.5-flash"), throttle_ms: None },
        ProviderSlot { id: "pro_1", label: "Gemini Pro v1",               limit: 250, key: GEMINI_KEY,  backend: Backend::Gemini("gemini-2.5-pro"), throttle_ms: None },
        ProviderSlot { id: "pro_2", label: "Gemini Pro v2",               limit: 250, key: GEMINI_KEY2, backend: Backend::Gemini("gemini-2.5-pro"), throttle_ms: None },
        ProviderSlot { id: "pro_3", label: "Gemini Pro v3",               limit: 250, key: GEMINI_KEY3, backend: Backend::Gemini("gemini-2.5-pro"), throttle_ms: None },
        ProviderSlot { id: "pro_4", label: "Gemini Pro v4",               limit: 250, key: GEMINI_KEY4, backend: Backend::Gemini("gemini-2.5-pro"), throttle_ms: None },
        ProviderSlot { id: "cerebras", label: "Cerebras Llama 70B",       limit: 250, key: CEREBRAS_KEY,  backend: Backend::Cerebras, throttle_ms: Some(1100) },
        ProviderSlot { id: "mistral",  label: "Mistral Small",            limit: 250, key: MISTRAL_KEY,   backend: Backend::Mistral,  throttle_ms: Some(1100) },
    ]
}

fn default_mode() -> String { "Brain".to_string() }
pub(crate) fn default_retries() -> u8 { 1 }

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct RequestTracker {
    #[serde(default)]
    pub(crate) usage: HashMap<String, u32>,
    #[serde(default)]
    pub(crate) eyes_calls: u32,
    #[serde(default)]
    pub(crate) last_reset_date: String,
    #[serde(default = "default_mode")]
    pub(crate) task_mode: String,
    #[serde(default)]
    pub(crate) requests_per_prompt: u32,
    #[serde(default)]
    pub(crate) max_steps: u32,
    #[serde(default = "crate::model::default_retries")]
    pub(crate) max_retries: u8,
    #[serde(default)]
    pub(crate) max_rollbacks: u32,
    #[serde(default)]
    pub(crate) validate_mode: bool,
    #[serde(default)]
    pub(crate) key_health: HashMap<String, bool>,
    #[serde(skip)]
    pub(crate) persona: String,
    #[serde(skip)]
    pub(crate) last_call: HashMap<String, Instant>,
}

impl RequestTracker {
    pub fn new() -> Self {
        let mut tracker = Self::load();
        tracker.check_reset();
        tracker.persona = "Quick".to_string();
        tracker
    }

    fn load() -> Self {
        if let Ok(data) = fs::read_to_string(TRACKER_FILE) {
            if let Ok(tracker) = serde_json::from_str(&data) {
                return tracker;
            }
        }
        Self::default_state()
    }

    fn default_state() -> Self {
        RequestTracker {
            usage: HashMap::new(),
            eyes_calls: 0,
            last_reset_date: Local::now().format("%Y-%m-%d").to_string(),
            task_mode: "Brain".to_string(),
            requests_per_prompt: 0,
            max_steps: 0,
            max_retries: 1,
            max_rollbacks: 0,
            validate_mode: false,
            key_health: HashMap::new(),
            persona: "Quick".to_string(),
            last_call: HashMap::new(),
        }
    }

    pub fn save(&self) {
        if let Ok(data) = serde_json::to_string(self) {
            let _ = fs::write(TRACKER_FILE, data);
        }
    }

    fn check_reset(&mut self) {
        let today = Local::now().format("%Y-%m-%d").to_string();
        if self.last_reset_date != today {
            eprintln!("🕒 A new day! Resetting API usage limits...");
            self.usage.clear();
            self.eyes_calls = 0;
            self.last_reset_date = today;
            self.save();
        }
    }

    fn slot_usage(&self, slot: &ProviderSlot) -> u32 {
        *self.usage.get(slot.id).unwrap_or(&0)
    }

    fn can_use(&self, slot: &ProviderSlot) -> bool {
        self.slot_usage(slot) < slot.limit
    }

    pub fn can_use_eyes(&self) -> bool { self.eyes_calls < 50 }

    pub fn is_healthy(&self, key_id: &str) -> bool {
        if !self.validate_mode { return true; }
        *self.key_health.get(key_id).unwrap_or(&true)
    }

    pub fn check_key_health(&mut self, client: &Client) {
        eprintln!("🩺 {} Keys...", "Validating".bold().cyan());

        let keys_to_test: Vec<(&str, &str, Backend)> = vec![
            ("GEMINI_KEY",      GEMINI_KEY,      Backend::Gemini("gemini-2.5-flash-lite")),
            ("GEMINI_KEY2",     GEMINI_KEY2,     Backend::Gemini("gemini-2.5-flash-lite")),
            ("GEMINI_KEY3",     GEMINI_KEY3,     Backend::Gemini("gemini-2.5-flash-lite")),
            ("GEMINI_KEY4",     GEMINI_KEY4,     Backend::Gemini("gemini-2.5-flash-lite")),
            ("GROQ_KEY",        GROQ_KEY,        Backend::Groq),
            ("GROQ_KEY2",       GROQ_KEY2,       Backend::Groq),
            ("GROQ_KEY3",       GROQ_KEY3,       Backend::Groq),
            ("GROQ_KEY4",       GROQ_KEY4,       Backend::Groq),
            ("OPEN_ROUTER_KEY", OPEN_ROUTER_KEY, Backend::OpenRouterGpt),
            ("GPT_120_KEY2",    GPT_120_KEY2,    Backend::OpenRouterGpt),
            ("GPT_120_KEY3",    GPT_120_KEY3,    Backend::OpenRouterGpt),
            ("GPT_120_KEY4",    GPT_120_KEY4,    Backend::OpenRouterGpt),
        ];

        for (name, key, backend) in keys_to_test {
            if key.is_empty() {
                self.key_health.insert(name.to_string(), false);
                eprintln!("  ⚠️  {:<16} : EMPTY", name);
                continue;
            }

            let ok = dispatch_call(client, "ping", key, &backend).is_ok();
            eprintln!("  {} {:<16} : {}", if ok { "✅" } else { "❌" }, name, if ok { "OK" } else { "FAILED" });
            self.key_health.insert(name.to_string(), ok);
        }

        self.save();
        eprintln!("🩺 Health check complete.\n");
    }
}

pub fn call_gemini(client: &Client, prompt: &str, model: &str, key: &str) -> Result<String, String> {
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
        model
    );

    let body = json!({
        "contents": [{ "parts": [{ "text": prompt }] }]
    });

    let response = client
        .post(&url)
        .header("x-goog-api-key", key)
        .json(&body)
        .send()
        .map_err(|e| format!("Request failed: {}", e))?;

    let result: serde_json::Value = response
        .json()
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    if let Some(error) = result.get("error") {
        return Err(format!("API Error: {}", error));
    }

    Ok(result["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .unwrap_or("No response")
        .to_string())
}

pub fn call_cerebras(client: &Client, prompt: &str) -> Result<String, String> {
    let body = json!({
        "model": "gpt-oss-120b",
        "messages": [{ "role": "user", "content": prompt }]
    });

    let response = client
        .post("https://api.cerebras.ai/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", CEREBRAS_KEY))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?;

    let result: serde_json::Value = response.json().map_err(|e| e.to_string())?;
    if let Some(error) = result.get("error") {
        return Err(format!("Cerebras error: {}", error["message"]));
    }

    Ok(result["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("No response")
        .to_string())
}

pub fn call_groq(client: &Client, prompt: &str, key: &str) -> Result<String, String> {
    let body = json!({
        "model": "openai/gpt-oss-120b",
        "messages": [{ "role": "user", "content": prompt }]
    });

    let response = client
        .post("https://api.groq.com/openai/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| format!("Request failed: {}", e))?;

    let status = response.status();
    let text = response.text().map_err(|e| format!("Failed to read response: {}", e))?;
    if !status.is_success() {
        return Err(format!("Groq API error {}: {}", status, text));
    }

    let result: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Failed to parse JSON: {}", e))?;

    result["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| format!("Unexpected response: {}", result))
}

pub fn call_mistral(client: &Client, prompt: &str) -> Result<String, String> {
    let body = json!({
        "model": "mistral-7b-instruct-v0.1.Q4_0.gguf",
        "messages": [{ "role": "user", "content": prompt }]
    });

    let response = client
        .post("https://api.mistral.ai/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", MISTRAL_KEY))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?;

    let result: serde_json::Value = response.json().map_err(|e| e.to_string())?;
    if let Some(error) = result.get("error") {
        return Err(format!("Mistral error: {}", error["message"]));
    }

    Ok(result["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("No response")
        .to_string())
}

pub fn call_openrouter(client: &Client, prompt: &str, key: &str) -> Result<String, String> {
    let body = json!({
        "model": "nvidia/nemotron-3-super-120b-a12b:free",
        "messages": [{ "role": "user", "content": prompt }]
    });

    let response = client
        .post("https://openrouter.ai/api/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?;

    let result: serde_json::Value = response.json().map_err(|e| e.to_string())?;
    if let Some(error) = result.get("error") {
        return Err(format!("OpenRouter error: {}", error["message"]));
    }

    Ok(result["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("No response")
        .to_string())
}

pub fn call_openrouter_gpt(client: &Client, prompt: &str, key: &str) -> Result<String, String> {
    let body = json!({
        "model": "openai/gpt-oss-120b:free",
        "messages": [{ "role": "user", "content": prompt }]
    });

    let response = client
        .post("https://openrouter.ai/api/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?;

    let result: serde_json::Value = response.json().map_err(|e| e.to_string())?;
    if let Some(error) = result.get("error") {
        return Err(format!("OpenRouter ChatGPT API error: {}", error["message"]));
    }

    Ok(result["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("No response")
        .to_string())
}

fn dispatch_call(client: &Client, prompt: &str, key: &str, backend: &Backend) -> Result<String, String> {
    match backend {
        Backend::Gemini(model)       => call_gemini(client, prompt, model, key),
        Backend::Groq                => call_groq(client, prompt, key),
        Backend::Cerebras            => call_cerebras(client, prompt),
        Backend::Mistral             => call_mistral(client, prompt),
        Backend::OpenRouterGpt       => call_openrouter_gpt(client, prompt, key),
        Backend::OpenRouterNemotron  => call_openrouter(client, prompt, key),
    }
}

fn throttle_if_needed(last_call: &Option<&Instant>, min_gap: Duration, provider: &str) {
    if let Some(last) = last_call {
        let elapsed = last.elapsed();
        if elapsed < min_gap {
            let wait = min_gap - elapsed;
            eprintln!("⏳ Rate limiting {}: waiting {}ms...", provider, wait.as_millis());
            std::thread::sleep(wait);
        }
    }
}

pub fn smart_prompt(
    client: &Client,
    tracker: &mut RequestTracker,
    prompt: &str,
    inject_persona: bool,
) -> String {
    let enriched_prompt = if inject_persona {
        format!("{}{}", persona_prefix(&tracker.persona), prompt)
    } else {
        prompt.to_string()
    };

    tracker.requests_per_prompt += 1;

    let table = provider_table();

    for slot in &table {
        if !tracker.can_use(slot) { continue; }
        if !tracker.is_healthy(slot.id) { continue; }
        if slot.key.is_empty() { continue; }

        if let Some(gap_ms) = slot.throttle_ms {
            let last = tracker.last_call.get(slot.id);
            throttle_if_needed(&last, Duration::from_millis(gap_ms), slot.label);
        }

        let current = tracker.slot_usage(slot);
        eprintln!("📡 Using: {} ({}/{})", slot.label, current + 1, slot.limit);

        match dispatch_call(client, &enriched_prompt, slot.key, &slot.backend) {
            Ok(response) => {
                *tracker.usage.entry(slot.id.to_string()).or_insert(0) += 1;
                if slot.throttle_ms.is_some() {
                    tracker.last_call.insert(slot.id.to_string(), Instant::now());
                }
                tracker.save();
                return response;
            }
            Err(e) => {
                if e.contains("403") || e.contains("401") {
                    tracker.key_health.insert(slot.id.to_string(), false);
                }
                if slot.throttle_ms.is_some() {
                    tracker.last_call.insert(slot.id.to_string(), Instant::now());
                }
                eprintln!("⚠️  {} failed: {} — trying next...", slot.label, e);
            }
        }
    }

    "❌ All providers exhausted for today. Try again tomorrow!".to_string()
}

pub fn build_client(timeout_secs: u64) -> Client {
    Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .unwrap_or_else(|_| Client::new())
}

pub fn set_control(prompt: &str) -> String {
    set_control_with_persona(prompt, "Quick")
}

pub fn set_control_with_persona(prompt: &str, persona: &str) -> String {
    let client = shared_client(); // reuse the cached client — no rebuild
    let mut tracker = RequestTracker::new();
    tracker.persona = persona.to_string();
    smart_prompt(client, &mut tracker, prompt, true)
}

pub fn control(persona_name: &str) {
    let client = build_client(60);
    let mut tracker = RequestTracker::new();
    tracker.persona = persona_name.to_string();

    println!("🤖 AI Router Ready! Type your prompt (or 'quit' to exit)");
    println!("💾 To save AI output to a file: enigma <filename> <prompt>\n");

    let table = provider_table();

    loop {
        println!("You: ");
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        let input = input.trim();

        if input == "quit" || input == "exit" {
            println!("Goodbye!");
            break;
        }
        if input.is_empty() { continue; }

        let response = smart_prompt(&client, &mut tracker, input, false);
        println!("\n{}", "AI:".green().bold());
        styles::print_styled(&response);
        println!();

        println!("📊 Remaining today:");
        for slot in &table {
            let used  = tracker.slot_usage(slot);
            let remaining = slot.limit.saturating_sub(used);
            eprint!("  {}: {}/{}", slot.label, remaining, slot.limit);
        }
        eprintln!("  Eyes: {}/50", 50u32.saturating_sub(tracker.eyes_calls));
        println!();

        if tracker.validate_mode {
            let broken = tracker.key_health.values().filter(|&&h| !h).count();
            if broken > 0 {
                println!("{}", format!("⚠️ {} API key(s) are currently marked FAILED.", broken).yellow());
            } else {
                println!("{}", "✅ All tested API keys are healthy.".green());
            }
        }
        println!();
    }
}