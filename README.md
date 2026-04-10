[![Commit Score](https://commit-backend.fly.dev/badge/github/hawkaa/commit.svg)](https://commit-backend.fly.dev/trust/github/hawkaa/commit)

# Commit

Behavioral trust layer that surfaces ZK-verified commitment signals alongside search results and GitHub repos.

Commit Score (0-100) measures long-term commitment based on public signals like project age, maintenance activity, and community size.

## How it works

- **Chrome extension** injects trust cards on GitHub repos and Google search results
- **Trust card pages** at `commit-backend.fly.dev/trust/{kind}/{id}` show score breakdowns
- **Badge API** generates embeddable SVG badges for READMEs
- **MCP server** lets AI assistants query Commit Scores

## Development

```bash
cargo run              # Start backend on :3000
cargo test             # Run tests
cargo clippy -- -D warnings
```

Load the `extension/` directory as an unpacked Chrome extension for local testing.
