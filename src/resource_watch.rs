use clap::Args;
use futures_util::StreamExt;
use std::{io::Write, time::Duration};
use uuid::Uuid;
use virse_api::{ChangeKind, ChangesWatchEvent, Client, Error, Result};

#[derive(Args)]
pub struct WatchArgs {
    /// Resume from a previously delivered changes/snapshot cursor. No local cursor is stored.
    #[arg(long)]
    pub cursor: Option<String>,
    /// Stop successfully after this many seconds (including connection/catch-up time).
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..=31_536_000))]
    pub timeout: Option<u64>,
}

pub async fn run(options: &WatchArgs, client: &Client, org: Uuid, json_output: bool) -> Result<u8> {
    let stream = client.watch_changes(org, options.cursor.as_deref());
    futures_util::pin_mut!(stream);
    let deadline = async {
        match options.timeout {
            Some(seconds) => tokio::time::sleep(Duration::from_secs(seconds)).await,
            None => std::future::pending::<()>().await,
        }
    };
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => return Ok(0),
            signal = tokio::signal::ctrl_c() => {
                signal.map_err(|_| Error::new("SIGNAL_ERROR", "Cannot listen for cancellation"))?;
                return Ok(130);
            }
            event = stream.next() => match event {
                Some(event) => write_event(&mut std::io::stdout().lock(), &event?, json_output)?,
                None => return Ok(0),
            }
        }
    }
}

fn write_event(
    output: &mut impl Write,
    event: &ChangesWatchEvent,
    json_output: bool,
) -> Result<()> {
    let failure = |_| Error::new("OUTPUT_ERROR", "Cannot write changes output");
    if json_output {
        // One complete page per line keeps events and their checkpoint together.
        serde_json::to_writer(&mut *output, event)
            .map_err(|_| Error::new("OUTPUT_ERROR", "Cannot serialize changes output"))?;
        writeln!(output).map_err(failure)?;
    } else {
        match event {
            ChangesWatchEvent::Page(page) => {
                if page.bootstrap {
                    writeln!(
                        output,
                        "Baseline cursor: {} (changes from now; no initial resource listing)",
                        page.next_cursor
                    )
                    .map_err(failure)?;
                }
                for item in &page.items {
                    if item.kind == ChangeKind::ScopeInvalidated {
                        writeln!(output, "scope_invalidated: workspace visibility changed; refresh your resource listing").map_err(failure)?;
                    } else {
                        let line = serde_json::to_string(item)
                            .map_err(|_| Error::new("OUTPUT_ERROR", "Cannot serialize change"))?;
                        writeln!(output, "{line}").map_err(failure)?;
                    }
                }
                if !page.bootstrap {
                    writeln!(
                        output,
                        "Cursor: {}{}",
                        page.next_cursor,
                        if page.has_more { " (catching up)" } else { "" }
                    )
                    .map_err(failure)?;
                }
            }
            ChangesWatchEvent::Notice {
                notice,
                cursor,
                attempt,
            } => {
                writeln!(
                    output,
                    "{notice} (attempt {attempt}); cursor: {}",
                    cursor.as_deref().unwrap_or("none")
                )
                .map_err(failure)?;
            }
        }
    }
    output.flush().map_err(failure)
}
