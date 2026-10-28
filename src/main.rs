mod args;
mod canvas_add;
mod download;
mod generation;
mod media_args;
mod resource;
mod resource_args;
mod resource_paths;
mod resource_tree;
mod resource_upload;
mod resource_watch;
mod share_args;
mod sharing;
mod storage;
mod tool_args;
mod tool_project;
mod tool_publish;
mod tools;

use args::*;
use clap::Parser;
use serde::Serialize;
use serde_json::json;
use std::io::{IsTerminal, Read};
use virse_api::{Client, Error, Result};

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            if error.exit_code() != 0 && std::env::args_os().any(|arg| arg == "--json") {
                eprintln!(
                    "{}",
                    json!({"error": Error::new("INVALID_INPUT", error.to_string())})
                );
                std::process::exit(2);
            }
            error.exit();
        }
    };
    let code = match run(&cli).await {
        Ok(code) => code,
        Err(error) => {
            if cli.json {
                eprintln!("{}", json!({"error":error}));
            } else {
                eprintln!("{error}");
                if let Some(details) = &error.details {
                    eprintln!(
                        "{}",
                        serde_json::to_string_pretty(details).unwrap_or_default()
                    );
                }
            }
            error_exit(&error)
        }
    };
    std::process::exit(i32::from(code));
}

fn error_exit(error: &Error) -> u8 {
    match error.code.as_str() {
        "INVALID_INPUT" => 2,
        "AUTH_REQUIRED" => 3,
        "no_share_permission"
        | "no_permission"
        | "not_org_member"
        | "external_sharing_disabled" => 4,
        "SUBMISSION_UNCERTAIN"
        | "RESOURCE_MUTATION_UNCERTAIN"
        | "FOLDER_CREATION_UNCERTAIN"
        | "CANVAS_PLACEMENT_UNCERTAIN"
        | "SHARE_MUTATION_UNCERTAIN"
        | "TOOL_MUTATION_UNCERTAIN"
        | "ASSET_WRITE_UNCERTAIN"
        | "TOOL_PUBLISH_UNCERTAIN" => 7,
        "WAIT_TIMEOUT" => 8,
        "INTERRUPTED" => 130,
        "TRANSPORT_ERROR" | "OBSERVATION_UNAVAILABLE" | "CHANGES_UNAVAILABLE" => 5,
        _ => match error.status {
            Some(401) => 3,
            Some(403) => 4,
            Some(429 | 500..=599) => 5,
            _ => 1,
        },
    }
}

fn print(value: &impl Serialize, compact: bool) -> Result<()> {
    let text = if compact {
        serde_json::to_string(value)
    } else {
        serde_json::to_string_pretty(value)
    }
    .map_err(|_| Error::new("OUTPUT_ERROR", "Cannot serialize result"))?;
    println!("{text}");
    Ok(())
}

fn required(value: Option<uuid::Uuid>, name: &str) -> Result<uuid::Uuid> {
    value.ok_or_else(|| {
        Error::new(
            "INVALID_INPUT",
            format!("Specify --{name} or config set {name} <id>"),
        )
    })
}

async fn run(cli: &Cli) -> Result<u8> {
    if let Command::Tool { command } = &cli.command
        && let Some(result) = tools::offline(command, cli.json).await
    {
        return result;
    }
    let backend = storage::backend()?;
    let store = storage::Store::open()?;
    if cli.profile.is_empty() {
        return Err(Error::new("INVALID_INPUT", "Profile must not be empty"));
    }
    if let Command::Config { command } = &cli.command {
        match command {
            Config::Get => print(&store.defaults(&cli.profile, &backend)?, cli.json)?,
            Config::Set { key, value } => {
                store.set(&cli.profile, &backend, key, *value)?;
                print(&store.defaults(&cli.profile, &backend)?, cli.json)?;
            }
        }
        return Ok(0);
    }
    if let Command::Auth { command } = &cli.command {
        match command {
            Auth::Login { token_stdin } => {
                let token = if *token_stdin {
                    let mut value = String::new();
                    std::io::stdin()
                        .take(16385)
                        .read_to_string(&mut value)
                        .map_err(storage::io_error)?;
                    value.trim_end_matches(['\r', '\n']).to_owned()
                } else {
                    if !std::io::stdin().is_terminal() {
                        return Err(Error::new(
                            "INVALID_INPUT",
                            "Use auth login --token-stdin in non-interactive mode",
                        ));
                    }
                    rpassword::prompt_password("API token: ").map_err(storage::io_error)?
                };
                if token.len() > 16384 {
                    return Err(Error::new("INVALID_INPUT", "Token is too long"));
                }
                let client = Client::new(&backend, &token)?;
                let identity = client.me().await?;
                store.save_token(&cli.profile, &backend, Some(&token))?;
                if std::env::var_os("VIRSE_API_TOKEN").is_some() {
                    eprintln!("VIRSE_API_TOKEN overrides the saved token until unset");
                }
                print(&identity, cli.json)?;
                return Ok(0);
            }
            Auth::Logout => {
                store.save_token(&cli.profile, &backend, None)?;
                if std::env::var_os("VIRSE_API_TOKEN").is_some() {
                    eprintln!(
                        "VIRSE_API_TOKEN is still set; logout only removes the saved credential"
                    );
                }
                print(&json!({"logged_out":true}), cli.json)?;
                return Ok(0);
            }
            Auth::Status => {}
        }
    }
    let token = store.token(&cli.profile, &backend)?;
    let client = Client::new(&backend, &token)?;
    let defaults = store.defaults(&cli.profile, &backend)?;
    let canvas = || required(cli.canvas.or(defaults.canvas), "canvas");
    let org = || required(cli.org.or(defaults.org), "org");
    match &cli.command {
        Command::Resource { command } => {
            return resource::run(command, &client, &store, &backend, org()?, cli.json).await;
        }
        Command::Auth { .. } => print(&client.me().await?, cli.json)?,
        Command::Tool { command } => {
            return tools::run(
                command,
                &client,
                &store,
                &backend,
                cli.org.or(defaults.org),
                cli.canvas.or(defaults.canvas),
                cli.json,
            )
            .await;
        }
        Command::Canvas { command } => match command {
            Canvas::List { limit, cursor } => print(
                &client.canvases(org()?, *limit, cursor.as_deref()).await?,
                cli.json,
            )?,
            Canvas::Get { id } => print(&client.canvas(*id).await?, cli.json)?,
            Canvas::Add(args) => {
                return canvas_add::run(
                    args,
                    &client,
                    &store,
                    &backend,
                    org()?,
                    canvas()?,
                    cli.json,
                )
                .await;
            }
        },
        Command::Generation { command } => {
            let canvas = canvas()?;
            let context = generation::SubmissionContext {
                client: &client,
                store: &store,
                backend: &backend,
                canvas,
                json: cli.json,
            };
            match command {
                Generation::Operations => print(&client.operations(canvas).await?, cli.json)?,
                Generation::Submit { file, options } => {
                    return context
                        .submit(generation::read_request(file).await?, options)
                        .await;
                }
                Generation::Image(args) => {
                    return context
                        .submit(
                            generation::media_request(
                                virse_api::MediaKind::Image,
                                &args.prompt,
                                &args.options,
                            )
                            .await?,
                            &args.options.submission,
                        )
                        .await;
                }
                Generation::Video(args) => {
                    return context
                        .submit(
                            generation::media_request(
                                virse_api::MediaKind::Video,
                                &args.prompt,
                                &args.options,
                            )
                            .await?,
                            &args.options.submission,
                        )
                        .await;
                }
                Generation::Model3d(args) => {
                    return context
                        .submit(
                            generation::media_request(
                                virse_api::MediaKind::Model3d,
                                args.prompt.as_deref().unwrap_or(""),
                                &args.options,
                            )
                            .await?,
                            &args.options.submission,
                        )
                        .await;
                }
                Generation::Get { id } => print(&client.task(canvas, *id).await?, cli.json)?,
                Generation::Cancel { id } => print(&client.cancel(canvas, *id).await?, cli.json)?,
                Generation::Watch { id, timeout } => {
                    return generation::wait(&client, canvas, *id, *timeout, cli.json, true).await;
                }
            }
        }
        Command::Asset { command } => match command {
            Asset::Get { id } => print(&client.asset(*id, org()?, canvas()?).await?, cli.json)?,
            Asset::Download { id, output } => {
                let size = download::save(&client, *id, org()?, canvas()?, output).await?;
                print(&json!({"asset_id":id,"path":output,"bytes":size}), cli.json)?;
            }
        },
        Command::Config { .. } => unreachable!(),
    }
    Ok(0)
}
