//! Verification codes in a notification ("Your code is 482 913"), offered for copying.
//! Every detector guesses at someone else's prose, so a false positive (offering "412"
//! from "412 passed") is worse than a miss: a code counts only near a word that says
//! it's one. The same rules as Omarchy's own notifications.

use regex::Regex;
use std::sync::LazyLock;

static CODE_WORDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(code|otp|one[\s-]?time|verification|verify|passcode|pin|2fa|two[\s-]factor|security key|confirm(?:ation)?)\b",
    )
    .unwrap()
});
// "482913", "482 913", "84 2139", "482-913", "G-482913".
static CODE_SHAPE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[^\w-])((?:[A-Za-z]-)?\d{2,4}[\s-]\d{2,5}|(?:[A-Za-z]-)?\d{4,8})(?:$|[^\w-])").unwrap());
// Uppercase letters and digits, at least one of each: "A9F3K2".
static CODE_ALNUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[^\w-])([A-Z0-9]{5,8})(?:$|[^\w-])").unwrap());
static WORD_BEFORE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([A-Za-z][\w.]*)\s+$").unwrap());
static PRODUCT_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([A-Z][a-z]+|[A-Z]{2,})$").unwrap());

const NEAR_BEFORE: usize = 24;
const NEAR_AFTER: usize = 72;
const MAX_CODES: usize = 3;

fn floor(t: &str, mut i: usize) -> usize {
    while i > 0 && !t.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil(t: &str, mut i: usize) -> usize {
    i = i.min(t.len());
    while i < t.len() && !t.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// "Claude Code", "VS Code": a capitalised word before a capitalised "Code" names a
/// product. "use code SAVE20" and "Your Verification Code" don't.
fn product_name(t: &str, at: usize, word: &str) -> bool {
    if !word.eq_ignore_ascii_case("code") || !word.starts_with('C') {
        return false;
    }
    let before = &t[floor(t, at.saturating_sub(20))..at];
    let Some(w) = WORD_BEFORE.captures(before).and_then(|c| c.get(1)) else { return false };
    PRODUCT_WORD.is_match(w.as_str()) && !CODE_WORDS.is_match(w.as_str())
}

/// Codes found in the title and text, at most three, without repeats.
pub fn codes(summary: &str, body: &str) -> Vec<String> {
    let text = format!("{summary}\n{body}");
    let mut spans = Vec::new();
    for m in CODE_WORDS.find_iter(&text) {
        if !product_name(&text, m.start(), m.as_str()) {
            spans.push((floor(&text, m.start().saturating_sub(NEAR_BEFORE)), ceil(&text, m.end() + NEAR_AFTER)));
        }
    }
    let mut out: Vec<String> = Vec::new();
    for re in [&*CODE_SHAPE, &*CODE_ALNUM] {
        for &(a, b) in &spans {
            let window = &text[a..b];
            // Matches may share a separator, so step one character at a time.
            let mut from = 0;
            while let Some(c) = re.captures_at(window, from) {
                let g = c.get(1).unwrap();
                let raw = g.as_str();
                let keep = if std::ptr::eq(re, &*CODE_ALNUM) {
                    raw.chars().any(|c| c.is_ascii_digit()) && raw.chars().any(|c| c.is_ascii_uppercase())
                } else {
                    true
                };
                // "G-482913" keeps its prefix; "482 913" and "482-913" are typed as one run.
                let prefixed = raw.as_bytes().get(1) == Some(&b'-') && raw.as_bytes()[0].is_ascii_alphabetic();
                let code = if prefixed { raw.to_string() } else { raw.replace(['-', ' '], "") };
                if keep && !out.contains(&code) && out.len() < MAX_CODES {
                    out.push(code);
                }
                from = g.end();
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_codes_near_their_word() {
        assert_eq!(codes("Your verification code", "482913 expires in 10 minutes"), vec!["482913"]);
        assert_eq!(codes("Sign-in", "Your code is 482 913."), vec!["482913"]);
        assert_eq!(codes("Google", "G-482913 is your Google verification code."), vec!["G-482913"]);
        assert_eq!(codes("Login", "Use one-time passcode A9F3K2 to continue"), vec!["A9F3K2"]);
    }

    #[test]
    fn leaves_other_numbers_alone() {
        assert!(codes("Tests", "412 passed, 3 failed").is_empty());
        assert!(codes("Order shipped", "Order 48291 is on its way").is_empty());
        assert!(codes("Claude Code", "Build 20261101 is ready").is_empty(), "a product name isn't a code word");
        assert!(codes("Code review", "ABCDEF looks good").is_empty(), "letters only isn't a code");
    }
}
