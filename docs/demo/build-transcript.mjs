/**
 * Builds transcript.json from the raw captures in raw/.
 *
 * The point of this file is that the command shown in the GIF and the output
 * shown under it cannot drift apart. Each step names one raw capture and one
 * jq filter, the filter is spliced into the displayed command line, and the
 * output is that filter run against that capture. There is nowhere to type a
 * result by hand.
 *
 * raw/ is the stdout of a real run, on one machine, on the date in `meta`.
 * `content` — the page body — is the one thing stripped from it: it is other
 * people's text and none of it is on screen.
 *
 * Usage:  node build-transcript.mjs      (needs jq on PATH)
 */

import { execFileSync } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));

/**
 * Six scenes, in the order a reader meets the tool: what this build is, what
 * happens at a wall, search without a key, structure induced from a listing,
 * a crawl to a file, and the offline check that says none of it is readable
 * from the page.
 *
 * `tag` is the bracket mark in the header, and every one of them is a literal
 * string out of the output below it (site rule R-D7-04).
 */
const STEPS = [
  {
    tag: 'impersonate',
    run: 'svipall --version',
    raw: 'version.json',
    /* Every field the command printed, with the feature list on its own line
       rather than four. It is the shortest scene and it sets the height of the
       panel the site draws, so the four lines it saves are four lines of empty
       panel under every other scene. */
    jq: '{features:(.features|join(", ")), impersonation, target, version}',
    work_ms: 480,
    hold_ms: 900,
  },
  {
    tag: 'tier_used: browser',
    run: 'svipall fetch https://www.glassdoor.com/Reviews/index.htm --cache bypass',
    raw: 'fetch-glassdoor.json',
    jq: '{status,tier_used,attempts}',
    work_ms: 1100,
    hold_ms: 1700,
  },
  {
    tag: 'engine: ddg-html',
    run: 'svipall search "rust cdp stealth automation"',
    raw: 'search.json',
    jq: '{engine, results:(.results|length), top:.results[0].title}',
    work_ms: 850,
    hold_ms: 1400,
  },
  {
    tag: 'induced_schema',
    run: 'svipall fetch https://news.ycombinator.com/newest --schema auto --cache bypass',
    raw: 'schema-hn.json',
    jq: '{base:.induced_schema.base_selector, fields:([.induced_schema.fields[].name]|join(",")), rows:.extracted.count}',
    work_ms: 950,
    hold_ms: 1700,
  },
  {
    tag: 'format: csv',
    run: 'svipall crawl https://doc.rust-lang.org/book/ --pages 6 --depth 2 --out rows.csv',
    raw: 'crawl-rust-book.json',
    jq: '{rows, stopped_by, pending_links, tokens_estimated}',
    work_ms: 1050,
    hold_ms: 1450,
  },
  {
    tag: '160 / 160',
    run: 'svipall-bench tells',
    raw: 'tells.json',
    jq: '{passed, failures, tiers:([.probes[].tier]|unique|join(", "))}',
    work_ms: 850,
    hold_ms: 2100,
  },
];

/* jq on Windows ends its lines with CRLF. The transcript is read by a
   browser, so it is normalised here and never carries a platform with it. */
const jq = (filter, file) =>
  execFileSync('jq', [filter, join(HERE, 'raw', file)], { encoding: 'utf8' })
    .split('\r\n')
    .join('\n')
    .trimEnd();

const steps = STEPS.map((s) => ({
  tag: s.tag,
  cmd: s.jq ? `${s.run} | jq '${s.jq}'` : s.run,
  out: jq(s.jq ?? '.', s.raw),
  /* Which capture this scene came out of. The GIF has no room to say it, but
     anything else that replays this file has to be able to name its log. */
  source: `docs/demo/raw/${s.raw}`,
  work_ms: s.work_ms,
  hold_ms: s.hold_ms,
}));

/* No version anywhere in the chrome. The GIF outlives any one release and a
   stale number in the header would be the only thing on screen that had to be
   kept in step by hand. */
const transcript = {
  meta: {
    recorded: '2026-09-08',
    caption: 'replayed from raw/, captured on one machine, no proxy',
    claim: 'no api key · no third-party solver · nothing leaves the machine',
  },
  steps,
};

writeFileSync(join(HERE, 'transcript.json'), `${JSON.stringify(transcript, null, 2)}\n`);

const widest = Math.max(
  ...steps.flatMap((s) => [s.cmd.length + 2, ...s.out.split('\n').map((l) => l.length)]),
);
const tallest = Math.max(...steps.map((s) => s.out.split('\n').length));
console.log(`transcript.json: ${steps.length} steps, widest ${widest} cols, tallest ${tallest} rows`);
