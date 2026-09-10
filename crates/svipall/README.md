# svipall

Read any page past its anti-bot wall, crawl sites, search without an API key, solve captchas
locally. The MCP server and the CLI of [svipall](https://github.com/ilien-dev/svipall), in one
crate.

[![Crates.io](https://img.shields.io/crates/v/svipall.svg?style=flat-square)](https://crates.io/crates/svipall)
[![License](https://img.shields.io/badge/license-AGPL--3.0--only-blue.svg?style=flat-square)](https://github.com/ilien-dev/svipall/blob/main/LICENSE)

Everything runs on the machine that installs it. No third-party solver, no API key, no account:
the ladder, the browser tiers, the identity emulation and the captcha models are all local.

## Install

```bash
cargo install svipall
```

That puts two binaries on `PATH`:

- `svipall-mcp` — the MCP server, over stdio, plus the human dashboard on 8787
- `svipall` — the same surface as a CLI, and `svipall doctor` to report on an install

A browser is not installed for you. The `http` tier needs none; the browser tiers do, and
`svipall browser install` fetches one when you want it.

## Use it from an MCP client

```json
{
  "mcpServers": {
    "svipall": { "command": "svipall-mcp" }
  }
}
```

If you would rather not install a Rust toolchain, the release publishes prebuilt binaries, an npm
wrapper and a container image — see the [project README][repo].

## Links

- Source, documentation and releases: [ilien-dev/svipall][repo]
- MCP Registry name: `mcp-name: dev.ilien.svipall/mcp`

The registry line above is deliberately visible rather than an HTML comment: crates.io strips
comments when it renders a README, so the form that works for PyPI and NuGet would leave the
registry's validator nothing to find.

[repo]: https://github.com/ilien-dev/svipall

## License

AGPL-3.0-only. The extraction engine is published separately as
[`svipall-extract`](https://crates.io/crates/svipall-extract) under MIT OR Apache-2.0.
