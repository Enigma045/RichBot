
use reqwest::blocking::Client;
use serde_json::json;
use std::io;
use colored::Colorize;

use crate::styles;


const GEMINI_KEY: &str ;
const GROQ_KEY: &str ;
const CEREBRAS_KEY: &str ;
const MISTRAL_KEY: &str ;

pub(crate) struct RequestTracker{
    pub(crate) gemini_flash: u32,
    pub(crate) gemini_flash_lite: u32,
    pub(crate) gemini_pro: u32,
    pub(crate) groq: u32,
    pub(crate) cerebras: u32,
    pub(crate) mistral: u32,
}

impl RequestTracker {
    pub fn new() -> Self{
        RequestTracker {
            gemini_flash: 0,
            gemini_flash_lite: 0,
            gemini_pro: 0,
            groq: 0,
            cerebras: 0,
            mistral: 0    
        }
    }

    // Returns true if limit not yet hit

    fn can_use_gemini_flash(&self) -> bool {self.gemini_flash < 250}
    fn can_use_gemini_flash_lite(&self) -> bool {self.gemini_flash_lite < 250}
    fn can_use_gemini_pro(&self) -> bool {self.gemini_pro < 250}
    fn can_use_groq(&self) -> bool {self.groq < 250}
    fn can_use_cerebras(&self) -> bool {self.cerebras < 250}
    fn can_use_mistral(&self) -> bool {self.mistral < 250}
}

pub fn call_gemini(client: &Client, prompt: &str, model: &str) -> Result<String, String>{
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model, GEMINI_KEY
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
        "model": "llama3.3-70b",
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

pub fn call_groq(client: &Client, prompt: &str) -> Result<String, String> {
    let body = json!({
        "model": "openai/gpt-oss-120b",
        "messages": [{ "role": "user", "content": prompt }]
    });

    let response = client
        .post("https://api.groq.com/openai/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", GROQ_KEY))
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

     pub fn smart_prompt(client: &Client,tracker: &mut RequestTracker,prompt: &str, quiet: bool) -> String {

        if tracker.can_use_gemini_flash() {
            if !quiet { println!("📡 Using: Gemini 2.5 Flash-Lite ({}/1000)", tracker.gemini_flash_lite + 1); }
            match call_gemini(client,prompt, "gemini-2.5-flash-lite") {
              Ok(response) => {tracker.gemini_flash_lite += 1; return response;}
              Err(e) => if !quiet { println!("⚠️  Gemini Flash-Lite failed: {} — trying next...", e); }
            }
            
        }

        if tracker.can_use_gemini_flash() {
            if !quiet { println!("📡 Using: Gemini 2.5 Flash ({}/250)", tracker.gemini_flash + 1); }
            match call_gemini(client, prompt, "gemini-2.5-flash") {
                Ok(response) => {tracker.gemini_flash += 1; return response;},
                Err(e) => if !quiet { println!("⚠️  Gemini Flash failed: {} — trying next...", e); }
            }
        }

        
    if tracker.can_use_groq(){
        if !quiet { println!("📡 Using: Groq Llama 4 ({}/500)", tracker.groq + 1); }
        match call_groq(client, prompt){
            Ok(response) => {tracker.groq += 1; return response;}
            Err(e) => if !quiet { println!("⚠️  Groq failed: {} — trying next...", e); }
        }
    }

    if tracker.can_use_cerebras(){
        if !quiet { println!("📡 Using: Cerebras Llama 70B ({}/500)", tracker.cerebras + 1); }
        match call_cerebras(client, prompt) {
            Ok(response) => {tracker.cerebras += 1; return response;}
            Err(e) => if !quiet { println!("⚠️  Cerebras failed: {} — trying next...", e); }
        }
    }

    if tracker.can_use_mistral(){
        if !quiet { println!("📡 Using: Mistral Small ({}/500)", tracker.mistral + 1); }
        match call_mistral(client, prompt) {
            Ok(response) => {tracker.mistral += 1; return response;}
            Err(e) => if !quiet { println!("⚠️  Mistral failed: {} — trying next...", e); }
        }
    }

    if tracker.can_use_gemini_pro(){
        if !quiet { println!("📡 Using: Gemini 2.5 Pro ({}/100)", tracker.gemini_pro + 1); }
        match call_gemini(client, prompt, "gemini-2.5-pro") {
            Ok(response) => {tracker.gemini_pro += 1; return response;}
            Err(e) => if !quiet { println!("⚠️  Gemini Pro failed: {}", e); }
        }
    }

        // All providers exhausted
    "❌ All providers exhausted for today. Try again tomorrow!".to_string()
     }

pub fn control(){
        let client = Client::new(); 
        let mut tracker = RequestTracker::new();

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
        println!(" Flash: {} | Flash-Lite: {} | Groq: {} | Cerebras: {} | Mistral: {} | Pro: {}",
            250 - tracker.gemini_flash,
            1000 - tracker.gemini_flash_lite,
            500 - tracker.groq,
            500 - tracker.cerebras,
            500 - tracker.mistral,
            100 - tracker.gemini_pro
        );
        println!();
        }
     }

     pub fn set_control(prompt: &str) -> String {
        let client = Client::new();
        let mut tracker = RequestTracker::new();

        let response = smart_prompt(&client, &mut tracker, prompt, true);
        return response;
     }