{
  cmake,
  llvmPackages,
  openssl,
  libcap ? null,
  rustPlatform,
  pkg-config,
  git,
  lib,
  stdenv,
  version ? "0.0.0",
  ...
}:
rustPlatform.buildRustPackage (_: {
  env = {
    PKG_CONFIG_PATH =
      lib.makeSearchPathOutput "dev" "lib/pkgconfig"
        ([ openssl ] ++ lib.optionals stdenv.isLinux [ libcap ]);

    LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";

    # rama-boring-sys honors target-specific CC/CXX vars (matches cc crate behavior).
    CC_x86_64_unknown_linux_gnu = "${llvmPackages.clang}/bin/clang";
    CXX_x86_64_unknown_linux_gnu = "${llvmPackages.clang}/bin/clang++";
  };

  pname = "codex-rs";
  inherit version;
  cargoLock.lockFile = ./Cargo.lock;
  doCheck = false;
  src = ./.;

  # Patch the workspace Cargo.toml so that cargo embeds the correct version in
  # CARGO_PKG_VERSION (which the binary reads via env!("CARGO_PKG_VERSION")).
  # On release commits the Cargo.toml already contains the real version and
  # this sed is a no-op.
  postPatch = ''
    sed -i 's/^version = "0\.0\.0"$/version = "${version}"/' Cargo.toml
  '';
  nativeBuildInputs = [
    cmake
    git
    llvmPackages.clang
    llvmPackages.libclang.lib
    libcap
    openssl
    pkg-config
  ] ++ lib.optionals stdenv.isLinux [
    libcap
  ];

  cargoLock.outputHashes = {
    "ratatui-0.29.0" = "sha256-HBvT5c8GsiCxMffNjJGLmHnvG77A6cqEL+1ARurBXho=";
    "crossterm-0.28.1" = "sha256-6qCtfSMuXACKFb9ATID39XyFDIEMFDmbx6SSmNe+728=";
    "nucleo-0.5.0" = "sha256-Hm4SxtTSBrcWpXrtSqeO0TACbUxq3gizg1zD/6Yw/sI=";
    "nucleo-matcher-0.3.1" = "sha256-Hm4SxtTSBrcWpXrtSqeO0TACbUxq3gizg1zD/6Yw/sI=";
    "runfiles-0.1.0" = "sha256-uJpVLcQh8wWZA3GPv9D8Nt43EOirajfDJ7eq/FB+tek=";
    "tokio-tungstenite-0.28.0" = "sha256-hJAkvWxDjB9A9GqansahWhTmj/ekcelslLUTtwqI7lw=";
    "tungstenite-0.27.0" = "sha256-AN5wql2X2yJnQ7lnDxpljNw0Jua40GtmT+w3wjER010=";
  };

  meta = with lib; {
    description = "OpenAI Codex command‑line interface rust implementation";
    license = licenses.asl20;
    homepage = "https://github.com/openai/codex";
    mainProgram = "codex";
  };
})
