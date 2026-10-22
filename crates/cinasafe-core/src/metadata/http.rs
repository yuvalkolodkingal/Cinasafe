//! Minimal HTTP abstraction so providers can be tested with recorded responses.

use std::io::Read;
use std::time::Duration;

use super::{Error, Result};

#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

pub trait HttpClient: Send + Sync {
    /// GET `url`, reading at most `max_bytes` of body (larger bodies are an error).
    fn get(&self, url: &str, headers: &[(&str, &str)], max_bytes: usize) -> Result<HttpResponse>;
}

/// Blocking reqwest client with rustls, a descriptive User-Agent, timeouts
/// and a bounded retry when the provider rate-limits us.
pub struct ReqwestClient {
    client: reqwest::blocking::Client,
}

impl ReqwestClient {
    pub fn new() -> Result<ReqwestClient> {
        let client = reqwest::blocking::Client::builder()
            .user_agent(concat!(
                "Cinasafe/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/yuvalkolodkingal/cinasafe)"
            ))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| Error::Network(e.to_string()))?;
        Ok(ReqwestClient { client })
    }
}

impl HttpClient for ReqwestClient {
    fn get(&self, url: &str, headers: &[(&str, &str)], max_bytes: usize) -> Result<HttpResponse> {
        let mut attempt = 0;
        loop {
            let mut req = self.client.get(url);
            for (k, v) in headers {
                req = req.header(*k, *v);
            }
            // reqwest errors can include the URL (and so the query); keep only the kind.
            let resp = req.send().map_err(|e| Error::Network(error_kind(&e)))?;
            let status = resp.status().as_u16();
            if status == 429 && attempt < 3 {
                let wait = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(1 << attempt)
                    .min(10);
                std::thread::sleep(Duration::from_secs(wait));
                attempt += 1;
                continue;
            }
            let content_type = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).map(str::to_owned);
            let mut body = Vec::new();
            resp.take(max_bytes as u64 + 1).read_to_end(&mut body).map_err(|e| Error::Network(e.kind().to_string()))?;
            if body.len() > max_bytes {
                return Err(Error::Decode("response too large".into()));
            }
            return Ok(HttpResponse { status, content_type, body });
        }
    }
}

fn error_kind(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "timed out".into()
    } else if e.is_connect() {
        "could not connect".into()
    } else {
        "request failed".into()
    }
}

/// Maps non-success statuses to errors and parses JSON.
pub(crate) fn json<T: serde::de::DeserializeOwned>(resp: HttpResponse) -> Result<T> {
    match resp.status {
        200..=299 => serde_json::from_slice(&resp.body).map_err(|e| Error::Decode(e.to_string())),
        401 | 403 => Err(Error::Unauthorized),
        404 => Err(Error::NotFound),
        429 => Err(Error::RateLimited),
        s => Err(Error::Status(s)),
    }
}

/// Percent-encodes a query-string value (RFC 3986 unreserved characters pass).
pub(crate) fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod fake {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use super::*;

    /// Serves fixture files for URLs containing a given substring, and records requests.
    pub struct FakeHttp {
        routes: Vec<(String, u16, Vec<u8>)>,
        pub requests: Mutex<Vec<(String, BTreeMap<String, String>)>>,
    }

    impl FakeHttp {
        pub fn new() -> FakeHttp {
            FakeHttp { routes: Vec::new(), requests: Mutex::default() }
        }

        pub fn route(mut self, contains: &str, fixture: &str) -> FakeHttp {
            let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/").to_owned() + fixture;
            let body = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
            self.routes.push((contains.to_owned(), 200, body));
            self
        }

        pub fn status(mut self, contains: &str, status: u16, body: &[u8]) -> FakeHttp {
            self.routes.push((contains.to_owned(), status, body.to_vec()));
            self
        }

        pub fn urls(&self) -> Vec<String> {
            self.requests.lock().unwrap().iter().map(|(u, _)| u.clone()).collect()
        }
    }

    impl HttpClient for FakeHttp {
        fn get(&self, url: &str, headers: &[(&str, &str)], _max: usize) -> Result<HttpResponse> {
            let h = headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
            self.requests.lock().unwrap().push((url.to_owned(), h));
            // Most specific (longest) matching route wins.
            let hit = self.routes.iter().filter(|(c, _, _)| url.contains(c.as_str())).max_by_key(|(c, _, _)| c.len());
            match hit {
                Some((_, status, body)) => Ok(HttpResponse {
                    status: *status,
                    content_type: Some("application/json".into()),
                    body: body.clone(),
                }),
                None => Ok(HttpResponse { status: 404, content_type: None, body: Vec::new() }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn encodes_query() {
        assert_eq!(super::encode("Fauda S01"), "Fauda%20S01");
        assert_eq!(super::encode("פאודה"), "%D7%A4%D7%90%D7%95%D7%93%D7%94");
        assert_eq!(super::encode("a&b=c"), "a%26b%3Dc");
    }
}
