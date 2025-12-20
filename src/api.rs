use anyhow::{Context, Result};
use base64::prelude::*;
use serde::{Deserialize, Serialize};

const BASE_URL: &str = "https://gmail.googleapis.com/gmail/v1";

use std::time::Duration;

pub struct Client {
    http: reqwest::Client,
    access_token: String,
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
    pub snippet: Option<String>,
    pub payload: Option<Payload>,
    #[serde(rename = "labelIds")]
    pub label_ids: Option<Vec<String>>,
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
        }
    }

    async fn check_response(resp: reqwest::Response) -> Result<reqwest::Response> {
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("HTTP {} - {}", status, body);
        }
        Ok(resp)
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, endpoint: &str) -> Result<T> {
        let url = format!("{}{}", BASE_URL, endpoint);

        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.access_token)
            .send()
            .await
            .context("Failed to send request")?;

        let resp = Self::check_response(resp).await?;
        resp.json().await.context("Failed to parse JSON response")
    }

    async fn post(&self, endpoint: &str) -> Result<()> {
        let url = format!("{}{}", BASE_URL, endpoint);

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.access_token)
            .send()
            .await
            .context("Failed to send request")?;

        Self::check_response(resp).await?;
        Ok(())
    }

    async fn post_json<T: Serialize>(&self, endpoint: &str, body: &T) -> Result<()> {
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

    async fn post_json_with_response<T: Serialize, R: serde::de::DeserializeOwned>(
        &self,
        endpoint: &str,
        body: &T,
    ) -> Result<R> {
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

    pub async fn list_labels(&self) -> Result<LabelList> {
        self.get("/users/me/labels").await
    }

    pub async fn create_label(&self, name: &str) -> Result<Label> {
        // Capitalize first letter for consistency
        let capitalized = capitalize_first(name);
        let body = serde_json::json!({
            "name": capitalized,
            "labelListVisibility": "labelShow",
            "messageListVisibility": "show"
        });
        self.post_json_with_response("/users/me/labels", &body).await
    }

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

    pub async fn list_messages(&self, query: Option<&str>, label: &str, max_results: u32) -> Result<MessageList> {
        let mut endpoint = format!("/users/me/messages?maxResults={}", max_results);
        if !label.is_empty() {
            endpoint.push_str(&format!("&labelIds={}", urlencoding::encode(label)));
        }
        if let Some(q) = query {
            endpoint.push_str(&format!("&q={}", urlencoding::encode(q)));
        }
        self.get(&endpoint).await
    }

    pub async fn get_message(&self, id: &str) -> Result<Message> {
        self.get(&format!("/users/me/messages/{}", urlencoding::encode(id))).await
    }

    pub async fn modify_labels(&self, id: &str, add: &[&str], remove: &[&str]) -> Result<()> {
        let endpoint = format!("/users/me/messages/{}/modify", urlencoding::encode(id));
        let body = serde_json::json!({
            "addLabelIds": add,
            "removeLabelIds": remove
        });
        self.post_json(&endpoint, &body).await
    }

    pub async fn archive(&self, id: &str) -> Result<()> {
        self.modify_labels(id, &[], &["INBOX"]).await
    }

    pub async fn mark_spam(&self, id: &str) -> Result<()> {
        self.modify_labels(id, &["SPAM"], &["INBOX"]).await
    }

    pub async fn unspam(&self, id: &str) -> Result<()> {
        self.modify_labels(id, &["INBOX"], &["SPAM"]).await
    }

    pub async fn add_label(&self, id: &str, label: &str) -> Result<()> {
        // For custom labels, we need to get/create the label ID first
        let label_id = if is_system_label(label) {
            label.to_string()
        } else {
            self.get_or_create_label(label).await?
        };
        self.modify_labels(id, &[&label_id], &[]).await
    }

    pub async fn remove_label(&self, id: &str, label: &str) -> Result<()> {
        // For custom labels, we need to find the label ID first
        let label_id = if is_system_label(label) {
            label.to_string()
        } else {
            self.find_label(label).await?.ok_or_else(|| anyhow::anyhow!("Label not found: {}", label))?
        };
        self.modify_labels(id, &[], &[&label_id]).await
    }

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

    pub async fn trash(&self, id: &str) -> Result<()> {
        self.post(&format!("/users/me/messages/{}/trash", urlencoding::encode(id))).await
    }

    pub async fn unsubscribe(&self, id: &str) -> Result<()> {
        self.post(&format!("/users/me/messages/{}/unsubscribe", urlencoding::encode(id))).await
    }
}

impl Message {
    pub fn get_header(&self, name: &str) -> Option<&str> {
        self.payload.as_ref()?.headers.as_ref()?.iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
    }

    pub fn get_body_text(&self) -> Option<String> {
        let payload = self.payload.as_ref()?;

        // Try direct body first
        if let Some(body) = &payload.body {
            if let Some(data) = &body.data {
                if let Ok(decoded) = BASE64_URL_SAFE_NO_PAD.decode(data) {
                    return String::from_utf8(decoded).ok();
                }
            }
        }

        // Try parts
        if let Some(parts) = &payload.parts {
            return find_text_part(parts);
        }

        None
    }
}

fn find_text_part(parts: &[Part]) -> Option<String> {
    for part in parts {
        if part.mime_type == "text/plain" {
            if let Some(body) = &part.body {
                if let Some(data) = &body.data {
                    if let Ok(decoded) = BASE64_URL_SAFE_NO_PAD.decode(data) {
                        return String::from_utf8(decoded).ok();
                    }
                }
            }
        }
        if let Some(nested) = &part.parts {
            if let Some(text) = find_text_part(nested) {
                return Some(text);
            }
        }
    }
    None
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

    fn make_message(payload: Option<Payload>) -> Message {
        Message {
            id: "test123".to_string(),
            snippet: Some("snippet".to_string()),
            payload,
            label_ids: None,
        }
    }

    fn make_body(text: &str) -> Body {
        Body {
            data: Some(BASE64_URL_SAFE_NO_PAD.encode(text)),
        }
    }

    #[test]
    fn test_get_header() {
        let msg = make_message(Some(Payload {
            headers: Some(vec![
                Header { name: "From".to_string(), value: "test@example.com".to_string() },
                Header { name: "Subject".to_string(), value: "Hello".to_string() },
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
}
