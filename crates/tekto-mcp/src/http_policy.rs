//! Trust checks for the local dev HTTP server.
use std::net::IpAddr;

fn authority(value: &str) -> Option<(String, u16)> {
    if value.is_empty()
        || !value.is_ascii()
        || value
            .bytes()
            .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
    {
        return None;
    }
    let (host, port) = if let Some(rest) = value.strip_prefix('[') {
        let (host, rest) = rest.split_once(']')?;
        let ip: IpAddr = host.parse().ok()?;
        if !matches!(ip, IpAddr::V6(_)) || !ip.is_loopback() {
            return None;
        }
        (format!("[{ip}]"), rest)
    } else {
        let (host, rest) = value
            .split_once(':')
            .map_or((value, ""), |(h, _)| (h, &value[h.len()..]));
        let host = host.to_ascii_lowercase();
        let trusted = host == "localhost"
            || host
                .parse::<IpAddr>()
                .is_ok_and(|ip| matches!(ip, IpAddr::V4(_)) && ip.is_loopback());
        if !trusted {
            return None;
        }
        (host, rest)
    };
    let port = if port.is_empty() {
        80
    } else {
        let digits = port.strip_prefix(':')?;
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        digits.parse::<u16>().ok().filter(|p| *p != 0)?
    };
    Some((host, port))
}

/// Require one loopback Host and, when present, an exactly matching HTTP Origin.
pub(crate) fn trusted(headers: &str) -> bool {
    let mut host = None;
    let mut origin = None;
    for line in headers.split("\r\n").skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            return false;
        };
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
        {
            return false;
        }
        let value = value.trim_matches([' ', '\t']);
        if value.bytes().any(|b| b.is_ascii_control() && b != b'\t') {
            return false;
        }
        if name.eq_ignore_ascii_case("host") {
            if host.replace(value).is_some() {
                return false;
            }
        } else if name.eq_ignore_ascii_case("origin") && origin.replace(value).is_some() {
            return false;
        }
    }
    let Some(host) = host.and_then(authority) else {
        return false;
    };
    match origin {
        None => true,
        Some(value) => value
            .strip_prefix("http://")
            .and_then(authority)
            .is_some_and(|origin| origin == host),
    }
}

#[cfg(test)]
mod tests {
    use super::trusted;
    #[test]
    fn loopback_and_same_origin() {
        for host in [
            "localhost:3000",
            "LOCALHOST:3000",
            "127.0.0.1:3000",
            "127.1.2.3:3000",
            "[::1]:3000",
        ] {
            assert!(
                trusted(&format!("GET / HTTP/1.1\r\nHost: {host}")),
                "{host}"
            );
            assert!(
                trusted(&format!(
                    "POST / HTTP/1.1\r\nHost: {host}\r\nOrigin: http://{host}"
                )),
                "{host}"
            );
        }
    }
    #[test]
    fn rejects_untrusted_and_ambiguous_headers() {
        for headers in ["Host: evil.test", "Host: localhost.evil:3000", "Host: evil-localhost:3000", "Host: 127.0.0.1.evil:3000", "Host: [::1]evil:3000", "Host: localhost:3000\r\nOrigin: http://evil.test", "Host: localhost:3000\r\nOrigin: http://127.0.0.1:3000", "Host: localhost:3000\r\nOrigin: https://localhost:3000", "Host: localhost:3000\r\nOrigin: null", "Host: localhost:3000\r\nOrigin: http://localhost:3000/path", "Host: localhost:3000\r\nHost: localhost:3000", "Host: localhost:3000\r\nOrigin: http://localhost:3000\r\norigin: http://localhost:3000", "Host : localhost", "Host: localhost,evil", "Host: localhost:3000\r\n folded: value", "Origin: http://localhost"] {
            assert!(!trusted(&format!("GET / HTTP/1.1\r\n{headers}")), "{headers}");
        }
    }
}
