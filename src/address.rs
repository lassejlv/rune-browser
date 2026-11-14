//! What the address field's text means: a URL to load, or a search.

/// The URL to load for what was typed. `search` is the engine's address for a query, where
/// `{}` takes the encoded text.
pub fn resolve(input: &str, search: &str) -> String {
    let input = input.trim();
    if input.is_empty() {
        return String::new();
    }
    if has_scheme(input) {
        return input.to_owned();
    }
    if looks_like_host(input) {
        let host = input.split(['/', ':', '?', '#']).next().unwrap_or(input);
        // Local servers rarely have certificates.
        let local = host == "localhost" || host.parse::<std::net::Ipv4Addr>().is_ok();
        return format!("{}://{input}", if local { "http" } else { "https" });
    }
    search.replace("{}", &encode(input))
}

fn has_scheme(input: &str) -> bool {
    let Some((scheme, rest)) = input.split_once(':') else { return false };
    let named = scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    // "localhost:3000" and "example.com:8080/x" are a host and a port, not a scheme.
    named && (rest.starts_with("//") || matches!(scheme, "about" | "data" | "file" | "view-source" | "chrome" | "devtools" | "mailto" | "javascript" | "blob"))
}

fn looks_like_host(input: &str) -> bool {
    if input.contains(char::is_whitespace) {
        return false;
    }
    let host = input.split(['/', '?', '#']).next().unwrap_or(input);
    let host = host.rsplit_once(':').filter(|(_, port)| !port.is_empty() && port.chars().all(|c| c.is_ascii_digit())).map_or(host, |(host, _)| host);
    if host == "localhost" {
        return true;
    }
    let labels: Vec<&str> = host.split('.').collect();
    labels.len() >= 2
        && labels.iter().all(|label| !label.is_empty() && label.chars().all(|c| c.is_alphanumeric() || c == '-'))
        // "3.14" is a number to search for; "1.1.1.1" is an address.
        && (labels.last().is_some_and(|tld| tld.chars().any(char::is_alphabetic)) || host.parse::<std::net::Ipv4Addr>().is_ok())
}

fn encode(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    for byte in query.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(char::from(byte)),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// A URL split for showing: the site, then the rest in a quieter colour.
/// `https://www.example.com/a?b` is `("example.com", "/a?b")`.
pub fn display(url: &str) -> (String, String) {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    if rest.len() == url.len() {
        // about:blank, data: and the like have no site.
        return (url.to_owned(), String::new());
    }
    let split = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (host, path) = rest.split_at(split);
    let host = host.strip_prefix("www.").unwrap_or(host);
    (host.to_owned(), if path == "/" { String::new() } else { path.to_owned() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn go(input: &str) -> String {
        resolve(input, "https://duckduckgo.com/?q={}")
    }

    #[test]
    fn urls_load_as_typed_and_hosts_get_a_scheme() {
        assert_eq!(go("https://example.com/a b"), "https://example.com/a b");
        assert_eq!(go("  example.com/docs "), "https://example.com/docs");
        assert_eq!(go("localhost:3000/x"), "http://localhost:3000/x");
        assert_eq!(go("192.168.1.10"), "http://192.168.1.10");
        assert_eq!(go("example.com:8080"), "https://example.com:8080");
        assert_eq!(go("about:blank"), "about:blank");
    }

    #[test]
    fn everything_else_is_a_search() {
        assert_eq!(go("rust iterators"), "https://duckduckgo.com/?q=rust+iterators");
        assert_eq!(go("3.14"), "https://duckduckgo.com/?q=3.14");
        assert_eq!(go("what is c++?"), "https://duckduckgo.com/?q=what+is+c%2B%2B%3F");
        assert_eq!(go("node:fs"), "https://duckduckgo.com/?q=node%3Afs");
        assert_eq!(go(""), "");
    }

    #[test]
    fn display_drops_the_scheme_and_www() {
        assert_eq!(display("https://www.example.com/a?b"), ("example.com".into(), "/a?b".into()));
        assert_eq!(display("https://example.com/"), ("example.com".into(), String::new()));
        assert_eq!(display("about:blank"), ("about:blank".into(), String::new()));
    }
}
