{ pkgs ? import <nixpkgs> { } }:

pkgs.mkShell {
  name = "haiker-dev";

  buildInputs = with pkgs; [
    rustc
    cargo
    rustfmt
    clippy
  ];
}
