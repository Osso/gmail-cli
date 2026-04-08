mod api;
mod auth;
mod config;

use anyhow::Result;
use clap::{Parser, Subcommand};

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

async fn cmd_config(client_id: String) -> Result<()> {
    let cfg = config::Config {
        client_id: Some(client_id),
        client_secret: None,
    };
    config::save_config(&cfg)?;
    println!("Custom client ID saved to {:?}", config::config_dir());
    Ok(())
}

async fn cmd_login() -> Result<()> {
    let cfg = config::load_config()?;
    let client_id = cfg.client_id();
    let client_secret = cfg.client_secret();
    auth::login(client_id, client_secret).await?;
    println!("Login successful! Tokens saved.");
    Ok(())
}

async fn cmd_labels(json: bool) -> Result<()> {
    let client = get_client().await?;
    let labels = client.list_labels().await?;

    if let Some(labels) = labels.labels {
        if json {
            println!("{}", serde_json::to_string(&labels)?);
        } else {
            let mut system: Vec<_> = labels
                .iter()
                .filter(|l| l.label_type.as_deref() == Some("system"))
                .collect();
            let mut user: Vec<_> = labels
                .iter()
                .filter(|l| l.label_type.as_deref() != Some("system"))
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
    }
    Ok(())
}

async fn cmd_list(
    max: u32,
    query: Option<String>,
    label: String,
    unread: bool,
    json: bool,
) -> Result<()> {
    let client = get_client().await?;
    let label_id = normalize_label(&label);
    let query = if unread {
        Some(match query {
            Some(q) => format!("is:unread {}", q),
            None => "is:unread".to_string(),
        })
    } else {
        query
    };
    let list = client
        .list_messages(query.as_deref(), &label_id, max)
        .await?;

    if let Some(messages) = list.messages {
        if json {
            let mut items = Vec::new();
            for msg_ref in messages {
                let msg = client.get_message(&msg_ref.id).await?;
                items.push(serde_json::json!({
                    "id": msg.id,
                    "from": msg.get_header("From"),
                    "to": msg.get_header("To"),
                    "subject": msg.get_header("Subject"),
                    "date": msg.get_header("Date"),
                    "snippet": msg.snippet,
                }));
            }
            println!("{}", serde_json::to_string(&items)?);
        } else {
            for msg_ref in messages {
                let msg = client.get_message(&msg_ref.id).await?;
                let from = msg.get_header("From").unwrap_or("Unknown");
                let subject = msg.get_header("Subject").unwrap_or("(no subject)");
                println!("{} | {} | {}", msg.id, from, subject);
            }
        }
    } else if !json {
        println!("No messages found.");
    } else {
        println!("[]");
    }
    Ok(())
}

async fn cmd_read(id: String, html: bool, json: bool) -> Result<()> {
    let client = get_client().await?;
    let msg = client.get_message(&id).await?;

    if html {
        if let Some(html_body) = msg.get_body_html() {
            println!("{}", html_body);
        } else {
            eprintln!("No HTML body found");
        }
    } else if json {
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
    } else {
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
    Ok(())
}

async fn cmd_archive(id: String) -> Result<()> {
    let client = get_client().await?;
    client.archive(&id).await?;
    println!("Archived {}", id);
    Ok(())
}

async fn cmd_spam(id: String) -> Result<()> {
    let client = get_client().await?;
    let _ = client.unsubscribe(&id).await;
    client.mark_spam(&id).await?;
    println!("Marked as spam {}", id);
    Ok(())
}

async fn cmd_unspam(id: String) -> Result<()> {
    let client = get_client().await?;
    client.unspam(&id).await?;
    println!("Moved to inbox {}", id);
    Ok(())
}

async fn cmd_label(id: String, label: String) -> Result<()> {
    let client = get_client().await?;
    let label_id = normalize_label(&label);
    client.add_label(&id, &label_id).await?;
    println!("Added label {} to {}", label, id);
    Ok(())
}

async fn cmd_unlabel(id: String, label: String) -> Result<()> {
    let client = get_client().await?;
    let label_id = normalize_label(&label);
    client.remove_label(&id, &label_id).await?;
    println!("Removed label {} from {}", label, id);
    Ok(())
}

async fn cmd_delete(id: String) -> Result<()> {
    let client = get_client().await?;
    client.trash(&id).await?;
    println!("Moved to trash {}", id);
    Ok(())
}

async fn cmd_undelete(id: String) -> Result<()> {
    let client = get_client().await?;
    client.untrash(&id).await?;
    println!("Restored from trash {}", id);
    Ok(())
}

async fn cmd_mark_read(id: String) -> Result<()> {
    let client = get_client().await?;
    client.mark_read(&id).await?;
    println!("Marked as read {}", id);
    Ok(())
}

async fn cmd_mark_unread(id: String) -> Result<()> {
    let client = get_client().await?;
    client.mark_unread(&id).await?;
    println!("Marked as unread {}", id);
    Ok(())
}

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

async fn cmd_unsubscribe(id: String) -> Result<()> {
    let client = get_client().await?;
    client.unsubscribe(&id).await?;
    println!("Unsubscribed from {}", id);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Config { client_id } => cmd_config(client_id).await?,
        Commands::Login => cmd_login().await?,
        Commands::Labels => cmd_labels(cli.json).await?,
        Commands::List {
            max,
            query,
            label,
            unread,
        } => cmd_list(max, query, label, unread, cli.json).await?,
        Commands::Read { id, html } => cmd_read(id, html, cli.json).await?,
        Commands::Archive { id } => cmd_archive(id).await?,
        Commands::Spam { id } => cmd_spam(id).await?,
        Commands::Unspam { id } => cmd_unspam(id).await?,
        Commands::Label { id, label } => cmd_label(id, label).await?,
        Commands::Unlabel { id, label } => cmd_unlabel(id, label).await?,
        Commands::Delete { id } => cmd_delete(id).await?,
        Commands::Undelete { id } => cmd_undelete(id).await?,
        Commands::MarkRead { id } => cmd_mark_read(id).await?,
        Commands::MarkUnread { id } => cmd_mark_unread(id).await?,
        Commands::ClearLabels { id } => cmd_clear_labels(id).await?,
        Commands::Unsubscribe { id } => cmd_unsubscribe(id).await?,
    }

    Ok(())
}
