# 🌌 Enigma Assistant

Enigma Assistant is a powerful, multi-modal AI orchestration system designed for advanced task automation, code analysis, and remote interaction. It features a high-performance Rust core for logic orchestration and a Go-based interface for ubiquitous access via WhatsApp and voice.

## 🚀 Key Features

### 🧠 Advanced Orchestration (Brain Mode)
- **Multi-Step Decomposition**: Automatically breaks down complex user requests into logical sub-tasks.
- **Dynamic Rollbacks**: Intelligent failure assessment that can "jump back" to a previous step to fix errors and re-execute.
- **Context Chaining**: Preserves state and results across sequential execution steps.
- **Real-Time Status**: Live updates on execution stages and steps, forwarded directly to the user.

### ⚡ Direct Execution (Improvise Mode)
- **Single-Step Routing**: Fast, direct handling of simple queries without full plan decomposition.
- **Category-Aware Dispatching**: Automatically routes tasks to specialized handlers (Chat, Research, Task Execution, Content Creation, Spotify).

### 🛠️ System & File Intelligence
- **Spatial Awareness**: Tracks current working directory state across multiple steps.
- **Proactive Patching**: Uses smart block-matching to safely modify specific lines in large source files.
- **Unified Search**: Integrated fuzzy search and project tree analysis.

---

## ⌨️ Keywords & Triggers

Enigma Assistant monitors your input for these specific control keywords. These can be used alone or embedded within a larger request.

| Keyword | Action | Example |
| :--- | :--- | :--- |
| `brain mode` | Switch to Multi-Step Orchestration. | "Switch to brain mode and fix the bug" |
| `improvise mode`| Switch to Single-Step Auto-Routing. | "improvise mode, who are you?" |
| `steps N` | Set the max steps for Brain decomposition. | "steps 5, build a website" |
| `steps default` | Reset max steps to default (16). | "steps default" |
| `retries N` | Set max retry attempts for failed steps. | "retries 3" |
| `retries default`| Reset retries to default (1). | "retries default" |
| `rollback N` | Set max rollback jumps allowed. | "rollback 2" |
| `rollback off` | Disable rollbacks (set to 0). | "rollback off" |
| `validate on` | Enable API health check skipping. | "validate on" |
| `validate off` | Disable API health check skipping. | "validate off" |
| `validate now` | Force a re-check of all API key health. | "validate now" |
| `voice note` | Trigger TTS generation for the response. | "Explain this as a voice note" |
| `quit` / `exit` | Terminate the assistant or return to menu. | "quit" |

---

## 🔑 API Keys Setup

To enable the assistant's capabilities, you must configure your API keys in `src/api_keys.rs`. Create this file with the following structure:

```rust
// Core Provider Slots
pub const GEMINI_KEY: &str = "YOUR_GEMINI_KEY_1";
pub const GEMINI_KEY2: &str = "YOUR_GEMINI_KEY_2";
pub const GEMINI_KEY3: &str = "YOUR_GEMINI_KEY_3";
pub const GEMINI_KEY4: &str = "YOUR_GEMINI_KEY_4";

pub const GROQ_KEY: &str = "YOUR_GROQ_KEY_1";
pub const GROQ_KEY2: &str = "YOUR_GROQ_KEY_2";
pub const GROQ_KEY4: &str = "YOUR_GROQ_KEY_4";

pub const CEREBRAS_KEY: &str = "YOUR_CEREBRAS_KEY";
pub const MISTRAL_KEY: &str = "YOUR_MISTRAL_KEY";

// OpenRouter & Special Models
pub const OPEN_ROUTER_KEY: &str = "YOUR_OPENROUTER_KEY_1";
pub const GPT_120_KEY: &str     = "YOUR_OPENROUTER_KEY_1"; // Often same as above
pub const GPT_120_KEY2: &str    = "YOUR_OPENROUTER_KEY_2";
pub const GPT_120_KEY4: &str    = "YOUR_OPENROUTER_KEY_4";

// Spotify Integration
pub const SPOTIFY_CLIENT_ID: &str     = "YOUR_CLIENT_ID";
pub const SPOTIFY_CLIENT_SECRET: &str = "YOUR_CLIENT_SECRET";
pub const SPOTIFY_USER_ID: &str       = "YOUR_USER_ID";
pub const SPOTIFY_STATE: &str         = "random_string_123";
pub const SPOTIFY_REDIRECT_URI: &str  = "http://localhost:8888/callback";
```

---

## 🏗️ Technical Architecture

### Core Stack
- **Language**: Rust (Standard 2024 Edition)
- **Interface**: Go (via `Go-bot` with `whatsmeow` for WhatsApp connectivity).
- **Automation**: Python (for local STT/TTS auxiliary scripts).

### Project Layout
- `src/`: Rust source code for orchestration, model routing, and system operations.
- `Go-bot/`: Go source code for the WhatsApp bridge and worker pool.
- `plans/`: Stores execution plans generated during Brain Mode tasks.
- `sandbox/`: A safe environment for the AI to create projects and run commands.
- `tracker.json`: Persistent usage tracking and API key health management.

---

## ⚙️ Setup & Configuration

### Prerequisites
- [Rust](https://www.rust-lang.org/tools/install) (2024 edition)
- [Go](https://go.dev/doc/install)
- [Python](https://www.python.org/downloads/) (for TTS/Whisper)
- Ngrok (optional, for remote Colab/TTS connectivity)

### Building & Running
1. **Start the Rust Core**:
   ```bash
   cargo build --release
   ./target/release/Code_analyzer
   ```
2. **Start the Go Bridge**:
   ```bash
   cd Go-bot
   go run main.go
   ```
   *Note: Scan the generated QR code in your terminal to link your WhatsApp account.*

---

## 📄 License
This project is private and intended for personal/internal use.
