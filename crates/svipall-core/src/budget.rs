//! Keeping a response inside a context window.
//!
//! `web_fetch` could return a whole page, and a large one would blow the caller's context before
//! anything could be done about it. Only `web_crawl` had a cap, and it was a character count that
//! cut mid-sentence.
//!
//! Truncation happens on block boundaries, using the same notion of a block as the BM25 filter, so
//! a code fence or a table is never cut in half — that would turn valid markdown into noise.
//!
//! A continuation carries two pieces of context so it is not read cold: the whole chain of
//! headings it sits under, and optionally the tail of the page before it (`overlap_blocks`). Both
//! are preamble, and preamble is dropped whole rather than allowed to crowd out the first block of
//! new content — a page that returns nothing leaves its cursor where it was, and a caller paging
//! through would never finish.

use std::fmt::Write as _;

/// Markdown blocks. Fenced code and GFM tables are atomic: splitting either produces something a
/// model reads as broken rather than as truncated.
pub fn blocks(markdown: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = markdown.as_bytes();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut in_fence = false;
    let mut line_start = 0usize;

    while i <= bytes.len() {
        let at_end = i == bytes.len();
        if !at_end && bytes[i] != b'\n' {
            i += 1;
            continue;
        }
        let line = &markdown[line_start..i];
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
        }
        // A blank line ends a block, unless we are inside a fence. GFM tables contain no blank
        // lines, so they stay whole without a special case.
        let blank = line.trim().is_empty();
        if blank && !in_fence {
            let block = markdown[start..line_start].trim();
            if !block.is_empty() {
                out.push(block);
            }
            start = i + 1;
        }
        if at_end {
            let block = markdown[start.min(markdown.len())..].trim();
            if !block.is_empty() {
                out.push(block);
            }
            break;
        }
        i += 1;
        line_start = i;
    }
    out
}

/// Conservative token estimate without pulling in a tokenizer.
///
/// ASCII runs about four characters per token; CJK and emoji are closer to one token each, and a
/// little over one for the ones that need surrogate pairs. Erring high is deliberate — an estimate
/// that undercounts overflows the very context it exists to protect.
pub fn estimate_tokens(s: &str) -> usize {
    let (mut narrow, mut wide) = (0usize, 0usize);
    for c in s.chars() {
        if (c as u32) > 0x2E80 {
            wide += 1;
        } else {
            narrow += 1;
        }
    }
    narrow.div_ceil(4) + wide + wide / 4
}

/// Where a truncated response left off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    /// Hash of the full markdown this cursor was produced from.
    pub content_hash: u64,
    pub block: usize,
    pub char_off: usize,
    /// The `query` the paged view was filtered by, `Some("")` for none.
    ///
    /// The hash is of the filtered view, so a continuation that filtered differently found a
    /// different document and restarted from the top, "the page changed since that cursor was
    /// issued", on a page that had not changed. Carried in the cursor, the view it was issued for
    /// is the view it continues, whatever the next call's `query` says. `None` for a cursor from
    /// before this field existed.
    pub query: Option<String>,
}

impl Cursor {
    pub fn encode(&self) -> String {
        let mut s = format!("{:x}:{}:{}", self.content_hash, self.block, self.char_off);
        if let Some(q) = &self.query {
            s.push_str(":q");
            for b in q.as_bytes() {
                let _ = write!(s, "{b:02x}");
            }
        }
        s
    }

    pub fn decode(s: &str) -> Option<Self> {
        let mut parts = s.split(':');
        let hash = u64::from_str_radix(parts.next()?, 16).ok()?;
        let block = parts.next()?.parse().ok()?;
        let char_off = parts.next()?.parse().ok()?;
        let query = match parts.next() {
            None => None,
            Some(q) => {
                let hex = q.strip_prefix('q')?;
                if hex.len() % 2 != 0 {
                    return None;
                }
                let bytes = (0..hex.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
                    .collect::<Option<Vec<u8>>>()?;
                Some(String::from_utf8(bytes).ok()?)
            }
        };
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            content_hash: hash,
            block,
            char_off,
            query,
        })
    }

    /// The same cursor, saying which view it pages through.
    pub fn for_query(encoded: &str, query: Option<&str>) -> String {
        match Self::decode(encoded) {
            Some(mut c) => {
                c.query = Some(query.map(str::trim).unwrap_or_default().to_string());
                c.encode()
            }
            None => encoded.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BudgetOpts {
    pub max_tokens: usize,
    pub cursor: Option<Cursor>,
    /// How many blocks of the previous page a continuation repeats before its new content, so a
    /// resumed read picks up the thread instead of starting cold. 0 = none.
    pub overlap_blocks: usize,
}

#[derive(Debug, Default)]
pub struct Budgeted {
    pub content: String,
    pub tokens: usize,
    pub truncated: bool,
    pub next_cursor: Option<String>,
    pub blocks_returned: usize,
    pub total_blocks: usize,
    /// The cursor referred to a version of the page that no longer matches.
    pub stale_cursor: bool,
}

/// Nearest sentence or line boundary at or before `limit` characters.
fn cut_point(s: &str, limit: usize) -> usize {
    if s.chars().count() <= limit {
        return s.len();
    }
    let hard = s
        .char_indices()
        .nth(limit)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    let window = &s[..hard];
    window
        .rfind(". ")
        .map(|i| i + 2)
        .or_else(|| window.rfind('\n').map(|i| i + 1))
        .or_else(|| window.rfind(' ').map(|i| i + 1))
        .unwrap_or(hard)
}

/// Below this many tokens left, a page that already returned something stops at the block boundary
/// rather than ending on a sliver of the next block.
const MIN_FILL_TOKENS: usize = 16;

/// The run of backticks or tildes that opens a fenced block, when `block` is one.
fn fence_marker(block: &str) -> Option<&str> {
    let first = block.lines().next()?.trim_start();
    let c = first.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = first.chars().take_while(|x| *x == c).count();
    (n >= 3).then(|| &first[..n])
}

/// Where to cut a block that follows others on the page: after a whole row or line for a table or
/// a fence, after a sentence, a line or a word for prose. `0` when no such boundary fits, and then
/// the block waits whole for the next page.
fn boundary_cut(rest: &str, limit: usize, whole: &str) -> usize {
    let hard = rest
        .char_indices()
        .nth(limit)
        .map(|(i, _)| i)
        .unwrap_or(rest.len());
    if fence_marker(whole).is_some() || whole.trim_start().starts_with('|') {
        // Never part of a row: the next page starts on the row after the last one shown.
        return rest[..hard].rfind('\n').map_or(0, |i| i + 1);
    }
    match cut_point(rest, limit) {
        c if c == hard && hard < rest.len() => 0,
        c => c,
    }
}

/// The chain of headings above `idx`, outermost first, so a continuation says where it sits.
///
/// Ancestors only: a heading at the same or a deeper level than one already collected belongs to a
/// section that has since closed, and naming it would file the continuation under a section it is
/// not in.
fn heading_path(blocks: &[&str], idx: usize) -> Vec<String> {
    let mut path = Vec::new();
    let mut need = usize::MAX;
    for b in blocks[..idx].iter().rev() {
        let Some(first) = b.lines().next().map(str::trim) else {
            continue;
        };
        let level = first.chars().take_while(|c| *c == '#').count();
        // Markdown stops at six hashes and wants a space after them, so `#hashtag` is a paragraph.
        if level == 0 || level > 6 || !first[level..].starts_with(' ') {
            continue;
        }
        let text = first[level..].trim();
        if text.is_empty() || level >= need {
            continue;
        }
        need = level;
        path.push(text.to_string());
        if level == 1 {
            break;
        }
    }
    path.reverse();
    path
}

/// The last blocks of the previous page, in reading order.
///
/// Bounded to a quarter of the budget: the rest belongs to text the caller has not seen, and an
/// overlap that ate the page would be repetition sold as progress.
fn overlap(blocks: &[&str], start_block: usize, opts: &BudgetOpts) -> String {
    if opts.overlap_blocks == 0 {
        return String::new();
    }
    let room = opts.max_tokens / 4;
    let from = start_block.saturating_sub(opts.overlap_blocks);
    let mut taken: Vec<&str> = Vec::new();
    let mut cost = 0usize;
    for b in blocks[from..start_block].iter().rev() {
        let c = estimate_tokens(b);
        if cost + c > room {
            break;
        }
        cost += c;
        taken.push(b);
    }
    taken.reverse();
    taken.join("\n\n")
}

/// What opens a continuation: where it sits, and optionally the tail it resumes after.
fn preamble(blocks: &[&str], start_block: usize, opts: &BudgetOpts) -> String {
    if start_block == 0 {
        return String::new();
    }
    let path = heading_path(blocks, start_block);
    let under = if path.is_empty() {
        String::new()
    } else {
        format!(" under \"{}\"", path.join(" > "))
    };
    let repeat = overlap(blocks, start_block, opts);
    let mut out = String::new();
    if repeat.is_empty() {
        let _ = write!(out, "*(continuing{under})*\n\n");
    } else {
        let _ = write!(
            out,
            "*(continuing{under}; everything above the rule repeats the end of the previous page)*\n\n{repeat}\n\n---\n\n"
        );
    }
    out
}

struct Page {
    out: String,
    block: usize,
    off: usize,
    truncated: bool,
    returned: usize,
}

/// Take blocks from `(start_block, start_off)` until the budget runs out, after `prefix`.
///
/// `prefix` is charged to the budget like any other text; `returned` counts blocks of new content
/// only, which is what tells the caller whether this page made progress at all.
fn fill(
    blocks: &[&str],
    start_block: usize,
    start_off: usize,
    max_tokens: usize,
    prefix: &str,
) -> Page {
    let total = blocks.len();
    let mut out = prefix.to_string();
    let mut used = estimate_tokens(&out);
    let mut i = start_block;
    let mut off = start_off;
    let mut truncated = false;
    let mut returned = 0usize;

    while i < total {
        let block = if off > 0 {
            let byte = blocks[i]
                .char_indices()
                .nth(off)
                .map(|(b, _)| b)
                .unwrap_or(blocks[i].len());
            &blocks[i][byte..]
        } else {
            blocks[i]
        };
        let fence = fence_marker(blocks[i]);
        // A fence resumed inside is reopened, so the rest of it still reads as code.
        let reopen = match (off > 0, fence) {
            (true, Some(_)) => blocks[i].lines().next().map(str::trim).unwrap_or_default(),
            _ => "",
        };
        let cost = estimate_tokens(block) + estimate_tokens(reopen);
        if used + cost <= max_tokens {
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            if !reopen.is_empty() {
                out.push_str(reopen);
                out.push('\n');
            }
            out.push_str(block);
            used += cost;
            returned += 1;
            i += 1;
            off = 0;
            continue;
        }
        // A block that does not fit is cut to fill what is left: always when nothing has been
        // returned yet (a block larger than the whole budget still has to make progress, or a
        // caller paging through would loop forever on it), and whenever a quarter of the budget
        // or more is still unused. Measured: a 300-token budget on a page that opened with one
        // short line and then an infobox table returned 10 tokens, the table left whole for later.
        let room = max_tokens.saturating_sub(used);
        let underfilled = room >= MIN_FILL_TOKENS && room * 4 >= max_tokens;
        if room > 0 && (returned == 0 || underfilled) {
            // A fence that will be closed costs its markers too.
            let markers = fence.map_or(0, |f| f.len() + reopen.len() + 2);
            let chars = (room * 4).saturating_sub(markers);
            let cut = if returned == 0 {
                cut_point(block, chars)
            } else {
                boundary_cut(block, chars, blocks[i])
            };
            if cut > 0 {
                if !out.is_empty() {
                    out.push_str("\n\n");
                }
                // A cut fence is closed here and reopened where the cursor resumes, so neither
                // part reads as an unterminated code block. The markers are syntax, not content.
                if !reopen.is_empty() {
                    out.push_str(reopen);
                    out.push('\n');
                }
                out.push_str(block[..cut].trim_end());
                if let Some(f) = fence.filter(|_| cut < block.len()) {
                    out.push('\n');
                    out.push_str(f);
                }
                off += block[..cut].chars().count();
                returned += 1;
            }
        }
        truncated = true;
        break;
    }

    Page {
        out,
        block: i,
        off,
        truncated,
        returned,
    }
}

/// Take as much as fits, starting where the cursor says.
pub fn take(markdown: &str, opts: &BudgetOpts) -> Budgeted {
    let hash = crate::domain::stable_hash(markdown);
    let blocks = blocks(markdown);
    let total = blocks.len();

    let mut stale = false;
    let (mut start_block, mut start_off) = match &opts.cursor {
        Some(c) if c.content_hash == hash => (c.block, c.char_off),
        Some(_) => {
            // The page changed under the cursor. Silently continuing would splice two different
            // documents together, so start over and say why.
            stale = true;
            (0, 0)
        }
        None => (0, 0),
    };
    if start_block >= total {
        start_block = 0;
        start_off = 0;
    }

    // Resuming inside a block already continues the same sentence, so there is nothing to
    // reintroduce and repeating anything would duplicate text the caller has just read.
    let preamble = if start_off == 0 {
        preamble(&blocks, start_block, opts)
    } else {
        String::new()
    };

    let mut page = fill(&blocks, start_block, start_off, opts.max_tokens, &preamble);
    if page.returned == 0 && !preamble.is_empty() {
        // The preamble left no room for a single block of new content. Context is worth less than
        // progress: without this the cursor would come back where it went in, forever.
        page = fill(&blocks, start_block, start_off, opts.max_tokens, "");
    }

    let next_cursor = page.truncated.then(|| {
        Cursor {
            content_hash: hash,
            block: page.block,
            char_off: page.off,
            query: None,
        }
        .encode()
    });

    Budgeted {
        tokens: estimate_tokens(&page.out),
        content: page.out,
        truncated: page.truncated,
        next_cursor,
        blocks_returned: page.returned,
        total_blocks: total,
        stale_cursor: stale,
    }
}

/// Take as much of a text that must come back as it is (a source file, plain text, an API's JSON)
/// as fits, starting where the cursor says.
///
/// `take` reads markdown: it trims each block and joins blocks with one blank line, which is right
/// for prose and changes a program. Here the unit is the line, kept with its own line ending, and
/// nothing is added: no heading path, no overlap. A text that fits comes back byte for byte, and the
/// parts a cursor pages through, put back together, are the text. A single line longer than the
/// whole budget (minified JSON) is cut inside the line, so paging always makes progress.
pub fn take_lines(text: &str, opts: &BudgetOpts) -> Budgeted {
    let hash = crate::domain::stable_hash(text);
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let total = lines.len();
    let mut stale = false;
    let (mut line, mut off) = match &opts.cursor {
        Some(c) if c.content_hash == hash => (c.block, c.char_off),
        Some(_) => {
            stale = true;
            (0, 0)
        }
        None => (0, 0),
    };
    if line >= total {
        (line, off) = (0, 0);
    }
    let mut out = String::new();
    let mut used = 0usize;
    let mut returned = 0usize;
    let mut truncated = false;
    while line < total {
        let rest = match lines[line].char_indices().nth(off) {
            Some((b, _)) => &lines[line][b..],
            None => "",
        };
        let cost = estimate_tokens(rest);
        if used + cost <= opts.max_tokens {
            out.push_str(rest);
            used += cost;
            returned += 1;
            line += 1;
            off = 0;
            continue;
        }
        if returned == 0 {
            // A line larger than the whole budget: as many characters as fit, and the rest of the
            // line next time.
            let chars = opts.max_tokens.max(1) * 4;
            let piece: String = rest.chars().take(chars).collect();
            off += piece.chars().count();
            out.push_str(&piece);
            returned += 1;
        }
        truncated = true;
        break;
    }
    let next_cursor = truncated.then(|| {
        Cursor {
            content_hash: hash,
            block: line,
            char_off: off,
            query: None,
        }
        .encode()
    });
    Budgeted {
        tokens: estimate_tokens(&out),
        content: out,
        truncated,
        next_cursor,
        blocks_returned: returned,
        total_blocks: total,
        stale_cursor: stale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_that_fits_comes_back_byte_for_byte_and_one_that_does_not_pages_back_together() {
        let text = "def f():\n    if x:\n\n\n        return 1\r\n    # end\n";
        let all = take_lines(text, &plain(1000, None));
        assert_eq!(all.content, text);
        assert!(!all.truncated);

        let mut joined = String::new();
        let mut cursor = None;
        loop {
            let part = take_lines(text, &plain(3, cursor.take()));
            assert!(!part.stale_cursor);
            joined.push_str(&part.content);
            match part.next_cursor {
                Some(c) => cursor = Cursor::decode(&c),
                None => break,
            }
        }
        assert_eq!(joined, text);

        // One line longer than the budget still makes progress, and still pages back together.
        let long = "x".repeat(100);
        let mut joined = String::new();
        let mut cursor = None;
        for _ in 0..100 {
            let part = take_lines(&long, &plain(5, cursor.take()));
            assert!(!part.content.is_empty());
            joined.push_str(&part.content);
            match part.next_cursor {
                Some(c) => cursor = Cursor::decode(&c),
                None => break,
            }
        }
        assert_eq!(joined, long);
    }

    /// The options every test that is not about paging context uses.
    fn plain(max_tokens: usize, cursor: Option<Cursor>) -> BudgetOpts {
        BudgetOpts {
            max_tokens,
            cursor,
            overlap_blocks: 0,
        }
    }

    #[test]
    fn blocks_keep_code_fences_whole() {
        let md = "intro\n\n```rust\nfn a() {}\n\nfn b() {}\n```\n\nafter";
        let b = blocks(md);
        assert_eq!(
            b.len(),
            3,
            "the blank line inside the fence split it: {b:?}"
        );
        assert!(b[1].starts_with("```") && b[1].ends_with("```"));
    }

    #[test]
    fn blocks_keep_gfm_tables_whole() {
        let md = "before\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n\nafter";
        let b = blocks(md);
        assert_eq!(b.len(), 3, "{b:?}");
        assert!(b[1].contains("| 1 | 2 |"));
    }

    #[test]
    fn a_budget_never_splits_a_code_fence() {
        let md = format!("intro\n\n```\n{}\n```", "x".repeat(4000));
        let out = take(&md, &plain(30, None));
        let fences = out.content.matches("```").count();
        assert!(
            fences == 0 || fences == 2,
            "a fence was left unclosed: {}",
            out.content
        );
    }

    #[test]
    fn paging_with_the_cursor_reassembles_the_whole_document() {
        let md = (0..40)
            .map(|i| format!("Paragraph number {i} with some words in it."))
            .collect::<Vec<_>>()
            .join("\n\n");
        let mut seen = Vec::new();
        let mut cursor = None;
        for _ in 0..50 {
            let out = take(&md, &plain(40, cursor.as_deref().and_then(Cursor::decode)));
            seen.push(out.content.clone());
            match out.next_cursor {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        let joined = seen.join("\n");
        for i in 0..40 {
            assert!(
                joined.contains(&format!("Paragraph number {i} ")),
                "paragraph {i} never appeared across the pages"
            );
        }
    }

    #[test]
    fn a_cursor_from_a_different_version_is_reported_and_restarted() {
        let a = "one\n\ntwo\n\nthree";
        let first = take(a, &plain(2, None));
        let cursor = Cursor::decode(&first.next_cursor.unwrap()).unwrap();

        let changed = "completely different content here\n\nand more of it";
        let out = take(changed, &plain(100, Some(cursor)));
        assert!(out.stale_cursor, "a stale cursor must be reported");
        assert!(out.content.contains("completely different"));
    }

    #[test]
    fn a_continuation_names_the_section_it_resumes_under() {
        let md = "# Title\n\nfirst paragraph here\n\nsecond paragraph here\n\nthird paragraph here";
        let first = take(md, &plain(8, None));
        let cursor = Cursor::decode(&first.next_cursor.expect("should truncate")).unwrap();
        let second = take(md, &plain(100, Some(cursor)));
        assert!(
            second.content.contains("continuing under \"Title\""),
            "{}",
            second.content
        );
    }

    #[test]
    fn a_continuation_names_the_whole_heading_path() {
        let md = "# Guide\n\n## Install\n\n### Windows\n\nfirst paragraph here\n\nsecond paragraph here\n\nthird paragraph here";
        let first = take(md, &plain(20, None));
        let cursor = Cursor::decode(&first.next_cursor.expect("should truncate")).unwrap();
        let second = take(md, &plain(200, Some(cursor)));
        assert!(
            second.content.contains("Guide > Install > Windows"),
            "a deep continuation must carry its ancestors: {}",
            second.content
        );
    }

    #[test]
    fn a_heading_path_keeps_ancestors_and_drops_closed_sections() {
        let md = "# Guide\n\n## First\n\nalpha text here\n\n## Second\n\nbeta text here\n\ngamma text here";
        let first = take(md, &plain(15, None));
        let cursor = Cursor::decode(&first.next_cursor.expect("should truncate")).unwrap();
        let second = take(md, &plain(200, Some(cursor)));
        let line = second.content.lines().next().unwrap_or_default();
        assert!(line.contains("Guide > Second"), "{line}");
        assert!(
            !line.contains("First"),
            "a section that has closed is not an ancestor: {line}"
        );
    }

    #[test]
    fn a_hash_that_is_not_a_heading_is_not_read_as_one() {
        let md = "#hashtag\n\nalpha text here\n\nbravo text here\n\ncharlie text here";
        let first = take(md, &plain(6, None));
        let cursor = Cursor::decode(&first.next_cursor.expect("should truncate")).unwrap();
        let second = take(md, &plain(200, Some(cursor)));
        assert!(
            !second.content.contains("under \"hashtag\""),
            "{}",
            second.content
        );
    }

    #[test]
    fn an_overlap_repeats_the_tail_of_the_previous_page_and_marks_it() {
        let md = "# T\n\nalpha alpha alpha\n\nbravo bravo bravo\n\ncharlie charlie charlie\n\ndelta delta delta";
        let opts = |cursor| BudgetOpts {
            max_tokens: 14,
            cursor,
            overlap_blocks: 1,
        };
        let first = take(md, &opts(None));
        assert!(first.content.contains("bravo"), "{}", first.content);
        assert!(!first.content.contains("charlie"), "{}", first.content);

        let cursor = Cursor::decode(&first.next_cursor.expect("should truncate")).unwrap();
        let second = take(
            md,
            &BudgetOpts {
                max_tokens: 200,
                cursor: Some(cursor.clone()),
                overlap_blocks: 1,
            },
        );
        assert!(
            second.content.contains("bravo"),
            "the tail of the previous page must come back: {}",
            second.content
        );
        assert!(
            second
                .content
                .contains("repeats the end of the previous page")
                && second.content.contains("\n---\n"),
            "the repeat must be marked and separated: {}",
            second.content
        );
        assert!(second.content.contains("charlie") && second.content.contains("delta"));

        let none = take(md, &plain(200, Some(cursor)));
        assert!(
            !none.content.contains("bravo"),
            "overlap_blocks = 0 must repeat nothing: {}",
            none.content
        );
    }

    #[test]
    fn an_overlap_never_takes_more_than_a_quarter_of_the_budget() {
        let long = "word ".repeat(200);
        let md = format!("# T\n\n{long}\n\n{long}\n\ntail text here");
        let blocks = blocks(&md);
        let opts = BudgetOpts {
            max_tokens: 40,
            cursor: None,
            overlap_blocks: 2,
        };
        // Both candidate blocks cost far more than a quarter of 40, so neither may be taken.
        assert_eq!(overlap(&blocks, 3, &opts), "");
    }

    #[test]
    fn a_generous_overlap_still_terminates_and_covers_the_document() {
        let md = (0..40)
            .map(|i| format!("Paragraph number {i} with some words in it."))
            .collect::<Vec<_>>()
            .join("\n\n");
        let mut seen = Vec::new();
        let mut cursor = None;
        for n in 0..200 {
            assert!(n < 199, "paging did not terminate with an overlap");
            let out = take(
                &md,
                &BudgetOpts {
                    max_tokens: 40,
                    cursor: cursor.as_deref().and_then(Cursor::decode),
                    overlap_blocks: 5,
                },
            );
            seen.push(out.content.clone());
            match out.next_cursor {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        let joined = seen.join("\n");
        for i in 0..40 {
            assert!(
                joined.contains(&format!("Paragraph number {i} ")),
                "paragraph {i} never appeared across the pages"
            );
        }
    }

    #[test]
    fn a_preamble_too_large_for_the_budget_is_dropped_rather_than_starving_the_page() {
        let md = "# A section heading longer than the whole budget will ever be\n\nalpha\n\nbravo\n\ncharlie\n\ndelta";
        let mut seen = Vec::new();
        let mut cursor = None;
        for n in 0..100 {
            assert!(n < 99, "paging did not terminate: the preamble starved it");
            let out = take(
                md,
                &BudgetOpts {
                    max_tokens: 4,
                    cursor: cursor.as_deref().and_then(Cursor::decode),
                    overlap_blocks: 3,
                },
            );
            seen.push(out.content.clone());
            match out.next_cursor {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        let joined = seen.join("\n");
        for w in ["alpha", "bravo", "charlie", "delta"] {
            assert!(joined.contains(w), "{w} never appeared: {joined}");
        }
    }

    #[test]
    fn a_resume_inside_a_block_repeats_nothing() {
        let md = format!("# T\n\n{}", "word ".repeat(2000));
        let page = |max_tokens, cursor| {
            take(
                &md,
                &BudgetOpts {
                    max_tokens,
                    cursor,
                    overlap_blocks: 2,
                },
            )
        };
        // A budget of one token takes the heading and stops on the block boundary after it.
        let first = page(1, None);
        let at_block = Cursor::decode(&first.next_cursor.expect("should truncate")).unwrap();
        assert_eq!(at_block.char_off, 0);

        // Resuming there is a clean boundary, so it is introduced; the block is far over budget,
        // so it is cut and the next cursor lands inside it.
        let second = page(60, Some(at_block));
        assert!(second.content.contains("continuing"), "{}", second.content);
        let mid_block = Cursor::decode(&second.next_cursor.expect("should truncate")).unwrap();
        assert!(mid_block.char_off > 0, "expected a mid-block cursor");

        let third = page(60, Some(mid_block));
        assert!(
            !third.content.contains("continuing"),
            "a mid-block resume is already contiguous: {}",
            third.content
        );
    }

    #[test]
    fn a_large_second_block_is_cut_on_a_row_to_fill_the_budget() {
        let rows: Vec<String> = (0..100).map(|i| format!("| r{i:03} | v |")).collect();
        let md = format!("short\n\n| k | v |\n| --- | --- |\n{}", rows.join("\n"));
        let out = take(&md, &plain(100, None));
        assert!(out.tokens >= 75, "{} tokens: {}", out.tokens, out.content);
        assert!(
            out.content.ends_with(" |"),
            "ends on a whole row: {}",
            out.content
        );
        let next = take(
            &md,
            &plain(10_000, out.next_cursor.as_deref().and_then(Cursor::decode)),
        );
        let both = format!("{}\n{}", out.content, next.content);
        for i in 0..100 {
            assert_eq!(both.matches(&format!("| r{i:03} |")).count(), 1, "{both}");
        }
    }

    #[test]
    fn a_fence_cut_to_fill_the_budget_is_closed_and_reopened() {
        let code: Vec<String> = (0..100).map(|i| format!("let x{i} = {i};")).collect();
        let md = format!("short\n\n```rust\n{}\n```", code.join("\n"));
        let first = take(&md, &plain(100, None));
        assert!(first.tokens >= 75, "{}", first.content);
        assert_eq!(first.content.matches("```").count(), 2, "{}", first.content);
        let c = first.next_cursor.as_deref().and_then(Cursor::decode);
        let rest = take(&md, &plain(10_000, c));
        assert!(rest.content.starts_with("```rust\n"), "{}", rest.content);
        assert_eq!(rest.content.matches("```").count(), 2, "{}", rest.content);
    }

    #[test]
    fn a_single_oversized_block_still_makes_progress() {
        let md = "x ".repeat(5000);
        let out = take(&md, &plain(50, None));
        assert!(out.truncated);
        assert!(out.blocks_returned > 0, "no progress on an oversized block");
        let c = Cursor::decode(&out.next_cursor.unwrap()).unwrap();
        assert!(c.char_off > 0, "the cursor must advance inside the block");
    }

    #[test]
    fn cursor_survives_a_round_trip() {
        let c = Cursor {
            content_hash: 0xdead_beef,
            block: 7,
            char_off: 42,
            query: None,
        };
        assert_eq!(Cursor::decode(&c.encode()), Some(c.clone()));
        assert_eq!(Cursor::decode("garbage"), None);
        // The view travels with it, punctuation and all.
        let q = Cursor::for_query(&c.encode(), Some("--effort: a/b <x>"));
        assert_eq!(
            Cursor::decode(&q).and_then(|c| c.query).as_deref(),
            Some("--effort: a/b <x>")
        );
        assert_eq!(
            Cursor::decode(&Cursor::for_query(&c.encode(), None))
                .and_then(|c| c.query)
                .as_deref(),
            Some(""),
            "no query is a view too"
        );
        assert_eq!(Cursor::decode("ab:1:2:zz"), None);
    }

    #[test]
    fn wide_characters_cost_more_than_ascii() {
        let ascii = estimate_tokens(&"a".repeat(400));
        let cjk = estimate_tokens(&"中".repeat(400));
        assert!(cjk > ascii * 3, "CJK must not be counted as 4 chars/token");
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn everything_fits_when_the_budget_is_generous() {
        let md = "one\n\ntwo\n\nthree";
        let out = take(md, &plain(10_000, None));
        assert!(!out.truncated);
        assert_eq!(out.next_cursor, None);
        assert_eq!(out.blocks_returned, 3);
        assert_eq!(out.total_blocks, 3);
    }
}
