#!/usr/bin/env node
// Checks a pull request against CONTRIBUTING.md. Every rule comes from .github/repo-rules.json;
// this file is the same in every repository that follows the standard, so do not edit it here.
//
// In CI (.github/workflows/pr-rules.yml) the inputs come from PR_BRANCH, PR_TITLE, PR_BODY,
// PR_FILES (changed paths, one per line) and PR_AUTHOR. Locally, before opening a PR:
//
//   node .github/scripts/check-pr.cjs --branch feat/short-name --title "feat: ..." \
//     --body-file pr.md [--files-from-git main]
//
// Exits with 1 and lists every problem when something does not follow the rules.

'use strict';

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

const RULES_FILE = path.join(__dirname, '..', 'repo-rules.json');

// Sections of the PR description, in order, with their character limits. Measurement is only
// part of the format when the repository measures changes (rules.measured.enabled).
const SECTIONS = [
  { name: 'What', max: 280 },
  { name: 'Why', max: 280 },
  { name: 'How', max: 500 },
  { name: 'Risk', max: 200 },
  { name: 'Measurement', max: 500, measuredOnly: true },
  { name: 'Evidence', max: 300, media: true },
  { name: 'Checklist' },
];

const NOT_APPLICABLE = /^\s*not applicable:\s*\S/i;

function loadRules(file = RULES_FILE) {
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}

function branchRegex(rules) {
  const max = rules.branch.maxWords;
  return new RegExp(`^(${rules.branch.types.join('|')})/[a-z0-9]+(-[a-z0-9]+){0,${max - 1}}$`);
}

function titleRegex(rules) {
  return new RegExp(`^(${rules.branch.types.join('|')})(\\([a-z0-9-]+\\))?(!)?: \\S`);
}

/** Glob with ** (any depth), * (inside one segment) and ? (one character). */
function globToRegex(glob) {
  let re = '';
  for (let i = 0; i < glob.length; i++) {
    const c = glob[i];
    if (c === '*' && glob[i + 1] === '*') {
      re += '.*';
      i++;
      if (glob[i + 1] === '/') i++;
    } else if (c === '*') re += '[^/]*';
    else if (c === '?') re += '[^/]';
    else re += c.replace(/[.+^${}()|[\]\\]/g, '\\$&');
  }
  return new RegExp(`^${re}$`);
}

function matchesAny(file, globs) {
  return (globs || []).some((g) => globToRegex(g).test(file));
}

function stripComments(text) {
  return text.replace(/<!--[\s\S]*?-->/g, '');
}

function stripMedia(text) {
  return text
    .replace(/!\[[^\]]*\]\([^)]*\)/g, '')
    .replace(/<img\b[^>]*>/gi, '')
    .replace(/<video\b[\s\S]*?<\/video>/gi, '')
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/https?:\/\/\S+/g, '');
}

/** Splits the body into its `## Heading` sections. */
function sections(body) {
  const found = new Map();
  let current = null;
  for (const line of stripComments(body).replace(/\r\n/g, '\n').split('\n')) {
    const heading = line.match(/^##\s+(.+?)\s*$/);
    if (heading) {
      current = heading[1];
      found.set(current, []);
    } else if (current) {
      found.get(current).push(line);
    }
  }
  return new Map([...found].map(([k, v]) => [k, v.join('\n').trim()]));
}

/** PRs the standard does not judge: the release PR and dependency bots. */
function exempt(rules, { branch, author }) {
  if ((branch || '').startsWith('release-please--')) return 'the release pull request';
  if (author === 'dependabot[bot]' && (branch || '').startsWith('dependabot/')) return 'a Dependabot update';
  return null;
}

function check(rules, { branch = '', title = '', body = '', files = [], author = '' }) {
  const problems = [];
  const types = rules.branch.types;

  const bm = branch.match(branchRegex(rules));
  if (!bm) {
    problems.push(
      `Branch "${branch}" must be a type (${types.join(', ')}), a slash and 1 to ` +
        `${rules.branch.maxWords} lowercase words joined by hyphens, for example feat/export-cover-image.`,
    );
  }

  const tm = title.match(titleRegex(rules));
  if (!tm) {
    problems.push(`The title "${title}" must be a Conventional Commit: "<type>: <summary>", <type> one of ${types.join(', ')}.`);
  } else if (bm && bm[1] !== tm[1]) {
    problems.push(`The branch says "${bm[1]}" and the title says "${tm[1]}"; use the same type in both.`);
  }
  if (title.length > rules.title.max) {
    problems.push(`The title has ${title.length} characters; the limit is ${rules.title.max}.`);
  }

  const type = tm ? tm[1] : null;
  const breaking = tm ? Boolean(tm[3]) : false;
  const shipped = files.filter((f) => matchesAny(f, rules.shipped));
  const releases = type && (breaking || rules.releaseTypes.includes(type));

  // A change to what users install has to publish a version, or they never receive it.
  if (type && shipped.length > 0 && !releases) {
    problems.push(
      `This PR changes files that reach users (${shipped.slice(0, 3).join(', ')}${shipped.length > 3 ? ', ...' : ''}), ` +
        `but a "${type}" title publishes no version. Use ${rules.releaseTypes.join(', ')}.`,
    );
  }

  const measured = rules.measured && rules.measured.enabled;
  const found = sections(body);
  for (const { name, max, media, measuredOnly } of SECTIONS) {
    if (measuredOnly && !measured) continue;
    if (!found.has(name)) {
      problems.push(`The description has no "## ${name}" section. Start from .github/pull_request_template.md.`);
      continue;
    }
    const text = found.get(name);
    if (name === 'Checklist') {
      const boxes = text.match(/^\s*[-*]\s+\[[ xX]\]/gm) || [];
      const open = text.match(/^\s*[-*]\s+\[ \]/gm) || [];
      if (boxes.length === 0) problems.push('The Checklist has no boxes.');
      if (open.length > 0) problems.push(`${open.length} box(es) of the Checklist are not ticked.`);
      continue;
    }
    if (text.length === 0) {
      problems.push(`"## ${name}" is empty.`);
      continue;
    }
    const counted = media ? stripMedia(text).trim() : text;
    if (counted.length > max) {
      problems.push(`"## ${name}" has ${counted.length} characters${media ? ' (not counting images and links)' : ''}; the limit is ${max}.`);
    }
    if (name === 'Measurement' && type && rules.measured.types.includes(type) && shipped.length > 0) {
      if (NOT_APPLICABLE.test(text) || !/\d/.test(text)) {
        problems.push(`A "${type}" change to what users get has to be measured: "## Measurement" needs the numbers before and after.`);
      }
    } else if (name === 'Measurement' && !/\d/.test(text) && !NOT_APPLICABLE.test(text)) {
      problems.push('"## Measurement" needs numbers, or "Not applicable:" and the reason.');
    }
  }

  if (rules.issueFirst && !/\b(close[sd]?|fix(e[sd])?|resolve[sd]?)\s+#\d+/i.test(body)) {
    problems.push('Every change here starts with an issue: link it in the description ("Closes #12").');
  }

  return problems;
}

function arg(name) {
  const i = process.argv.indexOf(name);
  return i > 0 ? process.argv[i + 1] : undefined;
}

function filesFromGit(base) {
  const out = execFileSync('git', ['diff', '--name-only', `${base}...HEAD`], { encoding: 'utf8' });
  return out.split('\n').filter(Boolean);
}

if (require.main === module) {
  const rules = loadRules(arg('--rules') || RULES_FILE);
  const bodyFile = arg('--body-file');
  const input = {
    branch: arg('--branch') ?? process.env.PR_BRANCH ?? '',
    title: arg('--title') ?? process.env.PR_TITLE ?? '',
    body: bodyFile ? fs.readFileSync(bodyFile, 'utf8') : process.env.PR_BODY ?? '',
    author: process.env.PR_AUTHOR ?? '',
    files: arg('--files-from-git')
      ? filesFromGit(arg('--files-from-git'))
      : (process.env.PR_FILES ?? '').split('\n').filter(Boolean),
  };
  const why = exempt(rules, input);
  if (why) {
    console.log(`Not checked: ${why}.`);
    process.exit(0);
  }
  const problems = check(rules, input);
  if (problems.length > 0) {
    console.error('This pull request does not follow CONTRIBUTING.md:\n');
    for (const p of problems) console.error(`- ${p}`);
    process.exit(1);
  }
  console.log('The pull request follows CONTRIBUTING.md.');
}

module.exports = { check, exempt, loadRules, branchRegex, titleRegex, globToRegex, sections, SECTIONS };
