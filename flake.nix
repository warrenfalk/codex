{
  description = "Development Nix flake for OpenAI Codex CLI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, rust-overlay, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems f;

      # Read the version from the workspace Cargo.toml (the single source of
      # truth used by the release workflow).
      cargoToml = builtins.fromTOML (builtins.readFile ./codex-rs/Cargo.toml);
      cargoVersion = cargoToml.workspace.package.version;

      # When building from a release commit the Cargo.toml already carries the
      # real version (e.g. "0.101.0").  On the main branch it is the placeholder
      # "0.0.0", so we fall back to a dev version derived from the flake source.
      version =
        if cargoVersion != "0.0.0"
        then cargoVersion
        else "0.0.0-dev+${self.shortRev or "dirty"}";
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          bundledShellToolManifest =
            builtins.fromJSON (builtins.readFile ./nix/bundled-shell-tools.json);
          popplerUtils = pkgs."poppler-utils";
          pythonExecutable = "${pkgs.python3}/bin/${pkgs.python3.meta.mainProgram or "python3"}";
          rustToolchain = pkgs.rust-bin.stable.latest.minimal;
          codex-rs-unwrapped = pkgs.callPackage ./codex-rs {
            inherit version;
            rustPlatform = pkgs.makeRustPlatform {
              cargo = rustToolchain;
              rustc = rustToolchain;
            };
          };
          # Keep a deliberately small, manifest-driven toolbelt in the package
          # closure so the packaged CLI has a predictable baseline PATH.
          bundledShellToolNames =
            bundledShellToolManifest.generalPurpose
            ++ bundledShellToolManifest.pdf;
          bundledShellToolSources = {
            file = "${pkgs.file}/bin/file";
            git = "${pkgs.git}/bin/git";
            gs = "${pkgs.ghostscript_headless}/bin/gs";
            mutool = "${pkgs.mupdf}/bin/mutool";
            pdfgrep = "${pkgs.pdfgrep}/bin/pdfgrep";
            pdfimages = "${popplerUtils}/bin/pdfimages";
            pdfinfo = "${popplerUtils}/bin/pdfinfo";
            pdftocairo = "${popplerUtils}/bin/pdftocairo";
            pdftohtml = "${popplerUtils}/bin/pdftohtml";
            pdftoppm = "${popplerUtils}/bin/pdftoppm";
            pdftotext = "${popplerUtils}/bin/pdftotext";
            python = pythonExecutable;
            python3 = pythonExecutable;
            qpdf = "${pkgs.qpdf}/bin/qpdf";
            rg = "${pkgs.ripgrep}/bin/rg";
            ruby = "${pkgs.ruby}/bin/ruby";
            sqlite3 = "${pkgs.sqlite}/bin/sqlite3";
            strings = "${pkgs.binutils}/bin/strings";
            xxd = "${pkgs.xxd}/bin/xxd";
          };
          bundledShellToolbelt = pkgs.runCommand "codex-bundled-shell-toolbelt" {} ''
            mkdir -p "$out/bin"
            ${pkgs.lib.concatMapStringsSep "\n" (name: ''
              ln -s "${bundledShellToolSources.${name}}" "$out/bin/${name}"
            '') bundledShellToolNames}
          '';
          codex-dev-wrapper = pkgs.writeShellScriptBin "codex-dev" ''
            set -euo pipefail

            dir="$PWD"
            while true; do
              candidate="$dir/codex-dev"
              if [ -x "$candidate" ] && [ ! -d "$candidate" ]; then
                exec "$candidate" "$@"
              fi

              if [ "$dir" = "/" ]; then
                break
              fi

              dir=''${dir%/*}
              if [ -z "$dir" ]; then
                dir="/"
              fi
            done

            printf 'No codex-dev found, do you want to run codex (y,n)? '
            read -r answer
            case "$answer" in
              [Yy]) exec codex "$@" ;;
              [Nn]) exit 1 ;;
              *) exit 1 ;;
            esac
          '';
          codex = pkgs.symlinkJoin {
            name = "codex-${version}";
            paths = [ codex-rs-unwrapped codex-dev-wrapper ];
            nativeBuildInputs = [ pkgs.makeWrapper ];
            postBuild = ''
              wrapProgram "$out/bin/codex" \
                --prefix PATH : "${pkgs.lib.makeBinPath [ bundledShellToolbelt ]}"
            '';
          };
        in
        {
          codex = codex;
          codex-rs = codex;
          codex-rs-unwrapped = codex-rs-unwrapped;
          default = codex;
        }
      );

      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          # Keep the local Bazel wrapper around for possible future use, but do
          # not pull it into the default dev shell for this fork.
          # bazel = pkgs.callPackage ./bazel.nix {
          #   inherit nixpkgs;
          #   version = "9.0.0";
          # };
          rust = pkgs.rust-bin.stable.latest.minimal.override {
            extensions = [ "clippy" "rust-src" "rust-analyzer" "rustfmt" ];
          };
        in
        {
          default = pkgs.mkShell {
            buildInputs = [
              rust
              # Bazel is intentionally not included in the default dev shell for
              # this fork.
              # bazel
              pkgs.just
              pkgs.dotslash
              pkgs.pkg-config
              pkgs.openssl
              pkgs.cmake
              pkgs.llvmPackages.clang
              pkgs.llvmPackages.libclang.lib
              pkgs.python3
              pkgs.uv
            ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [ pkgs.libcap ];
            PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig";
            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
            # Use clang for BoringSSL compilation (avoids GCC 15 warnings-as-errors)
            shellHook = ''
              export CC=clang
              export CXX=clang++
              export UV_CACHE_DIR="''${UV_CACHE_DIR:-$PWD/.cache/uv}"
            '';
          };
        }
      );
    };
}
