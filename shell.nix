{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  buildInputs = with pkgs; [
    rustc
    cargo
    pkg-config

    # X11 dependencies
    xorg.libX11
    xorg.libXcursor
    xorg.libXrandr
    xorg.libXi
    xorg.libxcb

    # For testing in nested X
    xorg.xorgserver
  ];

  shellHook = ''
    echo "gar development shell"
    echo "Run 'cargo build' to build"
    echo "Test with: Xephyr -br -ac -noreset -screen 1280x720 :1 &"
    echo "Then: DISPLAY=:1 cargo run"
  '';

  RUST_BACKTRACE = "1";
}
