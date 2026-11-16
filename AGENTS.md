# Agents

- Agent contract: [CLAUDE.md](CLAUDE.md)
- Estate conventions: https://github.com/JorisJonkers-dev/workspace/blob/main/CLAUDE.md
- Upstream relationship and porting: [FORK.md](FORK.md)

## Build and test

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo test --workspace --locked`
- Coverage and the 70% changed-line gate: [docs/COVERAGE.md](docs/COVERAGE.md)
- Fuzz harnesses: [fuzz/README.md](fuzz/README.md)
- `mise install`, then `mise exec -- cargo deny --locked check` and `mise exec -- cargo audit`
- Apply the branch rulesets: `scripts/apply-ruleset.sh`
