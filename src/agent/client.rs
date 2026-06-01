//! LLM API client for OpenAI and Anthropic.
//!
//! Supports:
//! - OpenAI Chat Completions API (`gpt-4o`, `gpt-4o-mini`, etc.)
//! - Anthropic Messages API (`claude-3-5-sonnet`, `claude-3-5-haiku`, etc.)
//! - Structured JSON output parsing for `EditBlock` responses
//! - Configurable via `ClientConfig` (model, base URL, API key, timeouts)

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::diff::format::EditBlock;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Which provider to use: `"openai"` or `"anthropic"`.
    pub provider: String,
    /// Model identifier, e.g. `"gpt-4o"` or `"claude-3-5-sonnet-20250620"`.
    pub model: String,
    /// Base URL for the API. Defaults to official endpoints if `None`.
    pub base_url: Option<String>,
    /// API key secret.
    pub api_key: Option<String>,
    /// Request timeout in seconds.
    pub timeout_secs: u64,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            provider: "openai".to_string(),
            model: "gpt-4o".to_string(),
            base_url: None,
            api_key: None,
            timeout_secs: 120,
        }
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// A single turn in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    #[serde(rename = "content")]
    pub content: MessageContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentPart {
    pub part_type: String, // "text" or "input_image"
    pub text: Option<String>,
    pub source: Option<InputImageSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputImageSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub media_type: Option<String>,
    pub data: Option<String>,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Message {
            role: "user".to_string(),
            content: MessageContent::Text(text.into()),
        }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Message {
            role: "assistant".to_string(),
            content: MessageContent::Text(text.into()),
        }
    }

    pub fn system(text: impl Into<String>) -> Self {
        Message {
            role: "system".to_string(),
            content: MessageContent::Text(text.into()),
        }
    }
}

// ---------------------------------------------------------------------------
// Response
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    /// Raw text content of the response.
    pub text: String,
    /// Model that generated the response.
    pub model: String,
    /// Tokens consumed (if available).
    pub usage: Option<Usage>,
    /// Finish reason string.
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: usize,
    pub output_tokens: usize,
}

impl LlmResponse {
    /// Parse a structured `EditBlock` from the response text.
    ///
    /// Tries three strategies in order:
    /// 1. **JSON blob** — looks for a code block or bare JSON object containing
    ///    the fields `file_path`, `old_anchor`, `new_anchor`, `old_lines`, `new_lines`.
    /// 2. **Unified diff** — parses a `<<<<<<< HEAD:XXXXXXXX\n...\n=======\n...\n>>>>>>> XXXXXXXX`
    ///    section directly.
    /// 3. **Anchor-only JSON** — accepts `{ "old_anchor": "...", "new_lines": [...] }`
    ///    and reconstructs `old_lines` by looking them up in `file_contents`.
    pub fn parse_edit_block(&self, file_contents: &str) -> Option<EditBlock> {
        if let Some(block) = Self::parse_json_blob(&self.text) {
            return Some(block);
        }
        if let Some(block) = Self::parse_unified_diff(&self.text) {
            return Some(block);
        }
        self.parse_anchor_only_json(&self.text, file_contents)
    }

    fn parse_json_blob(text: &str) -> Option<EditBlock> {
        let text = text.trim();
        let start = text.find('{')?;
        let mut depth = 0i32;
        let mut end_idx = start;

        for (i, c) in text[start..].chars().enumerate() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end_idx = start + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }

        let json_str = &text[start..end_idx];
        let value: Value = serde_json::from_str(json_str).ok()?;
        Self::edit_block_from_json(&value)
    }

    fn parse_unified_diff(text: &str) -> Option<EditBlock> {
        use regex::Regex;

        // Match the unified diff format with non-greedy capture groups.
        // (?s:...) enables dot-all mode so . matches newlines; *? is non-greedy.
        let re = Regex::new(
            r"<<<<<<< HEAD:([a-fA-F0-9]{8,})\n(?s:.*?)\n=======\n(?s:.*?)\n>>>>>>> ([a-fA-F0-9]{8,})"
        ).ok()?;

        let caps = re.captures(text)?;
        Some(EditBlock {
            file_path: String::new(), // caller must fill in
            old_anchor: caps.get(1)?.as_str().to_string(),
            new_anchor: caps.get(4)?.as_str().to_string(),
            old_lines: caps
                .get(2)?
                .as_str()
                .trim_end()
                .lines()
                .map(|s| s.to_string())
                .collect(),
            new_lines: caps
                .get(3)?
                .as_str()
                .trim_end()
                .lines()
                .map(|s| s.to_string())
                .collect(),
            context_above: 0,
            context_below: 0,
        })
    }

    fn parse_anchor_only_json(&self, text: &str, file_contents: &str) -> Option<EditBlock> {
        let value: Value = serde_json::from_str(text.trim()).ok()?;

        let old_anchor = value.get("old_anchor")?.as_str()?.to_string();
        let new_lines: Vec<String> = value
            .get("new_lines")?
            .as_array()?
            .iter()
            .map(|v| v.as_str().unwrap_or("").to_string())
            .collect();

        let old_lines = Self::reconstruct_old_lines(&old_anchor, file_contents)?;

        let file_path = value
            .get("file_path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        Some(EditBlock {
            file_path,
            old_anchor: old_anchor.clone(),
            new_anchor: old_anchor,
            old_lines,
            new_lines,
            context_above: 0,
            context_below: 0,
        })
    }

    fn edit_block_from_json(value: &Value) -> Option<EditBlock> {
        let file_path = value.get("file_path")?.as_str()?.to_string();
        let old_anchor = value.get("old_anchor")?.as_str()?.to_string();
        let new_anchor = value
            .get("new_anchor")
            .or_else(|| value.get("old_anchor"))?
            .as_str()?
            .to_string();

        let old_lines = value.get("old_lines")?.as_array()?
            .iter()
            .map(|v| v.as_str().unwrap_or("").to_string())
            .collect();

        let new_lines = value
            .get("new_lines")
            .or_else(|| value.get("new_content").or_else(|| value.get("content")))?
            .as_array()?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(String::from)
                    .unwrap_or_else(|| v.to_string())
            })
            .collect();

        Some(EditBlock {
            file_path,
            old_anchor,
            new_anchor,
            old_lines,
            new_lines,
            context_above: 0,
            context_below: 0,
        })
    }

    /// Reconstruct `old_lines` by scanning the file for a context block whose
    /// hash prefix matches `anchor`. Tries windows of 1–10 lines.
    fn reconstruct_old_lines(anchor: &str, content: &str) -> Option<Vec<String>> {
        use crate::diff::format::compute_context_hash;

        let lines: Vec<&str> = content.lines().collect();
        for window_size in 1..=10 {
            for start in 0..=lines.len().saturating_sub(window_size) {
                let window: Vec<String> = lines[start..start + window_size]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                let candidate = compute_context_hash("", &window);
                // Both anchor and candidate are SHA-256 hex prefixes (typically 8 chars).
                // Accept only exact-length or longer candidate matching the anchor prefix.
                if candidate.len() >= anchor.len() && candidate.starts_with(anchor) {
                    return Some(window);
                }
            }
        }
        None // anchor not found — caller must handle
    }
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// Generic LLM client that dispatches to OpenAI or Anthropic.
#[derive(Clone)]
pub struct LlmClient {
    http: Client,
    config: ClientConfig,
}

impl LlmClient {
    /// Create a new client from configuration.
    pub fn new(config: ClientConfig) -> Self {
        Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(config.timeout_secs))
                .build()
                .expect("valid HTTP client"),
            config,
        }
    }

    /// Create a client from environment variables (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`).
    pub fn from_env() -> Self {
        let provider = if std::env::var("OPENAI_API_KEY").is_ok() {
            "openai".to_string()
        } else if std::env::var("ANTHROPIC_API_KEY").is_ok() {
            "anthropic".to_string()
        } else {
            "openai".to_string()
        };

        let api_key = std::env::var("OPENAI_API_KEY")
            .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
            .ok();

        Self::new(ClientConfig {
            provider,
            api_key,
            ..Default::default()
        })
    }

    /// Send a messages request and return a structured response.
    pub async fn complete(&self, messages: &[Message]) -> Result<LlmResponse, ClientError> {
        match self.config.provider.as_str() {
            "anthropic" => self.complete_anthropic(messages).await,
            _ => self.complete_openai(messages).await,
        }
    }

    /// Send a request expecting a structured `EditBlock` in the response.
    pub async fn complete_edit_block(
        &self,
        messages: &[Message],
        file_contents: &str,
    ) -> Result<EditBlock, ClientError> {
        let response = self.complete(messages).await?;
        response
            .parse_edit_block(file_contents)
            .ok_or_else(|| {
                ClientError::ParseError(format!(
                    "Failed to parse EditBlock from response: {}",
                    response.text.chars().take(200).collect::<String>()
                ))
            })
    }

    async fn complete_openai(&self, messages: &[Message]) -> Result<LlmResponse, ClientError> {
        let base_url = self
            .config
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string());

        let url = format!("{}/chat/completions", base_url);

        #[derive(Serialize)]
        struct Request<'a> {
            model: &'a str,
            messages: &'a [Message],
            #[serde(rename = "response_format")]
            response_format: Option<ResponseFormat>,
        }

        #[derive(Serialize)]
        struct ResponseFormat {
            #[serde(rename = "type")]
            format_type: String,
        }

        let request = Request {
            model: &self.config.model,
            messages,
            response_format: Some(ResponseFormat {
                format_type: "json_object".to_string(),
            }),
        };

        let mut req_builder = self.http.post(&url).json(&request);

        if let Some(key) = &self.config.api_key {
            req_builder = req_builder.header("Authorization", format!("Bearer {}", key));
        }

        let resp = req_builder.send().await?;
        let status = resp.status();

        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::HttpError(status.as_u16(), body));
        }

        #[derive(Deserialize)]
        struct OpenAiResponse {
            choices: Vec<Choice>,
            model: String,
            usage: Option<UsageResponse>,
        }

        #[derive(Deserialize)]
        struct Choice {
            message: MessageField,
            finish_reason: Option<String>,
        }

        #[derive(Deserialize)]
        struct MessageField {
            content: Option<String>,
        }

        #[derive(Deserialize)]
        struct UsageResponse {
            prompt_tokens: usize,
            completion_tokens: usize,
        }

        let out: OpenAiResponse = resp.json().await?;

        let choice = out
            .choices
            .first()
            .ok_or_else(|| ClientError::ParseError("No choices in response".into()))?;

        let text = choice
            .message
            .content
            .as_ref()
            .ok_or_else(|| ClientError::ParseError("No content in message".into()))?
            .to_string();

        Ok(LlmResponse {
            text,
            model: out.model,
            usage: out.usage.map(|u| Usage {
                input_tokens: u.prompt_tokens,
                output_tokens: u.completion_tokens,
            }),
            finish_reason: choice.finish_reason.clone(),
        })
    }

    async fn complete_anthropic(&self, messages: &[Message]) -> Result<LlmResponse, ClientError> {
        let base_url = self
            .config
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.anthropic.com/v1".to_string());

        let url = format!("{}/messages", base_url);

        // Anthropic uses a top-level `system` field for system instructions.
        // We extract it from the messages so it doesn't appear twice.
        let system_content: Option<String> = messages
            .iter()
            .find(|m| m.role == "system")
            .and_then(|m| match &m.content {
                MessageContent::Text(t) => Some(t.clone()),
                MessageContent::Parts(parts) => {
                    Some(parts.iter().filter_map(|p| p.text.clone()).collect::<Vec<_>>().join("\n"))
                }
            });

        let non_system: Vec<&Message> = messages
            .iter()
            .filter(|m| m.role != "system")
            .collect();

        let anthropic_messages: Vec<serde_json::Value> = non_system
            .iter()
            .map(|msg| {
                let role = match msg.role.as_str() {
                    "assistant" => "assistant",
                    _ => "user",
                };
                let content = match &msg.content {
                    MessageContent::Text(t) => t.clone(),
                    MessageContent::Parts(parts) => {
                        parts.iter().filter_map(|p| p.text.clone()).collect::<Vec<_>>().join("\n")
                    }
                };
                serde_json::json!({ "role": role, "content": content })
            })
            .collect();

        let mut request_body = serde_json::json!({
            "model": self.config.model,
            "messages": anthropic_messages,
            "max_tokens": 4096,
        });

        if let Some(sys) = system_content {
            request_body["system"] = serde_json::json!(sys);
        }

        let mut req_builder = self.http.post(&url).json(&request_body);

        if let Some(key) = &self.config.api_key {
            req_builder = req_builder
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01");
        }

        let resp = req_builder.send().await?;
        let status = resp.status();

        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::HttpError(status.as_u16(), body));
        }

        #[derive(Deserialize)]
        struct AnthropicResponse {
            content: Vec<ContentBlock>,
            model: String,
            usage: Option<UsageAnthropic>,
            stop_reason: Option<String>,
        }

        #[derive(Deserialize)]
        struct ContentBlock {
            text: Option<String>,
        }

        #[derive(Deserialize)]
        struct UsageAnthropic {
            input_tokens: usize,
            output_tokens: usize,
        }

        let out: AnthropicResponse = resp.json().await?;

        let text = out
            .content
            .first()
            .and_then(|c| c.text.clone())
            .unwrap_or_default();

        Ok(LlmResponse {
            text,
            model: out.model,
            usage: out.usage.map(|u| Usage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
            }),
            finish_reason: out.stop_reason,
        })
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum ClientError {
    HttpError(u16, String),
    ParseError(String),
    NetworkError(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::HttpError(code, body) => write!(f, "HTTP {}: {}", code, body),
            ClientError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            ClientError::NetworkError(msg) => write!(f, "Network error: {}", msg),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<reqwest::Error> for ClientError {
    fn from(e: reqwest::Error) -> Self {
        ClientError::NetworkError(e.to_string())
    }
}