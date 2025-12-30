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

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Config { client_id } => {
            let cfg = config::Config {
                client_id: Some(client_id),
                client_secret: None,
            };
            config::save_config(&cfg)?;
            println!("Custom client ID saved to {:?}", config::config_dir());
        }
        Commands::Login => {
            let cfg = config::load_config()?;
            let client_id = cfg.client_id();
            let client_secret = cfg.client_secret();

            auth::login(client_id, client_secret).await?;
            println!("Login successful! Tokens saved.");
        }
        Commands::Labels => {
            let client = get_client().await?;
            let labels = client.list_labels().await?;

            if let Some(labels) = labels.labels {
                if cli.json {
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
        }
        Commands::List {
            max,
            query,
            label,
            unread,
        } => {
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
                if cli.json {
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
            } else if !cli.json {
                println!("No messages found.");
            } else {
                println!("[]");
            }
        }
        Commands::Read { id } => {
            let client = get_client().await?;
            let msg = client.get_message(&id).await?;

            if cli.json {
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
        }
        Commands::Archive { id } => {
            let client = get_client().await?;
            client.archive(&id).await?;
            println!("Archived {}", id);
        }
        Commands::Spam { id } => {
            let client = get_client().await?;
            // Try to unsubscribe first, ignore errors (not all messages have unsubscribe)
            let _ = client.unsubscribe(&id).await;
            client.mark_spam(&id).await?;
            println!("Marked as spam {}", id);
        }
        Commands::Unspam { id } => {
            let client = get_client().await?;
            client.unspam(&id).await?;
            println!("Moved to inbox {}", id);
        }
        Commands::Label { id, label } => {
            let client = get_client().await?;
            let label_id = normalize_label(&label);
            client.add_label(&id, &label_id).await?;
            println!("Added label {} to {}", label, id);
        }
        Commands::Unlabel { id, label } => {
            let client = get_client().await?;
            let label_id = normalize_label(&label);
            client.remove_label(&id, &label_id).await?;
            println!("Removed label {} from {}", label, id);
        }
        Commands::Delete { id } => {
            let client = get_client().await?;
            client.trash(&id).await?;
            println!("Moved to trash {}", id);
        }
        Commands::MarkRead { id } => {
            let client = get_client().await?;
            client.mark_read(&id).await?;
            println!("Marked as read {}", id);
        }
        Commands::MarkUnread { id } => {
            let client = get_client().await?;
            client.mark_unread(&id).await?;
            println!("Marked as unread {}", id);
        }
        Commands::ClearLabels { id } => {
            let client = get_client().await?;
            let removed = client.clear_labels(&id).await?;
            if removed.is_empty() {
                println!("No user labels to remove from {}", id);
            } else {
                println!("Removed {} labels from {}", removed.len(), id);
            }
        }
        Commands::Unsubscribe { id } => {
            let client = get_client().await?;
            client.unsubscribe(&id).await?;
            println!("Unsubscribed from {}", id);
        }
    }

    Ok(())
}
