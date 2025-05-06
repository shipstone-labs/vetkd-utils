# Claude Configuration

This file contains instructions for Claude when working with this repository.

## Pre-Commit Checks

Before committing code, Claude should run the following checks:

```bash
cargo test

# Run clippy
cargo clippy

# Check compilation
cargo check --all
```

## Project Guidelines

- Always use workspace dependencies when possible
- Maintain consistent dependency versions across packages
- Standardize on ic-stable-structures version 0.6.8
- Standardize on serde_json version 1.0.135
- Use workspace references for common dependencies like serde