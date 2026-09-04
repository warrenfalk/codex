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
        archiveHash = "sha256-AK27SHmISMd1UEQcaGc6XoUpuOG3PqvN7iMss5tA9KE=";
        bindingHash = "sha256-ylrfDPicmnCtRgrnNkiy/om3SqETs8t/dXtqArdYOU8=";
      };
      "aarch64-linux" = {
        name = "aarch64-unknown-linux-gnu";
        archiveHash = "sha256-0VF+7UBUaFNwKbAF1f6ZfsdNXI01H5FrOm3yC30oEbo=";
        bindingHash = "sha256-dyeCauR5vbZF6Acjn7EtH44uI956bPFvXuWSaQ0dhQY=";
      };
      "x86_64-darwin" = {
        name = "x86_64-apple-darwin";
        archiveHash = "sha256-4Nm7ZOizoDTCkwyDly8/NXYCERSDQvoEB7OCUO8zCFY=";
        bindingHash = "sha256-ylrfDPicmnCtRgrnNkiy/om3SqETs8t/dXtqArdYOU8=";
      };
      "x86_64-linux" = {
        name = "x86_64-unknown-linux-gnu";
        archiveHash = "sha256-o1x10fJuapg4haRbM0kKTr5U8FBQVosyuJz7QhswtYM=";
        bindingHash = "sha256-dyeCauR5vbZF6Acjn7EtH44uI956bPFvXuWSaQ0dhQY=";
      };
    }
    .${stdenv.hostPlatform.system}
      or (throw "unsupported system for rusty_v8 prebuilt archive: ${stdenv.hostPlatform.system}");

  rustyV8BaseUrl = "https://github.com/openai/codex/releases/download/rusty-v8-v150.4.0";
  rustyV8Archive = fetchurl {
    url = "${rustyV8BaseUrl}/librusty_v8_ptrcomp_sandbox_release_${rustyV8Target.name}.a.gz";
    hash = rustyV8Target.archiveHash;
  };
  rustyV8Binding = fetchurl {
    url = "${rustyV8BaseUrl}/src_binding_ptrcomp_sandbox_release_${rustyV8Target.name}.rs";
    hash = rustyV8Target.bindingHash;
  };
in
rustPlatform.buildRustPackage (_: {
  env = {
    PKG_CONFIG_PATH =
      lib.makeSearchPathOutput "dev" "lib/pkgconfig"
        ([ openssl ] ++ lib.optionals stdenv.isLinux [ libcap ]);

    LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";
    RUSTY_V8_ARCHIVE = rustyV8Archive;
    RUSTY_V8_SRC_BINDING_PATH = rustyV8Binding;

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
    "appcontainer_common-0.8.0" = "sha256-XUkT2R+RYk9WIqgKnmIAagNW4xOTyp4bWHmQL1iznHw=";
    "crossterm-0.29.0" = "sha256-cQxQQuV+YEutuQiPurXVISq6F/99vCEk8qe5PU8BCSo=";
    "nucleo-0.5.0" = "sha256-Hm4SxtTSBrcWpXrtSqeO0TACbUxq3gizg1zD/6Yw/sI=";
    "nucleo-matcher-0.3.1" = "sha256-Hm4SxtTSBrcWpXrtSqeO0TACbUxq3gizg1zD/6Yw/sI=";
    "runfiles-0.1.0" = "sha256-uJpVLcQh8wWZA3GPv9D8Nt43EOirajfDJ7eq/FB+tek=";
    "tokio-tungstenite-0.28.0" = "sha256-V1xmnrfRWOcZZogelZEA4vvyMj2awCfHVA5/glQ6KAI=";
    "tungstenite-0.27.0" = "sha256-VVHhk7l9J/sEmG3q/UuV/sQ3f+fGsmq5vumSy8vbMvw=";
  };

  meta = with lib; {
    description = "OpenAI Codex command‑line interface rust implementation";
    license = licenses.asl20;
    homepage = "https://github.com/openai/codex";
    mainProgram = "codex";
  };
})
