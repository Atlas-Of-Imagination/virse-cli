use crate::{
    args::Submission,
    generation::{SubmissionContext, wait},
    print, workflow_state as state,
};
use clap::Subcommand;
use serde_json::json;
use uuid::Uuid;
use virse_api::{Error, Result, TaskQuery};
#[derive(Subcommand)]
pub enum TaskCommands {
    /// List persisted tasks in the selected canvas; pagination is offset-based.
    List {
        #[arg(long)]
        id: Option<Uuid>,
        #[arg(long)]
        element: Option<Uuid>,
        #[arg(long)]
        artifact_version: Option<Uuid>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        operation: Option<String>,
        #[arg(long, default_value_t = 0)]
        offset: u32,
        #[arg(long,default_value_t=50,value_parser=clap::value_parser!(u16).range(1..=100))]
        limit: u16,
    },
    /// Retry only server-eligible failed outputs as a new linked task.
    Retry {
        id: Uuid,
        #[command(flatten)]
        options: Submission,
    },
}
pub async fn run(command: &TaskCommands, context: &SubmissionContext<'_>) -> Result<u8> {
    match command {
        TaskCommands::List {
            id,
            element,
            artifact_version,
            status,
            operation,
            offset,
            limit,
        } => {
            let page = context
                .client
                .generation_tasks(
                    context.canvas,
                    &TaskQuery {
                        task_id: *id,
                        element_id: *element,
                        artifact_version_id: *artifact_version,
                        status: status.clone(),
                        operation: operation.clone(),
                        offset: *offset,
                        limit: *limit,
                    },
                )
                .await?;
            if page.tasks.len() == usize::from(*limit) {
                eprintln!(
                    "Full page received; continue with the same filters and --offset {}",
                    u64::from(*offset) + page.tasks.len() as u64
                );
            }
            print(&page, context.json)?;
        }
        TaskCommands::Retry { id, options } => {
            let user = state::user(context.client).await?;
            let key = state::key(&options.idempotency_key)?;
            let scope = json!([
                context.backend,
                user,
                context.canvas,
                "generation-retry",
                key
            ]);
            let request = json!({"task_id":id});
            state::freeze(context.store, &scope, &request, Some(request.clone()))?;
            eprintln!("Retry key: {key}");
            let task = context
                .client
                .retry_generation(context.canvas, *id, &key)
                .await
                .map_err(|e| {
                    state::uncertain(
                        e,
                        "Retry result unknown; repeat original task ID and key",
                        Some(&key),
                    )
                })?;
            if options.wait {
                let id = Uuid::parse_str(&task.task_id)
                    .map_err(|_| Error::new("INVALID_RESPONSE", "Invalid accepted task ID"))?;
                return wait(
                    context.client,
                    context.canvas,
                    id,
                    options.timeout,
                    context.json,
                    false,
                )
                .await;
            }
            print(&task, context.json)?;
        }
    }
    Ok(0)
}
