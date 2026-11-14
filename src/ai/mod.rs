//! Optional language-model suggestions: playlists from a sentence, what to hear next and artists
//! that might fit. Off unless `AUTOREVERSE_AI_PROVIDER` is set.
//!
//! Only names from the library leave the server. Every id a model answers with is checked against
//! what was offered, so an invented entry never reaches a client.

pub mod feed;
pub mod music;
pub mod musicbrainz;

use std::fmt;
use std::time::Duration;

use serde_json::{Value, json};

const ANTHROPIC_URL: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2026-10-21";
const ANTHROPIC_DEFAULT_MODEL: &str = "claude-haiku-5-5";
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
/// Models whose declined requests the API can re-run on another model.
const MODELS_WITH_FALLBACK: [&str; 4] = [
    "claude-opus-5-5",
    "claude-opus-5",
    "claude-sonnet-5-5",
    "claude-fable-5-1",
];
const OLLAMA_URL: &str = "http://localhost:11434";
const OLLAMA_OUTPUT_TOKENS: usize = 4096;
/// Long enough for a local model on modest hardware.
const TIMEOUT: Duration = Duration::from_secs(300);
const MAX_OUTPUT_TOKENS: u32 = 16_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    Anthropic,
    Ollama,
}

impl Provider {
    pub fn wire(self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::Ollama => "ollama",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct AiConfig {
    pub provider: Provider,
    pub model: String,
    pub base_url: String,
    api_key: Option<String>,
}

/// By hand, so that a logged configuration never contains the key.
impl fmt::Debug for AiConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AiConfig")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

impl AiConfig {
    /// `None` when no provider is set. A half-finished setup stops the server instead of failing
    /// on the first click.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Option<Self>, String> {
        let value = |key: &str| lookup(key).filter(|v| !v.trim().is_empty());
        let Some(provider) = value("AUTOREVERSE_AI_PROVIDER") else {
            return Ok(None);
        };
        let url = value("AUTOREVERSE_AI_URL").map(|u| u.trim_end_matches('/').to_owned());
        let model = value("AUTOREVERSE_AI_MODEL");
        match provider.trim().to_lowercase().as_str() {
            "anthropic" => {
                let api_key = value("AUTOREVERSE_AI_API_KEY")
                    .ok_or("AUTOREVERSE_AI_API_KEY fehlt für den Anbieter anthropic")?;
                Ok(Some(AiConfig {
                    provider: Provider::Anthropic,
                    model: model.unwrap_or_else(|| ANTHROPIC_DEFAULT_MODEL.into()),
                    base_url: url.unwrap_or_else(|| ANTHROPIC_URL.into()),
                    api_key: Some(api_key),
                }))
            }
            "ollama" => Ok(Some(AiConfig {
                provider: Provider::Ollama,
                model: model.ok_or("AUTOREVERSE_AI_MODEL fehlt für den Anbieter ollama")?,
                base_url: url.unwrap_or_else(|| OLLAMA_URL.into()),
                api_key: None,
            })),
            other => Err(format!("AUTOREVERSE_AI_PROVIDER unbekannt: {other}")),
        }
    }

    /// Sends one request and returns the JSON object the model produced under `schema`.
    pub async fn generate(
        &self,
        http: &reqwest::Client,
        system: &str,
        user: &str,
        schema: &Value,
    ) -> Result<Value, String> {
        let (path, body) = match self.provider {
            Provider::Anthropic => ("/v1/messages", anthropic_body(self, system, user, schema)),
            Provider::Ollama => ("/api/chat", ollama_body(self, system, user, schema)),
        };
        let mut request = http
            .post(format!("{}{path}", self.base_url))
            .timeout(TIMEOUT)
            .header("content-type", "application/json");
        if self.provider == Provider::Anthropic {
            request = request
                .header("x-api-key", self.api_key.as_deref().unwrap_or_default())
                .header("anthropic-version", ANTHROPIC_VERSION);
            if MODELS_WITH_FALLBACK.contains(&self.model.as_str()) {
                request = request.header("anthropic-beta", FALLBACK_BETA);
            }
        }
        let response = request
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| format!("nicht erreichbar: {e}"))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| format!("Antwort abgebrochen: {e}"))?;
        if !status.is_success() {
            let excerpt: String = text.chars().take(300).collect();
            return Err(format!("HTTP {status}: {excerpt}"));
        }
        let answer: Value =
            serde_json::from_str(&text).map_err(|e| format!("kein JSON als Antwort: {e}"))?;
        match self.provider {
            Provider::Anthropic => parse_anthropic(&answer),
            Provider::Ollama => parse_ollama(&answer),
        }
    }
}

fn anthropic_body(config: &AiConfig, system: &str, user: &str, schema: &Value) -> Value {
    let mut body = json!({
        "model": config.model,
        "max_tokens": MAX_OUTPUT_TOKENS,
        "system": system,
        "messages": [{"role": "user", "content": user}],
        "output_config": {"format": {"type": "json_schema", "schema": schema}},
    });
    if MODELS_WITH_FALLBACK.contains(&config.model.as_str()) {
        body["fallbacks"] = json!("default");
        body["output_config"]["effort"] = json!("low");
    }
    body
}

/// Ollama's default window is small and it cuts longer prompts without an error, so a large
/// catalog would silently lose albums. Three characters per token is a cautious estimate.
fn context_window(system: &str, user: &str) -> usize {
    (system.len() + user.len()) / 3 + OLLAMA_OUTPUT_TOKENS
}

fn ollama_body(config: &AiConfig, system: &str, user: &str, schema: &Value) -> Value {
    json!({
        "model": config.model,
        "stream": false,
        "format": schema,
        "options": {"num_ctx": context_window(system, user)},
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
    })
}

fn parse_anthropic(answer: &Value) -> Result<Value, String> {
    match answer["stop_reason"].as_str() {
        Some("refusal") => return Err("Anfrage abgelehnt".into()),
        Some("max_tokens") => return Err("Antwort abgeschnitten".into()),
        _ => {}
    }
    let text = answer["content"]
        .as_array()
        .and_then(|blocks| blocks.iter().find(|b| b["type"] == "text"))
        .and_then(|b| b["text"].as_str())
        .ok_or("Antwort ohne Text")?;
    serde_json::from_str(text).map_err(|e| format!("Antwort passt nicht zum Schema: {e}"))
}

fn parse_ollama(answer: &Value) -> Result<Value, String> {
    let text = answer["message"]["content"]
        .as_str()
        .ok_or("Antwort ohne Text")?;
    serde_json::from_str(text).map_err(|e| format!("Antwort passt nicht zum Schema: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn config(pairs: &[(&str, &str)]) -> Result<Option<AiConfig>, String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        AiConfig::from_lookup(|key| map.get(key).cloned())
    }

    #[test]
    fn stays_off_without_a_provider() {
        assert_eq!(config(&[]).unwrap(), None);
    }

    #[test]
    fn refuses_incomplete_setups() {
        assert!(config(&[("AUTOREVERSE_AI_PROVIDER", "anthropic")]).is_err());
        assert!(config(&[("AUTOREVERSE_AI_PROVIDER", "ollama")]).is_err());
        assert!(config(&[("AUTOREVERSE_AI_PROVIDER", "other")]).is_err());
    }

    #[test]
    fn debug_output_hides_the_key() {
        let cfg = config(&[
            ("AUTOREVERSE_AI_PROVIDER", "anthropic"),
            ("AUTOREVERSE_AI_API_KEY", "geheim"),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(cfg.model, "claude-haiku-5-5");
        assert!(!format!("{cfg:?}").contains("geheim"));
    }

    #[test]
    fn fallback_is_only_sent_to_models_that_have_it() {
        let schema = json!({});
        let mut cfg = config(&[
            ("AUTOREVERSE_AI_PROVIDER", "anthropic"),
            ("AUTOREVERSE_AI_API_KEY", "k"),
        ])
        .unwrap()
        .unwrap();
        assert!(
            anthropic_body(&cfg, "s", "u", &schema)
                .get("fallbacks")
                .is_none()
        );
        cfg.model = "claude-opus-5-5".into();
        assert_eq!(
            anthropic_body(&cfg, "s", "u", &schema)["fallbacks"],
            "default"
        );
        cfg.model = "claude-haiku-4-5".into();
        assert!(
            anthropic_body(&cfg, "s", "u", &schema)
                .get("fallbacks")
                .is_none()
        );
    }

    #[test]
    fn reads_the_text_block_after_a_fallback_marker() {
        let answer = json!({"stop_reason": "end_turn", "content": [
            {"type": "fallback"}, {"type": "text", "text": "{\"a\":1}"}]});
        assert_eq!(parse_anthropic(&answer).unwrap(), json!({"a": 1}));
    }

    #[test]
    fn reports_refusals_and_cut_off_answers() {
        assert!(parse_anthropic(&json!({"stop_reason": "refusal", "content": []})).is_err());
        assert!(parse_anthropic(&json!({"stop_reason": "max_tokens", "content": []})).is_err());
    }

    #[test]
    fn the_ollama_window_grows_with_the_prompt() {
        let long = "x".repeat(90_000);
        assert_eq!(context_window("", &long), 30_000 + OLLAMA_OUTPUT_TOKENS);
    }

    #[test]
    fn reads_an_ollama_answer() {
        let answer = json!({"message": {"content": "{\"a\":2}"}});
        assert_eq!(parse_ollama(&answer).unwrap(), json!({"a": 2}));
    }
}
