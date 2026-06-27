{ pkgs ? import <nixpkgs> {} }:

let
  libraries = with pkgs; [
    udev
    alsa-lib
    vulkan-loader
    libxkbcommon
    wayland
    libx11
    libxcursor
    libxi
    libxrandr
  ];

  packages = with pkgs; [
    cargo
    rustc
    pkg-config
    systemd        
    wayland
    libxkbcommon
    libGL
    vulkan-loader
    alsa-lib
  ];
in
pkgs.mkShell {
  buildInputs = packages;

  nativeBuildInputs = with pkgs; [
    pkg-config
  ];

  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath libraries;
}
