//! Text for the X3 reader (ADR 020): plain paragraphs, `# ` headings, `• `
//! list items and soft hyphens (U+00AD). The device breaks lines only at
//! spaces and soft hyphens, and every newline is a line break there.
use crate::link;
use hypher::{Lang, hyphenate_bounded};

/// Pasted text from the browser or a Shortcut: Markdown, plain text or HTML.
pub fn prepare(input: &str, url: &str) -> String {
    let input = input.replace("\r\n", "\n").replace('\r', "\n");
    let markdown = if looks_like_html(&input) {
        match link::extract(&input, url) {
            Ok(page) => return finish(page.text),
            Err(_) => strip_tags(&input),
        }
    } else {
        input
    };
    finish(link::simplify(&markdown))
}

/// Reader text after conversion: soft hyphens, then the length cap.
pub fn finish(text: String) -> String {
    link::shorten(hyphenate(&latex(&text)))
}

/// WordPress writes maths as `$latex … $`. The reader has no maths layout,
/// so common commands become plain text and the rest loses its markup:
/// `$latex \frac{1}{2^s} + \cdots$` → `1/2^s + …`.
fn latex(text: &str) -> String {
    const WORDS: [(&str, &str); 30] = [
        ("cdots", "…"),
        ("ldots", "…"),
        ("dots", "…"),
        ("infty", "∞"),
        ("cdot", "·"),
        ("times", "×"),
        ("pm", "±"),
        ("leq", "≤"),
        ("le", "≤"),
        ("geq", "≥"),
        ("ge", "≥"),
        ("neq", "≠"),
        ("ne", "≠"),
        ("approx", "≈"),
        ("alpha", "α"),
        ("beta", "β"),
        ("gamma", "γ"),
        ("delta", "δ"),
        ("epsilon", "ε"),
        ("zeta", "ζ"),
        ("theta", "θ"),
        ("lambda", "λ"),
        ("mu", "μ"),
        ("pi", "π"),
        ("rho", "ρ"),
        ("sigma", "σ"),
        ("tau", "τ"),
        ("phi", "φ"),
        ("chi", "χ"),
        ("omega", "ω"),
    ];
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("$latex") {
        let body = &rest[start + 6..];
        let Some(end) = body.find('$').filter(|&e| !body[..e].contains('\n')) else {
            break;
        };
        out.push_str(&rest[..start]);
        let mut tex = String::new();
        let mut chars = body[..end].chars().peekable();
        while let Some(c) = chars.next() {
            if c != '\\' {
                tex.push(c);
                continue;
            }
            let mut name = String::new();
            while let Some(&n) = chars.peek().filter(|n| n.is_ascii_alphabetic()) {
                name.push(n);
                chars.next();
            }
            match name.as_str() {
                "" => {
                    // `\,` `\;` `\ ` are spaces; `\{` is a literal brace.
                    if let Some(n) = chars.next() {
                        tex.push(if matches!(n, ',' | ';' | ' ' | '!') {
                            ' '
                        } else {
                            n
                        });
                    }
                }
                "frac" => tex.push_str("\u{1}frac"),
                "left" | "right" => {}
                _ => match WORDS.iter().find(|(w, _)| *w == name) {
                    Some((_, v)) => tex.push_str(v),
                    None => tex.push_str(&name),
                },
            }
        }
        out.push_str(
            &fractions(&tex)
                .replace(['{', '}'], "")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
        );
        rest = &body[end + 1..];
    }
    out.push_str(rest);
    out
}

/// `\u{1}frac{a}{b}` → `a/b`, parenthesising a part with spaces or signs.
fn fractions(tex: &str) -> String {
    // The `{…}` group at the start of `s` (after spaces) and the bytes used.
    let group = |s: &str| -> Option<(String, usize)> {
        let skipped = s.len() - s.trim_start().len();
        let s = &s[skipped..];
        let mut depth = 0;
        for (i, c) in s.char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some((s[1..i].to_owned(), skipped + i + 1));
                    }
                }
                _ if depth == 0 => return None,
                _ => {}
            }
        }
        None
    };
    let wrap = |s: String| {
        if s.chars().any(|c| matches!(c, ' ' | '+' | '-' | '/')) {
            format!("({s})")
        } else {
            s
        }
    };
    let mut out = String::new();
    let mut rest = tex;
    while let Some(at) = rest.find("\u{1}frac") {
        out.push_str(&rest[..at]);
        let after = rest[at + 5..].trim_start();
        match group(after).and_then(|(a, n)| group(&after[n..]).map(|(b, m)| (a, b, n + m))) {
            Some((a, b, n)) => {
                out.push_str(&wrap(fractions(&a)));
                out.push('/');
                out.push_str(&wrap(fractions(&b)));
                rest = &after[n..];
            }
            None => rest = after,
        }
    }
    out.push_str(rest);
    out
}

fn looks_like_html(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    ["<html", "<body", "<p>", "<p ", "</p>", "<div", "<br"]
        .iter()
        .any(|t| lower.contains(t))
}

/// Fallback for HTML too short for the readability pass: block tags become
/// line breaks, other tags vanish, the common entities are decoded.
fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('>') else {
            rest = &rest[open..];
            break;
        };
        let tag = rest[open + 1..open + close].to_ascii_lowercase();
        let name = tag
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("");
        match name {
            "br" => out.push_str("  \n"),
            "p" | "div" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                out.push_str("\n\n")
            }
            _ => {}
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out.replace("&nbsp;", "\u{a0}")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Polish if Polish letters are common, English if the letters are nearly
/// all ASCII, otherwise none (wrong patterns are worse than no hyphens).
fn language(text: &str) -> Option<(Lang, usize, usize)> {
    let mut letters = 0usize;
    let mut ascii = 0usize;
    let mut polish = 0usize;
    for c in text.chars().filter(|c| c.is_alphabetic()) {
        letters += 1;
        if c.is_ascii() {
            ascii += 1;
        } else if "ąćęłńóśźżĄĆĘŁŃÓŚŹŻ".contains(c) {
            polish += 1;
        }
    }
    if letters == 0 {
        None
    } else if polish * 200 >= letters {
        Some((Lang::Polish, 2, 2))
    } else if ascii * 100 >= letters * 98 {
        // TeX's hyphenmins for American English.
        Some((Lang::English, 2, 3))
    } else {
        None
    }
}

/// Inserts soft hyphens into ordinary words: letters only, at least six,
/// lower case after the first letter. Links, addresses, acronyms, names
/// with inner capitals and words with digits are left alone.
pub fn hyphenate(text: &str) -> String {
    let Some((lang, left, right)) = language(text) else {
        return text.to_owned();
    };
    let mut out = String::with_capacity(text.len() + text.len() / 8);
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        // Headings are short and bold; a break inside one reads badly.
        if line.starts_with("# ") {
            out.push_str(line);
            continue;
        }
        for (j, token) in line.split(' ').enumerate() {
            if j > 0 {
                out.push(' ');
            }
            if token.contains("://") || token.contains('@') || token.contains('/') {
                out.push_str(token);
                continue;
            }
            let mut word = String::new();
            for c in token.chars() {
                if c.is_alphabetic() {
                    word.push(c);
                    continue;
                }
                push_word(&mut out, &word, lang, left, right, c.is_ascii_digit());
                word.clear();
                out.push(c);
            }
            push_word(&mut out, &word, lang, left, right, false);
        }
    }
    out
}

fn push_word(out: &mut String, word: &str, lang: Lang, left: usize, right: usize, digit: bool) {
    let plain = word.chars().count() >= 6
        && !digit
        && word.chars().skip(1).all(|c| !c.is_uppercase())
        && !out.chars().next_back().is_some_and(|c| c.is_ascii_digit());
    if !plain {
        out.push_str(word);
        return;
    }
    let lower = word.to_lowercase();
    if lower.chars().count() != word.chars().count() {
        out.push_str(word);
        return;
    }
    // Patterns work on lower case; map the breaks back by character count.
    let mut chars = word.chars();
    let mut first = true;
    for syllable in hyphenate_bounded(&lower, lang, left, right) {
        if !first {
            out.push('\u{ad}');
        }
        first = false;
        for _ in syllable.chars() {
            if let Some(c) = chars.next() {
                out.push(c);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn visible(s: &str) -> String {
        s.replace('\u{ad}', "-")
    }
    #[test]
    fn english_and_polish_words() {
        assert_eq!(
            visible(&hyphenate("The hyphenation patterns are wonderful.")),
            "The hy-phen-ation pat-terns are won-der-ful."
        );
        let pl = hyphenate("Zażółć gęślą jaźń: przedsiębiorstwo i konstantynopolitańczyk.");
        assert!(
            visible(&pl).contains("przed-się-bior-stwo"),
            "{}",
            visible(&pl)
        );
        assert_eq!(
            pl.replace('\u{ad}', ""),
            "Zażółć gęślą jaźń: przedsiębiorstwo i konstantynopolitańczyk."
        );
    }
    #[test]
    fn leaves_special_tokens_alone() {
        let s = "Visit https://example.org/understanding or mail someone@example.org about NASA, iPhone, Wonderful and 2026wonderful words.";
        let h = visible(&hyphenate(s));
        assert!(h.contains("https://example.org/understanding"));
        assert!(h.contains("someone@example.org"));
        assert!(h.contains(" NASA,") && h.contains(" iPhone,"));
        assert!(h.contains("2026wonderful"));
        assert!(h.contains("Won-der-ful"), "{h}");
        // German: neither Polish nor plain ASCII, so no hyphens.
        let de = "Größenwahnsinnige Straßenbahnhaltestelle überall müssen.";
        assert_eq!(hyphenate(de), de);
        assert_eq!(hyphenate(""), "");
    }
    #[test]
    fn pasted_markdown_html_and_wrapped_text() {
        let wrapped = "# Notes\n\nThis paragraph was\nhard wrapped at\nforty columns.\n\n- item one\n  continues here\n- item two\n\nLine one  \nLine two\\\nLine three";
        assert_eq!(
            prepare(wrapped, "https://example.org/").replace('\u{ad}', ""),
            "# Notes\n\nThis paragraph was hard wrapped at forty columns.\n\n•\u{a0}item one continues here\n•\u{a0}item two\n\nLine one\nLine two\nLine three"
        );
        let html = "<p>Hello &amp; welcome,<br>friend.</p><div>Second <b>part</b> &lt;ok&gt;</div>";
        assert_eq!(
            prepare(html, "https://example.org/").replace('\u{ad}', ""),
            "Hello & welcome,\nfriend.\n\nSecond part <ok>"
        );
        assert_eq!(
            visible(&prepare("Unclosed <tag", "https://example.org/")),
            "Un-closed <tag"
        );
    }

    #[test]
    fn headings_are_not_hyphenated() {
        let h = hyphenate("# Understanding everything\n\nUnderstanding everything");
        assert!(!h.lines().next().unwrap().contains('\u{ad}'));
        assert!(h.lines().last().unwrap().contains('\u{ad}'));
    }
    #[test]
    fn wordpress_latex_becomes_plain_text() {
        assert_eq!(
            latex(
                "Sum $latex 1 + \\frac{1}{2} + \\frac {1} {3} + \\cdots $. Then $latex \\zeta (s) = 1 + \\frac{1}{2^s} + \\frac{a+b}{c}$ ok"
            ),
            "Sum 1 + 1/2 + 1/3 + …. Then ζ (s) = 1 + 1/2^s + (a+b)/c ok"
        );
        assert_eq!(
            latex("Costs $5 and $latex x \\leq \\infty$."),
            "Costs $5 and x ≤ ∞."
        );
        assert_eq!(latex("Broken $latex \\frac{1}"), "Broken $latex \\frac{1}");
        assert_eq!(latex("No end $latex x\nnext $"), "No end $latex x\nnext $");
        assert_eq!(
            latex("$latex \\frac{1}{x}$ and \\{x\\}$"),
            "1/x and \\{x\\}$"
        );
    }
}
