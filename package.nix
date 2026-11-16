{
  lib,
  rustPlatform,
  pkg-config,
  zlib,
}:
rustPlatform.buildRustPackage {
  pname = "ttwright";
  version = "0.2.0";

  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./src
      ./examples
    ];
  };

  cargoLock.lockFile = ./Cargo.lock;

  nativeBuildInputs = [pkg-config];

  # libz-sys links against zlib.
  buildInputs = [zlib];

  meta = {
    description = "Installer for Tale of Two Wastelands and other MPI packages, driven from the command line";
    homepage = "https://github.com/JorisJonkers-dev/ttwright-rs";
    license = lib.licenses.gpl3Only;
    platforms = lib.platforms.linux;
    mainProgram = "mpi_installer";
  };
}
