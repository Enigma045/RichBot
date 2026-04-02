
use colored::*;

// ================================
// OUTPUT STYLER
// ================================
pub fn print_styled(response: &str) {
    let mut inside_code_block = false;
    let mut code_language = String::new();

    for line in response.lines() {

        // ── Code block start/end (```) ──────────────────
        if line.starts_with("```") {
            if inside_code_block {
                // End of code block
                println!("{}", "─────────────────────────────".dimmed());
                inside_code_block = false;
                code_language.clear();
            } else {
                // Start of code block
                code_language = line.trim_start_matches('`').to_string();
                let label = if code_language.is_empty() {
                    "code".to_string()
                } else {
                    code_language.clone()
                };
                println!("{}", "─────────────────────────────".dimmed());
                println!("{}", format!(" 📄 {}", label).cyan().bold());
                println!("{}", "─────────────────────────────".dimmed());
                inside_code_block = true;
            }
            continue;
        }

        // ── Inside code block ───────────────────────────
        if inside_code_block {
            println!("{}", line.yellow());
            continue;
        }

        // ── Headers (## Heading) ────────────────────────
        if line.starts_with("### ") {
            println!("{}", line.trim_start_matches('#').trim().bold().underline());
            continue;
        }
        if line.starts_with("## ") {
            println!("\n{}", line.trim_start_matches('#').trim().green().bold().underline());
            continue;
        }
        if line.starts_with("# ") {
            println!("\n{}", line.trim_start_matches('#').trim().magenta().bold().underline());
            continue;
        }

        // ── Bullet points (- item or * item) ───────────
        if line.starts_with("- ") || line.starts_with("* ") {
            let content = &line[2..];
            println!("  {} {}", "•".cyan(), style_inline(content));
            continue;
        }

        // ── Numbered list (1. item) ─────────────────────
        if let Some(rest) = get_numbered_item(line) {
            println!("  {}", style_inline(rest));
            continue;
        }

        // ── Horizontal rule (---) ───────────────────────
        if line.trim() == "---" || line.trim() == "***" {
            println!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".dimmed());
            continue;
        }

        // ── Empty line ──────────────────────────────────
        if line.trim().is_empty() {
            println!();
            continue;
        }

        // ── Normal text (with inline styles) ───────────
        println!("{}", style_inline(line));
    }
}

// ================================
// INLINE STYLER — Bold & Inline Code
// ================================
pub fn style_inline(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {

        // Inline code (`code`)
        if ch == '`' {
            let mut code = String::new();
            for inner in chars.by_ref() {
                if inner == '`' { break; }
                code.push(inner);
            }
            result.push_str(&format!("{}", code.on_bright_black().yellow()));

        // Bold (**text**)
        } else if ch == '*' && chars.peek() == Some(&'*') {
            chars.next(); // skip second *
            let mut bold_text = String::new();
            loop {
                match chars.next() {
                    Some('*') if chars.peek() == Some(&'*') => {
                        chars.next(); // skip closing **
                        break;
                    }
                    Some(c) => bold_text.push(c),
                    None => break,
                }
            }
            result.push_str(&format!("{}", bold_text.bold().white()));

        // Normal character
        } else {
            result.push(ch);
        }
    }

    result
}

// ================================
// HELPER — Detect numbered list
// ================================
pub fn get_numbered_item(line: &str) -> Option<&str> {
    let mut i = 0;
    let bytes = line.as_bytes();
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i > 0 && bytes.get(i) == Some(&b'.') && bytes.get(i + 1) == Some(&b' ') {
        Some(&line[i + 2..])
    } else {
        None
    }
}