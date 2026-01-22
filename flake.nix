{
  description = "Diaryx - Command-line interface and development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, crane, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        inherit (pkgs) lib;

        rustToolchain = pkgs.rust-bin.stable."1.91.0".default.override {
          extensions = [ "rust-src" "rust-analyzer" "clippy" "rustfmt" ];
        };

        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        src = craneLib.cleanCargoSource (craneLib.path ./.);

        commonArgs = {
          inherit src;
          strictDeps = true;

          buildInputs = [
            pkgs.stdenv.cc.cc.lib
          ] ++ lib.optionals pkgs.stdenv.isDarwin (
            with pkgs.apple-sdk_15; [
              pkgs.libiconv
              Security
              CoreFoundation
              SystemConfiguration
            ]
          );

          nativeBuildInputs = [
            pkgs.pkg-config
          ] ++ lib.optionals pkgs.stdenv.isDarwin [
            pkgs.apple-sdk_15
          ];
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        diaryx-cli = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          pname = "diaryx";
          cargoExtraArgs = "-p diaryx";
        });

      in
      {
        packages.default = diaryx-cli;

        apps.default = flake-utils.lib.mkApp {
          drv = diaryx-cli;
        };

        devShells.default = craneLib.devShell {
          inputsFrom = [ diaryx-cli ];

          packages = with pkgs; [
            rustToolchain
            cargo-release
            cargo-binstall
            wasm-pack
            bun
          ];

          shellHook = ''
            echo "Welcome to the Diaryx development environment!"
            echo "Rust: $(rustc --version)"
            echo "Bun:  $(bun --version)"
          '';
        };
      });
}
