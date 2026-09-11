use crate::permissions_toml::PermissionProfileToml;
use crate::permissions_toml::PermissionsToml;
use codex_file_system::ExecutorFileSystem;
use codex_file_system::ReadFileOptions;
use codex_utils_path_uri::PathUri;
use std::collections::BTreeMap;
use std::io;

// Keep user-authored policy text within roughly 1,000 estimated tokens.
const MAX_INSTRUCTIONS_BYTES: usize = 4_000;

/// Load effective instruction files once per configuration, retaining empty overrides.
/// The caller supplies the same built-in parents used to resolve runtime permissions.
pub async fn load_permission_profile_instructions(
    fs: &dyn ExecutorFileSystem,
    permissions: Option<&PermissionsToml>,
    mut builtin_parent: impl FnMut(&str) -> Option<PermissionProfileToml> + Send,
) -> io::Result<BTreeMap<String, String>> {
    let mut instructions = BTreeMap::new();
    let Some(permissions) = permissions.filter(|permissions| {
        permissions
            .entries
            .values()
            .any(|profile| profile.instructions_file.is_some())
    }) else {
        return Ok(instructions);
    };
    let mut files = BTreeMap::new();
    for id in permissions.entries.keys() {
        let profile = permissions
            .resolve_profile(id, &mut builtin_parent)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let Some(path) = profile.instructions_file else {
            continue;
        };
        let text = match files.get(&path) {
            Some(text) => String::clone(text),
            None => {
                let path_display = path.display();
                let text = fs
                    .read_file_text(
                        &PathUri::from_abs_path(&path),
                        ReadFileOptions::default(),
                        /*sandbox*/ None,
                    )
                    .await
                    .map_err(|error| {
                        io::Error::new(
                            error.kind(),
                            format!(
                                "failed to read instructions_file for permission profile `{id}` at {path_display}: {error}"
                            ),
                        )
                    })?;
                if text.len() > MAX_INSTRUCTIONS_BYTES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!(
                            "instructions_file for permission profile `{id}` at {path_display} exceeds the {MAX_INSTRUCTIONS_BYTES}-byte limit"
                        ),
                    ));
                }
                files.insert(path, text.clone());
                text
            }
        };
        instructions.insert(id.clone(), text);
    }
    Ok(instructions)
}
