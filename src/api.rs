use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const BASE_URL: &str = "https://gmail.googleapis.com/gmail/v1";
const MIN_REQUEST_INTERVAL: Duration = Duration::from_millis(100);
const SEND_DRAFT_ENDPOINT: &str = "/users/me/drafts/send";

pub struct Client {
    http: reqwest::Client,
    access_token: String,
    last_request: Mutex<Option<Instant>>,
}

#[derive(Debug, Deserialize)]
pub struct MessageList {
    pub messages: Option<Vec<MessageRef>>,
}

#[derive(Debug, Deserialize)]
pub struct LabelList {
    pub labels: Option<Vec<Label>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Label {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub label_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MessageRef {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct Message {
    pub id: String,
    #[serde(rename = "threadId")]
    pub thread_id: Option<String>,
    pub snippet: Option<String>,
    pub payload: Option<Payload>,
    #[serde(rename = "labelIds")]
    pub label_ids: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct DraftMessage {
    pub raw: String,
    #[serde(rename = "threadId")]
    pub thread_id: String,
}

#[derive(Debug, Serialize)]
pub struct CreateDraftRequest {
    pub message: DraftMessage,
}

#[derive(Debug, Serialize)]
pub struct SendDraftRequest {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct Draft {
    pub id: String,
    pub message: Option<MessageRef>,
}

#[derive(Debug, Deserialize)]
pub struct Payload {
    pub headers: Option<Vec<Header>>,
    pub body: Option<Body>,
    pub parts: Option<Vec<Part>>,
}

#[derive(Debug, Deserialize)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Deserialize)]
pub struct Body {
    pub data: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Part {
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    pub body: Option<Body>,
    pub parts: Option<Vec<Part>>,
}

impl Client {
    pub fn new(access_token: &str) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("Failed to build HTTP client"),
            access_token: access_token.to_string(),
            last_request: Mutex::new(None),
        }
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    async fn rate_limit(&self) {
        let wait_duration = {
            let mut last = self.last_request.lock().unwrap();
            let now = Instant::now();
            let wait = last
                .map(|t| MIN_REQUEST_INTERVAL.saturating_sub(now.duration_since(t)))
                .unwrap_or(Duration::ZERO);
            *last = Some(now + wait);
            wait
        };
        if !wait_duration.is_zero() {
            tokio::time::sleep(wait_duration).await;
        }
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    async fn check_response(resp: reqwest::Response) -> Result<reqwest::Response> {
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("HTTP {} - {}", status, body);
        }
        Ok(resp)
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    async fn send(&self, method: reqwest::Method, endpoint: &str) -> Result<reqwest::Response> {
        self.rate_limit().await;
        let url = format!("{}{}", BASE_URL, endpoint);

        let mut req = self
            .http
            .request(method, &url)
            .bearer_auth(&self.access_token);
        req = req.header("Content-Length", "0");
        let resp = req.send().await.context("Failed to send request")?;
        Self::check_response(resp).await
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    async fn get<T: serde::de::DeserializeOwned>(&self, endpoint: &str) -> Result<T> {
        self.send(reqwest::Method::GET, endpoint)
            .await?
            .json()
            .await
            .context("Failed to parse JSON response")
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    async fn post(&self, endpoint: &str) -> Result<()> {
        self.send(reqwest::Method::POST, endpoint).await?;
        Ok(())
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    async fn post_json<T: Serialize>(&self, endpoint: &str, body: &T) -> Result<()> {
        self.rate_limit().await;
        let url = format!("{}{}", BASE_URL, endpoint);

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.access_token)
            .json(body)
            .send()
            .await
            .context("Failed to send request")?;

        Self::check_response(resp).await?;
        Ok(())
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    async fn post_json_with_response<T: Serialize, R: serde::de::DeserializeOwned>(
        &self,
        endpoint: &str,
        body: &T,
    ) -> Result<R> {
        self.rate_limit().await;
        let url = format!("{}{}", BASE_URL, endpoint);

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.access_token)
            .json(body)
            .send()
            .await
            .context("Failed to send request")?;

        let resp = Self::check_response(resp).await?;
        resp.json().await.context("Failed to parse JSON response")
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn list_labels(&self) -> Result<LabelList> {
        self.get("/users/me/labels").await
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn create_label(&self, name: &str) -> Result<Label> {
        // Capitalize first letter for consistency
        let capitalized = capitalize_first(name);
        let body = serde_json::json!({
            "name": capitalized,
            "labelListVisibility": "labelShow",
            "messageListVisibility": "show"
        });
        self.post_json_with_response("/users/me/labels", &body)
            .await
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn get_or_create_label(&self, name: &str) -> Result<String> {
        // Check if label already exists (case-insensitive, Gmail is case-insensitive)
        let labels = self.list_labels().await?;
        if let Some(existing) = labels.labels {
            for label in existing {
                if label.name.eq_ignore_ascii_case(name) {
                    return Ok(label.id);
                }
            }
        }
        // Create new label
        let label = self.create_label(name).await?;
        Ok(label.id)
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn list_messages(
        &self,
        query: Option<&str>,
        label: &str,
        max_results: u32,
    ) -> Result<MessageList> {
        let mut endpoint = format!("/users/me/messages?maxResults={}", max_results);
        if !label.is_empty() {
            endpoint.push_str(&format!("&labelIds={}", urlencoding::encode(label)));
        }
        if let Some(q) = query {
            endpoint.push_str(&format!("&q={}", urlencoding::encode(q)));
        }
        self.get(&endpoint).await
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn get_message(&self, id: &str) -> Result<Message> {
        self.get(&format!("/users/me/messages/{}", urlencoding::encode(id)))
            .await
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn create_draft(&self, raw: &str, thread_id: &str) -> Result<Draft> {
        let body = CreateDraftRequest {
            message: DraftMessage {
                raw: raw.to_string(),
                thread_id: thread_id.to_string(),
            },
        };
        self.post_json_with_response("/users/me/drafts", &body)
            .await
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn send_draft(&self, id: &str) -> Result<Message> {
        let body = SendDraftRequest { id: id.to_string() };
        self.post_json_with_response(SEND_DRAFT_ENDPOINT, &body)
            .await
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn modify_labels(&self, id: &str, add: &[&str], remove: &[&str]) -> Result<()> {
        let endpoint = format!("/users/me/messages/{}/modify", urlencoding::encode(id));
        let body = serde_json::json!({
            "addLabelIds": add,
            "removeLabelIds": remove
        });
        self.post_json(&endpoint, &body).await
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn archive(&self, id: &str) -> Result<()> {
        self.modify_labels(id, &[], &["INBOX"]).await
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn mark_spam(&self, id: &str) -> Result<()> {
        self.modify_labels(id, &["SPAM"], &["INBOX"]).await
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn unspam(&self, id: &str) -> Result<()> {
        self.modify_labels(id, &["INBOX"], &["SPAM"]).await
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn mark_read(&self, id: &str) -> Result<()> {
        self.modify_labels(id, &[], &["UNREAD"]).await
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn mark_unread(&self, id: &str) -> Result<()> {
        self.modify_labels(id, &["UNREAD"], &[]).await
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn clear_labels(&self, id: &str) -> Result<Vec<String>> {
        let msg = self.get_message(id).await?;
        let labels = msg.label_ids.unwrap_or_default();
        let user_labels: Vec<&str> = labels
            .iter()
            .filter(|l| !is_system_label(l))
            .map(|s| s.as_str())
            .collect();
        if !user_labels.is_empty() {
            self.modify_labels(id, &[], &user_labels).await?;
        }
        Ok(user_labels.into_iter().map(|s| s.to_string()).collect())
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn add_label(&self, id: &str, label: &str) -> Result<()> {
        // For custom labels, we need to get/create the label ID first
        let label_id = if is_system_label(label) {
            label.to_string()
        } else {
            self.get_or_create_label(label).await?
        };
        self.modify_labels(id, &[&label_id], &[]).await
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn remove_label(&self, id: &str, label: &str) -> Result<()> {
        // For custom labels, we need to find the label ID first
        let label_id = if is_system_label(label) {
            label.to_string()
        } else {
            self.find_label(label)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Label not found: {}", label))?
        };
        self.modify_labels(id, &[], &[&label_id]).await
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    async fn find_label(&self, name: &str) -> Result<Option<String>> {
        let labels = self.list_labels().await?;
        if let Some(label_list) = labels.labels {
            for label in label_list {
                if label.name.eq_ignore_ascii_case(name) {
                    return Ok(Some(label.id));
                }
            }
        }
        Ok(None)
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn trash(&self, id: &str) -> Result<()> {
        self.post(&format!(
            "/users/me/messages/{}/trash",
            urlencoding::encode(id)
        ))
        .await
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn untrash(&self, id: &str) -> Result<()> {
        self.post(&format!(
            "/users/me/messages/{}/untrash",
            urlencoding::encode(id)
        ))
        .await
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn unsubscribe(&self, id: &str) -> Result<()> {
        self.post(&format!(
            "/users/me/messages/{}/unsubscribe",
            urlencoding::encode(id)
        ))
        .await
    }
}

impl Message {
    pub fn get_header(&self, name: &str) -> Option<&str> {
        self.payload
            .as_ref()?
            .headers
            .as_ref()?
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
    }

    pub fn get_body_text(&self) -> Option<String> {
        let payload = self.payload.as_ref()?;

        // Try direct body first
        if let Some(body) = &payload.body {
            if let Some(data) = &body.data {
                if let Some(decoded) = decode_base64(data) {
                    return String::from_utf8(decoded).ok();
                }
            }
        }

        // Try parts - prefer text/plain, fallback to text/html
        if let Some(parts) = &payload.parts {
            if let Some(text) = find_text_part(parts, "text/plain") {
                return Some(text);
            }
            if let Some(html) = find_text_part(parts, "text/html") {
                return Some(html_to_text(&html));
            }
        }

        None
    }

    pub fn get_body_html(&self) -> Option<String> {
        let payload = self.payload.as_ref()?;
        payload
            .body
            .as_ref()
            .and_then(|body| body.data.as_deref())
            .and_then(decode_body_data)
            .filter(|text| looks_like_html(text))
            .or_else(|| {
                payload
                    .parts
                    .as_deref()
                    .and_then(|parts| find_text_part(parts, "text/html"))
            })
    }
}

fn find_text_part(parts: &[Part], mime_type: &str) -> Option<String> {
    parts.iter().find_map(|part| {
        decode_part_for_mime(part, mime_type).or_else(|| {
            part.parts
                .as_deref()
                .and_then(|nested| find_text_part(nested, mime_type))
        })
    })
}

fn decode_part_for_mime(part: &Part, mime_type: &str) -> Option<String> {
    if part.mime_type != mime_type {
        return None;
    }

    let data = part.body.as_ref()?.data.as_deref()?;
    decode_body_data(data)
}

fn decode_body_data(data: &str) -> Option<String> {
    let decoded = decode_base64(data)?;
    String::from_utf8(decoded).ok()
}

fn looks_like_html(text: &str) -> bool {
    text.contains("<html") || text.contains("<body") || text.contains("<div")
}

fn decode_base64(data: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    use base64::engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD};

    // Try URL-safe without padding first (Gmail's format)
    if let Ok(decoded) = URL_SAFE_NO_PAD.decode(data) {
        return Some(decoded);
    }

    // Try URL-safe with padding
    if let Ok(decoded) = URL_SAFE.decode(data) {
        return Some(decoded);
    }

    // Try standard base64
    if let Ok(decoded) = STANDARD.decode(data) {
        return Some(decoded);
    }

    // Try with manually added padding
    let padded = match data.len() % 4 {
        2 => format!("{}==", data),
        3 => format!("{}=", data),
        _ => data.to_string(),
    };
    URL_SAFE.decode(&padded).ok()
}

fn html_to_text(html: &str) -> String {
    let text = replace_links_with_markdown(html);
    let text = replace_images_with_alt_text(&text);
    let text = remove_images_without_alt(&text);
    let text = replace_html_line_breaks(&text);
    let text = replace_block_tags_with_newlines(&text);
    let text = strip_remaining_html_tags(&text);
    let text = decode_common_entities(&text);
    let text = collapse_multiple_newlines(&text);
    trim_text_lines(&text)
}

fn replace_links_with_markdown(html: &str) -> String {
    use regex::Regex;
    let link_re = Regex::new(r#"<a[^>]*href="([^"]*)"[^>]*>([^<]*)</a>"#).unwrap();
    link_re.replace_all(html, "[$2]($1)").to_string()
}

fn replace_images_with_alt_text(html: &str) -> String {
    use regex::Regex;
    let img_re = Regex::new(r#"<img[^>]*alt="([^"]*)"[^>]*/?\s*>"#).unwrap();
    img_re.replace_all(html, "[$1]").to_string()
}

fn remove_images_without_alt(html: &str) -> String {
    use regex::Regex;
    let img_no_alt_re = Regex::new(r#"<img[^>]*/?\s*>"#).unwrap();
    img_no_alt_re.replace_all(html, "").to_string()
}

fn replace_html_line_breaks(html: &str) -> String {
    html.replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
}

fn replace_block_tags_with_newlines(html: &str) -> String {
    use regex::Regex;
    let block_re = Regex::new(r"</?(p|div|tr|table|td|th|li|ul|ol|h[1-6])[^>]*>").unwrap();
    block_re.replace_all(html, "\n").to_string()
}

fn strip_remaining_html_tags(html: &str) -> String {
    use regex::Regex;
    let tag_re = Regex::new(r"<[^>]+>").unwrap();
    tag_re.replace_all(html, "").to_string()
}

fn decode_common_entities(html: &str) -> String {
    html.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&copy;", "©")
}

fn collapse_multiple_newlines(html: &str) -> String {
    use regex::Regex;
    let newlines_re = Regex::new(r"\n{3,}").unwrap();
    newlines_re.replace_all(html, "\n\n").to_string()
}

fn trim_text_lines(text: &str) -> String {
    text.lines()
        .map(|line| line.trim())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().chain(chars).collect(),
    }
}

fn is_system_label(label: &str) -> bool {
    matches!(
        label,
        "INBOX"
            | "SENT"
            | "DRAFT"
            | "TRASH"
            | "SPAM"
            | "STARRED"
            | "IMPORTANT"
            | "UNREAD"
            | "CATEGORY_PERSONAL"
            | "CATEGORY_SOCIAL"
            | "CATEGORY_PROMOTIONS"
            | "CATEGORY_UPDATES"
            | "CATEGORY_FORUMS"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    fn make_message(payload: Option<Payload>) -> Message {
        Message {
            id: "test123".to_string(),
            thread_id: None,
            snippet: Some("snippet".to_string()),
            payload,
            label_ids: None,
        }
    }

    fn make_body(text: &str) -> Body {
        Body {
            data: Some(URL_SAFE_NO_PAD.encode(text)),
        }
    }

    #[test]
    fn serializes_send_draft_request_with_only_the_draft_id() {
        let request = SendDraftRequest {
            id: "r-123".to_string(),
        };

        let json = serde_json::to_value(request).expect("send request should serialize");

        assert_eq!(json, serde_json::json!({"id": "r-123"}));
    }

    #[test]
    fn send_draft_uses_the_official_fixed_endpoint() {
        assert_eq!(SEND_DRAFT_ENDPOINT, "/users/me/drafts/send");
    }

    #[test]
    fn deserializes_sent_message_response_and_rejects_malformed_response() {
        let message: Message = serde_json::from_str(
            r#"{"id":"sent-123","threadId":"thread-123","labelIds":["SENT"]}"#,
        )
        .expect("sent message response should deserialize");

        assert_eq!(message.id, "sent-123");
        assert_eq!(message.thread_id.as_deref(), Some("thread-123"));
        assert_eq!(message.label_ids, Some(vec!["SENT".to_string()]));
        assert!(serde_json::from_str::<Message>(r#"{"threadId":"thread-123"}"#).is_err());
    }

    #[test]
    fn test_get_header() {
        let msg = make_message(Some(Payload {
            headers: Some(vec![
                Header {
                    name: "From".to_string(),
                    value: "test@example.com".to_string(),
                },
                Header {
                    name: "Subject".to_string(),
                    value: "Hello".to_string(),
                },
            ]),
            body: None,
            parts: None,
        }));

        assert_eq!(msg.get_header("From"), Some("test@example.com"));
        assert_eq!(msg.get_header("from"), Some("test@example.com")); // case insensitive
        assert_eq!(msg.get_header("Subject"), Some("Hello"));
        assert_eq!(msg.get_header("X-Missing"), None);
    }

    #[test]
    fn test_get_header_no_payload() {
        let msg = make_message(None);
        assert_eq!(msg.get_header("From"), None);
    }

    #[test]
    fn test_get_body_text_direct() {
        let msg = make_message(Some(Payload {
            headers: None,
            body: Some(make_body("Hello world")),
            parts: None,
        }));

        assert_eq!(msg.get_body_text(), Some("Hello world".to_string()));
    }

    #[test]
    fn test_get_body_text_from_parts() {
        let msg = make_message(Some(Payload {
            headers: None,
            body: None,
            parts: Some(vec![
                Part {
                    mime_type: "text/html".to_string(),
                    body: Some(make_body("<b>HTML</b>")),
                    parts: None,
                },
                Part {
                    mime_type: "text/plain".to_string(),
                    body: Some(make_body("Plain text")),
                    parts: None,
                },
            ]),
        }));

        assert_eq!(msg.get_body_text(), Some("Plain text".to_string()));
    }

    #[test]
    fn test_get_body_text_nested_parts() {
        let msg = make_message(Some(Payload {
            headers: None,
            body: None,
            parts: Some(vec![Part {
                mime_type: "multipart/alternative".to_string(),
                body: None,
                parts: Some(vec![Part {
                    mime_type: "text/plain".to_string(),
                    body: Some(make_body("Nested text")),
                    parts: None,
                }]),
            }]),
        }));

        assert_eq!(msg.get_body_text(), Some("Nested text".to_string()));
    }

    #[test]
    fn test_get_body_text_no_body() {
        let msg = make_message(Some(Payload {
            headers: None,
            body: None,
            parts: None,
        }));

        assert_eq!(msg.get_body_text(), None);
    }

    #[test]
    fn test_get_body_html_prefers_direct_html_and_parts() {
        let direct = make_message(Some(Payload {
            headers: None,
            body: Some(make_body("<html><body>Hello</body></html>")),
            parts: None,
        }));
        let part = make_message(Some(Payload {
            headers: None,
            body: None,
            parts: Some(vec![Part {
                mime_type: "text/html".to_string(),
                body: Some(make_body("<div>Part</div>")),
                parts: None,
            }]),
        }));

        assert_eq!(
            direct.get_body_html().as_deref(),
            Some("<html><body>Hello</body></html>")
        );
        assert_eq!(part.get_body_html().as_deref(), Some("<div>Part</div>"));
    }

    #[test]
    fn test_html_to_text_converts_common_elements() {
        let html = r#"<p>Hello&nbsp;<a href="https://example.com">site</a><br><img alt="logo"> &amp; more</p>"#;

        let text = html_to_text(html);

        assert!(text.contains("Hello"));
        assert!(text.contains("[site](https://example.com)"));
        assert!(text.contains("[logo]"));
        assert!(text.contains("& more"));
    }

    #[test]
    fn test_decode_base64_accepts_url_safe_and_standard() {
        let url_safe = URL_SAFE_NO_PAD.encode("hello");
        let standard = base64::engine::general_purpose::STANDARD.encode("hello");

        assert_eq!(decode_base64(&url_safe).unwrap(), b"hello");
        assert_eq!(decode_base64(&standard).unwrap(), b"hello");
        assert!(decode_base64("%%%").is_none());
    }

    #[test]
    fn test_text_helpers_trim_and_capitalize() {
        assert_eq!(trim_text_lines("  one  \n\n two "), "one\n\ntwo");
        assert_eq!(collapse_multiple_newlines("a\n\n\nb"), "a\n\nb");
        assert_eq!(capitalize_first("hello"), "Hello");
        assert_eq!(capitalize_first(""), "");
    }

    #[test]
    fn test_system_label_detection() {
        assert!(is_system_label("INBOX"));
        assert!(is_system_label("CATEGORY_PROMOTIONS"));
        assert!(!is_system_label("Receipts"));
    }
}
