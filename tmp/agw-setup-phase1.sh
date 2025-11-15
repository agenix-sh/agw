#!/usr/bin/env bash
set -e

REPO="agenix-sh/agw"

echo "🚀 Initialising Phase 1 scaffolding for AGW"

gh repo clone $REPO agw
cd agw || exit 1

# Minimal Rust worker binary
gh issue create --title "AGW-001: Create minimal Rust worker binary" --body "Create a Rust binary that connects to AGQ via RESP, performs AUTH, and prints a heartbeat message."

# Session-key auth client
gh issue create --title "AGW-002: Implement session-key authentication" --body "Implement AUTH <session-key> sequence on connect and fail closed if authentication is rejected."

# RESP client implementation
gh issue create --title "AGW-003: Implement RESP client" --body "Implement RESP serialization/deserialization for commands like AUTH, BRPOP, HSET, HGET."

# Worker heartbeat
gh issue create --title "AGW-004: Implement worker heartbeat" --body "Send heartbeat keys to AGQ using SET worker:<id>:alive 1 EX 10 on a recurring interval."

# Blocking job fetch
gh issue create --title "AGW-005: Implement blocking job fetch (BRPOP equivalent)" --body "Continuously fetch jobs using BRPOP queue:ready to obtain next job step."

# Job execution engine
gh issue create --title "AGW-006: Implement step execution engine" --body "Execute Unix tools or agent tools using subprocess. Capture stdout, stderr, exit code."

# Result posting
gh issue create --title "AGW-007: Implement result posting to AGQ" --body "Store job output into job:<id>:stdout, stderr, and update job status."

# Tool discovery interface
gh issue create --title "AGW-008: Implement tool availability metadata" --body "Allow AGW to declare supported tools (e.g., 'agx-ocr') to AGQ for future capability negotiation."

# Graceful shutdown
gh issue create --title "AGW-009: Implement shutdown and job interruption" --body "Support SIGINT/SIGTERM, stop fetching new jobs, allow current job to complete safely."

# Logging + error handling
gh issue create --title "AGW-010: Add structured logging and error handling" --body "Implement tracing-based structured logs, categorized errors, and worker-level diagnostics."

echo "✅ Phase 1 issues created for $REPO"
