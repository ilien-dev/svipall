# Svipall documentation

The technical side of Svipall: how to install it, what each part does, the numbers behind it and how
it is built. This page is also the index of the docs site at https://svipall.ilien.dev/docs/, and
only the pages linked below are published there.

## Start here

- [Install](install.md): every way to install Svipall, written so an agent can follow it.
- [Update](update.md): one command that updates the binaries and the skills you copied by hand.
- [FAQ](faq.md): short answers about keys, privacy and what it will not get past, plus how it compares with similar projects.

## Using it

- [Features](features.md): the whole surface, tool by tool.
- [Extraction](extraction.md): how a page becomes the text a model reads, and how good that is.
- [Captcha](captcha.md): the local strategies and the human dashboard for what they cannot answer.
- [REST API](rest.md): the same server over HTTP, with its job queue.
- [Configuration](configuration.md): settings and the identity policy.
- [Exits](exits.md): proxies you supply, and how their health is tracked.
- [HTTP/3](http3.md): the opt-in QUIC engine, and what it does and does not match.
- [Models](models.md): the local models it runs and how to bring your own.
- [Firefox](firefox.md): the Firefox identity that ships today, and the patched build that does not.

## Evidence

- [Proof](proof.md): every published number, with the command that reproduces it.
- [Benchmarks](bench.md): the benchmark modes and the rules their results are read under.
- [Limits](limits.md): what Svipall does not do, on purpose.
- [Privacy and safety](privacy.md): what leaves the machine and how secrets stay out of tool calls.

## Internals

- [Architecture](architecture.md): the crates and what each one is responsible for.
- [Development](development.md): building from source and the quality gate.

## Reference

- [Reference](reference/README.md): generated from the code, every MCP tool and REST route with its parameters.
