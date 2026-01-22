{
  description = "Diaryx - Command-line interface and development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          inherit (pkgs) lib;

          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              (lib.fileset.fileFilter (file: file.hasExt "rs") ./.)
              (lib.fileset.fileFilter (file: file.hasExt "toml") ./.)
              (lib.fileset.fileFilter (file: file.name == "Cargo.lock") ./.)
              (lib.fileset.fileFilter (file: file.hasExt "md") ./.)
            ];
          };

          diaryx-cli = pkgs.rustPlatform.buildRustPackage {
            pname = "diaryx";
            version = "0.11.0";
            inherit src;
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = [ "-p" "diaryx" ];

            buildInputs = lib.optionals pkgs.stdenv.isDarwin [
              pkgs.apple-sdk_15
            ];

            nativeBuildInputs = [ pkgs.pkg-config ];
          };
        in
        {
          default = diaryx-cli;
        });

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/diaryx";
        };
      });

      devShells = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              rustc
              cargo
              rust-analyzer
              clippy
              rustfmt
              cargo-binstall
              wasm-pack
              bun
              pkg-config
              prek
            ] ++ lib.optionals pkgs.stdenv.isDarwin [
              apple-sdk_15
            ];

            shellHook = ''
              echo "Welcome to the Diaryx development environment!"
              echo "Rust: $(rustc --version)"
              echo "Bun:  $(bun --version)"
            '';
          };
        });
    };
}
