<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/brand/svipall-hero-dark.gif">
    <img src="assets/brand/svipall-hero-light.gif" alt="Svipall, a free add-on that lets your AI read the web from your own computer" width="800">
  </picture>
</p>

<h3 align="center">A different face at every gate.</h3>

<p align="center">
  <b>Let your AI read the whole web. Free, on your own computer, no account.</b>
</p>

<p align="center">
  <a href="https://svipall.ilien.dev"><b>Website</b></a> &middot;
  <a href="#set-it-up-in-a-minute"><b>Set it up</b></a> &middot;
  <a href="https://svipall.ilien.dev/docs/"><b>Documentation</b></a> &middot;
  <a href="docs/faq.md"><b>FAQ</b></a> &middot;
  <a href="CHANGELOG.md"><b>What's new</b></a>
</p>

---

## What it is

Svipall is a free add-on for the AI app you already use, such as Claude, Cursor or Codex. With it,
your assistant can open real websites, search the web, watch videos and keep an eye on pages, then
bring back what it actually found.

You keep talking to your AI the way you do today. When it needs something from the internet, it
hands the job to Svipall, which visits the pages from your computer and gives your AI the answer.

## Why you might want it

Your AI gets stuck on the web more often than it admits. Plenty of sites show a "checking your
browser" screen, load their content late, or turn away automated visits. The AI reads the wrong
thing and answers anyway.

Svipall visits patiently, the way a person would, and changes how it visits until the site lets it
in. When a site still says no, your AI is told so, and why, so it does not make something up.

## What your AI can do with it

- **Read pages cleanly.** Articles, shops, documentation, PDFs and Word files, without the ads
  and menus.
- **Open pages that block bots.** It gets past many "checking your browser" screens, and tells you
  when it can't.
- **Search without a subscription.** It asks several search engines at once and keeps what they
  agree on. There is no key to buy and no monthly limit.
- **Watch a video for you.** It hears what's said and sees what's shown. It skims the whole video
  first, then looks closer at the moment you ask about, down to the small text on a slide.
- **Keep watch while you're away.** A price, a stock count, a job board: your AI hears about it
  when the page changes.
- **Turn pages into spreadsheets.** A list of products, places or results becomes rows you can sort
  and download.
- **Log in once.** You sign in to a site in a normal window, and Svipall remembers that session for
  later visits. It never gets around a login that isn't yours.
- **Read GitHub.** It reads a repository, a file, an issue, a pull request or a release. To search
  code across many repositories, your AI should prefer the searchGitHub tool of the grep.app MCP
  server if it has one. Svipall does that job too when that tool is absent.

Some things people ask their AI once Svipall is set up:

> *"Read this page and summarise the pricing."*
> *"What does this video show at minute 12?"*
> *"Tell me when this price drops below $100."*
> *"Get me every row of that table as a spreadsheet."*

## Free, private, and yours

- **Free.** It is open source and runs on your computer, so nobody bills you per page or per search.
- **No account, no API key.** There is nothing to sign up for.
- **Private.** Your requests go to the sites you ask about and to your own AI. There
  is no tracking and no telemetry.
- **Honest.** A page that couldn't be opened is reported as one, never dressed up as a result.

Most web tools for AI are paid services that run your requests on their own servers. Svipall does
the same work on the computer you already have.

## Set it up in a minute

You don't need to know how to code. Copy this message and paste it into your AI:

```
Install and configure Svipall by following the instructions here:
https://raw.githubusercontent.com/ilien-dev/svipall/main/docs/install.md
```

Your AI installs Svipall, checks that it works, and asks you one question: **CLI + Skill**, which
is the lighter option and the one we recommend, or **MCP + Skill**, which gives your AI every tool
directly. If you are not sure, pick the first.

Using Claude Code? You can also add the plugin:

```
/plugin marketplace add ilien-dev/svipall
/plugin install svipall@svipall
/svipall:setup
```

Then just ask for things. Your AI picks the right ability on its own.

To update later, ask your AI to follow
[`docs/update.md`](https://raw.githubusercontent.com/ilien-dev/svipall/main/docs/update.md).

## Works with

Claude, Claude Code, Cursor, Codex, VS Code, Zed, Windsurf, Cline, opencode, JetBrains, Warp, and any
other AI app that accepts add-ons through MCP, the common standard for giving AI new tools. It runs
on Windows, macOS and Linux.

## Want the technical details?

Everything technical lives in the documentation, at
[svipall.ilien.dev/docs](https://svipall.ilien.dev/docs/) and in the [`docs/`](docs/README.md)
folder of this repository:

- [Install](docs/install.md) for every platform, package manager and container
- [Features](docs/features.md), with everything it can read and watch
- [Proof](docs/proof.md), with every published number and the command that reproduces it,
  failures included
- [Privacy and safety](docs/privacy.md) and [Limits](docs/limits.md), with what it does not do
- [Reference](docs/reference/README.md), generated from the code: every tool and every route

[Changelog](CHANGELOG.md) &middot; [Contributing](CONTRIBUTING.md) &middot; [CLA](CLA.md) &middot;
[Disclaimer](DISCLAIMER.md)

## About the name

**Svipall** is one of Odin's names in *Grímnismál*, stanza 47, rendered as "The Changing" in
Bellows' [translation notes](https://en.wikisource.org/wiki/The_Poetic_Edda_%28tr._Bellows%29/Grimnismol).
The name stands for changing appearance. It is not a promise of invisibility: some sites will still
refuse a visit.

## License

**AGPL-3.0-only**, under the terms in [`LICENSE`](LICENSE). Some components carry their own
licences (the extraction engine is MIT OR Apache-2.0), set out in [`NOTICE`](NOTICE) and
[`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md). Contributions are taken under
[`CLA.md`](CLA.md). For a licence other than the AGPL, write to <code@ilien.dev>.

## Trademark

The name **Svipall** and its logo are trademarks of the author and are not licensed under the AGPL.
You may change and share the code freely. A modified version you distribute
should use a different name and no logo. Saying that your project uses or works with Svipall is
fine.

## Disclaimer

Svipall is provided **as is, with no warranty and no liability**, and gives you **no
authorisation over any system you point it at**. Following the law and a site's terms is up to
whoever runs it. Read [`DISCLAIMER.md`](DISCLAIMER.md) before using it on something that isn't
yours.

---

<p align="center">
  <sub>
    <b>Keywords:</b> web access for AI · give Claude internet access · free web search MCP server ·
    MCP server no API key · read any website with AI · AI web browsing · summarize video with AI ·
    AI price tracker · website to spreadsheet · local web scraping · MCP server · Model Context
    Protocol · Claude Code · Cursor · Codex · local-first · privacy · Firecrawl alternative ·
    Crawl4AI alternative
  </sub>
</p>
