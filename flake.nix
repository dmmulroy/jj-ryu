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
    {
      overlays.default = final: prev: {
        jj-ryu = self.packages.${final.system}.ryu;
      };
    }
    // flake-utils.lib.eachDefaultSystem (
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

        version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;
        versionSuffix = if self ? rev then "-${builtins.substring 0 7 self.rev}" else "-dirty";

        src = pkgs.lib.fileset.toSource {
          root = ./.;
          fileset = pkgs.lib.fileset.unions [
            ./Cargo.toml
            ./Cargo.lock
            ./src
            ./tests
          ];
        };

        meta = with pkgs.lib; {
          description = "Stacked PRs for Jujutsu with GitHub/GitLab support";
          homepage = "https://github.com/dmmulroy/jj-ryu";
          changelog = "https://github.com/dmmulroy/jj-ryu/releases/tag/v${version}";
          license = licenses.mit;
          maintainers = [ ];
          mainProgram = "ryu";
          platforms = platforms.unix;
        };

        ryu = rustPlatform.buildRustPackage {
          pname = "jj-ryu";
          version = "${version}${versionSuffix}";
          inherit src meta;
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

        checks = {
          ryu = ryu;
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ ryu ];
          packages = [
            rust
            pkgs.pkg-config
            pkgs.rust-analyzer
            pkgs.cargo-watch
          ];
        };

        formatter = pkgs.nixfmt-rfc-style;
      }
    );
}
