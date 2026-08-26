//! Read-only capture of SGF records embedded in a public live-game page.
//!
//! The adapter intentionally has no authentication surface. It accepts an
//! explicitly supplied HTTPS URL, fetches it through an injected client, then
//! extracts a complete SGF collection from the public response or a linked SGF
//! file. Site-specific WebSocket/private APIs are out of scope here.
//!
//! # SSRF safety
//!
//! Both the initial page URL and any SGF URL derived from the page are parsed
//! and re-validated before a fetch is issued. Only explicit public `https`
//! URLs with no embedded userinfo and a non-private, non-loopback host are
//! accepted. Relative links found in the page are resolved with the standard
//! URL `join` operation against the validated page URL, and the result is
//! subjected to the *same* validation before it is fetched.

use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, Ipv6Addr};
use url::{Host, Url};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StarRiverCapture {
    pub page_url: String,
    pub sgf: String,
    pub fetched_sgf_url: Option<String>,
}

pub trait PublicPageFetch {
    fn get(&mut self, url: &str) -> Result<String, String>;
}

pub fn capture_public_live_sgf(
    page_url: &str,
    fetch: &mut impl PublicPageFetch,
) -> Result<StarRiverCapture, StarRiverCaptureError> {
    let page_url = parse_public_https_url(page_url)?;
    let page = fetch
        .get(page_url.as_str())
        .map_err(StarRiverCaptureError::Fetch)?;
    if let Some(sgf) = extract_sgf_collection(&page) {
        return Ok(StarRiverCapture {
            page_url: page_url.to_string(),
            sgf,
            fetched_sgf_url: None,
        });
    }

    let raw_sgf_url = extract_sgf_link(&page).ok_or(StarRiverCaptureError::NoPublicSgfInPage)?;
    let sgf_url = resolve_public_sgf_url(&page_url, &raw_sgf_url)?;
    let sgf = fetch
        .get(sgf_url.as_str())
        .map_err(StarRiverCaptureError::Fetch)?;
    let sgf = extract_sgf_collection(&sgf).ok_or(StarRiverCaptureError::InvalidSgfResponse)?;
    Ok(StarRiverCapture {
        page_url: page_url.to_string(),
        sgf,
        fetched_sgf_url: Some(sgf_url.to_string()),
    })
}

/// Validates that `url` is an explicit, public, HTTPS URL with no userinfo and
/// a non-private / non-loopback / non-reserved host. The URL is parsed with the
/// standard `url` crate rather than raw string matching, so alternate spellings
/// (decimal/hex IP forms, IPv4-mapped IPv6, IPv6 loopback, etc.) are caught.
pub fn validate_public_https_url(url: &str) -> Result<(), StarRiverCaptureError> {
    let parsed = Url::parse(url.trim()).map_err(|_| StarRiverCaptureError::NonPublicUrl)?;
    ensure_public_https(&parsed)
}

fn parse_public_https_url(url: &str) -> Result<Url, StarRiverCaptureError> {
    let parsed = Url::parse(url.trim()).map_err(|_| StarRiverCaptureError::NonPublicUrl)?;
    ensure_public_https(&parsed)?;
    Ok(parsed)
}

/// Resolves a (possibly relative) SGF link against the validated page URL and
/// re-validates the result so that a malicious or mis-directed secondary link
/// can never escape the public-HTTPS, non-private policy.
fn resolve_public_sgf_url(base: &Url, raw: &str) -> Result<Url, StarRiverCaptureError> {
    let joined = base
        .join(raw.trim())
        .map_err(|_| StarRiverCaptureError::NonPublicUrl)?;
    ensure_public_https(&joined)?;
    Ok(joined)
}

fn ensure_public_https(url: &Url) -> Result<(), StarRiverCaptureError> {
    if is_public_https(url) {
        Ok(())
    } else {
        Err(StarRiverCaptureError::NonPublicUrl)
    }
}

/// Core policy: `https` scheme, no userinfo, resolvable non-reserved host.
fn is_public_https(url: &Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    match url.host() {
        Some(Host::Domain(domain)) => !is_loopback_hostname(domain),
        Some(Host::Ipv4(ip)) => !is_private_ipv4(ip),
        Some(Host::Ipv6(ip)) => !is_private_ipv6(ip),
        // No host (e.g. opaque `https:` reference) is not a fetchable public URL.
        None => false,
    }
}

/// `localhost` and any sub-domain of it (`foo.localhost`) map to the loopback
/// interface and are rejected regardless of case.
fn is_loopback_hostname(domain: &str) -> bool {
    let lower = domain.to_ascii_lowercase();
    lower == "localhost" || lower.ends_with(".localhost")
}

fn is_private_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_loopback() // 127.0.0.0/8
        || ip.is_private() // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
        || ip.is_link_local() // 169.254.0.0/16
        || ip.is_unspecified() // 0.0.0.0
        || ip.is_multicast() // 224.0.0.0/4
        || ip.is_broadcast() // 255.255.255.255
        || ip.is_documentation() // 192.0.2.0/24, 198.51.100.0/24, 203.0.113.0/24
        || is_cgnat(ip) // 100.64.0.0/10
        || is_benchmark(ip) // 198.18.0.0/15
        || is_reserved_v4(ip) // 240.0.0.0/4, 192.0.0.0/24 etc.
}

/// Shared-address space used by carrier-grade NAT (RFC 6598): 100.64.0.0/10.
fn is_cgnat(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (octets[1] & 0b1100_0000) == 0b0100_0000
}

/// Benchmarking network (RFC 2544): 198.18.0.0/15.
fn is_benchmark(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 198 && (octets[1] & 0xfe) == 0x12
}

/// Covers 192.0.0.0/24 (incl. 192.0.0.9/10 protocol assignments) and 240.0.0.0/4.
fn is_reserved_v4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 240 || (octets[0] == 192 && octets[1] == 0)
}

fn is_private_ipv6(ip: Ipv6Addr) -> bool {
    // IPv4-mapped addresses (`::ffff:0:0/96`) re-use the IPv4 policy.
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_private_ipv4(v4);
    }
    ip.is_loopback() // ::1
        || ip.is_unspecified() // ::
        || ip.is_unique_local() // fc00::/7
        || ip.is_unicast_link_local() // fe80::/10
        || ip.is_multicast() // ff00::/8
        || is_documentation_v6(ip) // 2001:db8::/32
        || is_special_purpose_v6(ip)
}

/// IPv4/IPv6 transition ranges and other special-purpose blocks that could
/// route to internal infrastructure (6to4, Teredo, ORCHID, 64:ff9b::/96).
fn is_special_purpose_v6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    let first = segments[0];
    match first {
        0x2001 => {
            // 2001:db8::/32 documentation is handled separately; the remaining
            // 2001::/16 blocks (6to4 2002::/16 handled below) cover Teredo
            // 2001:0000::/32 and 2001:20::/28 ORCHIDv2.
            let second = segments[1];
            second == 0x0000 // Teredo 2001::/32
                || (second & 0xfff0) == 0x0020 // ORCHIDv2 2001:20::/28
        }
        0x2002 => true, // 6to4 2002::/16
        _ => false,
    }
}

fn is_documentation_v6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    segments[0] == 0x2001 && segments[1] == 0x0db8
}

/// Finds a complete, balanced SGF collection, handling escaped brackets in
/// property values. The parsed output remains source-exact for SGF import.
pub fn extract_sgf_collection(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let start = bytes.windows(2).position(|pair| pair == b"(;")?;
    let mut depth = 0usize;
    let mut in_property_value = false;
    let mut escaped = false;

    for (index, byte) in bytes.iter().enumerate().skip(start) {
        if escaped {
            escaped = false;
            continue;
        }
        if in_property_value && *byte == b'\\' {
            escaped = true;
            continue;
        }
        match *byte {
            b'[' => in_property_value = true,
            b']' => in_property_value = false,
            b'(' if !in_property_value => depth += 1,
            b')' if !in_property_value => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return std::str::from_utf8(&bytes[start..=index])
                        .ok()
                        .map(ToOwned::to_owned);
                }
            }
            _ => {}
        }
    }
    None
}

/// Extracts the raw `.sgf` link candidate from a page. The candidate may be
/// absolute, root-relative, or relative; it is resolved and re-validated by
/// [`resolve_public_sgf_url`] against the already-validated page URL.
fn extract_sgf_link(page: &str) -> Option<String> {
    let lower = page.to_ascii_lowercase();
    let extension_offset = lower.find(".sgf")? + 4;
    let before = &page[..extension_offset];
    let start = before.rfind(['"', '\'']).map(|index| index + 1)?;
    Some(page[start..extension_offset].trim().to_owned())
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum StarRiverCaptureError {
    #[error("only explicit public HTTPS live-game URLs are supported")]
    NonPublicUrl,
    #[error("public page fetch failed: {0}")]
    Fetch(String),
    #[error("the public page contains no embedded or linked SGF record")]
    NoPublicSgfInPage,
    #[error("the linked SGF response did not contain a complete SGF collection")]
    InvalidSgfResponse,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[derive(Default)]
    struct FixtureFetch(BTreeMap<String, String>);

    impl PublicPageFetch for FixtureFetch {
        fn get(&mut self, url: &str) -> Result<String, String> {
            self.0
                .get(url)
                .cloned()
                .ok_or_else(|| "missing fixture".to_owned())
        }
    }

    fn capture_ok(page_url: &str, page_body: &str, sgf: &str) -> StarRiverCapture {
        let mut fetch = FixtureFetch(BTreeMap::from([
            (page_url.to_owned(), page_body.to_owned()),
            (sgf.to_owned(), "(;SZ[19];B[pd])".to_owned()),
        ]));
        capture_public_live_sgf(page_url, &mut fetch).expect("capture succeeds")
    }

    fn capture_err(page_url: &str, page_body: &str, sgf: &str) -> StarRiverCaptureError {
        let mut fetch = FixtureFetch(BTreeMap::from([
            (page_url.to_owned(), page_body.to_owned()),
            (sgf.to_owned(), "(;SZ[19];B[pd])".to_owned()),
        ]));
        capture_public_live_sgf(page_url, &mut fetch).expect_err("capture is rejected")
    }

    #[test]
    fn captures_an_embedded_public_sgf() {
        let url = "https://example.org/live/42";
        let mut fetch = FixtureFetch(BTreeMap::from([(
            url.to_owned(),
            "<script>const game = '(;SZ[9]PB[Black];B[dd];W[ee])';</script>".to_owned(),
        )]));
        let capture = capture_public_live_sgf(url, &mut fetch).expect("capture succeeds");
        assert_eq!(capture.sgf, "(;SZ[9]PB[Black];B[dd];W[ee])");
        assert_eq!(capture.fetched_sgf_url, None);
    }

    #[test]
    fn follows_a_public_relative_sgf_link() {
        let page_url = "https://example.org/live/42";
        let sgf_url = "https://example.org/records/42.sgf";
        let mut fetch = FixtureFetch(BTreeMap::from([
            (
                page_url.to_owned(),
                "<a href=\"/records/42.sgf\">download</a>".to_owned(),
            ),
            (sgf_url.to_owned(), "(;SZ[19];B[pd])".to_owned()),
        ]));
        let capture = capture_public_live_sgf(page_url, &mut fetch).expect("capture succeeds");
        assert_eq!(capture.fetched_sgf_url.as_deref(), Some(sgf_url));
    }

    #[test]
    fn follows_an_https_absolute_sgf_link() {
        let page_url = "https://example.org/live/42";
        let sgf_url = "https://cdn.example.org/games/42.sgf";
        let capture = capture_ok(
            page_url,
            &format!("<a href=\"{sgf_url}\">download</a>"),
            sgf_url,
        );
        assert_eq!(capture.fetched_sgf_url.as_deref(), Some(sgf_url));
    }

    #[test]
    fn follows_a_same_directory_relative_sgf_link() {
        let page_url = "https://example.org/live/42";
        let sgf_url = "https://example.org/live/42.sgf";
        let capture = capture_ok(page_url, "<a href=\"42.sgf\">download</a>", sgf_url);
        assert_eq!(capture.fetched_sgf_url.as_deref(), Some(sgf_url));
    }

    #[test]
    fn rejects_non_public_urls() {
        for bad in [
            "http://localhost:8080/live",
            "http://127.0.0.1/live",
            "ftp://example.org/live",
            "https://user:pass@example.org/live",
            "https://user@example.org/live",
            "",
            "not a url",
            "https://[::1]/live",
        ] {
            assert!(
                matches!(
                    validate_public_https_url(bad),
                    Err(StarRiverCaptureError::NonPublicUrl)
                ),
                "expected {bad:?} to be rejected"
            );
        }
    }

    #[test]
    fn accepts_well_formed_public_https_urls() {
        for good in [
            "https://example.org/live",
            "https://example.org/live/42",
            "https://www.example.org/live",
            "https://example.org/live?a=1#frag",
        ] {
            assert!(
                validate_public_https_url(good).is_ok(),
                "{good:?} should be accepted"
            );
        }
    }

    #[test]
    fn rejects_loopback_and_localhost_hostnames() {
        for bad in [
            "https://localhost/live",
            "https://localhost:8443/live",
            "https://LOCALHOST/live",
            "https://foo.localhost/live",
            "https://bar.foo.localhost/live",
        ] {
            assert!(
                matches!(
                    validate_public_https_url(bad),
                    Err(StarRiverCaptureError::NonPublicUrl)
                ),
                "expected {bad:?} to be rejected"
            );
        }
    }

    #[test]
    fn rejects_ipv4_private_and_reserved_targets() {
        for bad in [
            "https://10.0.0.1/live",
            "https://172.16.0.1/live",
            "https://172.31.255.254/live",
            "https://192.168.1.1/live",
            "https://100.64.0.1/live",      // CGNAT
            "https://169.254.169.254/live", // link-local metadata
            "https://127.0.0.1/live",
            "https://127.0.0.2/live",
            "https://0.0.0.0/live",
            "https://224.0.0.1/live", // multicast
            "https://255.255.255.255/live",
            "https://198.18.0.1/live", // benchmark
            "https://192.0.2.1/live",  // documentation
        ] {
            assert!(
                matches!(
                    validate_public_https_url(bad),
                    Err(StarRiverCaptureError::NonPublicUrl)
                ),
                "expected {bad:?} to be rejected"
            );
        }
    }

    #[test]
    fn rejects_ipv6_loopback_private_and_reserved_targets() {
        for bad in [
            "https://[::1]/live",
            "https://[::]/live",
            "https://[fc00::1]/live",          // ULA
            "https://[fd12:3456::1]/live",     // ULA
            "https://[fe80::1]/live",          // link-local
            "https://[ff02::1]/live",          // multicast
            "https://[2001:db8::1]/live",      // documentation
            "https://[2002::1]/live",          // 6to4
            "https://[::ffff:127.0.0.1]/live", // IPv4-mapped loopback
            "https://[::ffff:10.0.0.1]/live",  // IPv4-mapped private
            "https://[0:0:0:0:0:0:0:1]/live",  // expanded ::1
        ] {
            assert!(
                matches!(
                    validate_public_https_url(bad),
                    Err(StarRiverCaptureError::NonPublicUrl)
                ),
                "expected {bad:?} to be rejected"
            );
        }
    }

    #[test]
    fn rejects_malicious_secondary_links() {
        // The page is public but every secondary link, once extracted and
        // resolved, targets a blocked host or scheme and is rejected before any
        // fetch. Each body is crafted so the extracted `.sgf` candidate is the
        // malicious URL itself.
        for page_body in [
            "<a href=\"https://127.0.0.1/records/42.sgf\">x</a>",
            "<a href=\"https://localhost/records/42.sgf\">x</a>",
            "<a href=\"https://169.254.169.254/latest/42.sgf\">x</a>",
            "<a href=\"//169.254.169.254/records/42.sgf\">x</a>", // protocol-relative
            "<a href=\"//[::1]/records/42.sgf\">x</a>",
            "<a href=\"//[fc00::1]/records/42.sgf\">x</a>",
            "<a href=\"https://user:pass@example.org/records/42.sgf\">x</a>",
            "<a href=\"http://example.org/records/42.sgf\">x</a>", // downgrade to http
            "<a href=\"file:///records/42.sgf\">x</a>",
            "<a href=\"javascript:/*.sgf*/\">x</a>",
            "<a href=\"data:text/plain,x.sgf\">x</a>",
        ] {
            let err = capture_err("https://example.org/live/42", page_body, "");
            assert_eq!(
                err,
                StarRiverCaptureError::NonPublicUrl,
                "body: {page_body}"
            );
        }
    }

    #[test]
    fn rejects_root_relative_link_pointing_into_private_network() {
        // The extracted raw link is trusted only after join + re-validation:
        // traversal (`../../`) and percent-encoded traversal are normalized by
        // the standard URL join and stay on the validated public host.
        let page_url = "https://example.org/live/42";
        let cases = [
            (
                "<a href=\"/../../records/42.sgf\">x</a>",
                "https://example.org/records/42.sgf",
            ),
            (
                "<a href=\"/records/%2e%2e/42.sgf\">x</a>",
                "https://example.org/42.sgf",
            ),
            (
                "<a href=\"../games/42.sgf\">x</a>",
                "https://example.org/games/42.sgf",
            ),
        ];
        for (body, expected) in cases {
            let capture = capture_ok(page_url, body, expected);
            assert_eq!(
                capture.fetched_sgf_url.as_deref(),
                Some(expected),
                "body: {body}"
            );
        }
    }

    #[test]
    fn extracts_only_real_sgf_links() {
        let page_url = "https://example.org/live/42";
        let sgf_url = "https://example.org/records/42.sgf";
        let mut fetch = FixtureFetch(BTreeMap::from([
            (
                page_url.to_owned(),
                "<a href=\"/records/42.sgf\">download</a>".to_owned(),
            ),
            (sgf_url.to_owned(), "(;SZ[19];B[pd])".to_owned()),
        ]));
        let capture = capture_public_live_sgf(page_url, &mut fetch).expect("capture succeeds");
        assert_eq!(capture.fetched_sgf_url.as_deref(), Some(sgf_url));
    }
}
