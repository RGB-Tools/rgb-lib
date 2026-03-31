use super::*;

/// Maximum size, in bytes, of a reject list line: the allow prefix (`!`), the opout
/// (`{opid}/{ty}/{no}`, a hex opid and two decimal `u16`s) and a CRLF line ending
const MAX_REJECT_LIST_LINE_SIZE: u64 = 1 + 64 + 1 + 5 + 1 + 5 + 2;
/// Maximum size, in bytes, of a reject list served at a URL (see [`MAX_REJECT_LIST_ENTRIES`])
const MAX_REJECT_LIST_SIZE: u64 = MAX_REJECT_LIST_ENTRIES * MAX_REJECT_LIST_LINE_SIZE;

pub struct RejectListClient {
    client: RestClient,
    base_url: String,
}

impl RejectListClient {
    /// A client for the reject list served at `base_url`.
    ///
    /// The URL is taken from the asset's genesis, so it's untrusted: only `http` and `https` URLs
    /// are accepted and redirects are not followed, to not let it make the client request other
    /// URLs (e.g. on the local network) than the one it states.
    pub(crate) fn new(base_url: &str) -> Result<Self, Error> {
        let url = reqwest::Url::parse(base_url).map_err(Self::req_err)?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(Self::req_err(format!(
                "unsupported reject list URL scheme: {}",
                url.scheme()
            )));
        }
        let client = RestClient::builder()
            .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT))
            .timeout(Duration::from_secs(READ_WRITE_TIMEOUT))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            client,
            base_url: base_url.to_string(),
        })
    }

    fn req_err(e: impl std::fmt::Display) -> Error {
        Error::RejectListService {
            details: e.to_string(),
        }
    }

    pub(crate) fn get_reject_list(&self) -> Result<String, Error> {
        let resp = self
            .client
            .get(&self.base_url)
            .send()
            .map_err(Self::req_err)?;
        // an error page (or a redirect, not followed) is not a reject list: taking it as one would
        // make every opout look allowed
        let status = resp.status();
        if !status.is_success() {
            return Err(Self::req_err(format!(
                "reject list request failed: HTTP {status}"
            )));
        }
        // read one byte past the bound to tell a body of exactly the maximum size from a longer one
        let mut body = vec![];
        resp.take(MAX_REJECT_LIST_SIZE + 1)
            .read_to_end(&mut body)
            .map_err(Self::req_err)?;
        if body.len() as u64 > MAX_REJECT_LIST_SIZE {
            return Err(Self::req_err(format!(
                "reject list too large: more than {MAX_REJECT_LIST_SIZE} bytes"
            )));
        }
        Ok(String::from_utf8_lossy(&body).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_reject_list_error() {
        // network error
        let client = RejectListClient::new("http://127.0.0.1:1").unwrap();
        let result = client.get_reject_list().unwrap_err();
        assert_matches!(result, Error::RejectListService { .. });

        // content-length mismatch (truncated body)
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/")
            .with_status(200)
            .with_header("content-length", "100")
            .with_body(&[0xFFu8, 0xFEu8][..])
            .create();
        let client = RejectListClient::new(&server.url()).unwrap();
        let result = client.get_reject_list().unwrap_err();
        assert_matches!(result, Error::RejectListService { .. });
        mock.assert();

        // error pages and redirects (not followed) are refused, not taken as an empty list
        for status in [301, 302, 404, 500] {
            let mut server = mockito::Server::new();
            let mock = server
                .mock("GET", "/")
                .with_status(status)
                .with_header("location", "http://127.0.0.1:1/elsewhere")
                .with_body("not a reject list")
                .create();
            let client = RejectListClient::new(&server.url()).unwrap();
            let result = client.get_reject_list().unwrap_err();
            assert_matches!(result, Error::RejectListService { ref details } if details.contains(&status.to_string()));
            mock.assert();
        }

        // URLs other than http(s) ones are refused
        for url in ["file:///etc/passwd", "ftp://127.0.0.1/list", "not a url"] {
            assert_matches!(
                RejectListClient::new(url).err(),
                Some(Error::RejectListService { .. })
            );
        }
    }

    #[test]
    fn reject_list_line_size() {
        // the longest line is an allowed opout with the largest assignment type and output number
        let opout = Opout::new(
            OpId::from([0xff; 32]),
            AssignmentType::with(u16::MAX),
            u16::MAX,
        );
        let line = format!("!{opout}\r\n");
        assert_eq!(line.len() as u64, MAX_REJECT_LIST_LINE_SIZE);
        assert_eq!(Opout::from_str(line[1..].trim_end()).unwrap(), opout);
    }

    #[test]
    fn get_reject_list_size() {
        // a list of the maximum size is read
        let mut server = mockito::Server::new();
        let body = "a".repeat(MAX_REJECT_LIST_SIZE as usize);
        let mock = server.mock("GET", "/").with_body(&body).create();
        let client = RejectListClient::new(&server.url()).unwrap();
        assert_eq!(client.get_reject_list().unwrap(), body);
        mock.assert();

        // a longer one is refused, with or without a content length
        for chunked in [false, true] {
            let mut server = mockito::Server::new();
            let body = "a".repeat(MAX_REJECT_LIST_SIZE as usize + 1);
            let mock = if chunked {
                server
                    .mock("GET", "/")
                    .with_chunked_body(move |w| w.write_all(body.as_bytes()))
            } else {
                server.mock("GET", "/").with_body(&body)
            }
            .create();
            let client = RejectListClient::new(&server.url()).unwrap();
            let result = client.get_reject_list().unwrap_err();
            assert_matches!(result, Error::RejectListService { ref details } if details.contains("reject list too large"));
            mock.assert();
        }
    }
}
