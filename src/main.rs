#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

use anyhow::{Context, Result};
use base64::Engine;
use clap::{Parser, Subcommand};
use gmail::{api, auth, config, mime};
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "gmail")]
#[command(about = "CLI tool to access Gmail API")]
struct Cli {
    /// Output as JSON
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Set custom OAuth client ID (optional - has built-in default)
    Config {
        /// Client ID (from Google Cloud Console)
        client_id: String,
    },
    /// Authenticate with Gmail (opens browser)
    Login,
    /// List available labels
    Labels,
    /// List messages
    List {
        /// Maximum number of messages to show
        #[arg(short = 'n', long, default_value = "100")]
        max: u32,
        /// Search query (Gmail search syntax)
        #[arg(short, long)]
        query: Option<String>,
        /// Label to filter by (inbox, sent, trash, spam, starred, all, drafts)
        #[arg(short, long, default_value = "inbox")]
        label: String,
        /// Show only unread messages
        #[arg(short, long)]
        unread: bool,
    },
    /// Read a specific message
    Read {
        /// Message ID
        id: String,
        /// Show raw HTML instead of text
        #[arg(long)]
        html: bool,
    },
    /// Create a reply draft without sending it
    #[command(name = "draft-reply")]
    DraftReply {
        /// Source message ID
        id: String,
        /// File containing the reply body
        #[arg(long, value_name = "PATH")]
        body_file: PathBuf,
        /// Attachment path; repeat for multiple attachments
        #[arg(long = "attach", value_name = "PATH")]
        attachments: Vec<PathBuf>,
    },
    /// Archive a message (remove from inbox)
    Archive {
        /// Message ID
        id: String,
    },
    /// Mark a message as spam
    Spam {
        /// Message ID
        id: String,
    },
    /// Remove from spam and move to inbox
    Unspam {
        /// Message ID
        id: String,
    },
    /// Add a label to a message
    Label {
        /// Message ID
        id: String,
        /// Label to add
        label: String,
    },
    /// Remove a label from a message
    Unlabel {
        /// Message ID
        id: String,
        /// Label to remove
        label: String,
    },
    /// Move a message to trash
    Delete {
        /// Message ID
        id: String,
    },
    /// Restore a message from trash
    Undelete {
        /// Message ID
        id: String,
    },
    /// Mark a message as read
    #[command(name = "mark-read")]
    MarkRead {
        /// Message ID
        id: String,
    },
    /// Mark a message as unread
    #[command(name = "mark-unread")]
    MarkUnread {
        /// Message ID
        id: String,
    },
    /// Remove all user labels from a message
    #[command(name = "clear-labels")]
    ClearLabels {
        /// Message ID
        id: String,
    },
    /// Unsubscribe from a mailing list (opens unsubscribe link)
    Unsubscribe {
        /// Message ID
        id: String,
    },
}

fn normalize_label(label: &str) -> String {
    match label.to_lowercase().as_str() {
        "inbox" => "INBOX".to_string(),
        "sent" => "SENT".to_string(),
        "trash" => "TRASH".to_string(),
        "spam" => "SPAM".to_string(),
        "starred" => "STARRED".to_string(),
        "unread" => "UNREAD".to_string(),
        "important" => "IMPORTANT".to_string(),
        "drafts" | "draft" => "DRAFT".to_string(),
        "all" => "".to_string(),
        "primary" => "CATEGORY_PERSONAL".to_string(),
        "social" => "CATEGORY_SOCIAL".to_string(),
        "promotions" => "CATEGORY_PROMOTIONS".to_string(),
        "updates" => "CATEGORY_UPDATES".to_string(),
        "forums" => "CATEGORY_FORUMS".to_string(),
        other => other.to_string(), // Keep custom labels as-is
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
async fn get_client() -> Result<api::Client> {
    let cfg = config::load_config()?;
    let client_id = cfg.client_id();
    let client_secret = cfg.client_secret();

    let tokens = match config::load_tokens() {
        Ok(t) => t,
        Err(_) => anyhow::bail!("Not logged in. Run 'gmail login' first"),
    };

    // Try to use existing token, refresh if needed
    let client = api::Client::new(&tokens.access_token);

    // Test if token works by making a simple request
    match client.list_messages(None, "INBOX", 1).await {
        Ok(_) => Ok(client),
        Err(_) => {
            // Token expired, try refresh
            let new_tokens =
                auth::refresh_token(client_id, client_secret, &tokens.refresh_token).await?;
            Ok(api::Client::new(&new_tokens.access_token))
        }
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
async fn cmd_config(client_id: String) -> Result<()> {
    let cfg = config::Config {
        client_id: Some(client_id),
        client_secret: None,
    };
    config::save_config(&cfg)?;
    println!("Custom client ID saved to {:?}", config::config_dir());
    Ok(())
}

#[cfg_attr(coverage_nightly, coverage(off))]
async fn cmd_login() -> Result<()> {
    let cfg = config::load_config()?;
    let client_id = cfg.client_id();
    let client_secret = cfg.client_secret();
    auth::login(client_id, client_secret).await?;
    println!("Login successful! Tokens saved.");
    Ok(())
}

#[cfg_attr(coverage_nightly, coverage(off))]
async fn cmd_labels(json: bool) -> Result<()> {
    let client = get_client().await?;
    let labels = client.list_labels().await?;
    let Some(labels) = labels.labels else {
        return Ok(());
    };

    if json {
        println!("{}", serde_json::to_string(&labels)?);
        return Ok(());
    }

    print_labels_human(&labels);
    Ok(())
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn print_labels_human(labels: &[api::Label]) {
    let mut system: Vec<_> = labels
        .iter()
        .filter(|label| label.label_type.as_deref() == Some("system"))
        .collect();
    let mut user: Vec<_> = labels
        .iter()
        .filter(|label| label.label_type.as_deref() != Some("system"))
        .collect();

    system.sort_by(|a, b| a.name.cmp(&b.name));
    user.sort_by(|a, b| a.name.cmp(&b.name));

    println!("System labels:");
    for label in system {
        println!("  {} ({})", label.name, label.id);
    }

    if !user.is_empty() {
        println!("\nUser labels:");
        for label in user {
            println!("  {} ({})", label.name, label.id);
        }
    }
}

fn with_unread_query(query: Option<String>, unread: bool) -> Option<String> {
    if !unread {
        return query;
    }

    Some(match query {
        Some(q) => format!("is:unread {}", q),
        None => "is:unread".to_string(),
    })
}

#[cfg_attr(coverage_nightly, coverage(off))]
async fn fetch_full_messages(
    client: &api::Client,
    message_refs: Vec<api::MessageRef>,
) -> Result<Vec<api::Message>> {
    let mut messages = Vec::with_capacity(message_refs.len());
    for msg_ref in message_refs {
        let message = client.get_message(&msg_ref.id).await?;
        messages.push(message);
    }
    Ok(messages)
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn print_messages_json(messages: &[api::Message]) -> Result<()> {
    let items: Vec<_> = messages
        .iter()
        .map(|msg| {
            serde_json::json!({
                "id": msg.id,
                "from": msg.get_header("From"),
                "to": msg.get_header("To"),
                "subject": msg.get_header("Subject"),
                "date": msg.get_header("Date"),
                "snippet": msg.snippet,
            })
        })
        .collect();
    println!("{}", serde_json::to_string(&items)?);
    Ok(())
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn print_messages_human(messages: &[api::Message]) {
    for msg in messages {
        let from = msg.get_header("From").unwrap_or("Unknown");
        let subject = msg.get_header("Subject").unwrap_or("(no subject)");
        println!("{} | {} | {}", msg.id, from, subject);
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn print_empty_message_list(json: bool) {
    if json {
        println!("[]");
    } else {
        println!("No messages found.");
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
async fn cmd_list(
    max: u32,
    query: Option<String>,
    label: String,
    unread: bool,
    json: bool,
) -> Result<()> {
    let client = get_client().await?;
    let label_id = normalize_label(&label);
    let query = with_unread_query(query, unread);
    let list = client
        .list_messages(query.as_deref(), &label_id, max)
        .await?;

    let Some(message_refs) = list.messages else {
        print_empty_message_list(json);
        return Ok(());
    };

    let messages = fetch_full_messages(&client, message_refs).await?;
    if json {
        print_messages_json(&messages)?;
    } else {
        print_messages_human(&messages);
    }

    Ok(())
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn print_html_body(msg: &api::Message) {
    if let Some(html_body) = msg.get_body_html() {
        println!("{}", html_body);
    } else {
        eprintln!("No HTML body found");
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn print_message_json(msg: &api::Message) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "id": msg.id,
            "from": msg.get_header("From"),
            "to": msg.get_header("To"),
            "subject": msg.get_header("Subject"),
            "date": msg.get_header("Date"),
            "body": msg.get_body_text(),
            "snippet": msg.snippet,
            "list_unsubscribe": msg.get_header("List-Unsubscribe"),
            "list_unsubscribe_post": msg.get_header("List-Unsubscribe-Post"),
            "authentication_results": msg.get_header("Authentication-Results"),
            "dkim_signature": msg.get_header("DKIM-Signature"),
            "received_spf": msg.get_header("Received-SPF"),
        }))?
    );
    Ok(())
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn print_message_text(msg: &api::Message) {
    println!("From: {}", msg.get_header("From").unwrap_or("Unknown"));
    println!("To: {}", msg.get_header("To").unwrap_or("Unknown"));
    println!(
        "Subject: {}",
        msg.get_header("Subject").unwrap_or("(no subject)")
    );
    println!("Date: {}", msg.get_header("Date").unwrap_or("Unknown"));
    println!("---");

    if let Some(body) = msg.get_body_text() {
        println!("{}", body);
    } else if let Some(snippet) = &msg.snippet {
        println!("{}", snippet);
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
async fn cmd_read(id: String, html: bool, json: bool) -> Result<()> {
    let client = get_client().await?;
    let msg = client.get_message(&id).await?;

    if html {
        print_html_body(&msg);
        return Ok(());
    }

    if json {
        print_message_json(&msg)?;
        return Ok(());
    }

    print_message_text(&msg);
    Ok(())
}

#[cfg_attr(coverage_nightly, coverage(off))]
async fn cmd_draft_reply(
    id: String,
    body_file: PathBuf,
    attachment_paths: Vec<PathBuf>,
) -> Result<()> {
    let client = get_client().await?;
    let source_message = client.get_message(&id).await?;
    let source = mime::ReplySource::from_message(&source_message)?;
    let thread_id = source
        .thread_id
        .as_deref()
        .context("Source message has no thread ID")?;
    let body = fs::read_to_string(&body_file)
        .with_context(|| format!("Failed to read reply body {}", body_file.display()))?;
    let attachments = attachment_paths
        .iter()
        .map(|path| mime::Attachment::from_path(path))
        .collect::<Result<Vec<_>>>()?;
    let raw = mime::build_reply_mime(&source, &body, &attachments, &mime::new_boundary())?;
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw);
    let draft = client.create_draft(&encoded, thread_id).await?;

    println!("Created draft {}", draft.id);
    Ok(())
}

#[cfg_attr(coverage_nightly, coverage(off))]
async fn cmd_archive(id: String) -> Result<()> {
    let client = get_client().await?;
    client.archive(&id).await?;
    println!("Archived {}", id);
    Ok(())
}

#[cfg_attr(coverage_nightly, coverage(off))]
async fn cmd_spam(id: String) -> Result<()> {
    let client = get_client().await?;
    let _ = client.unsubscribe(&id).await;
    client.mark_spam(&id).await?;
    println!("Marked as spam {}", id);
    Ok(())
}

#[cfg_attr(coverage_nightly, coverage(off))]
async fn cmd_unspam(id: String) -> Result<()> {
    let client = get_client().await?;
    client.unspam(&id).await?;
    println!("Moved to inbox {}", id);
    Ok(())
}

#[cfg_attr(coverage_nightly, coverage(off))]
async fn cmd_label(id: String, label: String) -> Result<()> {
    let client = get_client().await?;
    let label_id = normalize_label(&label);
    client.add_label(&id, &label_id).await?;
    println!("Added label {} to {}", label, id);
    Ok(())
}

#[cfg_attr(coverage_nightly, coverage(off))]
async fn cmd_unlabel(id: String, label: String) -> Result<()> {
    let client = get_client().await?;
    let label_id = normalize_label(&label);
    client.remove_label(&id, &label_id).await?;
    println!("Removed label {} from {}", label, id);
    Ok(())
}

#[cfg_attr(coverage_nightly, coverage(off))]
async fn cmd_delete(id: String) -> Result<()> {
    let client = get_client().await?;
    client.trash(&id).await?;
    println!("Moved to trash {}", id);
    Ok(())
}

#[cfg_attr(coverage_nightly, coverage(off))]
async fn cmd_undelete(id: String) -> Result<()> {
    let client = get_client().await?;
    client.untrash(&id).await?;
    println!("Restored from trash {}", id);
    Ok(())
}

#[cfg_attr(coverage_nightly, coverage(off))]
async fn cmd_mark_read(id: String) -> Result<()> {
    let client = get_client().await?;
    client.mark_read(&id).await?;
    println!("Marked as read {}", id);
    Ok(())
}

#[cfg_attr(coverage_nightly, coverage(off))]
async fn cmd_mark_unread(id: String) -> Result<()> {
    let client = get_client().await?;
    client.mark_unread(&id).await?;
    println!("Marked as unread {}", id);
    Ok(())
}

#[cfg_attr(coverage_nightly, coverage(off))]
async fn cmd_clear_labels(id: String) -> Result<()> {
    let client = get_client().await?;
    let removed = client.clear_labels(&id).await?;
    if removed.is_empty() {
        println!("No user labels to remove from {}", id);
    } else {
        println!("Removed {} labels from {}", removed.len(), id);
    }
    Ok(())
}

#[cfg_attr(coverage_nightly, coverage(off))]
async fn cmd_unsubscribe(id: String) -> Result<()> {
    let client = get_client().await?;
    client.unsubscribe(&id).await?;
    println!("Unsubscribed from {}", id);
    Ok(())
}

#[cfg_attr(coverage_nightly, coverage(off))]
async fn run_message_command(command: Commands) -> Result<()> {
    match command {
        Commands::Archive { id } => cmd_archive(id).await,
        Commands::Spam { id } => cmd_spam(id).await,
        Commands::Unspam { id } => cmd_unspam(id).await,
        Commands::Label { id, label } => cmd_label(id, label).await,
        Commands::Unlabel { id, label } => cmd_unlabel(id, label).await,
        Commands::Delete { id } => cmd_delete(id).await,
        Commands::Undelete { id } => cmd_undelete(id).await,
        Commands::MarkRead { id } => cmd_mark_read(id).await,
        Commands::MarkUnread { id } => cmd_mark_unread(id).await,
        Commands::ClearLabels { id } => cmd_clear_labels(id).await,
        Commands::Unsubscribe { id } => cmd_unsubscribe(id).await,
        _ => unreachable!("Unsupported command in run_message_command"),
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
async fn run_command(command: Commands, json: bool) -> Result<()> {
    match command {
        Commands::Config { client_id } => cmd_config(client_id).await,
        Commands::Login => cmd_login().await,
        Commands::Labels => cmd_labels(json).await,
        Commands::List {
            max,
            query,
            label,
            unread,
        } => cmd_list(max, query, label, unread, json).await,
        Commands::Read { id, html } => cmd_read(id, html, json).await,
        Commands::DraftReply {
            id,
            body_file,
            attachments,
        } => cmd_draft_reply(id, body_file, attachments).await,
        command => run_message_command(command).await,
    }
}

#[tokio::main]
#[cfg_attr(coverage_nightly, coverage(off))]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    run_command(cli.command, cli.json).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    fn message_with_labels(labels: Option<Vec<&str>>) -> api::Message {
        api::Message {
            id: "id-1".to_string(),
            thread_id: None,
            snippet: Some("Snippet".to_string()),
            payload: Some(api::Payload {
                headers: Some(vec![
                    api::Header {
                        name: "From".to_string(),
                        value: "sender@example.com".to_string(),
                    },
                    api::Header {
                        name: "Subject".to_string(),
                        value: "Subject".to_string(),
                    },
                    api::Header {
                        name: "Date".to_string(),
                        value: "Wed, 24 Jun 2026 12:00:00 +0000".to_string(),
                    },
                ]),
                body: None,
                parts: None,
            }),
            label_ids: labels.map(|items| items.into_iter().map(str::to_string).collect()),
        }
    }

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_draft_reply_with_repeated_attachments() {
        let cli = Cli::try_parse_from([
            "gmail",
            "draft-reply",
            "message-id",
            "--body-file",
            "reply.txt",
            "--attach",
            "invoice.pdf",
            "--attach",
            "chair.jpg",
        ])
        .expect("draft-reply command should parse");

        match cli.command {
            Commands::DraftReply {
                id,
                body_file,
                attachments,
            } => {
                assert_eq!(id, "message-id");
                assert_eq!(body_file, std::path::PathBuf::from("reply.txt"));
                assert_eq!(
                    attachments,
                    vec![
                        std::path::PathBuf::from("invoice.pdf"),
                        std::path::PathBuf::from("chair.jpg")
                    ]
                );
            }
            _ => panic!("expected draft-reply command"),
        }
    }

    #[test]
    fn normalize_label_maps_common_aliases() {
        assert_eq!(normalize_label("inbox"), "INBOX");
        assert_eq!(normalize_label("sent"), "SENT");
        assert_eq!(normalize_label("spam"), "SPAM");
        assert_eq!(normalize_label("custom"), "custom");
    }

    #[test]
    fn with_unread_query_combines_existing_query() {
        assert_eq!(
            with_unread_query(Some("from:me".to_string()), true).as_deref(),
            Some("is:unread from:me")
        );
        assert_eq!(with_unread_query(None, true).as_deref(), Some("is:unread"));
        assert_eq!(
            with_unread_query(Some("from:me".to_string()), false).as_deref(),
            Some("from:me")
        );
    }

    #[test]
    fn print_empty_message_list_does_not_panic() {
        print_empty_message_list(true);
        print_empty_message_list(false);
    }

    #[test]
    fn message_json_serialization_contains_expected_fields() {
        let msg = message_with_labels(Some(vec!["INBOX", "Receipts"]));

        let json = serde_json::to_value(serde_json::json!({
            "id": msg.id,
            "from": msg.get_header("From"),
            "subject": msg.get_header("Subject"),
            "date": msg.get_header("Date"),
            "snippet": msg.snippet,
            "labels": msg.label_ids,
        }))
        .unwrap();

        assert_eq!(json["id"], "id-1");
        assert_eq!(json["from"], "sender@example.com");
        assert_eq!(json["labels"][1], "Receipts");
    }
}
