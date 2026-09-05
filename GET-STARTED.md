# Getting started with Svipall

No jargon, no options. If you want the full reference instead, read
[docs/install.md](docs/install.md).

## What this is

Your AI assistant can read text you give it, but it cannot really browse. When it tries, a lot of
websites quietly hand it a "checking your browser" page — and the assistant summarises *that* as if
it were the article. It never says anything went wrong, because as far as it can tell, nothing did.

Svipall is a program that runs on your own computer and does the browsing properly. It looks and
behaves like a real visitor, so most sites let it through. When one does not, it says so plainly
instead of making something up.

Nothing you do with it is sent anywhere. There is no account, no API key and no subscription, and
no service ever sees your pages.

## Ask your assistant to do it

If you already use Claude Code, Cursor, Codex or something similar, paste this to it:

```
Install and configure Svipall by following the instructions here:
https://raw.githubusercontent.com/ilien-dev/svipall/main/docs/install.md
```

It will work out which version your computer needs, install it, and connect it up, asking you
before each step.

## In Claude Code specifically

Three lines, in the Claude Code prompt:

```
/plugin marketplace add ilien-dev/svipall
/plugin install svipall@svipall
/svipall:setup
```

`/svipall:setup` installs the program if it is not there yet, connects it, and asks whether you want
Claude to use it for all web access from now on. Say no to anything you would rather not have.

## Doing it yourself

Open a terminal and paste one line.

**macOS or Linux**

```
curl -fsSL https://raw.githubusercontent.com/ilien-dev/svipall/main/install.sh | sh
```

**Windows** (in PowerShell — search the Start menu for it)

```
irm https://raw.githubusercontent.com/ilien-dev/svipall/main/install.ps1 | iex
```

You will see it download a file, check it, and print where it put things. It never asks for your
password, and it only writes inside your own user folder.

At the end it may offer to download a browser of its own, about 190 MB. Say yes if you can: without
it, Svipall can only read the simplest sites. You can also do it later by running
`svipall browser install`.

Then **close the terminal and open a new one**, and check it worked:

```
svipall doctor
```

That prints a report. If it says `"ok": true`, you are done. Otherwise each problem it lists comes
with the exact command that fixes it.

## Trying it

```
svipall fetch https://example.com
```

You get the page back as clean text.

## If something goes wrong

- **`svipall: command not found`** — you are probably still in the old terminal window. Close it and
  open a new one.
- **macOS says the developer cannot be verified** — that happens when a file arrives through a
  browser. The one-line installer above avoids it. If you already downloaded it by hand, run
  `xattr -d com.apple.quarantine` followed by the path to the file.
- **Every page comes back blocked** — you probably skipped the browser download. Run
  `svipall browser install`.
- **Anything else** — run `svipall doctor` and read what it says. It is written to be understood,
  and every problem it reports comes with its fix.

## Removing it

**macOS or Linux**

```
curl -fsSL https://raw.githubusercontent.com/ilien-dev/svipall/main/install.sh | sh -s -- --uninstall
```

**Windows**

```
irm https://raw.githubusercontent.com/ilien-dev/svipall/main/install.ps1 -OutFile i.ps1; ./i.ps1 -Uninstall
```

That removes the program. It leaves the folder `.svipall` in your home directory alone — that is
where any sites you logged into are kept — so delete it by hand if you want everything gone.
