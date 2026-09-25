//! Caption files to timed text: WebVTT, SRT and the JSON event format some players fetch.
//!
//! The format is sniffed from the body, never taken from the URL or the content type: a caption
//! endpoint that honours a `fmt` parameter one day and ignores it the next is the normal case.

use super::Cue;

/// Parse whatever caption body came back. `None` when it is none of the formats we read, which
/// the caller reports rather than treating as a video with nothing said in it.
///
/// `rolling` is for machine-made captions that repeat the previous line at the top of every cue
/// so the display can scroll: without folding them, every sentence arrives two or three times.
pub fn parse(body: &str, rolling: bool) -> Option<Vec<Cue>> {
    let body = body.trim_start_matches('\u{feff}').trim_start();
    let cues = if body.starts_with('{') {
        json3(body)?
    } else if body.starts_with("WEBVTT") || body.contains("-->") {
        text_cues(body)
    } else {
        return None;
    };
    Some(if rolling { fold_rolling(cues) } else { cues })
}

/// `hh:mm:ss.mmm`, `mm:ss.mmm`, or SRT's `hh:mm:ss,mmm`.
pub fn timestamp(s: &str) -> Option<f64> {
    let s = s.trim().replace(',', ".");
    let mut parts: Vec<&str> = s.split(':').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return None;
    }
    let secs: f64 = parts.pop()?.parse().ok()?;
    let mins: f64 = parts.pop()?.parse().ok()?;
    let hours: f64 = match parts.pop() {
        Some(h) => h.parse().ok()?,
        None => 0.0,
    };
    Some(hours * 3600.0 + mins * 60.0 + secs)
}

fn timing(line: &str) -> Option<(f64, f64)> {
    let (a, rest) = line.split_once("-->")?;
    // Cue settings (`align:start position:0%`) follow the end time on the same line.
    let b = rest.split_whitespace().next()?;
    Some((timestamp(a)?, timestamp(b)?))
}

/// WebVTT and SRT share the only part that matters: a timing line, then text until a blank line.
/// Everything else — the header, NOTE, STYLE and REGION blocks, SRT's counters, cue identifiers —
/// is a block without a timing line, and is skipped for that reason alone.
fn text_cues(body: &str) -> Vec<Cue> {
    let body = body.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = Vec::new();
    for block in body.split("\n\n") {
        let mut lines = block.lines().skip_while(|l| !l.contains("-->"));
        let Some((start, end)) = lines.next().and_then(timing) else {
            continue;
        };
        let text = lines
            .map(clean)
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if !text.is_empty() {
            out.push(Cue { start, end, text });
        }
    }
    out
}

/// Tags out (`<c>`, `<i>`, `<v Speaker>`, inline `<00:00:01.920>` timings), entities decoded.
fn clean(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_tag = false;
    for ch in line.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lrm;", "")
        .replace("&rlm;", "")
        .replace("&amp;", "&");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Events with `tStartMs`, `dDurationMs` and `segs[].utf8`. Events with no words in them are
/// window and style declarations, or a bare line break appended to the one before.
fn json3(body: &str) -> Option<Vec<Cue>> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let events = v.get("events")?.as_array()?;
    let mut out = Vec::new();
    for e in events {
        let Some(segs) = e.get("segs").and_then(|s| s.as_array()) else {
            continue;
        };
        let text: String = segs
            .iter()
            .filter_map(|s| s.get("utf8").and_then(|t| t.as_str()))
            .collect();
        let text = clean(&text);
        if text.is_empty() {
            continue;
        }
        let start = e.get("tStartMs").and_then(|t| t.as_f64()).unwrap_or(0.0) / 1000.0;
        let dur = e.get("dDurationMs").and_then(|t| t.as_f64()).unwrap_or(0.0) / 1000.0;
        out.push(Cue {
            start,
            end: start + dur,
            text,
        });
    }
    Some(out)
}

/// Drop the lines a cue repeats from the one before it; a cue left with nothing new is dropped.
fn fold_rolling(cues: Vec<Cue>) -> Vec<Cue> {
    let mut out: Vec<Cue> = Vec::with_capacity(cues.len());
    let mut prev: Vec<String> = Vec::new();
    for cue in cues {
        let lines: Vec<String> = cue.text.lines().map(str::to_string).collect();
        let repeated = (0..=lines.len().min(prev.len()))
            .rev()
            .find(|&n| prev[prev.len() - n..] == lines[..n])
            .unwrap_or(0);
        let fresh = lines[repeated..].join("\n");
        prev = lines;
        if fresh.is_empty() {
            if let Some(last) = out.last_mut() {
                last.end = last.end.max(cue.end);
            }
            continue;
        }
        out.push(Cue { text: fresh, ..cue });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_with_and_without_hours_and_with_a_comma() {
        assert_eq!(timestamp("00:01:02.500"), Some(62.5));
        assert_eq!(timestamp("01:02.250"), Some(62.25));
        assert_eq!(timestamp("01:00:00,001"), Some(3600.001));
        assert_eq!(timestamp("12"), None);
    }

    #[test]
    fn webvtt_with_header_notes_identifiers_and_settings() {
        let vtt = "WEBVTT\nKind: captions\nLanguage: es\n\nNOTE a comment\n\n1\n00:00:05.237 --> 00:00:08.043 align:start\nEn las oficinas\nnos enfocamos\n\n00:08.043 --> 00:11.022\n<v Ana>que los <i>videos</i> &amp; más</v>\n";
        let cues = parse(vtt, false).unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start, 5.237);
        assert_eq!(cues[0].text, "En las oficinas\nnos enfocamos");
        assert_eq!(cues[1].text, "que los videos & más");
    }

    #[test]
    fn srt_with_crlf() {
        let srt = "1\r\n00:00:01,000 --> 00:00:02,500\r\nHello\r\n\r\n2\r\n00:00:03,000 --> 00:00:04,000\r\nworld\r\n";
        let cues = parse(srt, false).unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!((cues[0].start, cues[0].end), (1.0, 2.5));
        assert_eq!(cues[1].text, "world");
    }

    #[test]
    fn json_events_skip_windows_and_bare_line_breaks() {
        let j = r#"{"wireMagic":"pb3","events":[
            {"tStartMs":0,"dDurationMs":6479,"id":1,"wpWinPosId":1},
            {"tStartMs":2220,"dDurationMs":2239,"segs":[{"utf8":"Congratulate"},{"utf8":" the","tOffsetMs":805}]},
            {"tStartMs":3459,"dDurationMs":1000,"aAppend":1,"segs":[{"utf8":"\n"}]},
            {"tStartMs":4361,"dDurationMs":2118,"segs":[{"utf8":" you"},{"utf8":" chose"}]}]}"#;
        let cues = parse(j, false).unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "Congratulate the");
        assert_eq!(cues[0].start, 2.22);
        assert!((cues[0].end - 4.459).abs() < 1e-9);
        assert_eq!(cues[1].text, "you chose");
    }

    #[test]
    fn rolling_captions_say_each_line_once() {
        // The shape machine captions arrive in: the line being spoken with inline word timings,
        // a 10 ms hold of the finished line, then the finished line again above the next one.
        let vtt = "WEBVTT\n\n00:00:01.360 --> 00:00:03.790 align:start position:0%\n \nnever<00:00:01.920><c> gonna</c><00:00:02.240><c> give</c>\n\n00:00:03.790 --> 00:00:03.800 align:start position:0%\nnever gonna give\n \n\n00:00:03.800 --> 00:00:06.000 align:start position:0%\nnever gonna give\nyou<00:00:04.100><c> up</c>\n";
        let cues = parse(vtt, true).unwrap();
        let texts: Vec<&str> = cues.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, vec!["never gonna give", "you up"]);
        assert_eq!(cues[0].end, 3.8, "the hold extends the cue it repeated");
    }

    #[test]
    fn a_line_said_twice_by_a_person_is_kept_when_captions_are_not_rolling() {
        let vtt = "WEBVTT\n\n00:01.000 --> 00:02.000\nNever gonna give you up\n\n00:02.000 --> 00:03.000\nNever gonna give you up\n";
        assert_eq!(parse(vtt, false).unwrap().len(), 2);
    }

    #[test]
    fn a_body_in_no_caption_format_is_not_an_empty_transcript() {
        assert_eq!(parse("<html><body>Sign in</body></html>", false), None);
        assert_eq!(parse("", false), None);
    }
}
