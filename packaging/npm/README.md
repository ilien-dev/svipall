# svipall

Local-first web scraping and captcha MCP server for AI agents. No cloud, no API keys, no paid
captcha service and no telemetry: everything runs on the machine you install it on.

This package carries no code of its own. On install it downloads the release build for your
platform from [GitHub](https://github.com/ilien-dev/svipall/releases), checks it against the
published `sha256sums.txt`, and puts the two binaries behind these shims. It exists because `npx`
is how a lot of people already run an MCP server.

```bash
npx --yes svipall doctor        # what this installation can do, and the fix for anything it cannot
npx --yes svipall fetch https://example.com
```

As an MCP server:

```bash
claude mcp add svipall -- npx --yes svipall-mcp
```

Platforms with a build: Linux x86-64 and arm64, macOS Intel and Apple silicon, Windows x86-64. On
anything else the install says why and stops, without failing the rest of your install.

Browser tiers need a Chromium-based browser. Run `npx svipall browser install` for a Chrome for
Testing of its own (~190 MB), or point `browser_path` at one you already have.

Full documentation: **https://github.com/ilien-dev/svipall** ·
installing: [docs/install.md](https://github.com/ilien-dev/svipall/blob/main/docs/install.md)

AGPL-3.0-only.
