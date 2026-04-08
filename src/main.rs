use std::{env, io::Write, path::Path};

mod model;
mod styles;
pub mod operations;
mod hands;
pub mod cmd_executor;
pub mod api_keys;
mod sport;
mod brain;
pub mod eyes;

fn analysis_task(task: &str, persona: &str, is_headless: bool, base_url: &str) {
    let see = operations::unified_search(task, base_url);
    let prompt = format!(
        "Return ONLY a valid JSON array, no markdown, no explanation, no decorators.\
         Filter this file tree to only documents or projects relevant to this request: {}",
        serde_json::to_string(&see).unwrap()
    );

    let ai_response = model::set_control_with_persona(&prompt, persona);

    let files_json = match operations::read_files_from_json(&ai_response) {
        Ok(files) => serde_json::to_string_pretty(&files).unwrap(),
        Err(e) => {
            eprintln!("Error: {}", e);
            return;
        }
    };

    let prompt2 = format!(
        "Help me with this request: '{}'. Use the following project/file context if helpful:\n{}",
        task, files_json
    );

    let ai_response2 = model::set_control_with_persona(&prompt2, persona);
    
    if is_headless {
        println!("{}", ai_response2);
    } else {
        println!("{}", ai_response2);
    }
}

fn filter_task(original_prompt: &str, ai_output: &str, persona: &str, _is_headless: bool) {
    let prompt = format!(
        "You are an expert communicator. Convert the following technical output into a clean, friendly response for the user. \
         The user's original request was: '{}'.\n\n\
         STRICT RULES:\n\
         1. NEVER include base64, hex, binary data, or encoded strings. If you see long strings of random characters, IGNORE them entirely.\n\
         2. If the output is JSON with file paths/content, extract ONLY the human-readable text message - not file paths or binary content.\n\
         3. For voice note requests, extract and return the EXACT, FULL message that should be spoken. DO NOT output a confirmation like 'Your voice note is ready!'. The text you output will be directly read aloud by TTS. Preserve all details.\n\
         4. If the output is already natural language, return it as-is (refined if needed, but keeping details intact).\n\
         5. Never mention JSON, base64, binary, or technical implementation details.\n\n\
         Technical output to process:\n{}",
        original_prompt, ai_output
    );

    let filtered_response = model::set_control_with_persona(&prompt, persona);
    println!("{}", filtered_response);
}

fn creation_task(task: &str, persona: &str, is_headless: bool, base_url: &str) {
    let see = operations::unified_search(task, base_url);
    let prompt = format!(
        "Return ONLY a valid JSON array, no markdown, no explanation, no decorators.\
         List any documents or files related to this topic: {}",
        serde_json::to_string(&see).unwrap()
    );

    let ai_response = model::set_control_with_persona(&prompt, persona);

    let task_prompt = format!(
        "{}. Check this project tree for context: {}",
        task, ai_response
    );

    let prompt2 = format!(
        "Return ONLY a valid JSON array of objects, each with 'path' and 'content'. \
         No markdown, no explanation. Task: {}",
        task_prompt
    );

    let ai_response2 = model::set_control_with_persona(&prompt2, persona);
    
    let base = Path::new(".");
    match hands::write_files_from_json(base, &ai_response2) {
        Ok(()) => {
            if is_headless {
                eprintln!("✨ Action completed successfully!");
            } else {
                println!("✨ Action completed successfully!");
            }
        },
        Err(e) => eprintln!("❌ Error processing request: {:?}", e),
    }

    if is_headless {
        println!("{}", &ai_response2);
    } else {
        println!("AI Response:\n{}", &ai_response2);
    }
}

fn auto_router(task: &str, persona: &str, is_headless: bool, base_url: &str) {
    let router_prompt = format!( r#"
        Examine the user request and categorize it into exactly one of these 5 categories.
        Return ONLY a JSON object with "thought" (your reasoning) and "category" (1-5).

        CATEGORIES:
        1: General Chat / Quick Question
           - Use for: Greetings, "Who are you?", simple math, or broad non-technical questions.
           - Example: "Hello", "What time is it?", "Tell me a joke".
        2: Research / Analysis / Synthesis
           - Use for: Explaining concepts, summarizing projects/files, finding information, or analyzing data/logs.
           - Example: "What does this project do?", "Summarize these logs", "Explain how the networking works".
        3: System Tasks / Command Execution / Local Media
           - Use for: Running commands, managing the filesystem, checking system status, or technical automation.
           - IMPORTANT: Use this for playing LOCAL files (Movies, MP4, MKV, local MP3) and opening documents (PDF, DOCX, TXT).
           - Example: "List files in Desktop", "Play movie.mp4", "Open report.pdf", "Run the local server".
        4: Content Creation / Modification
           - Use for: Writing documents, editing files, generating code, creating scripts, or fixing specific content.
           - Example: "Write a report on today's logs", "Add a new feature", "Edit the configuration file".
        5: Spotify API / Music Streaming
           - Use for: ONLY controlling music playback via Spotify API, searching Spotify playlists, or managing your Spotify library.
           - DO NOT use for local files or 'tracking' non-music items.
           - Example: "Show my playlists on Spotify", "Search for jazz music", "Play some Chill vibes".

        User Request: "{}"
    "#, task);

    let res = model::set_control_with_persona(&router_prompt, persona);
    
    // Attempt to extract the choice from JSON or fallback to contains
    let choice_data: serde_json::Value = match serde_json::from_str(res.trim().trim_matches('`').trim_start_matches("json").trim()) {
        Ok(v) => v,
        Err(_) => {
            // Fallback for less smart models that fail the JSON rule
            let c = if res.contains('2') { 2 }
                    else if res.contains('3') { 3 }
                    else if res.contains('4') { 4 }
                    else if res.contains('5') { 5 }
                    else { 1 };
            serde_json::json!({"category": c, "thought": "Fallback detection"})
        }
    };

    let choice = choice_data["category"].as_u64().unwrap_or(1);
    
    if choice == 2 {
        if is_headless { eprintln!("🔍 Enigma Analysis:"); } else { println!("🔍 Enigma Analysis:"); }
        analysis_task(task, persona, is_headless, base_url);
    } else if choice == 3 {
        if is_headless { eprintln!("🛠️ Enigma Action:"); } else { println!("🛠️ Enigma Action:"); }
        let search_results = operations::unified_search(task, base_url);
        let context = serde_json::to_string(&search_results).unwrap_or_default();
        cmd_executor::execute_task(task, &context);
    } else if choice == 4 {
        if is_headless { eprintln!("✍️ Enigma Creation:"); } else { println!("✍️ Enigma Creation:"); }
        creation_task(task, persona, is_headless, base_url);
    } else if choice == 5 {
        if is_headless { eprintln!("🎵 Enigma Spotify:"); } else { println!("🎵 Enigma Spotify:"); }
        if let Some(saved) = sport::load_tokens() {
            let new_token = sport::refresh_access_token(&saved.refresh_token);
            let refresh = new_token.refresh_token.unwrap_or(saved.refresh_token);
            sport::save_tokens(&new_token.access_token, &refresh);
            sport::process_ai_command(&new_token.access_token, task);
        } else {
            println!("❌ Spotify is not authorized. Please run the assistant and select option 6 first.");
        }
    } else {
        if is_headless {
            eprintln!("💬 Enigma Chat:");
            let search_results = operations::unified_search(task, base_url);
            let context = serde_json::to_string(&search_results).unwrap_or_default();
            let prompt = format!(
                "Assist with this request: '{}'. Use this context if relevant: {}",
                task, context
            );
            println!("{}", model::set_control_with_persona(&prompt, persona));
        } else {
            println!("💬 Enigma Chat:");
            let search_results = operations::unified_search(task, base_url);
            let context = serde_json::to_string(&search_results).unwrap_or_default();
            let prompt = format!(
                "Assist with this request: '{}'. Use this context if relevant: {}",
                task, context
            );
            println!("{}", model::set_control_with_persona(&prompt, persona));
        }
    }
}

// Preserve interactive versions for local usage
fn code_analyzer() {
    analysis_task("Analyze these files and suggest general improvements.", "Quick", false, "");
}

fn content_creator() {
    print!("What do you want to build? ");
    std::io::stdout().flush().unwrap();

    let mut task = String::new();
    std::io::stdin().read_line(&mut task).unwrap();
    let task = task.trim();
    
    if !task.is_empty() {
        creation_task(task, "Quick", false, "");
    }
}

fn main() {
    // Initialize rustls crypto provider for Spotify integration (v0.23)
    let _ = rustls::crypto::ring::default_provider().install_default();

    let args: Vec<String> = env::args().collect();
    let mut persona = "Quick".to_string();
    let mut target_url = "".to_string();

    let mut task: Option<String> = None;
    let mut filter: Option<(String, String)> = None;
    let mut brain_task: Option<String> = None;

    // Robust CLI parsing
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--persona" if i + 1 < args.len() => {
                persona = args[i+1].clone();
                i += 2;
            }
            "--prompt" if i + 1 < args.len() => {
                task = Some(args[i+1].clone());
                i += 2;
            }
            "--filter" if i + 2 < args.len() => {
                filter = Some((args[i+1].clone(), args[i+2].clone()));
                i += 3;
            }
            "--url" if i + 1 < args.len() => {
                target_url = args[i+1].clone();
                i += 2;
            }
            "--brain" if i + 1 < args.len() => {
                brain_task = Some(args[i+1].clone());
                i += 2;
            }
            _ => {
                i += 1;
            }
        }
    }


    
    // Headless execution for integration
    if let Some(t) = task {
        auto_router(&t, &persona, true, &target_url);
        return;
    }

    if let Some(t) = brain_task {
        eprintln!("🧠 Enigma Brain Mode");
        let result = brain::run(&t, &persona, &target_url);
        println!("{}", result);
        return;
    }

    if let Some((orig, out)) = filter {
        filter_task(&orig, &out, &persona, true);
        return;
    }

    // Interactive Menu Mode
    loop {
        println!("\n╔══════════════════════════════════════════╗");
        println!("║       🌌 Enigma AI Assistant 🌌         ║");
        println!("╠══════════════════════════════════════════╣");
        println!("║ [Mode: {:<32}] ║", format!("{} Assistant", persona));
        println!("╠══════════════════════════════════════════╣");
        println!("║  1. 💬 Quick AI Chat                     ║");
        println!("║  2. 🔍 Analyze Project / Knowledge       ║");
        println!("║  3. 🛠️  Execute Tasks                    ║");
        println!("║  4. ✍️  Generate Content / Files          ║");
        println!("║  5. 🎭 Switch to {} Persona      ║", if persona == "Quick" { "Helpful  " } else { "Quick    " });
        println!("║  6. 🎵 Spotify Integration               ║");
        println!("║  7. 🧠 Brain Mode (Multi-Step)           ║");
        println!("║  8. 🚪 Exit                              ║");
        println!("╚══════════════════════════════════════════╝");
        print!("\nEnigma Assistant > ");
        std::io::stdout().flush().unwrap();
        let mut choice = String::new();
        std::io::stdin().read_line(&mut choice).unwrap();
        
        match choice.trim() {
            "1" => {
               println!("Chatting in {} mode...", persona);
               // Simple wrapper to call control with persona
               println!("(Type 'quit' to exit chat)");
               model::control(&persona);
            }
            "2" => analysis_task("Provide a supportive summary and help with the project.", &persona, false, &target_url),
            "3" => cmd_executor::execute_ai_commands(),
            "4" => {
                print!("What should we create today? ");
                std::io::stdout().flush().unwrap();
                let mut task = String::new();
                std::io::stdin().read_line(&mut task).unwrap();
                let task = task.trim();
                if !task.is_empty() {
                    creation_task(task, &persona, false, &target_url);
                }
            },
            "5" => {
                if persona == "Quick" {
                    persona = "Helpful".to_string();
                } else {
                    persona = "Quick".to_string();
                }
                println!("🎭 Switched persona to {}!", persona);
            }
            "6" => {
                println!("🎵 Opening Spotify Integration...");
                sport::control();
            }
            "7" => {
                print!("🧠 Brain Prompt > ");
                std::io::stdout().flush().unwrap();
                let mut bt = String::new();
                std::io::stdin().read_line(&mut bt).unwrap();
                let bt = bt.trim();
                if !bt.is_empty() {
                    let result = brain::run(bt, &persona, &target_url);
                    println!("\n🧠 Brain Result:\n{}", result);
                }
            }
            "8" | "quit" | "exit" => {
                println!("See you later!");
                break;
            },
            _ => println!("I didn't quite catch that. Pick 1-8!"),
        }
    }
}