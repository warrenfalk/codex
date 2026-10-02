use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

/// Local speech preferences for the interactive terminal client.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct TtsConfig {
    /// Initial speech mode for each new TUI instance. Defaults to off.
    #[serde(rename = "defaultMode")]
    pub default_mode: TtsMode,
    /// Executable and arguments, without shell expansion. Text is sent as UTF-8 on stdin.
    /// Defaults to ["say"]. The command must wait until playback finishes.
    pub command: Vec<String>,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            default_mode: TtsMode::Off,
            command: vec!["say".to_string()],
        }
    }
}

/// Which assistant messages the terminal client speaks aloud.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TtsMode {
    #[default]
    Off,
    Final,
    ProgressAndFinal,
}
