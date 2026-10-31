{
  lib,
  formats,
  lockFile,
}:
let
  lock = builtins.fromTOML (builtins.readFile lockFile);
  sdk =
    lib.findSingle (package: package.name == "mxc-sdk") (throw "mxc-sdk is missing from Cargo.lock")
      (throw "multiple mxc-sdk packages are locked")
      lock.package;
  dependency =
    lib.findSingle (name: name == "windows" || lib.hasPrefix "windows " name)
      (throw "mxc-sdk has no locked windows dependency")
      (throw "mxc-sdk has multiple windows dependency edges")
      sdk.dependencies;
  parts = lib.splitString " " dependency;
  windows =
    lib.findSingle
      (
        package:
        package.name == "windows"
        && (builtins.length parts == 1 || package.version == builtins.elemAt parts 1)
        && (builtins.length parts < 3 || "(${package.source or ""})" == builtins.elemAt parts 2)
      )
      (throw "mxc-sdk's resolved windows package is missing")
      (throw "mxc-sdk's resolved windows package is ambiguous")
      lock.package;
in
{
  crateDirectory = "mxc-sdk-${sdk.version}";
  # Its binding checker reads the first windows entry. Keep the complete graph,
  # but put the SDK's actual dependency first in its private lockfile view.
  lockFile = (formats.toml { }).generate "mxc-sdk-Cargo.lock" (
    lock // { package = [ windows ] ++ builtins.filter (package: package != windows) lock.package; }
  );
}
