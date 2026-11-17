{
  lib,
  formats,
  runCommand,
  openssl,
  libcap,
  stdenv,
  codex-rs-unwrapped,
}:
let
  root = ../codex-rs;
  manifest = builtins.fromTOML (builtins.readFile (root + "/Cargo.toml"));
  version =
    if manifest.workspace.package.version == "0.0.0" then
      "0.0.0-dev"
    else
      manifest.workspace.package.version;

  # Cargo resolves development and platform-specific dependencies even when we
  # build only release binaries. Follow their manifests too, without pulling in
  # unrelated workspace members such as the TUI and the combined CLI.
  packages = builtins.genericClosure {
    startSet = map (member: { key = toString (root + "/${member}"); }) (
      [
        "app-server"
        "code-mode-host"
      ]
      ++ lib.optional stdenv.hostPlatform.isLinux "bwrap"
    );
    operator =
      package:
      let
        cargo = builtins.fromTOML (builtins.readFile (package.key + "/Cargo.toml"));
        sections = [ cargo ] ++ builtins.attrValues (cargo.target or { });
      in
      lib.concatMap (
        section:
        lib.concatMap
          (
            kind:
            lib.concatMap (
              name:
              let
                declared = section.${kind}.${name};
                inherited = builtins.isAttrs declared && (declared.workspace or false);
                dependency = if inherited then manifest.workspace.dependencies.${name} else declared;
                base = if inherited then toString root else package.key;
              in
              lib.optional (builtins.isAttrs dependency && dependency ? path) {
                key = toString (/. + "${base}/${dependency.path}");
              }
            ) (builtins.attrNames (section.${kind} or { }))
          )
          [
            "dependencies"
            "build-dependencies"
            "dev-dependencies"
          ]
      ) sections;
  };
  members = lib.sort builtins.lessThan (
    map (package: lib.removePrefix "${toString root}/" package.key) packages
  );
  source = lib.fileset.toSource {
    inherit root;
    fileset = lib.fileset.unions (
      [
        (root + "/Cargo.lock")
        (root + "/.cargo")
      ]
      ++ lib.optional stdenv.hostPlatform.isLinux (root + "/vendor/bubblewrap")
      ++ map (package: /. + package.key) packages
    );
  };
  workspaceManifest = (formats.toml { }).generate "Cargo.toml" (
    manifest
    // {
      workspace = manifest.workspace // {
        inherit members;
        package = manifest.workspace.package // {
          inherit version;
        };
      };
    }
  );
in
codex-rs-unwrapped.overrideAttrs (old: {
  pname = "codex-app-server";
  inherit version;
  src = runCommand "codex-app-server-source" { } ''
    cp -R ${source} "$out"
    chmod u+w "$out"
    cp ${workspaceManifest} "$out/Cargo.toml"
  '';
  postPatch = "";
  cargoBuildFlags = [
    "-p"
    "codex-app-server"
    "--bin"
    "codex-app-server"
    "-p"
    "codex-code-mode-host"
    "--bin"
    "codex-code-mode-host"
  ]
  ++ lib.optionals stdenv.hostPlatform.isLinux [
    "-p"
    "codex-bwrap"
    "--bin"
    "bwrap"
  ];
  # Audio capture belongs to the frontend; the server needs no native audio runtime.
  buildInputs = [ ];
  env = old.env // {
    PKG_CONFIG_PATH = lib.makeSearchPathOutput "dev" "lib/pkgconfig" (
      [ openssl ] ++ lib.optionals stdenv.hostPlatform.isLinux [ libcap ]
    );
  };
  meta = old.meta // {
    description = "Codex app server and local code-mode host";
    mainProgram = "codex-app-server";
  };
})
