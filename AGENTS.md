# Agents

- Agent contract: [CLAUDE.md](CLAUDE.md)
- Estate conventions: https://github.com/JorisJonkers-dev/workspace/blob/main/CLAUDE.md
- Upstream relationship and porting: [FORK.md](FORK.md)

## Build and test

- `cargo fmt --all --check`
- `cargo clippy --all-targets --locked -- -D warnings`
- `cargo test --locked`
- Apply the branch rulesets: `scripts/apply-ruleset.sh`
