# Updating Svipall

This page is written to be executed. Paste this to the agent you use Svipall with:

```
Update Svipall by following https://raw.githubusercontent.com/ilien-dev/svipall/main/docs/update.md
```

Or run the one command yourself, in a terminal:

```bash
svipall update
```

Asking for the update is the consent, so the agent does not ask again.

## For the agent

1. Run `svipall update`. It replaces `svipall` and `svipall-mcp` in place, using the channel that
   installed them, and refreshes every Svipall skill that was copied by hand
   (`skills_refreshed` lists them). `~/.svipall` and its data are not touched.
2. If `installed` is false and `update_available` is still true, read `note`:
   - Another channel (Homebrew, Scoop, Cargo, npm, container): run `install_command`, then
     `svipall update` once more to refresh the skills.
   - Windows: the user closes every harness running `svipall-mcp`, runs `install_command` in
     PowerShell, then `svipall update` once more.
   - `channel` is `unknown`: stop and say where the executable is. Never install a second copy.
3. If there is no `svipall` at all, follow [install.md](install.md) instead.
4. The Claude Code plugin updates its own files through the marketplace; `/svipall:update` does
   the rest.

End with the `note` the command printed, word for word: the user has to close every session that
uses Svipall and open it again before the new version runs.

## Turning the notice off

When a newer release exists, Svipall says so once per session, with the step above. It checks
GitHub at most once a day. To turn it off, set `update_check = false` in
`~/.svipall/config.toml`.
