//! Fetch one public web page and turn its readable part into reader text.
//! The companion shares a network with other home services, so every hop must
//! resolve to a public address: private, loopback, link-local, CGNAT/Tailscale
//! and documentation ranges are refused, also after redirects.
use crate::{Error, Result, invalid};
use reqwest::{
    Url,
    dns::{Addrs, Name, Resolve, Resolving},
};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};
/// Largest page body read from the network.
pub const PAGE_CAP: usize = 5 * 1024 * 1024;
/// Text cap left after the title and source link (ar1 allows 256 KiB of text).
const TEXT_CAP: usize = 250 * 1024;
const SHORTENED: &str = "\n\n[Article shortened to fit the reader.]";
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub title: String,
    pub text: String,
}
pub fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return public_v4(v4);
            }
            let s = v6.segments();
            !(v6.is_unspecified()
                || v6.is_loopback()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00 // unique local
                || (s[0] & 0xffc0) == 0xfe80 // link-local
                || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
                || (s[0] == 0x0064 && s[1] == 0xff9b) // NAT64
                || s[0] == 0x2002 // 6to4
                || (s[0] == 0x2001 && s[1] < 0x0200)) // Teredo and special-purpose
        }
    }
}
fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0
        || a == 10
        || a == 127
        || a >= 224
        || (a == 100 && (64..128).contains(&b)) // CGNAT, includes Tailscale
        || (a == 169 && b == 254)
        || (a == 172 && (16..32).contains(&b))
        || (a == 192 && b == 168)
        || (a == 192 && b == 0 && (c == 0 || c == 2))
        || (a == 192 && b == 88 && c == 99)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113))
}
/// Syntax rules for a link the companion may fetch. Hostnames are checked
/// again at connection time by `PublicResolver`.
pub fn allowed(url: &Url) -> bool {
    let host_ok = match url.host() {
        Some(url::Host::Domain(d)) => {
            !d.is_empty()
                && d.contains('.')
                && !d.ends_with(".local")
                && !d.ends_with(".internal")
                && !d.ends_with(".lan")
                && !d.ends_with(".home.arpa")
                && !d.eq_ignore_ascii_case("localhost")
                && !d.ends_with(".localhost")
        }
        Some(url::Host::Ipv4(ip)) => public_v4(ip),
        Some(url::Host::Ipv6(ip)) => public_ip(IpAddr::V6(ip)),
        None => false,
    };
    matches!(url.scheme(), "http" | "https")
        && host_ok
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.as_str().len() <= 2048
}
/// Resolves names normally, then drops every non-public address.
struct PublicResolver;
impl Resolve for PublicResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let found: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(|a| public_ip(a.ip()))
                .collect();
            if found.is_empty() {
                return Err("no public address".into());
            }
            Ok(Box::new(found.into_iter()) as Addrs)
        })
    }
}
async fn has_public_address(url: &Url) -> Result<bool> {
    let Some(url::Host::Domain(host)) = url.host() else {
        return Ok(true);
    };
    let addrs: Vec<_> = tokio::net::lookup_host((host, 0))
        .await
        .map_err(|_| Error(502, "upstream_failed"))?
        .collect();
    Ok(addrs.iter().any(|a| public_ip(a.ip())))
}
pub struct Fetcher {
    client: reqwest::Client,
}
impl Fetcher {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .no_proxy()
                .dns_resolver(Arc::new(PublicResolver))
                .redirect(reqwest::redirect::Policy::custom(|attempt| {
                    if attempt.previous().len() >= 5 {
                        attempt.error("too many redirects")
                    } else if allowed(attempt.url()) {
                        attempt.follow()
                    } else {
                        attempt.stop()
                    }
                }))
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(25))
                .user_agent("Mozilla/5.0 (compatible; X3Companion/0.1; personal e-ink reader)")
                .build()
                .map_err(|_| invalid())?,
        })
    }
    /// 502 means try again later; 422 means this page cannot become an article.
    pub async fn fetch(&self, url: &str) -> Result<Page> {
        let unreadable = Error(422, "conversion_failed");
        let url = Url::parse(url).map_err(|_| invalid())?;
        if !allowed(&url) {
            return Err(invalid());
        }
        if !has_public_address(&url).await? {
            return Err(unreadable);
        }
        let response = self
            .client
            .get(url.clone())
            .header("Accept", "text/html,application/xhtml+xml,text/plain;q=0.8")
            .send()
            .await
            .map_err(|_| Error(502, "upstream_failed"))?;
        let status = response.status().as_u16();
        if status == 429 || status >= 500 {
            return Err(Error(502, "upstream_failed"));
        }
        if !response.status().is_success() {
            return Err(unreadable);
        }
        let final_url = response.url().to_string();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        let html = content_type.starts_with("text/html")
            || content_type.starts_with("application/xhtml+xml");
        if !html && !content_type.starts_with("text/plain") {
            return Err(unreadable);
        }
        let bytes = crate::upstream::bounded(response, PAGE_CAP)
            .await
            .map_err(|_| unreadable)?;
        let body = decode(&bytes, &content_type);
        tokio::task::spawn_blocking(move || {
            if html {
                extract(&body, &final_url)
            } else {
                page_from_text(&body, &final_url)
            }
        })
        .await
        .map_err(|_| unreadable)?
    }
}
/// Page source used by the preparation worker; tests swap in fixtures.
pub trait Links: Send + Sync {
    fn page<'a>(
        &'a self,
        url: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Page>> + Send + 'a>>;
}
impl Links for Fetcher {
    fn page<'a>(
        &'a self,
        url: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Page>> + Send + 'a>> {
        Box::pin(self.fetch(url))
    }
}
/// Charset from the Content-Type header, then an early `<meta charset>`;
/// UTF-8 otherwise. Invalid sequences become U+FFFD.
fn decode(bytes: &[u8], content_type: &str) -> String {
    let label = charset(content_type).or_else(|| {
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(2048)]).to_ascii_lowercase();
        let at = head.find("charset=")?;
        let rest = head[at + 8..].trim_start_matches(['"', '\'']);
        let end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .unwrap_or(rest.len());
        Some(rest[..end].to_owned())
    });
    let encoding = label
        .and_then(|l| encoding_rs::Encoding::for_label(l.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);
    encoding.decode(bytes).0.into_owned()
}
fn charset(content_type: &str) -> Option<String> {
    content_type
        .split(';')
        .filter_map(|p| p.trim().strip_prefix("charset="))
        .map(|v| v.trim_matches(['"', '\'']).to_owned())
        .next()
}
pub fn extract(html: &str, url: &str) -> Result<Page> {
    let unreadable = Error(422, "conversion_failed");
    let config = dom_smoothie::Config {
        text_mode: dom_smoothie::TextMode::Markdown,
        max_elements_to_parse: 100_000,
        ..Default::default()
    };
    let mut reader =
        dom_smoothie::Readability::new(html, Some(url), Some(config)).map_err(|_| unreadable)?;
    let article = reader.parse().map_err(|_| unreadable)?;
    let text = simplify(&article.text_content);
    if text.chars().filter(|c| c.is_alphanumeric()).count() < 80 {
        return Err(unreadable);
    }
    Ok(Page {
        title: title(
            &strip_site(&article.title, article.site_name.as_deref()),
            url,
        ),
        text,
    })
}
fn page_from_text(body: &str, url: &str) -> Result<Page> {
    let text = body.replace("\r\n", "\n");
    if text.trim().is_empty() {
        return Err(Error(422, "conversion_failed"));
    }
    Ok(Page {
        title: title("", url),
        text: text.trim().to_owned(),
    })
}
/// "Story | Site" → "Story" when the suffix is the page's own site name.
fn strip_site(title: &str, site: Option<&str>) -> String {
    let title = title.trim();
    if let Some(site) = site.map(str::trim).filter(|s| !s.is_empty()) {
        for sep in [" | ", " – ", " — ", " - ", " · ", " :: "] {
            if let Some(story) = title.strip_suffix(site).and_then(|t| t.strip_suffix(sep))
                && !story.trim().is_empty()
            {
                return story.trim().to_owned();
            }
        }
    }
    title.to_owned()
}
/// A clean one-line title of at most 256 bytes; the host name if none.
pub fn title(raw: &str, url: &str) -> String {
    let mut t: String = raw
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    if t.is_empty() {
        t = Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_else(|| "Article".into());
    }
    truncate(&mut t, 256);
    t
}
fn truncate(s: &mut String, max: usize) {
    if s.len() > max {
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
}
/// Cuts text over the cap at a paragraph break and says so at the end.
pub fn shorten(mut text: String) -> String {
    if text.len() <= TEXT_CAP {
        return text;
    }
    truncate(&mut text, TEXT_CAP - SHORTENED.len());
    if let Some(cut) = text.rfind("\n\n") {
        text.truncate(cut);
    }
    text.push_str(SHORTENED);
    text
}
/// Markdown from the extractor → the reader's plain format: `# ` headings,
/// blank-line paragraphs, `• ` list items. Images are dropped and links keep
/// only their words.
pub fn simplify(markdown: &str) -> String {
    let mut out = String::new();
    let mut fenced = false;
    // Markdown paragraphs: a line that continues a paragraph, list item or
    // quote joins the previous one with a space, unless that one ended with
    // a hard break (two spaces or a backslash).
    let mut joinable = false;
    let mut hard_break = false;
    for raw in markdown.lines() {
        let line = raw.trim_end();
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            joinable = false;
            continue;
        }
        let slash = !fenced && line.ends_with('\\') && !line.ends_with("\\\\");
        let breaks = raw.ends_with("  ") || slash;
        let line = if slash { &line[..line.len() - 1] } else { line };
        let trimmed = line.trim_start();
        let mut continues = false;
        let rendered = if fenced {
            line.to_owned()
        } else if let Some(cells) = table_row(trimmed) {
            // Separator rows (`| --- |`) carry no words; skip without a break.
            if cells
                .iter()
                .all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':')))
            {
                continue;
            }
            cells
                .iter()
                .map(|c| inline(c))
                .filter(|c| !c.trim().is_empty())
                .collect::<Vec<_>>()
                .join(" · ")
        } else if let Some(h) = heading(trimmed) {
            format!("# {}", inline(h))
        } else if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            format!("• {}", inline(item))
        } else if let Some((n, item)) = numbered(trimmed) {
            format!("{n}. {}", inline(item))
        } else if let Some(quote) = trimmed.strip_prefix('>') {
            continues = true;
            inline(quote.trim_start())
        } else if trimmed.chars().all(|c| matches!(c, '-' | '*' | '_' | ' ')) && trimmed.len() >= 3
        {
            String::new()
        } else {
            continues = true;
            inline(trimmed)
        };
        let rendered = rendered.trim_end();
        if rendered.trim().is_empty() || rendered == "#" || rendered == "•" {
            if !out.is_empty() && !out.ends_with("\n\n") {
                out.push('\n');
            }
            joinable = false;
            continue;
        }
        let join = continues && joinable && !hard_break && out.ends_with('\n');
        joinable = !fenced
            && !rendered.starts_with("# ")
            && (continues || rendered.starts_with("• ") || numbered(rendered).is_some());
        hard_break = breaks;
        if join {
            out.pop();
            out.push(' ');
            out.push_str(rendered.trim_start());
            out.push('\n');
            continue;
        }
        if rendered.starts_with("# ") && !out.is_empty() && !out.ends_with("\n\n") {
            out.push('\n');
        }
        out.push_str(rendered);
        out.push('\n');
        if rendered.starts_with("# ") {
            out.push('\n');
        }
    }
    let mut text = String::with_capacity(out.len());
    for line in out.lines() {
        if line.is_empty() && (text.is_empty() || text.ends_with("\n\n")) {
            continue;
        }
        // A no-break space after a list marker: it never stretches on a
        // justified line and never leaves the marker alone at a line end.
        if let Some(rest) = line.strip_prefix("• ") {
            text.push_str("•\u{a0}");
            text.push_str(rest);
        } else if let Some((n, rest)) = numbered(line) {
            text.push_str(n);
            text.push_str(".\u{a0}");
            text.push_str(rest);
        } else {
            text.push_str(line);
        }
        text.push('\n');
    }
    drop_empty_sections(text.trim())
}
/// Removes headings with no text under them, such as the "Comments" and
/// "Related" headings a site leaves after the article body.
fn drop_empty_sections(text: &str) -> String {
    let blocks: Vec<&str> = text.split("\n\n").collect();
    let mut kept: Vec<&str> = Vec::with_capacity(blocks.len());
    let mut body_follows = false;
    for block in blocks.iter().rev() {
        let is_heading = block.starts_with("# ") && !block.contains('\n');
        if is_heading && !body_follows {
            continue;
        }
        body_follows = !is_heading;
        kept.push(block);
    }
    kept.reverse();
    kept.join("\n\n")
}
/// Cells of a Markdown table row such as `| a | b |`.
fn table_row(line: &str) -> Option<Vec<&str>> {
    let inner = line.strip_prefix('|')?.strip_suffix('|')?;
    Some(inner.split('|').map(str::trim).collect())
}
/// Length of a citation marker such as `[12]` or `[citation needed]` at `i`.
fn citation(chars: &[char], i: usize) -> Option<usize> {
    let close = chars[i..].iter().take(20).position(|&c| c == ']')?;
    let inside: String = chars[i + 1..i + close].iter().collect();
    let numeric =
        !inside.is_empty() && inside.len() <= 4 && inside.chars().all(|c| c.is_ascii_digit());
    (numeric || inside == "citation needed").then_some(close + 1)
}
/// An ordered list item such as `12. text` or `3) text`.
fn numbered(line: &str) -> Option<(&str, &str)> {
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if !(1..=3).contains(&digits) {
        return None;
    }
    let rest = line[digits..]
        .strip_prefix(". ")
        .or_else(|| line[digits..].strip_prefix(") "))?;
    Some((&line[..digits], rest))
}
fn heading(line: &str) -> Option<&str> {
    let level = line.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = line[level..].strip_prefix(' ')?;
    Some(rest.trim_end_matches('#').trim())
}
/// Removes images, link targets, emphasis markers, inline code ticks and
/// backslash escapes from one line.
fn inline(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && chars.get(i + 1).is_some_and(|n| n.is_ascii_punctuation()) {
            out.push(chars[i + 1]);
            i += 2;
            continue;
        }
        let image = c == '!' && chars.get(i + 1) == Some(&'[');
        if c == '[' || image {
            let open = if image { i + 1 } else { i };
            if let Some((label_end, after)) = link_end(&chars, open) {
                if !image {
                    out.push_str(&inline(
                        &chars[open + 1..label_end].iter().collect::<String>(),
                    ));
                }
                i = after;
                continue;
            }
            if !image && let Some(skip) = citation(&chars, i) {
                // Drop the space before the marker too: "word [1]." → "word."
                if out.ends_with(' ') {
                    out.pop();
                }
                i += skip;
                continue;
            }
        }
        if (c == '*' || c == '_') && chars.get(i + 1) == Some(&c) {
            i += 2;
            continue;
        }
        if c == '*' || c == '`' {
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}
/// For `[label](target)` starting at `open`, returns the label's closing
/// bracket index and the index after `)`.
fn link_end(chars: &[char], open: usize) -> Option<(usize, usize)> {
    let mut depth = 0;
    let mut close = None;
    for (j, &c) in chars.iter().enumerate().skip(open) {
        match c {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(j);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close?;
    if chars.get(close + 1) != Some(&'(') {
        return None;
    }
    let mut depth = 0;
    for (j, &c) in chars.iter().enumerate().skip(close + 1) {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((close, j + 1));
                }
            }
            _ => {}
        }
    }
    None
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn addresses() {
        for ip in [
            "8.8.8.8",
            "1.1.1.1",
            "151.101.1.69",
            "2606:4700::1111",
            "2a00:1450:4001::200e",
        ] {
            assert!(public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "0.0.0.0",
            "10.1.2.3",
            "127.0.0.1",
            "100.64.0.1",
            "169.254.169.254",
            "172.18.0.1",
            "192.168.1.20",
            "192.0.2.1",
            "198.18.0.1",
            "224.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "::ffff:192.168.1.20",
            "::ffff:127.0.0.1",
            "fd7a:115c:a1e0::1",
            "fe80::1",
            "2001:db8::1",
            "64:ff9b::c0a8:2b",
            "2002:c0a8:2b::1",
            "ff02::1",
        ] {
            assert!(!public_ip(ip.parse().unwrap()), "{ip}");
        }
    }
    #[test]
    fn link_rules() {
        for u in [
            "https://example.org/a",
            "http://example.org/a?b=1#c",
            "https://8.8.8.8/x",
        ] {
            assert!(allowed(&u.parse().unwrap()), "{u}");
        }
        for u in [
            "ftp://example.org/",
            "file:///etc/passwd",
            "https://localhost/",
            "https://x.localhost/",
            "http://192.168.1.20/",
            "http://[::1]/",
            "http://[::ffff:7f00:1]/",
            "https://example.org:8443/",
            "https://user:pw@example.org/",
            "http://n8n/",
            "http://nas.local/",
            "http://router.lan/",
            "http://0x7f000001/",
            "http://2130706433/",
        ] {
            assert!(!allowed(&u.parse().unwrap()), "{u}");
        }
    }
    #[tokio::test]
    async fn resolver_drops_private_addresses() {
        let resolved = PublicResolver.resolve("localhost".parse().unwrap()).await;
        assert!(resolved.is_err());
    }
    #[tokio::test]
    async fn fetcher_refuses_loopback_server() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let fetcher = Fetcher::new().unwrap();
        // Rejected before any connection: an explicit port, then a loopback name.
        let e = fetcher
            .fetch(&format!("http://127.0.0.1:{port}/"))
            .await
            .unwrap_err();
        assert_eq!(e.0, 400);
        let e = fetcher.fetch("http://localhost/").await.unwrap_err();
        assert_eq!(e.0, 400);
        drop(listener);
    }
    #[test]
    fn markdown_to_reader_text() {
        let md = "## Zażółć *gęślą* jaźń ##\n\nA [link](https://x.org/a_(b)) and ![pic](i.png) **bold** \\*star\\*.\n\n- one\n* two\n\n> quoted `code`\n\n---\n\n\n\nEnd.";
        assert_eq!(
            simplify(md),
            "# Zażółć gęślą jaźń\n\nA link and  bold *star*.\n\n•\u{a0}one\n•\u{a0}two\n\nquoted code\n\nEnd."
        );
        assert_eq!(simplify("[a [b]](u) [unclosed"), "a [b] [unclosed");
        assert_eq!(
            simplify(
                "# Title\n\nBody.\n\n# Also in Biology\n\n## Comments\n\n# Next\n\nA teaser.\n\n# Newsletter"
            ),
            "# Title\n\nBody.\n\n# Next\n\nA teaser."
        );
        assert_eq!(
            simplify("Founded in 1997.[1][23] Tall [citation needed] trees. Keep [x] and [12345]."),
            "Founded in 1997. Tall trees. Keep [x] and [12345]."
        );
        assert_eq!(
            simplify("| Legend | Item |\n| ------ | :--: |\n| 1 | **Upper** layer |\n|  | empty |"),
            "Legend · Item\n1 · Upper layer\nempty"
        );
        assert_eq!(
            simplify("#nospace\n\n####### seven"),
            "#nospace\n\n####### seven"
        );
        assert_eq!(
            simplify("1. First item\n   goes on\n2) Second\n\n2026. A year, not a list."),
            "1.\u{a0}First item goes on\n2.\u{a0}Second\n\n2026. A year, not a list."
        );
    }
    #[test]
    fn extracts_article_and_rejects_thin_pages() {
        let body = "<p>Pchnąć w tę łódź jeża lub ośm skrzyń fig. ".repeat(20) + "</p>";
        let html = format!(
            "<html><head><title>Ignored site title</title><meta property=\"og:title\" content=\"Łódź notes\"></head><body><nav><a href=/>Home</a></nav><article><h1>Łódź notes</h1>{body}<h2>Second part</h2>{body}</article><footer>© footer</footer></body></html>"
        );
        let page = extract(&html, "https://example.org/notes").unwrap();
        assert_eq!(page.title, "Łódź notes");
        assert!(page.text.contains("# Second part"), "{}", page.text);
        assert!(page.text.contains("Pchnąć w tę łódź"));
        assert!(!page.text.contains("footer") && !page.text.contains("Home"));
        let e = extract(
            "<html><body><p>Too short.</p></body></html>",
            "https://example.org/",
        )
        .unwrap_err();
        assert_eq!(e.0, 422);
    }
    #[test]
    fn site_name_suffix_is_removed() {
        let q = Some("Quanta Magazine");
        assert_eq!(
            strip_site("Malaria Parasite | Quanta Magazine", q),
            "Malaria Parasite"
        );
        assert_eq!(strip_site("A – B - Quanta Magazine", q), "A – B");
        assert_eq!(strip_site("Quanta Magazine", q), "Quanta Magazine");
        assert_eq!(strip_site("Story | Other Site", q), "Story | Other Site");
        assert_eq!(strip_site("Story | Site", None), "Story | Site");
    }
    #[test]
    fn titles_charsets_and_length() {
        assert_eq!(title("  A \n  title\t", "https://e.org/"), "A title");
        assert_eq!(title("", "https://news.example.org/x"), "news.example.org");
        assert_eq!(title(&"ż".repeat(200), "https://e.org").len(), 256);
        let latin2 = [0x50, 0xb3, 0xf3, 0x64, 0xbc]; // "Płódź" in ISO-8859-2
        assert_eq!(decode(&latin2, "text/plain; charset=iso-8859-2"), "Płódź");
        let meta = b"<meta charset=\"windows-1250\"><p>\xb9</p>";
        assert!(decode(meta, "text/html").contains('ą'));
        assert_eq!(decode("zażółć".as_bytes(), "text/html"), "zażółć");
        let long = format!("{}\n\n{}", "a".repeat(200 * 1024), "b".repeat(100 * 1024));
        let short = shorten(long);
        assert!(short.len() <= TEXT_CAP && short.ends_with(SHORTENED));
        assert!(!short.contains('b'));
    }
}
