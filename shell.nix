# https://nixos.wiki/wiki/Rust
# https://github.com/mozilla/nixpkgs-mozilla

let
  # 2021-08-17
  moz_overlay = import (builtins.fetchTarball https://github.com/mozilla/nixpkgs-mozilla/archive/0510159186dd2ef46e5464484fbdf119393afa58.tar.gz);
  nixpkgs = import <nixpkgs> { overlays = [ moz_overlay ]; };
  #rustChannel = nixpkgs.latest.rustChannels.nightly;
  rustChannel = (nixpkgs.rustChannelOf { date = "2021-09-28"; channel = "nightly"; });
  #rustChannel = (nixpkgs.rustChannelOf { rustToolchain = ./rust-toolchain.toml; }); # use the project's rust-toolchain file
in
  with nixpkgs;
  stdenv.mkDerivation {
    name = "moz_overlay_shell";
    buildInputs = [
      rustChannel.rust
      rustChannel.cargo
    ];
  }
  
