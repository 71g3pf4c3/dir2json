{
  pkgs ? import <nixpkgs> { },
}:

pkgs.rustPlatform.buildRustPackage {
  pname = "dir2json";
  version = "0.1.0";
  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;

  meta = {
    description = "Tool that converts directory trees to JSON objects";
    homepage = "https://github.com/71g3pf4c3/dir2json";
    license = pkgs.lib.licenses.gpl3Only;
    mainProgram = "dir2json";
  };
}
