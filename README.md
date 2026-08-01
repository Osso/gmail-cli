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

`draft-reply` preserves the source Gmail thread and creates an unsent RFC 2822 reply draft. Repeat `--attach PATH` for multiple real files; the command MIME-encodes each attachment and never sends the message.

## License

MIT
