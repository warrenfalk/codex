{
  description = "Haskell sketch of the Codex TUI event-sourced transcript model";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { nixpkgs, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
    in
    {
      devShells.${system}.default = pkgs.mkShell {
        packages = [
          pkgs.cabal-install
          pkgs.ghc
        ];
      };
    };
}
