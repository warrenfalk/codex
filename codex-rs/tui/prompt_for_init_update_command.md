Create or refresh the file `AGENTS.md` in the current working directory.

Scope and safety:

- Target only `./AGENTS.md`. Do not create, update, or remove parent, child, or sibling instruction files.
- Before editing, read `./AGENTS.md` from disk if it exists. Do not rely only on instructions already loaded into this session.
- Inspect the current repository before writing. Use concrete evidence such as directory structure, manifests, build and test scripts, formatter or lint config, and existing docs when relevant.

If `./AGENTS.md` already exists:

- Update it so it reflects the current repository state.
- Preserve useful guidance that is still accurate.
- Rewrite or remove stale, redundant, or inaccurate guidance.
- Preserve deliberate structure and title choices when they still help future agents.
- Fully rewrite the file if that produces a clearer, more accurate guide.

If `./AGENTS.md` does not exist:

- Create it as a concise repository guide.
- Title it `Repository Guidelines`.
- Use Markdown headings.
- Aim for 200-400 words unless this repository genuinely needs more.

The final file should be actionable for future agents. Prefer specific paths, commands, and repository conventions over generic advice.
