use anyhow::Context;
use clap::{Parser, Subcommand};
use http_body_util::{Empty, Full};
use hyper::{body::Bytes, server::conn::http1, service::service_fn, Request, Response};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::{convert::Infallible, sync::Arc};
use tokio::net::{TcpListener, TcpStream};
use tokio::time;

#[derive(Parser, Debug)]
struct Args {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[clap(about = "Runs a server")]
    Server {
        #[clap(short, long, default_value = "0.0.0.0:3000")]
        addr: SocketAddr,
    },
    #[clap(about = "Runs a client")]
    Client {
        #[clap(short, long, default_value = "127.0.0.1:3000")]
        authority: String,

        #[clap(short, long, default_value = "1")]
        rate: f64,

        #[clap(short, long, default_value = "10")]
        timeout: f64,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let Args { command } = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .with_env_var("DROPSY_LOG")
                .with_default_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    match command {
        Command::Client {
            authority,
            rate,
            timeout,
        } => {
            let timeout = time::Duration::from_secs_f64(timeout);
            let url = format!("http://{authority}/").parse::<hyper::Uri>()?;
            let authority = Arc::new(authority);
            let host = Arc::new(url.host().unwrap().to_string());
            let port = url.port_u16().unwrap_or(80);

            let mut interval = time::interval(time::Duration::from_secs_f64(1.0 / rate));
            loop {
                let now = interval.tick().await;
                let url = url.clone();
                let host = host.clone();
                let authority = authority.clone();
                tokio::spawn(async move {
                    match time::timeout(timeout, fetch(url, &host, port, &authority)).await {
                        Ok(Ok(status)) => {
                            tracing::info!(status, elapsed = ?now.elapsed(), "Fetch succeeded");
                        }
                        Ok(Err(error)) => {
                            tracing::error!(%error, "Fetch failed");
                        }
                        Err(_) => {
                            tracing::error!("Fetch timed out");
                        }
                    }
                });
            }
        }

        Command::Server { addr } => {
            let listener = TcpListener::bind(addr).await?;

            // We start a loop to continuously accept incoming connections
            loop {
                let io = match listener.accept().await {
                    Ok((io, _)) => TokioIo::new(io),
                    Err(error) => {
                        tracing::error!(%error, "Error accepting connection");
                        continue;
                    }
                };

                tokio::spawn(async move {
                    if let Err(error) = http1::Builder::new()
                        .serve_connection(
                            io,
                            service_fn(|_req: Request<_>| async move {
                                let data: &'static [u8] = include_bytes!("dogfluff.jpeg");
                                let rsp = Response::builder()
                                    .header("content-type", "image/jpeg")
                                    .body(Full::new(Bytes::from(data)))
                                    .unwrap();
                                Ok::<_, Infallible>(rsp)
                            }),
                        )
                        .await
                    {
                        tracing::debug!(%error, "Error serving connection");
                    }
                });
            }
        }
    }
}

async fn fetch(url: hyper::Uri, host: &str, port: u16, authority: &str) -> anyhow::Result<u16> {
    let io = TcpStream::connect((host, port))
        .await
        .with_context(|| format!("connecting to {host}:{port}"))?;

    let (mut sender, mut conn) = hyper::client::conn::http1::handshake(TokioIo::new(io)).await?;

    let req = Request::builder()
        .uri(url)
        .header(hyper::header::HOST, authority)
        .body(Empty::<Bytes>::new())
        .unwrap();

    let rsp = tokio::select! {
        res = sender.send_request(req) => res?,
        res = &mut conn => {
            res?;
            anyhow::bail!("connection closed before response");
        }
    };

    let status = rsp.status().as_u16();
    // We explicitly do NOT read the body here. We want to drop
    // the connection with unread data so we can cause the proxy
    // to deal with the situation.
    drop((rsp, conn));

    Ok(status)
}
