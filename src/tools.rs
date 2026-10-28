use crate::{
    print, required,
    storage::{self, PlacementIdentity, Store},
    tool_args::{Tool, ToolDraft, ToolRunArgs, ToolRuns},
};
use futures_util::StreamExt;
use serde_json::{Map, Value, json};
use std::{io::Read, time::Duration};
use uuid::Uuid;
use virse_api::{
    Client, ElementLayout, Error, Point, Result, Size, ToolElement, ToolIdentity, ToolListOptions,
    ToolMutation, ToolRun, ToolRunAction, ToolRunRequest, ToolTarget,
};

/// Must be dispatched before resolving backend, profiles or API credentials.
pub async fn offline(command: &Tool, json: bool) -> Option<Result<u8>> {
    let result = match command {
        Tool::Init { directory, name } => crate::tool_project::init(directory, name.as_deref()),
        Tool::Validate {
            directory,
            remote: false,
        } => crate::tool_project::validate(directory),
        _ => return None,
    };
    Some(result.and_then(|value| {
        print(&value, json)?;
        Ok(0)
    }))
}

pub async fn run(
    command: &Tool,
    client: &Client,
    store: &Store,
    backend: &str,
    org: Option<Uuid>,
    canvas: Option<Uuid>,
    json: bool,
) -> Result<u8> {
    match command {
        Tool::Validate {
            directory,
            remote: true,
        } => {
            let package = crate::tool_project::pack(directory)?;
            let report = client
                .tool_validate_package(required(org, "org")?, package.bytes)
                .await?;
            let code = if report.get("valid").and_then(Value::as_bool) == Some(true) {
                0
            } else {
                2
            };
            print(&report, json)?;
            return Ok(code);
        }
        Tool::Init { .. } | Tool::Validate { remote: false, .. } => {
            return offline(command, json)
                .await
                .ok_or_else(|| Error::new("INVALID_INPUT", "Not an offline tool command"))?;
        }
        Tool::Build { directory, resume } => {
            let user = account(client).await?;
            let identity =
                crate::tool_publish::AuthoringIdentity::new(backend, user, required(org, "org")?)?;
            print(
                &crate::tool_publish::author(
                    client,
                    directory,
                    &store.root,
                    &identity,
                    None,
                    *resume,
                    true,
                )
                .await?,
                json,
            )?;
        }
        Tool::Publish {
            directory,
            artifact_id,
            tool_id,
            scope,
            resume,
        } => {
            let user = account(client).await?;
            let identity =
                crate::tool_publish::AuthoringIdentity::new(backend, user, required(org, "org")?)?;
            let result = if let Some(artifact) = artifact_id {
                crate::tool_publish::publish_artifact(
                    client,
                    &identity,
                    required(*tool_id, "tool-id")?,
                    *artifact,
                    scope.map(|s| s.as_str()),
                )
                .await?
            } else {
                let directory = directory
                    .as_ref()
                    .ok_or_else(|| Error::new("INVALID_INPUT", "directory is required"))?;
                crate::tool_publish::author(
                    client,
                    directory,
                    &store.root,
                    &identity,
                    scope.map(|s| s.as_str()),
                    *resume,
                    false,
                )
                .await?
            };
            print(&result, json)?;
        }
        Tool::Pull {
            tool_id,
            directory,
            version,
        } => {
            let user = account(client).await?;
            let identity =
                crate::tool_publish::AuthoringIdentity::new(backend, user, required(org, "org")?)?;
            let release_id = if let Some(selector) = version {
                let versions = client.tool_versions(&tool_id.to_string(), org).await?;
                let matches: Vec<_> = versions
                    .versions
                    .iter()
                    .filter(|v| v.id == *selector || v.version == *selector)
                    .collect();
                if matches.len() != 1 {
                    return Err(Error::new(
                        "INVALID_INPUT",
                        "Version must identify exactly one published release from tool versions",
                    ));
                }
                // Legacy projection uses the artifact ID as id; pass it only when it is a real release.
                if matches[0].id == matches[0].artifact_version_id {
                    return Err(Error::new(
                        "INVALID_INPUT",
                        "This legacy version has no release ID; pull the current source without --version",
                    ));
                }
                Some(
                    Uuid::parse_str(&matches[0].id)
                        .map_err(|_| Error::new("INVALID_RESPONSE", "Invalid release ID"))?,
                )
            } else {
                None
            };
            print(
                &crate::tool_publish::pull(
                    client,
                    *tool_id,
                    directory,
                    release_id,
                    &store.root,
                    &identity,
                )
                .await?,
                json,
            )?;
        }
        Tool::List {
            view,
            query,
            category,
            limit,
            offset,
            locale,
        } => {
            print(
                &client
                    .tools(
                        view.api(),
                        &ToolListOptions {
                            organization_id: org,
                            query: query.clone(),
                            category_id: category.clone(),
                            limit: *limit,
                            offset: *offset,
                            locale: locale.clone(),
                        },
                    )
                    .await?,
                json,
            )?;
        }
        Tool::Get { id, locale } => print(&client.tool(id, org, locale).await?, json)?,
        Tool::Versions { id } => print(&client.tool_versions(id, org).await?, json)?,
        Tool::Draft {
            command:
                ToolDraft::Abandon {
                    tool_id,
                    revision_id,
                },
        } => {
            let result = client.abandon_tool_draft(tool_id, revision_id, org).await
                .map_err(|error| {
                    let mut error = mutation_error(error);
                    if error.code == "TOOL_MUTATION_UNCERTAIN" {
                        error.message = "Draft abandonment outcome is unknown; check draft state before retrying or publishing".into();
                        error.details = Some(Box::new(json!({"skillId":tool_id,"revisionId":revision_id,"organizationId":org})));
                    }
                    error
                })?;
            print(&result, json)?;
        }
        Tool::Scope { id, scope } => print(
            &client
                .set_tool_scope(id, org, scope.api())
                .await
                .map_err(mutation_error)?,
            json,
        )?,
        Tool::Schema { id, version } => {
            let tool = ToolIdentity {
                skill_id: id.clone(),
                version_id: version.version_id.clone(),
                version: version.version.clone(),
            };
            print(
                &client
                    .tool_schema(
                        required(canvas, "canvas")?,
                        required(org, "org")?,
                        &tool,
                        &version.locale,
                    )
                    .await?,
                json,
            )?;
        }
        Tool::Run(args) => {
            return RunContext {
                client,
                store,
                backend,
                org: required(org, "org")?,
                canvas: required(canvas, "canvas")?,
                json,
            }
            .submit(args)
            .await;
        }
        Tool::Runs { command } => {
            let canvas = required(canvas, "canvas")?;
            let org = required(org, "org")?;
            let (id, action) = match command {
                ToolRuns::Watch { id, timeout } => {
                    return wait(client, canvas, org, *id, *timeout, json, true).await;
                }
                ToolRuns::Get { id } => {
                    print(&client.tool_run(canvas, org, *id).await?, json)?;
                    return Ok(0);
                }
                ToolRuns::Cancel { id } => (*id, ToolRunAction::Cancel),
                ToolRuns::Interrupt { id } => (*id, ToolRunAction::Interrupt),
                ToolRuns::Resume { id } => (*id, ToolRunAction::Resume),
            };
            let run = client.tool_run_action(canvas, org, id, action).await.map_err(|mut error| {
                if uncertain(&error) { error.code = "TOOL_MUTATION_UNCERTAIN".into(); error.message = "Run control outcome is unknown; use tool runs get before another action".into(); error.task_id = Some(id.to_string().into_boxed_str()); error.retryable = false; }
                error
            })?;
            print(&run, json)?;
        }
        _ => {
            let (id, action) = match command {
                Tool::Add { id } => (id, ToolMutation::Add),
                Tool::Remove { id } => (id, ToolMutation::Remove),
                Tool::Enable { id } => (id, ToolMutation::Enable),
                Tool::Disable { id } => (id, ToolMutation::Disable),
                Tool::Star { id } => (id, ToolMutation::Star),
                Tool::Unstar { id } => (id, ToolMutation::Unstar),
                Tool::Delete { id } => (id, ToolMutation::Delete),
                _ => unreachable!(),
            };
            print(
                &client
                    .mutate_tool(id, org, action)
                    .await
                    .map_err(mutation_error)?,
                json,
            )?;
        }
    }
    Ok(0)
}
async fn account(client: &Client) -> Result<Uuid> {
    let identity = client.me().await?;
    identity["id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| Error::new("INVALID_RESPONSE", "Identity has no valid user ID"))
}
fn uncertain(error: &Error) -> bool {
    error.code == "TRANSPORT_ERROR"
        || error.code == "INVALID_RESPONSE"
        || error.status.is_some_and(|s| s >= 500)
}
fn mutation_error(mut error: Error) -> Error {
    if uncertain(&error) {
        error.code = "TOOL_MUTATION_UNCERTAIN".into();
        error.message =
            "Tool mutation outcome is unknown; read tool get/list before another mutation".into();
        error.retryable = false;
    }
    error
}
struct RunContext<'a> {
    client: &'a Client,
    store: &'a Store,
    backend: &'a str,
    org: Uuid,
    canvas: Uuid,
    json: bool,
}
impl RunContext<'_> {
    async fn submit(&self, args: &ToolRunArgs) -> Result<u8> {
        let inputs = if let Some(file) = &args.inputs {
            read_inputs(file)?
        } else {
            Map::new()
        };
        let instruction = args.instruction.clone().unwrap_or_default();
        let selector = ToolIdentity {
            skill_id: args.id.clone(),
            version_id: args.version.version_id.clone(),
            version: args.version.version.clone(),
        };
        selector.validate()?;
        let layout = ElementLayout {
            position: (args.x.is_some() || args.y.is_some()).then(|| Point {
                x: args.x.unwrap_or(0.0),
                y: args.y.unwrap_or(0.0),
            }),
            size: (args.width.is_some() || args.height.is_some()).then(|| Size {
                width: args.width.unwrap_or(820.0),
                height: args.height.unwrap_or(400.0),
            }),
        };
        layout.validate()?;
        let key = args
            .submission
            .idempotency_key
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        if key.trim().is_empty()
            || key.trim() != key
            || key.len() > 200
            || !key.is_ascii()
            || key.chars().any(char::is_control)
        {
            return Err(Error::new("INVALID_INPUT", "Invalid idempotency key"));
        }
        if instruction.chars().count() > 100_000 {
            return Err(Error::new(
                "INVALID_INPUT",
                "Instruction exceeds 100000 characters",
            ));
        }
        let user = account(self.client).await?;
        let identity = PlacementIdentity {
            backend: self.backend,
            user,
            org: self.org,
            canvas: self.canvas,
            key: &key,
        };
        let intent = json!({"tool":selector,"inputs":inputs,"instruction":instruction,"locale":args.version.locale,"target_element":args.target_element,"layout":layout,"name":args.name});
        let body = match self.store.tool_run_command(&identity, &intent, None)? {
            Some(frozen) => frozen,
            None => {
                let schema = self
                    .client
                    .tool_schema(self.canvas, self.org, &selector, &args.version.locale)
                    .await?;
                let tool = schema.pinned_identity()?;
                let body = ToolRunRequest {
                    org_id: self.org,
                    target: ToolTarget {
                        element_id: args.target_element,
                        create_element: args.target_element.is_none().then(|| ToolElement {
                            kind: "skill".into(),
                            skill_id: tool.skill_id.clone(),
                            version_id: tool.version_id.clone(),
                            name: args.name.clone(),
                            layout,
                        }),
                    },
                    tool,
                    inputs,
                    instruction,
                    locale: args.version.locale.clone(),
                    options: Map::new(),
                    aesthetic_sources: Vec::new(),
                };
                body.validate()?;
                // A concurrent process may already have frozen this intent; always use the winner.
                self.store
                    .tool_run_command(&identity, &intent, Some(&body))?
                    .ok_or_else(|| Error::new("CONFIG_ERROR", "Tool run record was not saved"))?
            }
        };
        eprintln!("Tool run submission key: {key}");
        let run = self.submit_frozen(&body, &key, user).await?;
        if !args.submission.wait || run.stop_waiting() {
            print(&run, self.json)?;
            return Ok(if run.stop_waiting() {
                run_exit(&run)
            } else {
                0
            });
        }
        eprintln!("Accepted run: {} (canvas {})", run.run_id, self.canvas);
        wait(
            self.client,
            self.canvas,
            self.org,
            run.run_id,
            args.submission.timeout,
            self.json,
            false,
        )
        .await
    }
    async fn submit_frozen(&self, body: &ToolRunRequest, key: &str, user: Uuid) -> Result<ToolRun> {
        for attempt in 0..4 {
            match self
                .client
                .submit_tool_run_as(self.canvas, body, key, user)
                .await
            {
                Ok(run) => return Ok(run),
                Err(error)
                    if matches!(
                        error.code.as_str(),
                        "CANVAS_HTTP_COMMANDS_NOT_ENABLED" | "GENERATION_ADMISSION_PAUSED"
                    ) =>
                {
                    return Err(error);
                }
                Err(error) if uncertain(&error) || error.retryable => {
                    if attempt == 3 {
                        return Err(Error {
                            retryable: true,
                            idempotency_key: Some(key.into()),
                            ..Error::new(
                                "SUBMISSION_UNCERTAIN",
                                format!(
                                    "Run acceptance unknown. Repeat this command with --idempotency-key {key}; preserve the local recovery record"
                                ),
                            )
                        });
                    }
                    tokio::time::sleep(Duration::from_secs(1 << attempt)).await;
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!()
    }
}
fn read_inputs(file: &str) -> Result<Map<String, Value>> {
    const MAX: u64 = 10 * 1024 * 1024;
    let reader: Box<dyn Read> = if file == "-" {
        Box::new(std::io::stdin())
    } else {
        Box::new(std::fs::File::open(file).map_err(storage::io_error)?)
    };
    let mut bytes = Vec::new();
    reader
        .take(MAX + 1)
        .read_to_end(&mut bytes)
        .map_err(storage::io_error)?;
    if bytes.len() as u64 > MAX {
        return Err(Error::new("INVALID_INPUT", "Tool inputs exceed 10 MiB"));
    }
    serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|v| v.as_object().cloned())
        .ok_or_else(|| Error::new("INVALID_INPUT", "Tool inputs must be a JSON object"))
}
fn run_exit(run: &ToolRun) -> u8 {
    match run.status.as_str() {
        "succeeded" => 0,
        "failed" | "cancelled" => 6,
        "needs_attention" => 7,
        _ => 1,
    }
}
pub async fn wait(
    client: &Client,
    canvas: Uuid,
    org: Uuid,
    id: Uuid,
    timeout: u64,
    json: bool,
    events: bool,
) -> Result<u8> {
    let observe = async {
        let stream = client.watch_tool_run(canvas, org, id);
        futures_util::pin_mut!(stream);
        while let Some(result) = stream.next().await {
            let run = result?;
            if events {
                if json {
                    print(&json!({"type":"run_snapshot","run":run}), true)?;
                } else {
                    print(&run, false)?;
                }
            } else if run.stop_waiting() {
                print(&run, json)?;
            } else {
                eprintln!("Run {}: {}", run.run_id, run.status);
            }
            if run.stop_waiting() {
                return Ok(run_exit(&run));
            }
        }
        Err(Error::new(
            "OBSERVATION_UNAVAILABLE",
            "Observation ended without a terminal or needs_attention snapshot",
        ))
    };
    let result = tokio::select! {
        result = observe => result,
        _ = tokio::time::sleep(Duration::from_secs(timeout)) => Err(Error::new("WAIT_TIMEOUT", format!("Stopped waiting; run {id} on canvas {canvas} continues. Use tool runs watch to reconnect"))),
        _ = tokio::signal::ctrl_c() => Err(Error::new("INTERRUPTED", format!("Stopped waiting; run {id} on canvas {canvas} continues. Use tool runs watch to reconnect"))),
    };
    result.map_err(|mut error| {
        error.task_id = Some(id.to_string().into_boxed_str());
        error.details = Some(Box::new(
            json!({"run_id":id,"canvas_id":canvas,"org_id":org}),
        ));
        error
    })
}
