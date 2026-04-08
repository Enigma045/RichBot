use serde::{Deserialize, Serialize};
use std::fs;
use crate::operations::{FileEntry, EntryType};
use crate::api_keys::OPEN_ROUTER_KEY;
use crate::model::RequestTracker;

#[derive(Serialize, Deserialize, Debug)]
struct RerankRequest {
    model: String,
    query: String,
    documents: Vec<String>,
}

#[derive(Deserialize, Debug)]
struct RerankResponse {
    results: Vec<RerankResult>,
}

#[derive(Deserialize, Debug)]
struct RerankResult {
    index: usize,
    relevance_score: f64,
}

const MODEL_NAME: &str = "cohere/rerank-4-pro";
const API_URL: &str = "https://openrouter.ai/api/v1/rerank";

pub fn search(query: &str) -> Result<Vec<FileEntry>, Box<dyn std::error::Error>> {
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let mut tracker = RequestTracker::new();
    if !tracker.can_use_eyes() {
        eprintln!("🛑 eyes::search: AI call limit reached (50/50).");
        return Err("AI call limit reached on eyes.rs".into());
    }

    // Load paths from paths.json
    let paths_content = fs::read_to_string("paths.json")?;
    let all_paths: Vec<String> = serde_json::from_str(&paths_content)?;
    eprintln!("🔍 eyes::search: Loaded {} paths from paths.json", all_paths.len());
    
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new());
    
    struct ScoredPath {
        path: String,
        score: f64,
    }
    let mut scored_results: Vec<ScoredPath> = Vec::new();

    // Chunk size (API limit is usually 1,000)
    let chunk_size = 1000;
    for chunk in all_paths.chunks(chunk_size) {
        let request_body = RerankRequest {
            model: MODEL_NAME.to_string(),
            query: query.to_string(),
            documents: chunk.to_vec(),
        };

        let response = client
            .post(API_URL)
            .header("Authorization", format!("Bearer {}", OPEN_ROUTER_KEY))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()?;

        // Increment tracker for each API call (chunk)
        tracker.eyes_calls += 1;
        tracker.save();

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text()?;
            eprintln!("❌ eyes::search API Error (Status {}): {}", status, error_text);
            return Err(format!("API Error: {}", error_text).into());
        }

        let rerank_response: RerankResponse = match response.json() {
            Ok(res) => res,
            Err(e) => {
                eprintln!("❌ eyes::search failed to parse JSON: {}", e);
                return Err(e.into());
            }
        };
        
        for res in rerank_response.results {
            scored_results.push(ScoredPath {
                path: chunk[res.index].clone(),
                score: res.relevance_score,
            });
        }
    }

    // Sort by score descending
    scored_results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    eprintln!("🔍 eyes::search: Found {} scored results across chunks", scored_results.len());

    // Return top 20
    let final_results: Vec<FileEntry> = scored_results.into_iter().take(20).map(|s| {
        eprintln!("✅ Match: {} (score: {:.4})", s.path, s.score);
        FileEntry {
            path: s.path,
            depth: 0,
            entry_type: EntryType::File,
        }
    }).collect();

    Ok(final_results)
}
