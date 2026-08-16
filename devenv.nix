{ pkgs, ... }:

{
  languages.rust = {
    enable = true;
    toolchainFile = ./rust-toolchain.toml;
  };

  packages = with pkgs; [
    actionlint
    libnotify
    libxkbcommon
    nixfmt-tree
    pkg-config
    tombi
    wayland
    wl-clipboard
  ];
}
