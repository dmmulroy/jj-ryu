{
  description = "jj-ryu - Stacked PRs CLI for Jujutsu";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      flake-utils,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        lib = pkgs.lib;

        rust = pkgs.rust-bin.stable."1.89.0".default;
        rustPlatform = pkgs.makeRustPlatform {
          cargo = rust;
          rustc = rust;
        };

        ryu = rustPlatform.buildRustPackage {
          pname = "jj-ryu";
          version = "0.0.1-alpha.9";
          src = ./.;
          cargoLock = {
            lockFile = ./Cargo.lock;
          };
          nativeBuildInputs = [
            pkgs.pkg-config
            pkgs.jujutsu
            pkgs.git
          ];
          buildInputs = lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];
          doCheck = true;
        };
      in
      {
        packages = {
          default = ryu;
          ryu = ryu;
        };

        apps.default = flake-utils.lib.mkApp { drv = ryu; };
        apps.ryu = flake-utils.lib.mkApp { drv = ryu; };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ ryu ];
          packages = [
            rust
            pkgs.pkg-config
          ];
        };
      }
    );
}
