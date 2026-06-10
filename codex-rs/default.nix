{
  cmake,
  fetchurl,
  git,
  llvmPackages,
  openssl,
  libcap ? null,
  rustPlatform,
  pkg-config,
  lib,
  stdenv,
  version ? "0.0.0",
  ...
}:
let
  rustyV8Target =
    {
      "aarch64-darwin" = {
        name = "aarch64-apple-darwin";
        archiveHash = "sha256-+rsuyNO6Wm3qY9uaNalg3FypheujLzQrm6Sqocc0sv4=";
      };
      "aarch64-linux" = {
        name = "aarch64-unknown-linux-gnu";
        archiveHash = "sha256-+XdRJ8pk3MSjZi0BpSGizvuluY+DOUOog9hHc7Kv88U=";
      };
      "x86_64-darwin" = {
        name = "x86_64-apple-darwin";
        archiveHash = "sha256-eUlAo4o/ZrfvUqXwA8awlPdDrQQKZK+z082frUlADwc=";
      };
      "x86_64-linux" = {
        name = "x86_64-unknown-linux-gnu";
        archiveHash = "sha256-iu2YY323533Iv7i7R1nsW95HLQv3lD9Y4OYqNQlFxVk=";
      };
    }
    .${stdenv.hostPlatform.system}
      or (throw "unsupported system for rusty_v8 prebuilt archive: ${stdenv.hostPlatform.system}");

  rustyV8Archive = fetchurl {
    url = "https://github.com/denoland/rusty_v8/releases/download/v149.2.0/librusty_v8_release_${rustyV8Target.name}.a.gz";
    hash = rustyV8Target.archiveHash;
  };
in
rustPlatform.buildRustPackage (_: {
  env = {
    PKG_CONFIG_PATH =
      lib.makeSearchPathOutput "dev" "lib/pkgconfig"
        ([ openssl ] ++ lib.optionals stdenv.isLinux [ libcap ]);

    LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";
    RUSTY_V8_ARCHIVE = rustyV8Archive;

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
    openssl
    pkg-config
  ] ++ lib.optionals stdenv.isLinux [
    libcap
  ];

  cargoLock.outputHashes = {
    "ratatui-0.29.0" = "sha256-HBvT5c8GsiCxMffNjJGLmHnvG77A6cqEL+1ARurBXho=";
    "crossterm-0.28.1" = "sha256-6qCtfSMuXACKFb9ATID39XyFDIEMFDmbx6SSmNe+728=";
    "libwebrtc-0.3.26" = "sha256-0HPuwaGcqpuG+Pp6z79bCuDu/DyE858VZSYr3DKZD9o=";
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
