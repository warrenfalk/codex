{
  callPackage,
  coreutils,
  fetchzip,
  lib,
  nixpkgs,
  version ? "9.0.0",
  writeText,
}:

let
  bazelDepsPatch = writeText "bazel-9-deps_patches.patch" ''
    diff --git a/MODULE.bazel b/MODULE.bazel
    --- a/MODULE.bazel
    +++ b/MODULE.bazel
    @@ -58,6 +58,16 @@ bazel_dep(name = "google_benchmark", version = "1.9.4", repo_name = None)

     # bazel_dep overrides
    +single_version_override(
    +    module_name = "rules_java",
    +    patches = ["//third_party:rules_java.patch"],
    +)
    +
    +single_version_override(
    +    module_name = "rules_python",
    +    patches = ["//third_party:rules_python.patch"],
    +)
    +
     single_version_override(
         module_name = "rules_jvm_external",
         patch_strip = 1,
  '';

  bazelBase = callPackage "${nixpkgs.outPath}/pkgs/by-name/ba/bazel_8/package.nix" {
    inherit version;
  };
in
bazelBase.overrideAttrs (old: {
  inherit version;

  src = fetchzip {
    url = "https://github.com/bazelbuild/bazel/releases/download/${version}/bazel-${version}-dist.zip";
    hash = "sha256-/veTQ/Fs5KKs58nJrazfLcJmwUWpP/m0d3tKfp34KmI=";
    stripRoot = false;
  };

  postPatch =
    (old.postPatch or "")
    + ''
      patchShebangs src scripts

      cat > third_party/rules_python.patch <<'EOF'
      diff --git python/private/runtime_env_toolchain.bzl python/private/runtime_env_toolchain.bzl
      --- python/private/runtime_env_toolchain.bzl
      +++ python/private/runtime_env_toolchain.bzl
      @@ -42,8 +42,8 @@
               name = "_runtime_env_py3_runtime",
               interpreter = "//python/private:runtime_env_toolchain_interpreter.sh",
               python_version = "PY3",
      -        stub_shebang = "#!/usr/bin/env python3",
      +        stub_shebang = "#!${coreutils}/bin/env python3",
               visibility = ["//visibility:private"],
               tags = ["manual"],
               supports_build_time_venv = supports_build_time_venv,
           )
      EOF
    '';

  patches =
    (lib.filter (
      patch:
        builtins.match ".*(deps_patches|env_bash|gen_completion|md5sum)\\.patch$" (toString patch) == null
    ) old.patches)
    ++ [ bazelDepsPatch ];
})
