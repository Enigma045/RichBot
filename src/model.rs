use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::io;
use std::time::{Duration, Instant};
use colored::Colorize;
use chrono::Local;

use crate::styles;

const TRACKER_FILE: &str = "tracker.json";

use crate::api_keys::*;

fn default_mode() -> String {
    "Brain".to_string()
}

pub(crate) fn default_retries() -> u8 { 1 }

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct RequestTracker {
    #[serde(default)]
    pub(crate) gemini_flash: u32,
    #[serde(default)]
    pub(crate) gemini_flash_2: u32,
    #[serde(default)]
    pub(crate) gemini_flash_3: u32,
    #[serde(default)]
    pub(crate) gemini_flash_4: u32,

    #[serde(default)]
    pub(crate) gemini_flash_lite: u32,
    #[serde(default)]
    pub(crate) gemini_flash_lite_2: u32,
    #[serde(default)]
    pub(crate) gemini_flash_lite_3: u32,
    #[serde(default)]
    pub(crate) gemini_flash_lite_4: u32,

    #[serde(default)]
    pub(crate) gemini_pro: u32,
    #[serde(default)]
    pub(crate) gemini_pro_2: u32,
    #[serde(default)]
    pub(crate) gemini_pro_3: u32,
    #[serde(default)]
    pub(crate) gemini_pro_4: u32,

    #[serde(default)]
    pub(crate) groq: u32,
    #[serde(default)]
    pub(crate) groq_2: u32,
    #[serde(default)]
    pub(crate) groq_4: u32,

    #[serde(default)]
    pub(crate) cerebras: u32,
    #[serde(default)]
    pub(crate) mistral: u32,
    #[serde(default)]
    pub(crate) openrouter: u32,

    #[serde(default)]
    pub(crate) openrouter_gpt: u32,
    #[serde(default)]
    pub(crate) openrouter_gpt_2: u32,
    #[serde(default)]
    pub(crate) openrouter_gpt_4: u32,

    #[serde(default)]
    pub(crate) eyes_calls: u32,
    #[serde(default)]
    pub(crate) last_reset_date: String,
    #[serde(default)]
    pub(crate) task_mode: String,
    #[serde(default)]
    pub(crate) requests_per_prompt: u32,
    /// Max decomposition steps Brain is allowed to plan (keyword: "steps N").
    /// 0 means use the built-in default (16).
    #[serde(default)]
    pub(crate) max_steps: u32,
    /// Max retry attempts by Brain reviewer (keyword: "retries N").
    #[serde(default = "crate::model::default_retries")]
    pub(crate) max_retries: u8,
    #[serde(skip)]
    pub(crate) persona: String,
    /// In-process timestamp of last Cerebras call (not persisted)
    #[serde(skip)]
    pub(crate) last_cerebras_call: Option<Instant>,
    /// In-process timestamp of last Mistral call (not persisted)
    #[serde(skip)]
    pub(crate) last_mistral_call: Option<Instant>,
}

impl RequestTracker {
    pub fn new() -> Self {
        let mut tracker = Self::load();
        tracker.check_reset();
        tracker.persona = "Quick".to_string(); // Always default to Quick on launch
        tracker
    }

    fn load() -> Self {
        if let Ok(data) = fs::read_to_string(TRACKER_FILE) {
            if let Ok(tracker) = serde_json::from_str(&data) {
                return tracker;
            }
        }
        RequestTracker {
            gemini_flash: 0, gemini_flash_2: 0, gemini_flash_3: 0, gemini_flash_4: 0,
            gemini_flash_lite: 0, gemini_flash_lite_2: 0, gemini_flash_lite_3: 0, gemini_flash_lite_4: 0,
            gemini_pro: 0, gemini_pro_2: 0, gemini_pro_3: 0, gemini_pro_4: 0,
            groq: 0, groq_2: 0, groq_4: 0,
            cerebras: 0,
            mistral: 0,
            openrouter: 0,
            openrouter_gpt: 0, openrouter_gpt_2: 0, openrouter_gpt_4: 0,
            eyes_calls: 0,
            last_reset_date: Local::now().format("%Y-%m-%d").to_string(),
            task_mode: "Brain".to_string(),
            requests_per_prompt: 0,
            max_steps: 0,
            max_retries: 1,
            persona: "Quick".to_string(),
            last_cerebras_call: None,
            last_mistral_call: None,
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
            self.reset_daily(today);
        }
    }

    fn reset_daily(&mut self, today: String) {
        eprintln!("🕒 A new day! Resetting API usage limits...");
        self.gemini_flash = 0; self.gemini_flash_2 = 0; self.gemini_flash_3 = 0; self.gemini_flash_4 = 0;
        self.gemini_flash_lite = 0; self.gemini_flash_lite_2 = 0; self.gemini_flash_lite_3 = 0; self.gemini_flash_lite_4 = 0;
        self.gemini_pro = 0; self.gemini_pro_2 = 0; self.gemini_pro_3 = 0; self.gemini_pro_4 = 0;
        self.groq = 0; self.groq_2 = 0; self.groq_4 = 0;
        self.cerebras = 0;
        self.mistral = 0;
        self.openrouter = 0;
        self.openrouter_gpt = 0; self.openrouter_gpt_2 = 0; self.openrouter_gpt_4 = 0;
        self.eyes_calls = 0;
        self.last_reset_date = today;
        self.save();
    }

    // Returns true if limit not yet hit

    fn can_use_gemini_flash(&self) -> bool {self.gemini_flash < 250}
    fn can_use_gemini_flash_2(&self) -> bool {self.gemini_flash_2 < 250}
    fn can_use_gemini_flash_3(&self) -> bool {self.gemini_flash_3 < 250}
    fn can_use_gemini_flash_4(&self) -> bool {self.gemini_flash_4 < 250}

    fn can_use_gemini_flash_lite(&self) -> bool {self.gemini_flash_lite < 1000}
    fn can_use_gemini_flash_lite_2(&self) -> bool {self.gemini_flash_lite_2 < 1000}
    fn can_use_gemini_flash_lite_3(&self) -> bool {self.gemini_flash_lite_3 < 1000}
    fn can_use_gemini_flash_lite_4(&self) -> bool {self.gemini_flash_lite_4 < 1000}

    fn can_use_gemini_pro(&self) -> bool {self.gemini_pro < 250}
    fn can_use_gemini_pro_2(&self) -> bool {self.gemini_pro_2 < 250}
    fn can_use_gemini_pro_3(&self) -> bool {self.gemini_pro_3 < 250}
    fn can_use_gemini_pro_4(&self) -> bool {self.gemini_pro_4 < 250}

    fn can_use_groq(&self) -> bool {self.groq < 250}
    fn can_use_groq_2(&self) -> bool {self.groq_2 < 250}
    fn can_use_groq_4(&self) -> bool {self.groq_4 < 250}

    fn can_use_cerebras(&self) -> bool {self.cerebras < 250}
    fn can_use_mistral(&self) -> bool {self.mistral < 250}
    fn can_use_openrouter(&self) -> bool {self.openrouter < 250}

    fn can_use_openrouter_gpt(&self) -> bool {self.openrouter_gpt < 250}
    fn can_use_openrouter_gpt_2(&self) -> bool {self.openrouter_gpt_2 < 250}
    fn can_use_openrouter_gpt_4(&self) -> bool {self.openrouter_gpt_4 < 250}

    pub fn can_use_eyes(&self) -> bool {self.eyes_calls < 50}
}

pub fn call_gemini(client: &Client, prompt: &str, model: &str, key: &str) -> Result<String, String>{
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model, key
    );

    let body = json!({
        "contents":[{"parts":[{"text": prompt}]}]
    });

    let response = client
                    .post(&url)
                    .json(&body)
                    .send()
                    .map_err(|e| format!("Failed to send request: {}", e))?;

    let result: serde_json::Value = response
                    .json()
                    .map_err(|e| format!("Failed to parse response: {}", e))?;

    if let Some(error) = result.get("error"){
        return Err(format!("API Error: {}", error));
    }

    Ok(result["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .unwrap_or("No response")
        .to_string())
       
}

pub fn call_cerebras(client: &Client,prompt: &str) -> Result<String, String> {

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

    let result: serde_json::Value = response.json()
                    .map_err(|e| e.to_string())?;

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

    let result: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("Failed to parse JSON: {}", e))?;


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

        if let Some(error) = result.get("error"){
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

        if let Some(error) = result.get("error"){
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

        if let Some(error) = result.get("error"){
            return Err(format!("OpenRouter ChatGPT API error: {}", error["message"]));
        }

        Ok(result["choices"][0]["message"]["content"]
           .as_str()
           .unwrap_or("No response")
           .to_string())
     }

     pub fn smart_prompt(client: &Client, tracker: &mut RequestTracker, prompt: &str, _quiet: bool) -> String {
        let persona_instruction = if tracker.persona == "Helpful" {
            "You are a detailed, conversational AI collaborator. Explain your reasoning and provide thorough answers. "
        } else {
            "You are a quick AI assistant. Be concise, direct, and efficient. Avoid unnecessary fluff. "
        };

        let voice_note_instruction = "IMPORTANT: You have the power to create voice notes. If the user mentions 'voice note', provide ONLY the pure text content of the message to be spoken. Do NOT generate code, scripts, or markdown; output the message directly unless instructed otherwise. ";

        let enriched_prompt = if _quiet {
            prompt.to_string()
        } else {
            format!("{}{}{}", persona_instruction, voice_note_instruction, prompt)
        };

        // Real-time tracking: increment requests for this prompt and save immediately
        tracker.requests_per_prompt += 1;
        tracker.save();

    // 1. OpenRouter GPT-OSS (v1, v2, v4)
    if tracker.can_use_openrouter_gpt() {
        eprintln!("📡 Using: OpenAI GPT-OSS v1 ({}/250)", tracker.openrouter_gpt + 1);
        match call_openrouter_gpt(client, &enriched_prompt, OPEN_ROUTER_KEY) {
            Ok(r) => { tracker.openrouter_gpt += 1; tracker.save(); return r; }
            Err(e) => eprintln!("⚠️ v1 failed: {} - trying backups...", e),
        }
    }
    if tracker.can_use_openrouter_gpt_2() {
        eprintln!("📡 Using: OpenAI GPT-OSS v2 ({}/250)", tracker.openrouter_gpt_2 + 1);
        match call_openrouter_gpt(client, &enriched_prompt, GPT_120_KEY2) {
            Ok(r) => { tracker.openrouter_gpt_2 += 1; tracker.save(); return r; }
            Err(e) => eprintln!("⚠️ v2 failed: {} - trying backups...", e),
        }
    }
    if tracker.can_use_openrouter_gpt_4() {
        eprintln!("📡 Using: OpenAI GPT-OSS v4 ({}/250)", tracker.openrouter_gpt_4 + 1);
        match call_openrouter_gpt(client, &enriched_prompt, GPT_120_KEY4) {
            Ok(r) => { tracker.openrouter_gpt_4 += 1; tracker.save(); return r; }
            Err(e) => eprintln!("⚠️ v4 failed: {}", e),
        }
    }

    // 2. OpenRouter Nemotron (v1)
    if tracker.can_use_openrouter() {
        eprintln!("📡 Using: Nvidia Nemotron ({}/250)", tracker.openrouter + 1);
        match call_openrouter(client, &enriched_prompt, OPEN_ROUTER_KEY) {
            Ok(r) => { tracker.openrouter += 1; tracker.save(); return r; }
            Err(e) => eprintln!("⚠️ Nemotron failed: {}", e),
        }
    }

    // 3. Groq (v1, v2, v4)
    if tracker.can_use_groq() {
        eprintln!("📡 Using: Groq v1 ({}/250)", tracker.groq + 1);
        match call_groq(client, &enriched_prompt, GROQ_KEY) {
            Ok(r) => { tracker.groq += 1; tracker.save(); return r; }
            Err(e) => eprintln!("⚠️ v1 failed: {} - trying backups...", e),
        }
    }
    if tracker.can_use_groq_2() {
        eprintln!("📡 Using: Groq v2 ({}/250)", tracker.groq_2 + 1);
        match call_groq(client, &enriched_prompt, GROQ_KEY2) {
            Ok(r) => { tracker.groq_2 += 1; tracker.save(); return r; }
            Err(e) => eprintln!("⚠️ v2 failed: {} - trying backups...", e),
        }
    }
    if tracker.can_use_groq_4() {
        eprintln!("📡 Using: Groq v4 ({}/250)", tracker.groq_4 + 1);
        match call_groq(client, &enriched_prompt, GROQ_KEY4) {
            Ok(r) => { tracker.groq_4 += 1; tracker.save(); return r; }
            Err(e) => eprintln!("⚠️ v4 failed: {}", e),
        }
    }

    // 4. Gemini Flash-Lite (v1, v2, v3, v4)
    let keys = [GEMINI_KEY, GEMINI_KEY2, GEMINI_KEY3, GEMINI_KEY4];
    if tracker.can_use_gemini_flash_lite() {
        eprintln!("📡 Using: Gemini Flash-Lite v1 ({}/1000)", tracker.gemini_flash_lite + 1);
        match call_gemini(client, &enriched_prompt, "gemini-2.5-flash-lite", keys[0]) {
            Ok(r) => { tracker.gemini_flash_lite += 1; tracker.save(); return r; }
            Err(e) => eprintln!("⚠️ v1 failed: {}", e),
        }
    }
    if tracker.can_use_gemini_flash_lite_2() {
        eprintln!("📡 Using: Gemini Flash-Lite v2 ({}/1000)", tracker.gemini_flash_lite_2 + 1);
        match call_gemini(client, &enriched_prompt, "gemini-2.5-flash-lite", keys[1]) {
            Ok(r) => { tracker.gemini_flash_lite_2 += 1; tracker.save(); return r; }
            Err(e) => eprintln!("⚠️ v2 failed: {}", e),
        }
    }
    if tracker.can_use_gemini_flash_lite_3() {
        eprintln!("📡 Using: Gemini Flash-Lite v3 ({}/1000)", tracker.gemini_flash_lite_3 + 1);
        match call_gemini(client, &enriched_prompt, "gemini-2.5-flash-lite", keys[2]) {
            Ok(r) => { tracker.gemini_flash_lite_3 += 1; tracker.save(); return r; }
            Err(e) => eprintln!("⚠️ v3 failed: {}", e),
        }
    }
    if tracker.can_use_gemini_flash_lite_4() {
        eprintln!("📡 Using: Gemini Flash-Lite v4 ({}/1000)", tracker.gemini_flash_lite_4 + 1);
        match call_gemini(client, &enriched_prompt, "gemini-2.5-flash-lite", keys[3]) {
            Ok(r) => { tracker.gemini_flash_lite_4 += 1; tracker.save(); return r; }
            Err(e) => eprintln!("⚠️ v4 failed: {}", e),
        }
    }

    // 5. Gemini Flash (v1, v2, v3, v4)
    if tracker.can_use_gemini_flash() {
        eprintln!("📡 Using: Gemini Flash v1 ({}/250)", tracker.gemini_flash + 1);
        match call_gemini(client, &enriched_prompt, "gemini-2.5-flash", keys[0]) {
            Ok(r) => { tracker.gemini_flash += 1; tracker.save(); return r; }
            Err(e) => eprintln!("⚠️ v1 failed: {}", e),
        }
    }
    if tracker.can_use_gemini_flash_2() {
        eprintln!("📡 Using: Gemini Flash v2 ({}/250)", tracker.gemini_flash_2 + 1);
        match call_gemini(client, &enriched_prompt, "gemini-2.5-flash", keys[1]) {
            Ok(r) => { tracker.gemini_flash_2 += 1; tracker.save(); return r; }
            Err(e) => eprintln!("⚠️ v2 failed: {}", e),
        }
    }
    if tracker.can_use_gemini_flash_3() {
        eprintln!("📡 Using: Gemini Flash v3 ({}/250)", tracker.gemini_flash_3 + 1);
        match call_gemini(client, &enriched_prompt, "gemini-2.5-flash", keys[2]) {
            Ok(r) => { tracker.gemini_flash_3 += 1; tracker.save(); return r; }
            Err(e) => eprintln!("⚠️ v3 failed: {}", e),
        }
    }
    if tracker.can_use_gemini_flash_4() {
        eprintln!("📡 Using: Gemini Flash v4 ({}/250)", tracker.gemini_flash_4 + 1);
        match call_gemini(client, &enriched_prompt, "gemini-2.5-flash", keys[3]) {
            Ok(r) => { tracker.gemini_flash_4 += 1; tracker.save(); return r; }
            Err(e) => eprintln!("⚠️ v4 failed: {}", e),
        }
    }

    // 6. Gemini Pro (v1, v2, v3, v4)
    if tracker.can_use_gemini_pro() {
        eprintln!("📡 Using: Gemini Pro v1 ({}/250)", tracker.gemini_pro + 1);
        match call_gemini(client, &enriched_prompt, "gemini-2.5-pro", keys[0]) {
            Ok(r) => { tracker.gemini_pro += 1; tracker.save(); return r; }
            Err(e) => eprintln!("⚠️ v1 failed: {}", e),
        }
    }
    if tracker.can_use_gemini_pro_2() {
        eprintln!("📡 Using: Gemini Pro v2 ({}/250)", tracker.gemini_pro_2 + 1);
        match call_gemini(client, &enriched_prompt, "gemini-2.5-pro", keys[1]) {
            Ok(r) => { tracker.gemini_pro_2 += 1; tracker.save(); return r; }
            Err(e) => eprintln!("⚠️ v2 failed: {}", e),
        }
    }
    if tracker.can_use_gemini_pro_3() {
        eprintln!("📡 Using: Gemini Pro v3 ({}/250)", tracker.gemini_pro_3 + 1);
        match call_gemini(client, &enriched_prompt, "gemini-2.5-pro", keys[2]) {
            Ok(r) => { tracker.gemini_pro_3 += 1; tracker.save(); return r; }
            Err(e) => eprintln!("⚠️ v3 failed: {}", e),
        }
    }
    if tracker.can_use_gemini_pro_4() {
        eprintln!("📡 Using: Gemini Pro v4 ({}/250)", tracker.gemini_pro_4 + 1);
        match call_gemini(client, &enriched_prompt, "gemini-2.5-pro", keys[3]) {
            Ok(r) => { tracker.gemini_pro_4 += 1; tracker.save(); return r; }
            Err(e) => eprintln!("⚠️ v4 failed: {}", e),
        }
    }

    if tracker.can_use_cerebras(){
        // Enforce 1 req/sec for Cerebras (60 RPM limit)
        throttle_if_needed(&tracker.last_cerebras_call, Duration::from_millis(1100), "Cerebras");
        eprintln!("📡 Using: Cerebras Llama 70B ({}/500)", tracker.cerebras + 1);
        match call_cerebras(client, &enriched_prompt) {
            Ok(response) => {
                tracker.last_cerebras_call = Some(Instant::now());
                tracker.cerebras += 1;
                tracker.save();
                //thread::sleep(time::Duration::from_secs(65));
                return response;
            }
            Err(e) => {
                tracker.last_cerebras_call = Some(Instant::now());
                eprintln!("⚠️  Cerebras failed: {} — trying next...", e);
            }
        }
    }

    if tracker.can_use_mistral(){
        // Enforce 1 req/sec for Mistral (60 RPM limit)
        throttle_if_needed(&tracker.last_mistral_call, Duration::from_millis(1100), "Mistral");
        eprintln!("📡 Using: Mistral Small ({}/500)", tracker.mistral + 1);
        match call_mistral(client, &enriched_prompt) {
            Ok(response) => {
                tracker.last_mistral_call = Some(Instant::now());
                tracker.mistral += 1;
                tracker.save();
                //thread::sleep(time::Duration::from_secs(65));
                return response;
            }
            Err(e) => {
                tracker.last_mistral_call = Some(Instant::now());
                eprintln!("⚠️  Mistral failed: {} — trying next...", e);
            }
        }
    }

    

        // All providers exhausted
    "❌ All providers exhausted for today. Try again tomorrow!".to_string()
}

/// Sleeps until at least `min_gap` has passed since `last_call`.
/// This enforces a per-provider rate limit (e.g. 1 req/sec for 60 RPM APIs).
fn throttle_if_needed(last_call: &Option<Instant>, min_gap: Duration, provider: &str) {
    if let Some(last) = last_call {
        let elapsed = last.elapsed();
        if elapsed < min_gap {
            let wait = min_gap - elapsed;
            eprintln!("⏳ Rate limiting {}: waiting {}ms...", provider, wait.as_millis());
            std::thread::sleep(wait);
        }
    }
}

pub fn control(persona_name: &str) {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .unwrap_or_else(|_| Client::new());
    let mut tracker = RequestTracker::new();
    tracker.persona = persona_name.to_string();

        println!("🤖 AI Router Ready! Type your prompt (or 'quit' to exit)");
        println!("💾 To save AI output to a file: enigma <filename> <prompt>\n");

        loop {
            println!("You: ");
            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap();
            let input = input.trim();

            if input == "quit" || input == "exit" {
            println!("Goodbye!");
            break;
        }

        if input.is_empty() {
            continue;
        }

        // Normal prompt
        let response = smart_prompt(&client, &mut tracker, input, false);
        println!("\n{}", "AI:".green().bold());
        styles::print_styled(&response);
        println!();

        println!("📊 Remaining today:");
        println!(" Flash: {} | Flash-Lite: {} | Groq: {} | Cerebras: {} | Mistral: {} | Nemotron: {} | GPT-OSS: {} | Pro: {} | Eyes: {}",
            250 - tracker.gemini_flash,
            1000 - tracker.gemini_flash_lite,
            500 - tracker.groq,
            500 - tracker.cerebras,
            500 - tracker.mistral,
            250 - tracker.openrouter,
            250 - tracker.openrouter_gpt,
            100 - tracker.gemini_pro,
            50 - tracker.eyes_calls
        );
        println!();
        }
     }

     pub fn set_control(prompt: &str) -> String {
        set_control_with_persona(prompt, "Quick")
     }

     pub fn set_control_with_persona(prompt: &str, persona: &str) -> String {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap_or_else(|_| Client::new());
        let mut tracker = RequestTracker::new();
        tracker.persona = persona.to_string();

        let response = smart_prompt(&client, &mut tracker, prompt, true);
        return response;
     }