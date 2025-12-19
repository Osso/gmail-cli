mod api;
mod auth;
mod config;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "gmail")]
#[command(about = "CLI tool to access Gmail API")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Set OAuth client credentials (from Google Cloud Console)
    Config {
        /// Client ID
        client_id: String,
    },
    /// Authenticate with Gmail (opens browser)
    Login,
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
    /// Unsubscribe from a mailing list (opens unsubscribe link)
    Unsubscribe {
        /// Message ID
        id: String,
    },
}

async fn get_client() -> Result<api::Client> {
    let cfg = config::load_config()?;
    let client_id = cfg.client_id.ok_or_else(|| {
        anyhow::anyhow!("Not configured. Run 'gmail config <client-id>' first")
    })?;
    let client_secret = cfg.client_secret.ok_or_else(|| {
        anyhow::anyhow!("Not configured. Run 'gmail config <client-id>' first")
    })?;

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
            let new_tokens = auth::refresh_token(&client_id, &client_secret, &tokens.refresh_token).await?;
            Ok(api::Client::new(&new_tokens.access_token))
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Config { client_id } => {
            let secret = rpassword::prompt_password("Client Secret: ")?;
            if secret.is_empty() {
                anyhow::bail!("Client secret cannot be empty");
            }

            let cfg = config::Config {
                client_id: Some(client_id),
                client_secret: Some(secret),
            };
            config::save_config(&cfg)?;
            println!("Credentials saved to {:?}", config::config_dir());
        }
        Commands::Login => {
            let cfg = config::load_config()?;
            let client_id = cfg.client_id.ok_or_else(|| {
                anyhow::anyhow!("Not configured. Run 'gmail config <client-id>' first")
            })?;
            let client_secret = cfg.client_secret.ok_or_else(|| {
                anyhow::anyhow!("Not configured. Run 'gmail config <client-id>' first")
            })?;

            auth::login(&client_id, &client_secret).await?;
            println!("Login successful! Tokens saved.");
        }
        Commands::List { max, query, label, unread } => {
            let client = get_client().await?;
            let label_lower = label.to_lowercase();
            let label_id = match label_lower.as_str() {
                "inbox" => "INBOX",
                "sent" => "SENT",
                "trash" => "TRASH",
                "spam" => "SPAM",
                "starred" => "STARRED",
                "drafts" => "DRAFT",
                "all" => "",
                _ => &label_lower,
            };
            let query = if unread {
                Some(match query {
                    Some(q) => format!("is:unread {}", q),
                    None => "is:unread".to_string(),
                })
            } else {
                query
            };
            let list = client.list_messages(query.as_deref(), label_id, max).await?;

            if let Some(messages) = list.messages {
                for msg_ref in messages {
                    let msg = client.get_message(&msg_ref.id).await?;
                    let from = msg.get_header("From").unwrap_or("Unknown");
                    let subject = msg.get_header("Subject").unwrap_or("(no subject)");
                    println!("{} | {} | {}", msg.id, from, subject);
                }
            } else {
                println!("No messages found.");
            }
        }
        Commands::Read { id } => {
            let client = get_client().await?;
            let msg = client.get_message(&id).await?;

            println!("From: {}", msg.get_header("From").unwrap_or("Unknown"));
            println!("To: {}", msg.get_header("To").unwrap_or("Unknown"));
            println!("Subject: {}", msg.get_header("Subject").unwrap_or("(no subject)"));
            println!("Date: {}", msg.get_header("Date").unwrap_or("Unknown"));
            println!("---");

            if let Some(body) = msg.get_body_text() {
                println!("{}", body);
            } else if let Some(snippet) = &msg.snippet {
                println!("{}", snippet);
            }
        }
        Commands::Archive { id } => {
            let client = get_client().await?;
            client.archive(&id).await?;
            println!("Archived {}", id);
        }
        Commands::Spam { id } => {
            let client = get_client().await?;
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
            client.add_label(&id, &label.to_uppercase()).await?;
            println!("Added label {} to {}", label, id);
        }
        Commands::Unlabel { id, label } => {
            let client = get_client().await?;
            client.remove_label(&id, &label.to_uppercase()).await?;
            println!("Removed label {} from {}", label, id);
        }
        Commands::Delete { id } => {
            let client = get_client().await?;
            client.trash(&id).await?;
            println!("Moved to trash {}", id);
        }
        Commands::Unsubscribe { id } => {
            let client = get_client().await?;
            let msg = client.get_message(&id).await?;

            if let Some(unsub) = msg.get_header("List-Unsubscribe") {
                // Extract URL from header (format: <url> or <mailto:...>)
                let url = unsub
                    .split(',')
                    .find_map(|part| {
                        let trimmed = part.trim().trim_matches(|c| c == '<' || c == '>');
                        if trimmed.starts_with("http") {
                            Some(trimmed)
                        } else {
                            None
                        }
                    });

                if let Some(url) = url {
                    println!("Opening unsubscribe link...");
                    if std::process::Command::new("vivaldi").arg(url).spawn().is_err() {
                        open::that(url)?;
                    }
                } else {
                    println!("No HTTP unsubscribe link found. Header: {}", unsub);
                }
            } else {
                println!("No unsubscribe header found in this message");
            }
        }
    }

    Ok(())
}
