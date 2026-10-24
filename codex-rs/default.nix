{
  alsa-lib ? null,
  cmake,
  fetchurl,
  formats,
  git,
  glib,
  gst_all_1,
  libopus,
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
  mxcSdkLock = import ../nix/mxc-sdk-lock.nix {
    inherit lib formats;
    lockFile = ./Cargo.lock;
  };
  nativeAudioLibraries = [
    glib
    gst_all_1.gstreamer
    gst_all_1.gst-plugins-base
    libopus
  ] ++ lib.optionals stdenv.hostPlatform.isLinux [ alsa-lib ];

  rustyV8Target =
    {
      "aarch64-darwin" = {
        name = "aarch64-apple-darwin";
        archiveHash = "sha256-/qua4DhL66qZwdAAZURiydQhI9WHGFx4dqru99dv90k=";
        bindingHash = "sha256-udVlLrylFesr9MLuSmVRslZxF7Wqcg9DDSAyYW3X47U=";
      };
      "aarch64-linux" = {
        name = "aarch64-unknown-linux-gnu";
        archiveHash = "sha256-EFRGSmIwBs314lriCHcAK/PtuaLC8+ZPRKhxR0KxXoc=";
        bindingHash = "sha256-Pk1f7Nvg+YpgF+adfKglGad/sDLF615M2+zdPi7FGdU=";
      };
      "x86_64-darwin" = {
        name = "x86_64-apple-darwin";
        archiveHash = "sha256-v+Gdm2M7YdmXgOzolpBCf+9LmFxrQZZTbq+UVxFm/CQ=";
        bindingHash = "sha256-udVlLrylFesr9MLuSmVRslZxF7Wqcg9DDSAyYW3X47U=";
      };
      "x86_64-linux" = {
        name = "x86_64-unknown-linux-gnu";
        archiveHash = "sha256-s7SQ8qqAntp/ascn+FH7AM6DVb5ygnV3z6rCjHE3280=";
        bindingHash = "sha256-Pk1f7Nvg+YpgF+adfKglGad/sDLF615M2+zdPi7FGdU=";
      };
    }
    .${stdenv.hostPlatform.system}
      or (throw "unsupported system for rusty_v8 prebuilt archive: ${stdenv.hostPlatform.system}");

  rustyV8BaseUrl = "https://github.com/openai/codex/releases/download/rusty-v8-v152.2.0";
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
        ([ openssl ] ++ nativeAudioLibraries ++ lib.optionals stdenv.hostPlatform.isLinux [ libcap ]);

    LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";
    RUSTY_V8_ARCHIVE = rustyV8Archive;
    RUSTY_V8_SRC_BINDING_PATH = rustyV8Binding;

    # cargoBuildHook overrides HOST_CC/CXX; use the AWS-LC-specific settings
    # so its compiler probe agrees with the target compiler below.
    AWS_LC_SYS_HOST_CC = "${llvmPackages.clang}/bin/clang";
    AWS_LC_SYS_HOST_CXX = "${llvmPackages.clang}/bin/clang++";

    # rama-boring-sys honors target-specific CC/CXX vars (matches cc crate behavior).
    CC_x86_64_unknown_linux_gnu = "${llvmPackages.clang}/bin/clang";
    CXX_x86_64_unknown_linux_gnu = "${llvmPackages.clang}/bin/clang++";
  };

  pname = "codex-rs";
  inherit version;
  cargoLock.lockFile = ./Cargo.lock;
  doCheck = false;
  src = ./.;
  buildInputs = nativeAudioLibraries;

  prePatch = ''
    cp ${mxcSdkLock.lockFile} "$cargoDepsCopy/${mxcSdkLock.crateDirectory}/Cargo.lock"
  '';

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
  ] ++ lib.optionals stdenv.hostPlatform.isLinux [
    libcap
  ];

  cargoLock.outputHashes = {
    "crossterm-0.29.0" = "sha256-7ZzFZ6c6uU27ZxCGOPK82Aia4CzTnJwksAfbpTMPikY=";
    "h3-0.0.8" = "sha256-fgE0AMj5d4iattTC/yQwnACV8uEu+KR7wD29xfEm8M0=";
    "mxc-sdk-1.0.0" = "sha256-AI4l2yCbB/4UWweLFhEefEPqSt/ZB1V+Mk/svdAT8No=";
    "nucleo-0.5.0" = "sha256-Hm4SxtTSBrcWpXrtSqeO0TACbUxq3gizg1zD/6Yw/sI=";
    "nucleo-matcher-0.3.1" = "sha256-Hm4SxtTSBrcWpXrtSqeO0TACbUxq3gizg1zD/6Yw/sI=";
    "rmcp-3.5.1" = "sha256-yczrrD6SnXhblos5oU6fbyDc7vkRah9DsF41hcaKzi4=";
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
