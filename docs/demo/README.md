# The demo GIF

`svipall.gif` is the animation at the top of the README: six commands, 28.6 seconds,
960x400, about 130 KB.

It is a **replay**, not a screen recording. Every command shown was run once on one
machine, its stdout was kept in `raw/`, and the recorder replays it at a readable
speed. Nothing on screen was typed by hand, shortened, or made up.

## Why replay

A live take cannot do this in half a minute. One fetch through the tier ladder is six
seconds of nothing happening, a crawl is two and a half minutes, and a site that
happens to be blocking that afternoon loses the take. Replaying real captures keeps
every byte true and lets the pauses be the length a reader needs rather than the
length the network took.

The one thing the timing does not represent is duration: the working mark between a
command and its output is a fixed beat, not the real elapsed time. The real elapsed
time is in the output itself, where the tool prints it — `browser: 200 (6259ms) OK`.

## The pieces

| File | What it is |
| --- | --- |
| `raw/*.json` | stdout of the real runs, verbatim. `content` (the page body) is stripped; nothing else is. |
| `build-transcript.mjs` | Splices a jq filter into each command line and runs that same filter over that capture. The command and its output cannot drift apart. |
| `transcript.json` | Generated. What the recorder reads. |
| `terminal.html` | The stage. A pure function of time: `__demo.render(t)`. Palette (the dark theme) and faces are the site's, from `svipall-site/src/styles/tokens.css`. |
| `gif.mjs` | PNG reader and GIF89a encoder. Says in its header why it exists. |
| `record.mjs` | Drives Chrome over CDP, one screenshot per frame, then encodes. |

## Regenerating it

```sh
cd docs/demo
node build-transcript.mjs     # needs jq
node record.mjs               # needs node >= 22 and Playwright's Chromium
```

`record.mjs --stills` writes one frame per scene to `.stills/` and stops, which is the
fast way to look at a change before spending six hundred screenshots on it.

Both directories it writes into, `.frames/` and `.stills/`, are ignored by git.

It needs no npm install and no ffmpeg. The only binary it looks for is the Chromium
Playwright unpacks under `ms-playwright/chromium-*`; if it is somewhere else, set
`SVIPALL_DEMO_CHROME`. To get it in the first place: `npx playwright install chromium`.

Recording is frame-stepped rather than timed, so the same input gives the same output
on any machine, fast or slow.

## Re-capturing the runs

If a scene should show something new, run the command, keep its stdout under `raw/`,
and add the step to `STEPS` in `build-transcript.mjs`. Two rules:

- The command in `run` must be the command that produced that capture, flags and all.
  `--cache bypass` is in two of them because without it the ladder never climbs.
- The bracket mark in `tag` must be a literal string out of the output below it. It is
  the site's rule for these marks (R-D7-04) and it applies here for the same reason: a
  bracket with an invented word in it is a fabricated claim in a small typeface.
