{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, rust-overlay, nixpkgs }:
    let
      overlays = [ (import rust-overlay) ];
      pkgs = import nixpkgs {
        system = "x86_64-linux";
        inherit overlays;
      };
    in
    {
      devShell.x86_64-linux = pkgs.mkShell {
        buildInputs = [
          (pkgs.rust-bin.selectLatestNightlyWith (toolchain: toolchain.default.override {
            targets = [ "thumbv7em-none-eabi" ];
            extensions = [ "rust-src" ];
          }))
          pkgs.rust-analyzer
          pkgs.flip-link
          pkgs.probe-rs
          pkgs.rustfmt
        ];
      };
    };
}