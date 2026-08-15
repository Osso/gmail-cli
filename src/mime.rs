use crate::api::Message;
use anyhow::{Context, Result, bail};
use base64::Engine;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct ReplySource {
    pub to: String,
    pub subject: String,
    pub message_id: Option<String>,
    pub references: Option<String>,
    pub thread_id: Option<String>,
}

impl ReplySource {
    pub fn from_message(message: &Message) -> Result<Self> {
        let to = required_header(message, "From", "Source message has no From header")?;
        let subject =
            optional_header(message, "Subject").unwrap_or_else(|| "(no subject)".to_string());
        let message_id = optional_header(message, "Message-ID");
        let references = optional_header(message, "References");

        validate_source_headers(&to, &subject, message_id.as_deref(), references.as_deref())?;

        Ok(Self {
            to,
            subject,
            message_id,
            references,
            thread_id: message.thread_id.clone(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct Attachment {
    pub filename: String,
    pub content_type: String,
    pub data: Vec<u8>,
}

impl Attachment {
    pub fn from_path(path: &Path) -> Result<Self> {
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .context("Attachment filename is not valid UTF-8")?
            .to_string();
        let data = fs::read(path)
            .with_context(|| format!("Failed to read attachment {}", path.display()))?;

        Ok(Self {
            content_type: mime_type_for_path(path).to_string(),
            filename,
            data,
        })
    }
}

pub fn new_boundary() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("gmail-cli-{nanos:x}")
}

pub fn build_reply_mime(
    source: &ReplySource,
    body: &str,
    attachments: &[Attachment],
    boundary: &str,
) -> Result<Vec<u8>> {
    build_reply_mime_with_recipient(source, body, attachments, boundary, None)
}

pub fn build_reply_mime_with_recipient(
    source: &ReplySource,
    body: &str,
    attachments: &[Attachment],
    boundary: &str,
    recipient: Option<&str>,
) -> Result<Vec<u8>> {
    validate_header_value("boundary", boundary)?;

    let mut message = String::new();
    append_reply_headers(&mut message, source, recipient)?;
    if attachments.is_empty() {
        append_text_part(&mut message, body);
    } else {
        append_multipart_body(&mut message, body, attachments, boundary)?;
    }

    Ok(message.into_bytes())
}

fn append_reply_headers(
    message: &mut String,
    source: &ReplySource,
    recipient: Option<&str>,
) -> Result<()> {
    validate_source_headers(
        &source.to,
        &source.subject,
        source.message_id.as_deref(),
        source.references.as_deref(),
    )?;

    let subject = reply_subject(&source.subject);
    let in_reply_to = source.message_id.as_deref();
    let references = reply_references(source.references.as_deref(), in_reply_to);
    let recipient = recipient.unwrap_or(&source.to);
    validate_header_value("To", recipient)?;

    message.push_str(&format!("To: {}\r\n", encode_address_header(recipient)));
    message.push_str(&format!("Subject: {}\r\n", encode_header_value(&subject)));
    if let Some(value) = in_reply_to {
        message.push_str(&format!("In-Reply-To: {value}\r\n"));
    }
    if let Some(value) = references {
        message.push_str(&format!("References: {value}\r\n"));
    }
    message.push_str("MIME-Version: 1.0\r\n");
    Ok(())
}

fn append_text_part(message: &mut String, body: &str) {
    message.push_str("Content-Type: text/plain; charset=utf-8\r\n");
    message.push_str("Content-Transfer-Encoding: 8bit\r\n\r\n");
    message.push_str(&normalize_body(body));
}

fn append_multipart_body(
    message: &mut String,
    body: &str,
    attachments: &[Attachment],
    boundary: &str,
) -> Result<()> {
    message.push_str(&format!(
        "Content-Type: multipart/mixed; boundary=\"{boundary}\"\r\n\r\n"
    ));
    message.push_str(&format!("--{boundary}\r\n"));
    append_text_part(message, body);
    message.push_str("\r\n");

    for attachment in attachments {
        append_attachment(message, attachment, boundary)?;
    }
    message.push_str(&format!("--{boundary}--\r\n"));
    Ok(())
}

fn append_attachment(message: &mut String, attachment: &Attachment, boundary: &str) -> Result<()> {
    validate_header_value("attachment filename", &attachment.filename)?;
    validate_header_value("attachment content type", &attachment.content_type)?;

    message.push_str(&format!("--{boundary}\r\n"));
    if attachment.filename.is_ascii() && is_safe_ascii_filename(&attachment.filename) {
        let filename = quote_parameter(&attachment.filename);
        message.push_str(&format!(
            "Content-Type: {}; name={filename}\r\n",
            attachment.content_type
        ));
        message.push_str(&format!(
            "Content-Disposition: attachment; filename={filename}\r\n"
        ));
    } else {
        let filename = rfc2231_filename(&attachment.filename);
        message.push_str(&format!(
            "Content-Type: {}; name*=utf-8''{filename}\r\n",
            attachment.content_type
        ));
        message.push_str(&format!(
            "Content-Disposition: attachment; filename*=utf-8''{filename}\r\n"
        ));
    }
    message.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
    message.push_str(&base64_lines(&attachment.data));
    message.push_str("\r\n");

    Ok(())
}

fn reply_subject(subject: &str) -> String {
    let subject = if subject.is_empty() {
        "(no subject)"
    } else {
        subject.trim()
    };
    if subject
        .get(..3)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("re:"))
    {
        subject.to_string()
    } else {
        format!("Re: {subject}")
    }
}

fn reply_references(references: Option<&str>, message_id: Option<&str>) -> Option<String> {
    match (
        references.map(str::trim).filter(|value| !value.is_empty()),
        message_id,
    ) {
        (Some(references), Some(message_id))
            if references
                .split_whitespace()
                .any(|value| value == message_id) =>
        {
            Some(references.to_string())
        }
        (Some(references), Some(message_id)) => Some(format!("{references} {message_id}")),
        (Some(references), None) => Some(references.to_string()),
        (None, Some(message_id)) => Some(message_id.to_string()),
        (None, None) => None,
    }
}

fn normalize_body(body: &str) -> String {
    let body = body.replace("\r\n", "\n").replace('\r', "\n");
    let mut normalized = body.replace('\n', "\r\n");
    if !normalized.ends_with("\r\n") {
        normalized.push_str("\r\n");
    }
    normalized
}

fn encode_address_header(value: &str) -> String {
    if value.is_ascii() {
        return value.to_string();
    }

    if let Some(start) = value.rfind('<') {
        let display_name = value[..start].trim();
        let address = value[start..].trim();
        if !display_name.is_empty() && address.ends_with('>') {
            return format!("{} {address}", encode_header_value(display_name));
        }
    }
    encode_header_value(value)
}

fn encode_header_value(value: &str) -> String {
    if value.is_ascii() {
        return value.to_string();
    }
    let encoded = base64::engine::general_purpose::STANDARD.encode(value.as_bytes());
    format!("=?UTF-8?B?{encoded}?=")
}

fn rfc2231_filename(filename: &str) -> String {
    urlencoding::encode(filename).into_owned()
}

fn quote_parameter(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn is_safe_ascii_filename(filename: &str) -> bool {
    filename
        .bytes()
        .all(|byte| (byte.is_ascii_graphic() || byte == b' ') && byte != b'"' && byte != b'\\')
}

fn base64_lines(data: &[u8]) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(data);
    encoded
        .as_bytes()
        .chunks(76)
        .map(|line| String::from_utf8_lossy(line).into_owned())
        .collect::<Vec<_>>()
        .join("\r\n")
}

fn required_header(message: &Message, name: &str, missing_message: &str) -> Result<String> {
    message
        .get_header(name)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .context(missing_message.to_string())
}

fn optional_header(message: &Message, name: &str) -> Option<String> {
    message
        .get_header(name)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn validate_source_headers(
    to: &str,
    subject: &str,
    message_id: Option<&str>,
    references: Option<&str>,
) -> Result<()> {
    validate_header_value("From", to)?;
    validate_header_value("Subject", subject)?;
    if let Some(value) = message_id {
        validate_header_value("Message-ID", value)?;
    }
    if let Some(value) = references {
        validate_header_value("References", value)?;
    }
    Ok(())
}

fn validate_header_value(name: &str, value: &str) -> Result<()> {
    if value.contains('\r') || value.contains('\n') {
        bail!("{name} contains an invalid line break");
    }
    Ok(())
}

fn mime_type_for_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("pdf") => "application/pdf",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("mp4") => "video/mp4",
        Some("txt") => "text/plain",
        Some("csv") => "text/csv",
        Some("html" | "htm") => "text/html",
        Some("json") => "application/json",
        Some("doc") => "application/msword",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("xls") => "application/vnd.ms-excel",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("zip") => "application/zip",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> ReplySource {
        ReplySource {
            to: "Sender <sender@example.com>".to_string(),
            subject: "Herman Miller Store".to_string(),
            message_id: Some("<original@example.com>".to_string()),
            references: Some("<earlier@example.com>".to_string()),
            thread_id: Some("thread-id".to_string()),
        }
    }

    #[test]
    fn builds_reply_headers_and_plain_text_body() {
        let raw = build_reply_mime(&source(), "First line\nSecond line", &[], "test-boundary")
            .expect("MIME should build");
        let text = String::from_utf8(raw).expect("MIME should be UTF-8 for this fixture");

        assert!(text.contains("To: Sender <sender@example.com>\r\n"));
        assert!(text.contains("Subject: Re: Herman Miller Store\r\n"));
        assert!(text.contains("In-Reply-To: <original@example.com>\r\n"));
        assert!(text.contains("References: <earlier@example.com> <original@example.com>\r\n"));
        assert!(text.contains("Content-Type: text/plain; charset=utf-8\r\n"));
        assert!(text.ends_with("First line\r\nSecond line\r\n"));
        assert_eq!(text.matches("Subject:").count(), 1);
        assert_eq!(text.matches("References:").count(), 1);
    }

    #[test]
    fn uses_recipient_override_without_changing_reply_metadata() {
        let raw = build_reply_mime_with_recipient(
            &source(),
            "Reply",
            &[],
            "test-boundary",
            Some("review@example.com"),
        )
        .expect("MIME should build");
        let text = String::from_utf8(raw).expect("MIME should be UTF-8 for this fixture");

        assert!(text.contains("To: review@example.com\r\n"));
        assert!(!text.contains("To: Sender <sender@example.com>\r\n"));
        assert!(text.contains("Subject: Re: Herman Miller Store\r\n"));
        assert!(text.contains("In-Reply-To: <original@example.com>\r\n"));
        assert!(text.contains("References: <earlier@example.com> <original@example.com>\r\n"));
    }

    #[test]
    fn rejects_recipient_header_injection() {
        let error = build_reply_mime_with_recipient(
            &source(),
            "Reply",
            &[],
            "test-boundary",
            Some("review@example.com\r\nBcc: attacker@example.com"),
        )
        .expect_err("recipient line breaks must be rejected");

        assert!(
            error
                .to_string()
                .contains("To contains an invalid line break")
        );
    }

    #[test]
    fn derives_reply_metadata_from_source_message() {
        let message = crate::api::Message {
            id: "message-id".to_string(),
            thread_id: Some("thread-id".to_string()),
            snippet: None,
            payload: Some(crate::api::Payload {
                headers: Some(vec![
                    crate::api::Header {
                        name: "From".to_string(),
                        value: "Original Sender <sender@example.com>".to_string(),
                    },
                    crate::api::Header {
                        name: "Subject".to_string(),
                        value: "Original subject".to_string(),
                    },
                    crate::api::Header {
                        name: "Message-ID".to_string(),
                        value: "<message@example.com>".to_string(),
                    },
                    crate::api::Header {
                        name: "References".to_string(),
                        value: "<prior@example.com>".to_string(),
                    },
                ]),
                body: None,
                parts: None,
            }),
            label_ids: None,
        };

        let reply = ReplySource::from_message(&message).expect("source metadata should parse");

        assert_eq!(reply.to, "Original Sender <sender@example.com>");
        assert_eq!(reply.subject, "Original subject");
        assert_eq!(reply.message_id.as_deref(), Some("<message@example.com>"));
        assert_eq!(reply.references.as_deref(), Some("<prior@example.com>"));
        assert_eq!(reply.thread_id.as_deref(), Some("thread-id"));
    }

    #[test]
    fn does_not_duplicate_existing_reply_subject_or_reference() {
        let source = ReplySource {
            subject: "Re: Existing subject".to_string(),
            references: Some("<earlier@example.com> <original@example.com>".to_string()),
            ..source()
        };
        let raw =
            build_reply_mime(&source, "Reply", &[], "test-boundary").expect("MIME should build");
        let text = String::from_utf8(raw).expect("MIME should be UTF-8 for this fixture");

        assert!(text.contains("Subject: Re: Existing subject\r\n"));
        assert!(text.contains("References: <earlier@example.com> <original@example.com>\r\n"));
        assert!(!text.contains("Re: Re:"));
    }

    #[test]
    fn serializes_create_draft_request_with_raw_message_and_thread() {
        let request = crate::api::CreateDraftRequest {
            message: crate::api::DraftMessage {
                raw: "cmF3".to_string(),
                thread_id: "thread-id".to_string(),
            },
        };

        let json = serde_json::to_value(request).expect("draft request should serialize");

        assert_eq!(json["message"]["raw"], "cmF3");
        assert_eq!(json["message"]["threadId"], "thread-id");
    }

    #[test]
    fn encodes_multiple_attachments_with_content_types_and_filenames() {
        let attachments = vec![
            Attachment {
                filename: "invoice.pdf".to_string(),
                content_type: "application/pdf".to_string(),
                data: b"pdf bytes".to_vec(),
            },
            Attachment {
                filename: "chair video.mp4".to_string(),
                content_type: "video/mp4".to_string(),
                data: b"video bytes".to_vec(),
            },
        ];
        let raw = build_reply_mime(&source(), "Reply", &attachments, "test-boundary")
            .expect("MIME should build");
        let text = String::from_utf8(raw).expect("MIME should be UTF-8 for this fixture");

        assert!(text.contains("Content-Type: multipart/mixed; boundary=\"test-boundary\""));
        assert!(text.contains("Content-Type: application/pdf; name=\"invoice.pdf\""));
        assert!(text.contains("Content-Disposition: attachment; filename=\"invoice.pdf\""));
        assert!(text.contains("Content-Type: video/mp4; name=\"chair video.mp4\""));
        assert!(text.contains("Content-Disposition: attachment; filename=\"chair video.mp4\""));
        assert!(text.contains("cGRmIGJ5dGVz"));
        assert!(text.contains("dmlkZW8gYnl0ZXM"));
        assert_eq!(text.matches("--test-boundary\r\n").count(), 3);
    }

    #[test]
    fn encodes_non_ascii_attachment_filename_with_rfc2231_parameter() {
        let attachments = vec![Attachment {
            filename: "évidence photo.jpg".to_string(),
            content_type: "image/jpeg".to_string(),
            data: vec![0xff, 0xd8, 0xff],
        }];
        let raw = build_reply_mime(&source(), "Reply", &attachments, "test-boundary")
            .expect("MIME should build");
        let text = String::from_utf8(raw).expect("MIME should be UTF-8 for this fixture");

        assert!(text.contains("Content-Type: image/jpeg; name*=utf-8''%C3%A9vidence%20photo.jpg"));
        assert!(text.contains(
            "Content-Disposition: attachment; filename*=utf-8''%C3%A9vidence%20photo.jpg"
        ));
    }
}
