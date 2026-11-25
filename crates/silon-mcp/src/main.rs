//! `silon-mcp [--url URL]`: serves MCP on stdin and stdout and forwards each
//! tool call to the introspection API at URL (default `http://127.0.0.1:3737`).

use std::process::ExitCode;

use reqwest::Url;
use rmcp::ServiceExt;
use silon_mcp::{DEFAULT_URL, SilonMcp};

const USAGE: &str = "usage: silon-mcp [--url URL]

Serves MCP over stdio for a Silon app started with `--features introspect`.
  --url URL  introspection API of the app (default: http://127.0.0.1:3737)";

#[tokio::main]
async fn main() -> ExitCode {
    let url = match parse_args(std::env::args().skip(1)) {
        Ok(Some(url)) => url,
        Ok(None) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(err) => {
            eprintln!("silon-mcp: {err}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    // stdout carries the protocol, so everything else goes to stderr.
    match run(url).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("silon-mcp: {err}");
            ExitCode::FAILURE
        }
    }
}

async fn run(url: Url) -> Result<(), Box<dyn std::error::Error>> {
    let server = SilonMcp::new(url)?.serve(rmcp::transport::stdio()).await?;
    server.waiting().await?;
    Ok(())
}

/// Returns the URL to use, or `None` for `--help`.
fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Option<Url>, String> {
    let mut url = DEFAULT_URL.to_owned();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "--url" => url = args.next().ok_or("--url needs a value")?,
            other => return Err(format!("unexpected argument `{other}`")),
        }
    }
    Url::parse(&url)
        .map(Some)
        .map_err(|err| format!("invalid --url `{url}`: {err}"))
}
