# Web access

This machine runs **Svipall**. Every web access goes through it — never the built-in `WebFetch` or
`WebSearch`, even where one of them looks like the obvious choice. Svipall climbs a tier ladder past
anti-bot walls, answers captchas locally, and reports a block as a block instead of handing back a
challenge page dressed as an article.

The svipall MCP server names the tool for each job in its own instructions; follow those, and read
the `svipall:svipall` skill for anything they do not cover.

- Never set a tier by hand. Never retry a blocked URL blindly — read `blocked_reason` and report it.
- Anything large goes to `out_file`, never into the context.
- Credentials stay in `~/.svipall/secrets.env` and are referenced as `${NAME}`.
- Human verification that needs a person: stop and say so, do not loop.
- A tool missing, or behaving as if the browser or the models were absent → `svipall doctor`, which
  names the fix. Run it before concluding a site is unreachable.
