# Web access

This machine runs **Svipall**. Every web access goes through it — never the built-in `WebFetch` or
`WebSearch`, even where one of them looks like the obvious choice. `WebFetch` answers your prompt
over the page with a small model, so the page itself never reaches you, and a wall it did not
recognise is summarised like an article. Svipall hands over the content, climbs a tier ladder past
those walls, answers captchas locally, and reports a block as a block with a `blocked_reason`.

The svipall MCP server names the tool for each job in its own instructions; follow those, and read
the `svipall:svipall` skill for anything they do not cover.

- Never set a tier by hand. Never retry a blocked URL blindly — read `blocked_reason` and report it.
- Anything large goes to `out_file`, never into the context.
- Credentials stay in `~/.svipall/secrets.env` and are referenced as `${NAME}`.
- Human verification that needs a person: stop and say so, do not loop.
- A tool missing, or behaving as if the browser or the models were absent → `svipall doctor`, which
  names the fix. Run it before concluding a site is unreachable.
