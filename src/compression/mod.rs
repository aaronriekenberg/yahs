use crate::config::PrecompressedEncoding;

/// Parse the `Accept-Encoding` header value and return an ordered list of
/// encodings the client accepts, sorted by q-value descending.
pub fn accepted_encodings(accept_encoding: &str) -> Vec<(PrecompressedEncoding, f32)> {
    let mut result: Vec<(PrecompressedEncoding, f32)> = Vec::new();

    for part in accept_encoding.split(',') {
        let part = part.trim();
        let (token, q) = if let Some((t, q_part)) = part.split_once(";q=") {
            let q_val: f32 = q_part.trim().parse().unwrap_or(1.0);
            (t.trim(), q_val)
        } else {
            (part, 1.0_f32)
        };

        let enc = match token {
            "zstd" => Some(PrecompressedEncoding::Zstd),
            "br" => Some(PrecompressedEncoding::Brotli),
            "gzip" => Some(PrecompressedEncoding::Gzip),
            _ => None,
        };

        if let Some(enc) = enc {
            result.push((enc, q));
        }
    }

    result.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    result
}

/// Choose the best precompressed encoding given:
/// - client's accepted encodings (from Accept-Encoding header),
/// - server's configured preference list.
///
/// Returns `None` if no acceptable precompressed variant should be served.
pub fn negotiate_encoding(
    accept_encoding: Option<&str>,
    server_encodings: &[PrecompressedEncoding],
) -> Option<PrecompressedEncoding> {
    let accept_str = accept_encoding?;
    let accepted = accepted_encodings(accept_str);

    // Prefer server order among accepted encodings.
    for &server_enc in server_encodings {
        for &(client_enc, q) in &accepted {
            if client_enc == server_enc && q > 0.0 {
                return Some(server_enc);
            }
        }
    }

    None
}
