use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rustls::{ClientConnection, RootCertStore};
use rustls::StreamOwned;
use rustls::pki_types::ServerName;
use rustls::ClientConfig;
use serde::Deserialize;
use std::fs;
use tiny_http::{Server, Response};
use serde_json::json;
use crate::api_keys::{
    SPOTIFY_CLIENT_ID as CLIENT_ID,
    SPOTIFY_CLIENT_SECRET as CLIENT_SECRET,
    // SPOTIFY_USER_ID as USER_ID,
    SPOTIFY_REDIRECT_URI as REDIRECT_URI,
    SPOTIFY_STATE as STATE,
};
use crate::styles;

// ─── Structs ───────────────────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
pub struct TokenResponse {
    pub(crate) access_token: String,
    pub(crate) refresh_token: Option<String>,
}

pub const TOKEN_FILE: &str = "tokens.json";

#[derive(Deserialize, serde::Serialize, Debug)]
pub struct SavedTokens {
    pub(crate) access_token: String,
    pub(crate) refresh_token: String,
}

// ─── Callback Server ──────────────────────────────────────────────────────

pub fn wait_for_callback() -> (String, String) {
    let server = Server::http("0.0.0.0:8888").unwrap();
    println!("Waiting for Spotify callback on http://localhost:8888/callback ...");

    loop {
        let request = server.recv().unwrap();
        let url = request.url().to_string();

        if !url.starts_with("/callback") {
            let response = Response::from_string("Not found");
            request.respond(response).unwrap();
            continue;
        }

        let query = url.split('?').nth(1).unwrap_or("");
        let params: std::collections::HashMap<String, String> = query
            .split('&')
            .filter_map(|pair| {
                let mut parts = pair.splitn(2, '=');
                let key = parts.next()?.to_string();
                let val = parts.next().unwrap_or("").to_string();
                Some((key, val))
            })
            .collect();

        let code = params.get("code").cloned().unwrap_or_default();
        let state = params.get("state").cloned().unwrap_or_default();

        let html = r#"
            <html>
            <body style="font-family:sans-serif;text-align:center;padding:50px;background:#121212;color:white;">
                <h1 style="color:#1DB954;">✅ Authenticated!</h1>
                <p>Enigma has captured your credentials. You can close this tab now.</p>
            </body>
            </html>
        "#;
        let response = Response::from_string(html)
            .with_header(tiny_http::Header::from_bytes("Content-Type", "text/html").unwrap());
        request.respond(response).unwrap();

        return (code, state);
    }
}

pub fn save_tokens(access_token: &str, refresh_token: &str) {
    let saved = SavedTokens {
        access_token: access_token.to_string(),
        refresh_token: refresh_token.to_string(),
    };
    let json = serde_json::to_string_pretty(&saved).unwrap();
    fs::write(TOKEN_FILE, json).unwrap();
}

pub fn load_tokens() -> Option<SavedTokens> {
    let content = fs::read_to_string(TOKEN_FILE).ok()?;
    serde_json::from_str(&content).ok()
}

// ─── TLS Helper ───────────────────────────────────────────────────────────

pub fn make_tls_stream(host: &str) -> StreamOwned<ClientConnection, TcpStream> {
    let socket = TcpStream::connect(format!("{}:443", host)).expect("Failed to connect to host");
    let root_store = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    let server_name = ServerName::try_from(host.to_string()).expect("Invalid server name").to_owned();
    StreamOwned::new(
        ClientConnection::new(Arc::new(config), server_name).expect("Failed to create connection"),
        socket,
    )
}

pub fn read_response(stream: &mut impl Read) -> String {
    let mut buffer = [0u8; 16384];
    let mut response = Vec::new();
    loop {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(n) => response.extend_from_slice(&buffer[..n]),
        }
    }
    String::from_utf8_lossy(&response).to_string()
}

pub fn extract_body(response: &str) -> &str {
    response.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("")
}

// ─── Output Formatters (Styled with Markdown) ─────────────────────────────

pub fn arrange_playlist_output(response: String) -> String {
    let mut output = String::new();
    let parsed: serde_json::Value = serde_json::from_str(&response).unwrap_or(json!({}));

    if let Some(items) = parsed["items"].as_array() {
        output.push_str(&format!("# Your Playlists ({} total)\n", parsed["total"]));
        for (i, item) in items.iter().enumerate() {
            let name = item["name"].as_str().unwrap_or("Unknown");
            let id = item["id"].as_str().unwrap_or("Unknown");
            let total_tracks = item["tracks"]["total"].as_u64().unwrap_or(0);
            let owner = item["owner"]["display_name"].as_str().unwrap_or("Unknown");

            output.push_str(&format!(
                "{}. **{}** | `ID: {}` | Tracks: {} | Owner: {}\n",
                i + 1, name, id, total_tracks, owner
            ));
        }
    }
    output
}

pub fn arrange_liked_output(response: String) -> String {
    let mut output = String::new();
    let parsed: serde_json::Value = serde_json::from_str(&response).unwrap_or(json!({}));

    if let Some(items) = parsed["items"].as_array() {
        output.push_str(&format!("# Liked Songs ({} total)\n", parsed["total"]));
        for (i, item) in items.iter().enumerate() {
            let track = &item["track"];
            let name = track["name"].as_str().unwrap_or("Unknown");
            let artist = track["artists"][0]["name"].as_str().unwrap_or("Unknown");
            let album = track["album"]["name"].as_str().unwrap_or("Unknown");
            let duration_ms = track["duration_ms"].as_u64().unwrap_or(0);
            let minutes = (duration_ms / 1000) / 60;
            let seconds = (duration_ms / 1000) % 60;

            output.push_str(&format!(
                "{}. **{}** - *{}* | Album: {} | {}:{:02}\n",
                i + 1, name, artist, album, minutes, seconds
            ));
        }
    }
    output
}

pub fn arrange_tracks_output(response: String) -> String {
    let mut output = String::new();
    let parsed: serde_json::Value = serde_json::from_str(&response).unwrap_or(json!({}));

    if parsed.get("error").is_some() {
        return format!("Error: {}", parsed["error"]["message"]);
    }

    if let Some(items) = parsed["items"].as_array() {
        output.push_str(&format!("## Tracks Found\n"));
        for (i, item) in items.iter().enumerate() {
            let track = if !item["track"].is_null() && item["track"].is_object() {
                &item["track"]
            } else if !item["item"].is_null() && item["item"].is_object() {
                &item["item"]
            } else {
                item
            };

            let name = track["name"].as_str().unwrap_or("Unknown");
            let artist = track["artists"][0]["name"].as_str().unwrap_or("Unknown");
            let album = track["album"]["name"].as_str().unwrap_or("Unknown");
            let duration_ms = track["duration_ms"].as_u64().unwrap_or(0);
            let minutes = (duration_ms / 1000) / 60;
            let seconds = (duration_ms / 1000) % 60;

            output.push_str(&format!(
                "{}. **{}** - *{}* | Album: {} | {}:{:02}\n",
                i + 1, name, artist, album, minutes, seconds
            ));
        }
    }
    output
}

// ─── OAuth2 Flow ──────────────────────────────────────────────────────────

pub fn get_auth_url() -> String {
    let scopes = "playlist-read-private playlist-read-collaborative user-library-read";
    format!(
        "https://accounts.spotify.com/authorize?client_id={}&response_type=code&redirect_uri={}&scope={}&state={}",
        CLIENT_ID,
        url::form_urlencoded::byte_serialize(REDIRECT_URI.as_bytes()).collect::<String>(),
        url::form_urlencoded::byte_serialize(scopes.as_bytes()).collect::<String>(),
        STATE
    )
}

pub fn exchange_code_for_token(code: &str) -> TokenResponse {
    let host = "accounts.spotify.com";
    let mut stream = make_tls_stream(host);
    let credentials = BASE64.encode(format!("{}:{}", CLIENT_ID, CLIENT_SECRET));
    let body = format!("code={}&redirect_uri={}&grant_type=authorization_code", code, 
        url::form_urlencoded::byte_serialize(REDIRECT_URI.as_bytes()).collect::<String>());

    let request = format!(
        "POST /api/token HTTP/1.1\r\nHost: {}\r\nAuthorization: Basic {}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        host, credentials, body.len(), body
    );

    stream.write_all(request.as_bytes()).unwrap();
    let response = read_response(&mut stream);
    let json_body = extract_body(&response);
    serde_json::from_str(json_body).expect("Failed to parse token response")
}

pub fn refresh_access_token(refresh_token: &str) -> TokenResponse {
    let host = "accounts.spotify.com";
    let mut stream = make_tls_stream(host);
    let credentials = BASE64.encode(format!("{}:{}", CLIENT_ID, CLIENT_SECRET));
    let body = format!("grant_type=refresh_token&refresh_token={}", refresh_token);

    let request = format!(
        "POST /api/token HTTP/1.1\r\nHost: {}\r\nAuthorization: Basic {}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        host, credentials, body.len(), body
    );

    stream.write_all(request.as_bytes()).unwrap();
    let response = read_response(&mut stream);
    let json_body = extract_body(&response);
    serde_json::from_str(json_body).expect("Failed to parse refresh token response")
}

// ─── Spotify API Calls ─────────────────────────────────────────────────────

pub fn get_user_playlists(token: &str) -> String {
    let host = "api.spotify.com";
    let mut stream = make_tls_stream(host);
    let request = format!(
        "GET /v1/me/playlists?limit=50 HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
        host, token
    );
    stream.write_all(request.as_bytes()).unwrap();
    let response = read_response(&mut stream);
    extract_body(&response).to_string()
}

pub fn get_current_user(token: &str) -> String {
    let host = "api.spotify.com";
    let mut stream = make_tls_stream(host);
    let request = format!(
        "GET /v1/me HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
        host, token
    );
    stream.write_all(request.as_bytes()).unwrap();
    let response = read_response(&mut stream);
    extract_body(&response).to_string()
}

pub fn get_playlist_tracks(token: &str, playlist_id: &str) -> String {
    let host = "api.spotify.com";
    let mut stream = make_tls_stream(host);
    let request = format!(
        "GET /v1/playlists/{}/tracks?limit=50 HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
        playlist_id, host, token
    );
    stream.write_all(request.as_bytes()).unwrap();
    let response = read_response(&mut stream);
    extract_body(&response).to_string()
}

pub fn get_liked_songs(token: &str) -> String {
    let host = "api.spotify.com";
    let mut stream = make_tls_stream(host);
    let request = format!(
        "GET /v1/me/tracks?limit=50 HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
        host, token
    );
    stream.write_all(request.as_bytes()).unwrap();
    let response = read_response(&mut stream);
    extract_body(&response).to_string()
}

// ─── Utilities ─────────────────────────────────────────────────────────────

pub fn find_playlist_id_by_name(token: &str, name: &str) -> Option<String> {
    let response = get_user_playlists(token);
    let parsed: serde_json::Value = serde_json::from_str(&response).ok()?;
    if let Some(items) = parsed["items"].as_array() {
        for item in items {
            let playlist_name = item["name"].as_str()?.to_lowercase();
            if playlist_name.contains(&name.to_lowercase()) {
                return Some(item["id"].as_str()?.to_string());
            }
        }
    }
    None
}

pub fn search_tracks_in_response(response: &str, query: &str) -> String {
    let mut output = String::new();
    let parsed: serde_json::Value = serde_json::from_str(response).unwrap_or(json!({}));
    let query = query.to_lowercase();

    if let Some(items) = parsed["items"].as_array() {
        output.push_str(&format!("## Results for '{}'\n", query));
        let mut count = 0;
        for item in items {
            let track = if !item["track"].is_null() { &item["track"] } else { item };
            let name = track["name"].as_str().unwrap_or("");
            let artist = track["artists"][0]["name"].as_str().unwrap_or("");
            if name.to_lowercase().contains(&query) || artist.to_lowercase().contains(&query) {
                count += 1;
                output.push_str(&format!("{}. **{}** - *{}*\n", count, name, artist));
            }
        }
        if count == 0 { output.push_str("No matches found."); }
    }
    output
}

// ─── AI Intent Processing ──────────────────────────────────────────────────

// ─── AI Intent Processing ──────────────────────────────────────────────────

pub fn post_process_spotify(token: &str, prompt: &str, api_response: &str) {
    let system_prompt = r#"
        You are 'Enigma Spotify Brain'. You have the extracted data from Spotify for the user's request.
        USER PROMPT: {PROMPT}
        SPOTIFY DATA: {RESPONSE}

        TASK:
        1. Clean and filter the data. If the user asked for something specific (e.g., 'What is the artist of the 3rd song?'), just answer it.
        2. Format the response beautifully using Markdown (headers, lists, bold text).
        3. If you found a playlist ID but the user also wanted to find a song INSIDE it, output a JSON 'chain' command.
        4. Otherwise, provide a friendly FINAL response.

        RESPONSE FORMAT:
        Respond with ONLY a valid JSON object:
        {
          "content": "Friendly Markdown response...",
          "next_action": { "intent": "SEARCH_IN_PLAYLIST", "playlist_id": "...", "query": "..." } // or null
        }
    "#;

    let ai_prompt = format!(
        "USER PROMPT: {}\n\nSPOTIFY DATA:\n{}",
        prompt, api_response
    );

    let raw_response = crate::model::set_control_with_persona(&ai_prompt, "SpotifyBrain");
    let clean_res = raw_response.trim().trim_matches('`').trim_start_matches("json").trim();

    let processed: serde_json::Value = match serde_json::from_str(clean_res) {
        Ok(val) => val,
        Err(_) => {
            println!("💬 Enigma: {}", raw_response); // Fallback to raw text
            return;
        }
    };

    if let Some(content) = processed["content"].as_str() {
        styles::print_styled(content);
    }

    if let Some(next) = processed["next_action"].as_object() {
        let intent = next.get("intent").and_then(|v| v.as_str()).unwrap_or("UNKNOWN");
        match intent {
             "SEARCH_IN_PLAYLIST" | "LIST_TRACKS" => {
                let id = next.get("playlist_id").and_then(|v| v.as_str()).unwrap_or("");
                let query = next.get("query").and_then(|v| v.as_str()).unwrap_or("");
                if !id.is_empty() {
                    let res = get_playlist_tracks(token, id);
                    if intent == "SEARCH_IN_PLAYLIST" {
                        styles::print_styled(&search_tracks_in_response(&res, query));
                    } else {
                        styles::print_styled(&arrange_tracks_output(res));
                    }
                }
             },
             _ => {}
        }
    }
}

pub fn process_ai_command(token: &str, prompt: &str) {
    let system_prompt = r#"
        Categorize the user's Spotify request into one of these JSON formats:
        - {"intent": "LIST_PLAYLISTS"}
        - {"intent": "LIST_LIKED_SONGS"}
        - {"intent": "USER_INFO"}
        - {"intent": "SEARCH_IN_PLAYLIST", "playlist": "...", "query": "..."}
        - {"intent": "LIST_TRACKS", "playlist": "..."}
        - {"intent": "UNKNOWN"}
        Return ONLY the JSON object.
    "#;
    
    let ai_prompt = format!("{}\nUser Request: {}", system_prompt, prompt);
    let response = crate::model::set_control_with_persona(&ai_prompt, "Quick");
    let clean_res = response.trim().trim_matches('`').trim_start_matches("json").trim();
    
    let intent_data: serde_json::Value = match serde_json::from_str(clean_res) {
        Ok(val) => val,
        Err(_) => {
            println!("⚠️  AI interpretation failed. Raw response: {}", response);
            json!({"intent": "UNKNOWN"})
        }
    };
    
    match intent_data["intent"].as_str().unwrap_or("UNKNOWN") {
        "LIST_PLAYLISTS" => {
            let res = get_user_playlists(token);
            post_process_spotify(token, prompt, &arrange_playlist_output(res));
        },
        "LIST_LIKED_SONGS" => {
            let res = get_liked_songs(token);
            post_process_spotify(token, prompt, &arrange_liked_output(res));
        },
        "USER_INFO" => {
            let res = get_current_user(token);
            post_process_spotify(token, prompt, &res);
        },
        "SEARCH_IN_PLAYLIST" | "LIST_TRACKS" => {
            let playlist_name = intent_data["playlist"].as_str().unwrap_or("");
            let query = intent_data["query"].as_str().unwrap_or("");
            
            if let Some(id) = find_playlist_id_by_name(token, playlist_name) {
                let res = get_playlist_tracks(token, &id);
                let simplified = if intent_data["intent"] == "SEARCH_IN_PLAYLIST" {
                    search_tracks_in_response(&res, query)
                } else {
                    arrange_tracks_output(res)
                };
                post_process_spotify(token, prompt, &simplified);
            } else {
                println!("❌ Could not find playlist: {}", playlist_name);
            }
        },
        _ => {
            println!("💬 Enigma: I'm not sure how to do that yet. Try 'show my playlists' or 'search Thriller in My 80s Mix'.");
        }
    }
}

// ─── Main Controller Loop ──────────────────────────────────────────────────

pub fn control() {
    let access_token = if let Some(saved) = load_tokens() {
        println!("✨ Found saved session! Refreshing access...");
        let new_token = refresh_access_token(&saved.refresh_token);
        let new_refresh = new_token.refresh_token.unwrap_or(saved.refresh_token);
        save_tokens(&new_token.access_token, &new_refresh);
        new_token.access_token
    } else {
        let auth_url = get_auth_url();
        println!("🚀 Authorize Enigma at this link:");
        println!("🔗 [AI-AUTH-LINK] {}", auth_url);
        println!("⏳ Waiting for automated authorization on port 8888...");
        
        // Use the local callback server to capture the code
        let (code, _state) = wait_for_callback();
        
        println!("✅ Received authorization code! Exchanging for tokens...");
        let token_data = exchange_code_for_token(&code);
        let refresh = token_data.refresh_token.expect("No refresh token received");
        save_tokens(&token_data.access_token, &refresh);
        token_data.access_token
    };

    println!("\n🎵 Spotify Integration Ready! (Type 'quit' to exit)");
    loop {
        print!("\nSpotify Prompt > ");
        std::io::stdout().flush().unwrap();
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).unwrap();
        let trimmed = input.trim();
        if trimmed == "quit" || trimmed == "exit" { break; }
        if !trimmed.is_empty() { process_ai_command(&access_token, trimmed); }
    }
}
// Spotify Module
