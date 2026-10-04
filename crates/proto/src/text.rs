// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! What text others write may contain. Shown text is never markup (the app sets `textContent`,
//! the plain pages escape it), so these rules are about **spoofing**, not execution: characters
//! that are invisible, or that reorder what is shown, would let a title or a post look like
//! something else (a right-to-left override turns "gpj.exe" into "exe.jpg"; a zero-width space
//! makes two equal-looking board titles differ).
//!
//! - [`line_ok`], one-line names (channel and board titles, thread subjects): no control
//!   characters, nothing invisible, no direction controls. Emoji variation selectors stay.
//! - [`body_ok`], multi-line text (posts, "about", rules): new lines and tabs allowed; no other
//!   control characters, no direction overrides, embeddings or isolates, no invisible tag or
//!   annotation characters. Joiners (U+200C, U+200D), marks (U+200E, U+200F) and variation
//!   selectors stay: scripts and emoji need them.

/// Characters that change the direction of what follows them: overrides, embeddings, isolates
/// and the Arabic letter mark.
#[inline]
pub fn direction(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Characters with no visible form that are not needed by any script in a title.
#[inline]
pub fn invisible(c: char) -> bool {
    matches!(c,
        '\u{00AD}' | '\u{034F}' | '\u{115F}' | '\u{1160}' | '\u{17B4}' | '\u{17B5}' | '\u{180E}'
        | '\u{200B}'..='\u{200F}' | '\u{2060}'..='\u{2065}' | '\u{206A}'..='\u{206F}' | '\u{3164}'
        | '\u{FEFF}' | '\u{FFA0}' | '\u{FFF9}'..='\u{FFFB}' | '\u{E0000}'..='\u{E007F}')
}

/// A one-line name others will see (titles, subjects).
pub fn line_ok(s: &str) -> bool {
    !s.chars().any(|c| c.is_control() || direction(c) || invisible(c))
}

/// Multi-line text others will see (posts, "about", rules).
pub fn body_ok(s: &str) -> bool {
    !s.chars().any(|c| (c.is_control() && c != '\n' && c != '\t') || direction(c) || matches!(c, '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}' | '\u{E0000}'..='\u{E007F}'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_bodies() {
        assert!(line_ok("Lab /b/ — Ελληνικά, العربية, 日本語 ❤️"));
        for bad in ["a\u{202E}gpj.exe", "Board\u{200B}", "a\nb", "x\u{2066}y", "tag\u{E0041}", "\u{FEFF}lead"] {
            assert!(!line_ok(bad), "{bad:?}");
        }
        assert!(body_ok("line one\n\tline two\n>greentext 👨\u{200D}👩 \u{200F}שלום"));
        for bad in ["text\u{202E}desrever", "a\u{2067}b", "hidden\u{E0041}tag", "bell\u{7}", "a\rb"] {
            assert!(!body_ok(bad), "{bad:?}");
        }
    }
}
