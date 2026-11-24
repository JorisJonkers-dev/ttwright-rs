# ttwright-rs

**ttwright installs [Tale of Two Wastelands](https://taleoftwowastelands.com/)**, the mod that brings Fallout 3's
world into Fallout: New Vegas, so you can play both games' stories in one. It builds it from your own copies of both
games, on Linux, Windows and macOS, and leaves both games untouched.

<p align="center">
  <img src="docs/images/how-it-works.svg" alt="ttwright-rs builds Tale of Two Wastelands from your own Fallout 3 and New Vegas and the TTW package, and raul adds it as one Mod" width="100%">
</p>

## What you see

ttwright is the **Tale of Two Wastelands** page of [raul](https://github.com/JorisJonkers-dev/rauls-trusty-toolbox):
pick your games and the TTW download, check the space it needs, and install.

<p align="center">
  <img src="https://raw.githubusercontent.com/JorisJonkers-dev/rauls-trusty-toolbox/main/crates/raul-gui/tests/snapshots/ttw/ttw-sources-ready.png" alt="raul's TTW page, step 1 of 4: the Fallout 3 folder, New Vegas and the TTW package are set" width="49%">
  <img src="https://raw.githubusercontent.com/JorisJonkers-dev/rauls-trusty-toolbox/main/crates/raul-gui/tests/snapshots/ttw/ttw-installing.png" alt="raul's TTW page while it installs, at 25 percent" width="49%">
</p>

## What it does for you

- 🐧 **Any system**: runs natively on Linux, Windows and macOS; Linux needs no Windows tools.
- ⏸️ **Stop and continue**: cancel an install and pick it up later where it stopped.
- 🛡️ **Safe**: it writes only into its own output folder, and checks the package before it writes anything.
- 🧩 **One Mod**: raul adds the result to your mod list as one Mod.

You need your own Fallout 3 and Fallout: New Vegas, and the TTW package from
[mod.pub](https://mod.pub/ttw/133/files) (TTW 3.3 and 3.4 are supported).

<!-- Progress picture: docs/coverage/tally.svg goes here once the feature catalogue reaches this repository. -->

## How it fits together

<p align="center">
  <img src="docs/images/raul-parts.svg" alt="raul and its five parts: nv-rs the game engine, nvmod-rs mods, loot-rs load order, lodwright-rs distant view, ttwright-rs Tale of Two Wastelands (this project)" width="100%">
</p>

ttwright is one part of raul. As its own project it is tested on its own, and it keeps the history of the installer it
was rewritten from, [SulfurNitride/TTW_Linux_Installer](https://github.com/SulfurNitride/TTW_Linux_Installer).

## Get it

**[Download raul](https://github.com/JorisJonkers-dev/rauls-trusty-toolbox/releases/latest)**: ttwright is built in.
It also works on its own from the command line ([docs/USAGE.md](docs/USAGE.md)).

## Learn more

- [Using ttwright](docs/USAGE.md): raul's TTW page, supported versions, getting the package, the commands, downloads.
- [More detail](docs/OVERVIEW.md): where it comes from, why it is its own project, building it.

No game files and no TTW package are in this repository or its releases. Licence: GPL-3.0-only ([LICENSE](LICENSE)).
Report problems here, not upstream ([FORK.md](FORK.md)).
