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
gmail draft-reply <id> --body-file reply.txt --attach invoice.pdf --attach photo.jpg  # Create an unsent reply draft
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
  --attach invoice.pdf \\
  --attach photo.jpg
```

- `--body-file PATH` is required and supplies the UTF-8 reply body.
- Repeat `--attach PATH` to include multiple local files. The reply body is a UTF-8 `text/plain` MIME part; attachments make the message `multipart/mixed`, use the local filename, and are base64-encoded. Content types are selected from common filename extensions; unknown extensions use `application/octet-stream`.
- The command creates a Gmail draft and prints its draft ID; it never sends the message.
- The source message ID determines the reply: the draft addresses the source message's sender, adds `Re:` when needed, sets `In-Reply-To` from the source `Message-ID` when available, preserves `References` and appends that `Message-ID` when it is not already present, and passes the source Gmail `threadId` so Gmail keeps the draft in that thread.
- The source message must have a Gmail thread ID and a `From` header.

## License

MIT
