use std::{env, io::Write, path::Path};

mod model;
mod styles;
mod operations;
mod hands;
pub mod cmd_executor;

fn code_analyzer_task(task: &str) {
    let see = operations::see();
    let prompt = format!(
        "Return ONLY a valid JSON array, no markdown, no explanation, no decorators.\
         Filter this file tree to only files needed for code editing: {}",
        serde_json::to_string(&see).unwrap()
    );

    let ai_response = model::set_control(&prompt);

    let files_json = match operations::read_files_from_json(&ai_response) {
        Ok(files) => serde_json::to_string_pretty(&files).unwrap(),
        Err(e) => {
            eprintln!("Error: {}", e);
            return;
        }
    };

    let prompt2 = format!(
        "Analyze the following project files according to this request: '{}'. \
         Suggest edits to improve the code quality, readability, and maintainability. Project files:\n{}",
        task, files_json
    );

    let ai_response2 = model::set_control(&prompt2);
    println!("{}", ai_response2);
}

fn filter_task(original_prompt: &str, ai_output: &str) {
    let prompt = format!(
        "Based on the user's original request: '{}', filter the following assistant output to return ONLY the specific information requested. Do not include metadata, explanations, or context not directly requested. Use this output as your reference:\n\n{}",
        original_prompt, ai_output
    );

    let filtered_response = model::set_control(&prompt);
    println!("{}", filtered_response);
}

fn content_creator_task(task: &str) {
    let see = operations::see();
    let prompt = format!(
        "Return ONLY a valid JSON array, no markdown, no explanation, no decorators.\
         Filter this file tree to only files needed for code editing: {}",
        serde_json::to_string(&see).unwrap()
    );

    let ai_response = model::set_control(&prompt);

    let task_prompt = format!(
        "{}. \
         Check this project tree: {} \
         If main.rs exists, name the function inside the file you created \
         something other than fn main() to avoid conflicts so i can call it in my main.rs. \
         Do not name any file similar to the ones already in the project tree.",
        task, ai_response
    );

    let prompt2 = format!(
        "Return ONLY a valid JSON array of objects, each with exactly two fields: 'path' (string) and 'content' (string). \
         No markdown, no explanation, no code blocks, no extra fields. \
         Each 'path' must be a valid path within this project tree. \
         Task: {}",
        task_prompt
    );

    let ai_response2 = model::set_control(&prompt2);

    let base = Path::new(".");
    match hands::write_files_from_json(base, &ai_response2) {
        Ok(()) => println!("All files written successfully!"),
        Err(e) => eprintln!("Error writing file: {:?}", e),
    }

    println!("AI Response:\n{}", &ai_response2);
}

fn auto_router(task: &str) {
    let router_prompt = format!(
        "Categorize the following user request into one of 4 categories. Return ONLY the number 1, 2, 3, or 4. Do not include any other text, markdown, or explanation.\n\
        1: General Chat / Question (e.g. 'how does rust work?', 'explain async')\n\
        2: Analyze Code (e.g. 'analyze my rust code', 'review operations.rs')\n\
        3: Run Commands / Create Project (e.g. 'run tests', 'setup a rust server', 'build the app', 'start a node server')\n\
        4: Create/Append a single File (e.g. 'create a script to print hello world')\n\
        Request: {}", 
        task
    );

    let res = model::set_control(&router_prompt);
    let choice = res.trim();
    
    if choice.contains("2") {
        println!("🔍 Auto-routed to: Code Analyzer");
        code_analyzer_task(task);
    } else if choice.contains("3") {
        println!("🛠️ Auto-routed to: Command Executor");
        cmd_executor::execute_task(task);
    } else if choice.contains("4") {
        println!("✍️ Auto-routed to: Content Creator");
        content_creator_task(task);
    } else {
        println!("💬 Auto-routed to: General Chat");
        println!("{}", model::set_control(&format!("Please help with the following request: {}", task)));
    }
}

// Preserve interactive versions for local usage
fn code_analyzer() {
    code_analyzer_task("Analyze these files and suggest general improvements.");
}

fn content_creator() {
    print!("What do you want to build? ");
    std::io::stdout().flush().unwrap();

    let mut task = String::new();
    std::io::stdin().read_line(&mut task).unwrap();
    let task = task.trim();
    
    if !task.is_empty() {
        content_creator_task(task);
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    // Headless execution for Go-bot integration
    if args.len() > 2 && args[1] == "--prompt" {
        let task = &args[2];
        auto_router(task);
        return;
    }

    if args.len() > 3 && args[1] == "--filter" {
        let original_prompt = &args[2];
        let ai_output = &args[3];
        filter_task(original_prompt, ai_output);
        return;
    }

    // Interactive Menu Mode
    loop {
        println!("\n╔══════════════════════════════╗");
        println!("║      Code Analyzer AI        ║");
        println!("╠══════════════════════════════╣");
        println!("║  1. 💬 AI Chat (free prompt) ║");
        println!("║  2. 🔍 Analyze project files ║");
        println!("║  3. 🛠️  Build / run commands  ║");
        println!("║  4. ✍️  Create new files      ║");
        println!("║  5. 🚪 Exit                  ║");
        println!("╚══════════════════════════════╝");
        print!("\n> ");
        std::io::stdout().flush().unwrap();

        let mut choice = String::new();
        std::io::stdin().read_line(&mut choice).unwrap();
        
        match choice.trim() {
            "1" => model::control(),
            "2" => code_analyzer(),
            "3" => cmd_executor::execute_ai_commands(),
            "4" => content_creator(),
            "5" | "quit" | "exit" => {
                println!("Goodbye!");
                break;
            },
            _ => println!("Invalid choice. Please pick 1-5."),
        }
    }
}