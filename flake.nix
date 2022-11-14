{
  description = "Example rust project";
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem
      (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ self.overlays.default ];
          };
        in
        {
          packages = rec {
            nix-otel = pkgs.nix-otel;
            default = nix-otel;
          };
          checks = self.packages.${system};

          # for debugging
          inherit pkgs;

          devShells.default = pkgs.nix-otel.overrideAttrs (
            old: {
              # make rust-analyzer work
              RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;

              # any dev tools you use in excess of the rust ones
              nativeBuildInputs = old.nativeBuildInputs ++ (
                with pkgs; [
                  nix
                  bear
                  rust-analyzer
                  rust-cbindgen
                  clang-tools_14
                ]
              );
            }
          );
        }
      )
    // {
      overlays.default = (
        final: prev:
          let
            inherit (prev) lib;
          in
          {
            nix-otel = final.rustPlatform.buildRustPackage {
              pname = "nix-otel";
              version = "0.1.0";

              cargoLock = {
                lockFile = ./Cargo.lock;
              };

              src = ./.;

              # tools on the builder machine needed to build; e.g. pkg-config
              nativeBuildInputs = with final; [
                pkg-config
                protobuf
              ];

              # native libs
              buildInputs = with final; [
                boost
                nix
              ] ++ lib.optional final.stdenv.isDarwin
                final.darwin.apple_sdk.frameworks.Security;
            };
          }
      );
    };
}
