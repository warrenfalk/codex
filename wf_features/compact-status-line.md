# Compact Status Line

The footer status line uses shorter values to leave more room for the selected
items, especially in narrow terminals.

- Remaining context displays as `100% left`, omitting the `Context ` prefix.
- The current working directory displays its last two path components after
  home-directory abbreviation. For example, `~/source/codex-cli/codex` becomes
  `codex-cli/codex`, while `~/mygame` and `/warren-desktop` remain unchanged.
  The home directory itself remains `~`, and filesystem roots remain intact.
  Native path separators are preserved, including on Windows.
- Both the model item and the model-with-reasoning item omit one leading `gpt-`.
  For example, `gpt-5.4 xhigh fast` becomes `5.4 xhigh fast`. Model names without
  that prefix, reasoning levels, and service-tier labels retain their text.
- The status-line setup preview uses the same compact formatting for live values
  and fallback examples. Existing item names, configuration, order, colors, and
  separators keep working.
- These changes apply to the footer status line and its setup preview. Terminal
  titles, `/status`, model selection, and actual working directories are unaffected.

Validation should cover the path examples above, root paths, Windows drive and
UNC paths, model names with and without the prefix, reasoning/service-tier
suffixes, and agreement between the footer and setup preview. Footer snapshots
must show the compact values.
