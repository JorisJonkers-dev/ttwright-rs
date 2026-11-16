# Agent contract

The estate-wide conventions live in one place and are **not duplicated here**:

**https://github.com/JorisJonkers-dev/workspace/blob/main/CLAUDE.md**

Read it before doing anything non-trivial in this repository. It covers pull request labels
(run `gh label list --repo JorisJonkers-dev/rauls-trusty-toolbox` before passing `--label`),
verifying values rather than commands, the testing standard, and traps around workflow runs and
detached submodule HEADs.

---

## This repository

- Read [FORK.md](FORK.md) first. `upstream` is written only by the sync workflow; work goes to `main`
  by pull request.
- Licence is GPL-3.0-only (upstream's). Keep upstream authors' credit on ported code.
- No game data, MPI packages or mod archives enter this repository. Fixtures are synthetic.
- Building on Linux needs `make`, a C compiler and `pkg-config` (`mp3lame-sys`, `vorbis_rs`).
