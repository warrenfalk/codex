use super::*;
use crate::intersect_effective_permission_profiles;
use pretty_assertions::assert_eq;

#[test]
fn pid_namespace_round_trips_and_old_profiles_stay_isolated() -> anyhow::Result<()> {
    let legacy = serde_json::json!({
        "type": "managed",
        "file_system": { "type": "unrestricted" },
        "network": "enabled",
    });
    let profile: PermissionProfile = serde_json::from_value(legacy.clone())?;
    assert_eq!(profile.pid_namespace(), PidNamespace::Isolated);
    assert_eq!(serde_json::to_value(&profile)?, legacy);
    let host = profile.with_pid_namespace(PidNamespace::Host);
    let mut serialized = legacy;
    serialized["pid_namespace"] = serde_json::json!("host");
    assert_eq!(serde_json::to_value(&host)?, serialized);
    assert_eq!(
        serde_json::from_value::<PermissionProfile>(serialized)?,
        host
    );
    Ok(())
}

#[test]
fn host_pid_namespace_survives_permission_transformations() -> anyhow::Result<()> {
    let cwd = tempfile::tempdir()?;
    let roots = [codex_utils_absolute_path::AbsolutePathBuf::try_from(
        cwd.path(),
    )?];
    let isolated = PermissionProfile::workspace_write();
    let host = isolated.clone().with_pid_namespace(PidNamespace::Host);
    assert_eq!(
        host.clone()
            .materialize_project_roots_with_workspace_roots(&roots),
        isolated
            .clone()
            .materialize_project_roots_with_workspace_roots(&roots)
            .with_pid_namespace(PidNamespace::Host),
    );
    assert_eq!(
        host.intersect_with_read_only(),
        isolated
            .intersect_with_read_only()
            .map(|profile| profile.with_pid_namespace(PidNamespace::Host)),
    );
    let file_system = FileSystemSandboxPolicy::read_only();
    assert_eq!(
        host.with_runtime_permissions(&file_system, NetworkSandboxPolicy::Enabled),
        PermissionProfile::from_runtime_permissions(&file_system, NetworkSandboxPolicy::Enabled)
            .with_pid_namespace(PidNamespace::Host),
    );
    Ok(())
}

#[test]
fn pid_namespace_intersection_never_relaxes_isolation() -> anyhow::Result<()> {
    let cwd = tempfile::tempdir()?;
    for left in [PidNamespace::Isolated, PidNamespace::Host] {
        for right in [PidNamespace::Isolated, PidNamespace::Host] {
            let expected = if left == PidNamespace::Host && right == PidNamespace::Host {
                PidNamespace::Host
            } else {
                PidNamespace::Isolated
            };
            assert_eq!(
                intersect_effective_permission_profiles(
                    &PermissionProfile::read_only().with_pid_namespace(left),
                    &PermissionProfile::read_only().with_pid_namespace(right),
                    cwd.path(),
                )?,
                PermissionProfile::read_only().with_pid_namespace(expected),
            );
        }
    }
    Ok(())
}

#[test]
fn pid_namespace_survives_filesystem_and_disabled_profile_intersections() -> anyhow::Result<()> {
    let cwd = tempfile::tempdir()?;
    let roots = [codex_utils_absolute_path::AbsolutePathBuf::try_from(
        cwd.path(),
    )?];
    let workspace =
        PermissionProfile::workspace_write().materialize_project_roots_with_workspace_roots(&roots);
    for (authority, requested) in [
        (PermissionProfile::read_only(), workspace.clone()),
        (workspace, PermissionProfile::read_only()),
        (PermissionProfile::Disabled, PermissionProfile::read_only()),
        (PermissionProfile::read_only(), PermissionProfile::Disabled),
    ] {
        let expected = intersect_effective_permission_profiles(&authority, &requested, cwd.path())?;
        assert_eq!(
            intersect_effective_permission_profiles(
                &authority.with_pid_namespace(PidNamespace::Host),
                &requested.with_pid_namespace(PidNamespace::Host),
                cwd.path(),
            )?,
            expected.with_pid_namespace(PidNamespace::Host),
        );
    }
    Ok(())
}
