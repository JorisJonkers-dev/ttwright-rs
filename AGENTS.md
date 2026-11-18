# Agents

- Agent contract: [CLAUDE.md](CLAUDE.md)
- Estate conventions: https://github.com/JorisJonkers-dev/workspace/blob/main/CLAUDE.md
- Upstream relationship and porting: [FORK.md](FORK.md)
- Contracts (result, events, handshake): `crates/ttwright-contract`, schemas in [contracts/](contracts); regenerate with `cargo run -p ttwright-cli -- schema <result|events|contracts> > contracts/ttwright-<name>.v1.json`

## Build and test

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo test --workspace --locked`
- Coverage and the 70% changed-line gate: [docs/COVERAGE.md](docs/COVERAGE.md)
- Fuzz harnesses: [fuzz/README.md](fuzz/README.md)
- `mise install`, then `mise exec -- cargo deny --locked check` and `mise exec -- cargo audit`
- Apply the branch rulesets: `cargo xtask apply-ruleset [--allow-bypass-removal] [<owner/repo>]`
