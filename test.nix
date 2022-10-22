# a test derivation that just sleeps a little
let
  pkgs = import <nixpkgs> {};
in
  pkgs.stdenv.mkDerivation {
    unpackPhase = ''
      sleep 0.5
    '';
    name = "test-0.0.0";
    configurePhase = ''
      sleep 0.5
    '';
    buildPhase = ''
      sleep 0.5
    '';
    installPhase = ''
      sleep 0.2
      mkdir $out
      exit 1
    '';
  }
