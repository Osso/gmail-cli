# gmail-cli

[![CI](https://github.com/Osso/gmail-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/Osso/gmail-cli/actions/workflows/ci.yml)
[![GitHub release](https://img.shields.io/github/v/release/Osso/gmail-cli)](https://github.com/Osso/gmail-cli/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

CLI for Gmail API access.

## Installation

```bash
cargo install --path .
```

## Setup

```bash
gmail login  # Opens browser for OAuth
```

## Usage

```bash
gmail list                  # List messages
gmail list --unread         # List unread messages
gmail read <id>             # Read a specific message
gmail draft-reply <id> --body-file reply.txt --to abusereply@cloudflare.com  # Create an unsent reply draft to an explicit recipient
gmail archive <id>          # Archive message
gmail spam <id>             # Mark as spam
gmail label <id> <label>    # Add label
gmail delete <id>           # Move to trash
gmail unsubscribe <id>      # Open unsubscribe link
```

### `draft-reply`

```bash
gmail draft-reply <MESSAGE_ID> \\
  --body-file reply.txt \\
  --to abusereply@cloudflare.com \\
  --attach invoice.pdf \\
  --attach photo.jpg
```

- `--body-file PATH` is required and supplies the UTF-8 reply body.
- By default, the draft replies to the source message's sender. Use `--to ADDRESS` to override that recipient; the command still preserves `Re:`, `In-Reply-To`, `References`, and the source Gmail `threadId`, so the result remains an unsent draft in the original reply thread.
- Repeat `--attach PATH` to include multiple local files. The reply body is a UTF-8 `text/plain` MIME part; attachments make the message `multipart/mixed`, use the local filename, and are base64-encoded. Content types are selected from common filename extensions; unknown extensions use `application/octet-stream`.
- The command creates a Gmail draft and prints its draft ID; it never sends the message.
- Example: reply to a Cloudflare abuse report through the review mailbox without sending: `gmail draft-reply <CLOUDFLARE_MESSAGE_ID> --body-file response.txt --to abusereply@cloudflare.com`.
- The source message must have a Gmail thread ID and a `From` header.

## License

MIT
