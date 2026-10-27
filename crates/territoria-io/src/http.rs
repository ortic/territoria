//! HTTP GET for the online services (boundary downloads, geocoding,
//! routing). On the desktop requests block, which is fine on the background
//! threads these run on. The browser only answers requests asynchronously,
//! so there these services aren't available (yet).

use std::time::Duration;

/// Requests share connections.
pub struct Client {
    #[cfg(not(target_arch = "wasm32"))]
    agent: ureq::Agent,
}

#[cfg(not(target_arch = "wasm32"))]
impl Client {
    /// `connect` limits connecting, `total` the whole request.
    pub fn new(user_agent: &str, connect: Option<Duration>, total: Duration) -> Client {
        let agent = ureq::Agent::config_builder()
            .timeout_connect(connect)
            .timeout_global(Some(total))
            .user_agent(user_agent)
            .build()
            .new_agent();
        Client { agent }
    }

    /// The answer as text (up to 64 MB); `query` is appended to the URL,
    /// encoded.
    pub fn text(&self, url: &str, query: &[(&str, &str)]) -> Result<String, String> {
        const MAX_TEXT: u64 = 64 << 20;
        let mut req = self.agent.get(url);
        for (k, v) in query {
            req = req.query(*k, *v);
        }
        req.call()
            .and_then(|mut r| r.body_mut().with_config().limit(MAX_TEXT).read_to_string())
            .map_err(|e| e.to_string())
    }

    /// The answer's length (if the server says) and body.
    pub fn open(&self, url: &str) -> Result<(Option<u64>, impl std::io::Read + use<>), String> {
        let response = self.agent.get(url).call().map_err(|e| e.to_string())?;
        let total = response.body().content_length();
        Ok((total, response.into_body().into_reader()))
    }
}

#[cfg(target_arch = "wasm32")]
impl Client {
    pub fn new(_user_agent: &str, _connect: Option<Duration>, _total: Duration) -> Client {
        Client {}
    }

    pub fn text(&self, _url: &str, _query: &[(&str, &str)]) -> Result<String, String> {
        Err(UNAVAILABLE.into())
    }

    pub fn open(&self, _url: &str) -> Result<(Option<u64>, std::io::Empty), String> {
        Err(UNAVAILABLE.into())
    }
}

#[cfg(target_arch = "wasm32")]
const UNAVAILABLE: &str = "online services aren't available in the browser version yet";

/// Waits between requests, to be polite to public services. Nothing to wait
/// for in the browser, where requests fail at once.
pub fn pause(d: Duration) {
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::sleep(d);
    #[cfg(target_arch = "wasm32")]
    let _ = d;
}
