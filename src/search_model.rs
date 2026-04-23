use std::fs;
use std::sync::OnceLock;
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use crate::operations::{FileEntry, EntryType};

// Cache paths.json in memory — parsed once, reused on every search call.
static PATHS_CACHE: OnceLock<Vec<String>> = OnceLock::new();

fn load_paths() -> &'static Vec<String> {
    PATHS_CACHE.get_or_init(|| {
        match fs::read_to_string("paths.json") {
            Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
            Err(e) => {
                eprintln!("⚠️ search_model: Failed to read paths.json: {}", e);
                Vec::new()
            }
        }
    })
}

fn fuzzy_search(query_keywords: &[String], paths: &[String]) -> Vec<(String, i64)> {
    let matcher = SkimMatcherV2::default();
    let mut results = Vec::new();
    
    for path in paths {
        let mut total_score = 0;
        let mut match_count = 0;
        let path_lower = path.to_lowercase();
        
        for k in query_keywords {
            let k_lower = k.to_lowercase();
            // Exact substring matches are strongly prioritized over spaced fuzzy matches
            if path_lower.contains(&k_lower) {
                total_score += 1000 + (k_lower.len() as i64 * 10);
                match_count += 1;
            } else if let Some(score) = matcher.fuzzy_match(path, &k_lower) {
                // Fallback to fuzzy match
                total_score += score;
                match_count += 1;
            }
        }
        
        if total_score > 0 {
            // Reward paths that match MORE of the generated keywords
            let final_score = total_score * match_count as i64;
            results.push((path.clone(), final_score));
        }
    }
    
    // Sort descending by score
    results.sort_by(|a, b| b.1.cmp(&a.1));
    results
}

fn extract_json_array(output: &str) -> Vec<String> {
    let text = output.trim();
    if text.is_empty() {
        return Vec::new();
    }

    let json_start = text.find('[');
    let json_end = text.rfind(']');

    if let (Some(start), Some(end)) = (json_start, json_end) {
        if start < end {
            let json_slice = &text[start..=end];
            let cleaned = json_slice.trim_matches('`').trim_start_matches("json").trim();
            if let Ok(parsed) = serde_json::from_str::<Vec<String>>(cleaned) {
                return parsed;
            }
        }
    }
    Vec::new()
}

/// AI-driven keyword search. Generates keywords then fuzzy searches paths.json.
pub fn search(query: &str) -> Vec<FileEntry> {
    if query.is_empty() {
        return Vec::new();
    }

    // Use cached paths — loaded from disk only once for the entire process lifetime.
    let paths = load_paths();
    if paths.is_empty() {
        return Vec::new();
    }

    eprintln!("🧠 search_model: Generating keywords for '{}'...", query);
    let prompt1 = format!(
        "Return ONLY a JSON array of search keyword fragments to help our file ranking engine find the target path. Break the user's request '{}' down into independent atomic concepts (e.g., base names, module identifiers, component tags, specific dates/versions, or relevant file extensions). For example, 'auth controller python test' converts to [\"auth\", \"controller\", \"test\", \".py\"]; 'show title year 2 vol 2' converts to [\"show title\", \"year 2\", \"volume 2\", \"Y2\", \"Vol 2\", \"V2\"]. Provide ~15-25 variations/abbreviations of these identifying fragments. No markdown, no explanations, ONLY the JSON array.",
        query
    );

    let keywords_json = crate::model::set_control_with_persona(&prompt1, "Quick");
    let keywords = extract_json_array(&keywords_json);

    if keywords.is_empty() {
        eprintln!("⚠️ search_model: AI failed to generate keywords. Raw: {}", keywords_json);
        return Vec::new();
    }

    eprintln!("🔍 search_model: Keywords: {:?}", keywords);

    let matched = fuzzy_search(&keywords, paths);
    let top_paths: Vec<String> = matched.into_iter().take(20).map(|(p, _)| p).collect();
    
    if top_paths.is_empty() {
        eprintln!("⚠️ search_model: No matches found for keywords. Falling back.");
        return Vec::new();
    }
    
    eprintln!("🤖 search_model: Asking AI to validate top {} matches...", top_paths.len());
    let prompt2 = format!(
        "Here are potential file paths matching the user's request: '{}'. Validate which paths are the most accurate matches. Return ONLY a JSON array of the best matching file path strings. No markdown, no explanations, no wrappers. Paths: {:?}", 
        query, top_paths
    );
    
    let validation_json = crate::model::set_control_with_persona(&prompt2, "Quick");
    let validated_paths = extract_json_array(&validation_json);
    
    if validated_paths.is_empty() {
        eprintln!("⚠️ search_model: AI validation failed or returned empty. Falling back to top 5 fuzzy matches.");
        return top_paths.into_iter().take(5).map(|p| {
            let is_dir = std::path::Path::new(&p).is_dir();
            eprintln!("✅ Match: {} ({})", p, if is_dir { "Dir" } else { "File" });
            FileEntry {
                path: p,
                depth: 0,
                entry_type: if is_dir { EntryType::Directory } else { EntryType::File },
            }
        }).collect();
    }
    
    eprintln!("✅ search_model: AI validated {} paths.", validated_paths.len());
    validated_paths.into_iter().map(|p| {
        let is_dir = std::path::Path::new(&p).is_dir();
        eprintln!("✅ Match: {} ({})", p, if is_dir { "Dir" } else { "File" });
        FileEntry {
            path: p,
            depth: 0,
            entry_type: if is_dir { EntryType::Directory } else { EntryType::File },
        }
    }).collect()
}
