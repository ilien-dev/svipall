//! Removing what a person cannot see before an agent reads it.
//!
//! An MCP server hands page text straight to a model, which makes any text on the page a potential
//! instruction to that model. `<script>`, `<style>` and `hidden` were already dropped, but the
//! interesting vector is text that is present, unhidden by any attribute, and invisible anyway:
//! `display:none`, `opacity:0`, `font-size:0`, or parked ten thousand pixels off the left edge.
//! A human reader never sees it; the model reads it as page content.
//!
//! Two rules, both deliberately narrow:
//!
//!   * an inline style that makes an element invisible drops the element;
//!   * characters with no visual width are stripped from the text that survives.
//!
//! Narrow because a false positive here silently deletes real content, which is worse than the
//! thing being prevented. Only inline `style` attributes are considered — resolving a stylesheet
//! would mean a CSS cascade, and a `.hidden` class can just as easily be a class named `hidden`
//! that shows something.

/// Whether an inline `style` attribute makes its element invisible to a reader.
///
/// Rendering is not simulated: this looks for the handful of declarations that are used to hide
/// text in practice, and nothing else.
pub fn is_visually_hidden(style: &str) -> bool {
    // Lowercase once; every needle below is ASCII.
    let s = style.to_ascii_lowercase();
    let value_of = |prop: &str| -> Option<String> {
        // Walk declarations rather than searching the whole string: `background-position` must not
        // match a search for `position`.
        s.split(';').find_map(|decl| {
            let (name, value) = decl.split_once(':')?;
            (name.trim() == prop).then(|| value.trim().to_string())
        })
    };

    if value_of("display").as_deref() == Some("none") {
        return true;
    }
    if matches!(
        value_of("visibility").as_deref(),
        Some("hidden" | "collapse")
    ) {
        return true;
    }
    // `opacity: 0`, `0.0`, `.0` — but not `0.5`.
    if let Some(o) = value_of("opacity") {
        if o.parse::<f32>().map(|v| v <= 0.0).unwrap_or(false) {
            return true;
        }
    }
    // Zero-size text. A unit-less `0` counts; `0.5em` does not.
    for prop in ["font-size", "width", "height", "max-height", "max-width"] {
        if let Some(v) = value_of(prop) {
            if is_zero_length(&v) {
                return true;
            }
        }
    }
    // Parked outside the viewport. The classic is `left: -9999px`, and the sign is what matters:
    // a large negative offset on a positioned element is never a layout anyone reads.
    if matches!(
        value_of("position").as_deref(),
        Some("absolute" | "fixed" | "relative")
    ) {
        for prop in ["left", "top", "right", "bottom", "text-indent"] {
            if let Some(v) = value_of(prop) {
                if is_far_negative(&v) {
                    return true;
                }
            }
        }
    }
    // `text-indent` is used to push text off-screen without positioning too.
    if let Some(v) = value_of("text-indent") {
        if is_far_negative(&v) {
            return true;
        }
    }
    // The screen-reader clip trick, in both its old and current spellings.
    if let Some(v) = value_of("clip") {
        if v.replace(' ', "") == "rect(0,0,0,0)" || v.replace(' ', "") == "rect(1px,1px,1px,1px)" {
            return true;
        }
    }
    false
}

fn is_zero_length(v: &str) -> bool {
    let v = v.trim();
    let num = v.trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '%');
    !num.is_empty() && num.parse::<f32>().map(|n| n == 0.0).unwrap_or(false)
}

fn is_far_negative(v: &str) -> bool {
    let v = v.trim();
    let num = v.trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '%');
    num.parse::<f32>().map(|n| n <= -500.0).unwrap_or(false)
}

/// Characters that occupy no space but carry text: zero-width joiners and spaces, the byte-order
/// mark, and the bidirectional overrides used to reorder what a reader sees.
///
/// These are how an instruction gets smuggled inside a sentence that looks innocent, so they are
/// removed from every string that reaches the model.
pub fn strip_invisible_chars(text: &str) -> String {
    text.chars()
        .filter(|c| {
            !matches!(
                c,
                '\u{200B}'..='\u{200F}'   // zero-width space/joiner, LTR/RTL marks
                | '\u{202A}'..='\u{202E}' // bidi embedding and overrides
                | '\u{2060}'..='\u{2064}' // word joiner, invisible operators
                | '\u{FEFF}'              // byte-order mark
                | '\u{00AD}'              // soft hyphen
            )
        })
        .collect()
}

/// True when stripping would change the string, so the common case allocates nothing.
pub fn has_invisible_chars(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(c,
            '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{FEFF}'
            | '\u{00AD}')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ways_text_is_actually_hidden_are_caught() {
        for style in [
            "display:none",
            "display: none;",
            "DISPLAY: NONE",
            "visibility:hidden",
            "visibility: collapse",
            "opacity:0",
            "opacity: 0.0",
            "font-size:0",
            "font-size: 0px",
            "height: 0; overflow: hidden",
            "position:absolute; left:-9999px",
            "position: fixed; top: -10000px",
            "text-indent: -9999px",
            "clip: rect(0, 0, 0, 0)",
        ] {
            assert!(is_visually_hidden(style), "missed: {style}");
        }
    }

    #[test]
    fn visible_text_is_never_dropped() {
        // A false positive deletes real content, which is worse than the injection it prevents.
        for style in [
            "color:red",
            "display:block",
            "display: flex; gap: 8px",
            "opacity: 0.5",
            "opacity:1",
            "font-size: 14px",
            "font-size: 0.9em",
            "position: absolute; left: 20px",
            "position:relative; top:-4px",
            "height: 100%",
            "width: 0.5rem",
            "margin: 0",
            "padding: 0; border: 0",
            "",
        ] {
            assert!(!is_visually_hidden(style), "false positive: {style}");
        }
    }

    #[test]
    fn a_property_name_is_not_matched_inside_another_one() {
        // `background-position` must not read as `position`, and `line-height: 0` is a real
        // typographic setting on a container whose children are still visible.
        assert!(!is_visually_hidden("background-position: 0 0"));
        assert!(!is_visually_hidden("border-top: 0"));
        assert!(!is_visually_hidden("line-height: 0"));
    }

    #[test]
    fn a_small_negative_offset_is_ordinary_layout() {
        // Overlapping an element by a few pixels is design; -9999px is hiding.
        assert!(!is_visually_hidden("position:absolute; left:-12px"));
        assert!(!is_visually_hidden("position:absolute; top:-200px"));
        assert!(is_visually_hidden("position:absolute; left:-5000px"));
    }

    #[test]
    fn zero_width_characters_are_stripped() {
        let smuggled = "Read\u{200B}this\u{FEFF} page\u{202E}";
        assert_eq!(strip_invisible_chars(smuggled), "Readthis page");
        assert!(has_invisible_chars(smuggled));
    }

    #[test]
    fn ordinary_text_survives_untouched() {
        let text = "Precio: 1.299,00 € — envío gratis. Ñandú, naïve, 日本語.";
        assert!(!has_invisible_chars(text));
        assert_eq!(strip_invisible_chars(text), text);
    }

    #[test]
    fn a_normal_space_is_not_invisible() {
        // Stripping real whitespace would run every word together.
        assert!(!has_invisible_chars("a b\tc\nd"));
        assert_eq!(strip_invisible_chars("a b\tc\nd"), "a b\tc\nd");
    }
}
