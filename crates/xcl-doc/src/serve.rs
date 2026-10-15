use anyhow::Result;
use std::path::{Path, PathBuf};
use tiny_http::{Server, Response, Header, StatusCode};

pub fn serve(root: PathBuf, port: u16) -> Result<()> {
    let addr = format!("127.0.0.1:{port}");
    let server = Server::http(&addr).map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("Serving on http://{addr}");
    println!("Root: {}", root.display());

    for request in server.incoming_requests() {
        let url = request.url();
        let rel = url.trim_start_matches('/').split('?').next().unwrap_or("");
        let path = if rel.is_empty() {
            root.join("index.html")
        } else {
            root.join(rel)
        };

        if let Some(response) = serve_file(&path) {
            let _ = request.respond(response);
        } else {
            let _ = request.respond(Response::empty(StatusCode(404)));
        }
    }
    Ok(())
}

fn serve_file(path: &Path) -> Option<Response<std::io::Cursor<Vec<u8>>>> {
    let body = std::fs::read(path).ok()?;
    let ctype = match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css")  => "text/css; charset=utf-8",
        Some("js")   => "application/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("svg")  => "image/svg+xml",
        _            => "text/plain; charset=utf-8",
    };
    Some(Response::from_data(body).with_header(
        Header::from_bytes(&b"Content-Type"[..], ctype.as_bytes()).ok()?
    ))
}
