{
  lib,
  rustPlatform,
  pkg-config,
  zlib,
}:
rustPlatform.buildRustPackage {
  pname = "ttwright";
  version = "0.4.1"; # x-release-please-version

  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./crates
      ./contracts
    ];
  };

  cargoLock.lockFile = ./Cargo.lock;

  cargoBuildFlags = ["-p" "ttwright-cli"];
  # The tests run in CI; they build fixtures in the temporary folder, which the sandbox allows, but they
  # double the build time of the package.
  doCheck = false;

  nativeBuildInputs = [pkg-config];

  # libz-sys links against zlib.
  buildInputs = [zlib];

  meta = {
    description = "Installer for Tale of Two Wastelands and other MPI packages, driven from the command line";
    homepage = "https://github.com/JorisJonkers-dev/ttwright-rs";
    license = lib.licenses.gpl3Only;
    platforms = lib.platforms.linux;
    mainProgram = "ttwright";
  };
}
