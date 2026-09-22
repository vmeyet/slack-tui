//! Typed judgments from TypeSafe's Jev model. Every failure is `Unavailable`, so callers keep their old behaviour.
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

const API_URL: &str = "https://api.typesafe.ai/v1/systemone";
const MODEL: &str = "jev-latest";
const KEYCHAIN_SERVICE: &str = "typesafe";
const TIMEOUT: Duration = Duration::from_secs(20);
const RETRY_DELAYS: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(4)];
const RETRYABLE_STATUSES: [u16; 6] = [429, 500, 502, 503, 504, 529];
const QUOTA_STATUS: u16 = 402;
const QUOTA_WORDS: [&str; 6] = ["quota", "credit", "billing", "balance", "insufficient", "exceeded"];
const PARALLEL_REQUESTS: usize = 8;

/// Why TypeSafe could not answer: no key, quota, outage, bad request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unavailable(pub String);

impl fmt::Display for Unavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Unavailable {}

impl Unavailable {
    /// The one line shown when a caller falls back.
    pub fn notice(&self) -> String {
        format!("⚠ typesafe unavailable: {}", self.0)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Question {
    /// Yes or no, answered as the probability of yes.
    Noul(&'static str),
    /// One of the named options, each with what it means.
    Choice(&'static str, &'static [(&'static str, &'static str)]),
    /// A place on ordered levels, lowest first.
    Score(&'static str, &'static [&'static str]),
}

impl Question {
    fn to_json(self) -> Value {
        match self {
            Question::Noul(instructions) => json!({"type": "noul", "instructions": instructions}),
            Question::Choice(instructions, options) => {
                let criteria: Map<String, Value> = options.iter().map(|(name, meaning)| ((*name).to_owned(), json!(meaning))).collect();
                json!({"type": "choice", "instructions": instructions, "criteria": criteria})
            }
            Question::Score(instructions, levels) => json!({"type": "score", "instructions": instructions, "criteria": levels}),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
struct Answer {
    noul: Option<f64>,
    choice: Option<String>,
    score: Option<f64>,
}

/// The answers of one request, by question id.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(transparent)]
pub struct Answers(HashMap<String, Answer>);

impl Answers {
    pub fn noul(&self, id: &str) -> Result<f64, Unavailable> {
        self.get(id)?.noul.ok_or_else(|| missing(id))
    }

    pub fn choice(&self, id: &str) -> Result<&str, Unavailable> {
        self.get(id)?.choice.as_deref().ok_or_else(|| missing(id))
    }

    /// The probability-weighted level, from 0 for the first one.
    pub fn score(&self, id: &str) -> Result<f64, Unavailable> {
        self.get(id)?.score.ok_or_else(|| missing(id))
    }

    fn get(&self, id: &str) -> Result<&Answer, Unavailable> {
        self.0.get(id).ok_or_else(|| missing(id))
    }
}

fn missing(id: &str) -> Unavailable {
    Unavailable(format!("no answer for `{id}`"))
}

/// Asks typed questions about a state; the real client, or a stub in tests.
pub trait Judge {
    fn ask(&self, state: &Value, questions: &[(&str, Question)]) -> impl Future<Output = Result<Answers, Unavailable>> + Send;
}

#[derive(Clone, Debug)]
pub struct TypeSafe {
    http: reqwest::Client,
    url: String,
    key: String,
    slots: Arc<Semaphore>,
}

impl TypeSafe {
    /// Fails fast when TypeSafe is switched off or has no key, before anything is sent.
    pub fn connect() -> Result<Self, Unavailable> {
        if let Some(reason) = disabled_reason() {
            return Err(Unavailable(reason.into()));
        }
        Self::new(API_URL, &read_api_key()?)
    }

    fn new(url: &str, key: &str) -> Result<Self, Unavailable> {
        let http = reqwest::Client::builder().timeout(TIMEOUT).build().map_err(|e| Unavailable(e.to_string()))?;
        Ok(Self { http, url: url.to_owned(), key: key.to_owned(), slots: Arc::new(Semaphore::new(PARALLEL_REQUESTS)) })
    }

    async fn post_with_retries(&self, body: &Value) -> Result<Answers, Unavailable> {
        for delay in RETRY_DELAYS {
            match self.post(body).await {
                Err(Failure::Retryable(_)) => tokio::time::sleep(delay).await,
                outcome => return outcome.map_err(Failure::into_unavailable),
            }
        }
        self.post(body).await.map_err(Failure::into_unavailable)
    }

    async fn post(&self, body: &Value) -> Result<Answers, Failure> {
        let response = self
            .http
            .post(&self.url)
            .bearer_auth(&self.key)
            .json(body)
            .send()
            .await
            .map_err(|e| Failure::Retryable(Unavailable(format!("network error: {}", e.without_url()))))?;
        let status = response.status().as_u16();
        if !response.status().is_success() {
            let message = read_error_message(response).await;
            return Err(classify_failure(status, &message));
        }
        let parsed: Response = response.json().await.map_err(|e| Failure::Final(Unavailable(format!("unreadable answer: {e}"))))?;
        Ok(parsed.answers)
    }
}

impl Judge for TypeSafe {
    async fn ask(&self, state: &Value, questions: &[(&str, Question)]) -> Result<Answers, Unavailable> {
        let _slot = self.slots.acquire().await.map_err(|e| Unavailable(e.to_string()))?;
        let questions: Map<String, Value> = questions.iter().map(|(id, q)| ((*id).to_owned(), q.to_json())).collect();
        self.post_with_retries(&json!({"model": MODEL, "state": state, "questions": questions})).await
    }
}

#[derive(Deserialize)]
struct Response {
    answers: Answers,
}

#[derive(Debug, PartialEq)]
enum Failure {
    Retryable(Unavailable),
    Final(Unavailable),
}

impl Failure {
    fn into_unavailable(self) -> Unavailable {
        match self {
            Failure::Retryable(u) | Failure::Final(u) => u,
        }
    }
}

fn classify_failure(status: u16, message: &str) -> Failure {
    let lower = message.to_lowercase();
    if status == QUOTA_STATUS || QUOTA_WORDS.iter().any(|w| lower.contains(w)) {
        return Failure::Final(Unavailable(format!("quota exhausted (HTTP {status}: {message})")));
    }
    if status == 401 {
        return Failure::Final(Unavailable("API key rejected (HTTP 401), check the keychain entry".into()));
    }
    let reason = Unavailable(format!("HTTP {status}: {message}"));
    if RETRYABLE_STATUSES.contains(&status) { Failure::Retryable(reason) } else { Failure::Final(reason) }
}

async fn read_error_message(response: reqwest::Response) -> String {
    let fallback = response.status().canonical_reason().unwrap_or("error").to_owned();
    let Ok(body) = response.json::<Value>().await else { return fallback };
    match &body["detail"] {
        Value::Object(detail) => {
            detail.get("message").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| body["detail"].to_string())
        }
        Value::String(detail) => detail.clone(),
        Value::Null => fallback,
        other => other.to_string(),
    }
}

fn disabled_reason() -> Option<&'static str> {
    if std::env::var("TYPESAFE_DISABLED").as_deref() == Ok("1") {
        return Some("disabled by TYPESAFE_DISABLED=1");
    }
    let flag = dirs::home_dir()?.join(".agents/typesafe/disabled");
    flag.exists().then_some("disabled by `typesafe off`")
}

fn read_api_key() -> Result<String, Unavailable> {
    if let Some(key) = std::env::var("TYPESAFE_API_KEY").ok().filter(|k| !k.is_empty()) {
        return Ok(key);
    }
    let output = std::process::Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", KEYCHAIN_SERVICE, "-w"])
        .output()
        .map_err(|e| Unavailable(format!("keychain unreadable: {e}")))?;
    let key = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !output.status.success() || key.is_empty() {
        return Err(Unavailable(format!("no API key (set TYPESAFE_API_KEY or keychain service '{KEYCHAIN_SERVICE}')")));
    }
    Ok(key)
}

#[cfg(test)]
pub mod stub {
    use super::*;

    /// Answers every request from the state alone, with no network.
    pub struct Stub(pub fn(&Value) -> Result<Value, Unavailable>);

    impl Judge for Stub {
        async fn ask(&self, state: &Value, _questions: &[(&str, Question)]) -> Result<Answers, Unavailable> {
            let raw = (self.0)(state)?;
            Ok(serde_json::from_value(raw).expect("stub answers are valid"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, header, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const QUESTIONS: [(&str, Question); 3] = [
        ("urgent", Question::Noul("Is this urgent?")),
        ("team", Question::Choice("Which team?", &[("ops", "Outages"), ("sales", "Deals")])),
        ("mood", Question::Score("How upset?", &["calm", "angry"])),
    ];

    #[test]
    fn questions_match_the_wire_shape() {
        let [noul, choice, score] = QUESTIONS.map(|(_, q)| q.to_json());
        assert_eq!(noul, json!({"type": "noul", "instructions": "Is this urgent?"}));
        assert_eq!(choice["criteria"], json!({"ops": "Outages", "sales": "Deals"}));
        assert_eq!(score["criteria"], json!(["calm", "angry"]));
    }

    #[test]
    fn failures_split_into_retryable_and_final() {
        assert!(matches!(classify_failure(503, "busy"), Failure::Retryable(_)));
        assert!(matches!(classify_failure(529, "overloaded"), Failure::Retryable(_)));
        let quota = classify_failure(429, "credit balance too low").into_unavailable();
        assert!(quota.0.starts_with("quota exhausted"), "{quota}");
        assert!(classify_failure(402, "pay up").into_unavailable().0.starts_with("quota exhausted"));
        assert!(classify_failure(401, "nope").into_unavailable().0.contains("API key rejected"));
        assert_eq!(classify_failure(400, "bad state"), Failure::Final(Unavailable("HTTP 400: bad state".into())));
    }

    #[test]
    fn a_missing_answer_is_unavailable() {
        let answers: Answers = serde_json::from_value(json!({"urgent": {"type": "noul", "noul": 0.9}})).unwrap();
        assert_eq!(answers.noul("urgent"), Ok(0.9));
        assert_eq!(answers.choice("urgent"), Err(missing("urgent")));
        assert_eq!(answers.score("mood"), Err(missing("mood")));
        assert_eq!(Unavailable("no key".into()).notice(), "⚠ typesafe unavailable: no key");
    }

    #[tokio::test]
    async fn asks_with_the_key_and_reads_typed_answers() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(header("authorization", "Bearer test-key"))
            .and(body_partial_json(json!({"model": MODEL, "state": {"text": "prod is down"}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"answers": {
                "urgent": {"type": "noul", "noul": 0.97},
                "team": {"type": "choice", "choice": "ops", "probabilities": {"ops": 0.9, "sales": 0.1}, "confidence": 0.8},
                "mood": {"type": "score", "score": 0.8, "legend": {"0": "calm", "1": "angry"}, "confidence": 0.7}
            }, "usage": {"input_tokens": 12}})))
            .mount(&server)
            .await;
        let client = TypeSafe::new(&server.uri(), "test-key").unwrap();
        let answers = client.ask(&json!({"text": "prod is down"}), &QUESTIONS).await.unwrap();
        assert_eq!((answers.noul("urgent"), answers.choice("team"), answers.score("mood")), (Ok(0.97), Ok("ops"), Ok(0.8)));
    }

    #[tokio::test]
    async fn quota_errors_are_not_retried() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(402).set_body_json(json!({"detail": {"message": "out of credits"}})))
            .expect(1)
            .mount(&server)
            .await;
        let client = TypeSafe::new(&server.uri(), "test-key").unwrap();
        let error = client.ask(&json!("x"), &QUESTIONS).await.unwrap_err();
        assert_eq!(error, Unavailable("quota exhausted (HTTP 402: out of credits)".into()));
    }
}
