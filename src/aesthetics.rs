use clap::{Subcommand, ValueEnum};
use futures_util::StreamExt;
use serde_json::{Value, json};
use uuid::Uuid;
use virse_api::{AestheticOperation as Op, Client, Error, Result, ToolRunAction};

#[derive(Clone, Copy, ValueEnum)]
pub enum Action {
    Create,
    Details,
    SaveResource,
    References,
    TestCanvas,
    DeleteTestCanvas,
    StartDraft,
    Draft,
    Context,
    Revise,
    SaveDraft,
    Restore,
    Snapshot,
}
impl Action {
    fn operation(self) -> Op {
        match self {
            Self::Create => Op::Create,
            Self::Details => Op::Details,
            Self::SaveResource => Op::SaveResource,
            Self::References => Op::References,
            Self::TestCanvas => Op::TestCanvas,
            Self::DeleteTestCanvas => Op::DeleteTestCanvas,
            Self::StartDraft => Op::StartDraft,
            Self::Draft => Op::Draft,
            Self::Context => Op::Context,
            Self::Revise => Op::Revise,
            Self::SaveDraft => Op::SaveDraft,
            Self::Restore => Op::Restore,
            Self::Snapshot => Op::Snapshot,
        }
    }
}
#[derive(Subcommand)]
pub enum Aesthetic {
    /// Call an existing authoring operation; JSON is the HTTP body, not an import package.
    Call {
        #[arg(value_enum)]
        action: Action,
        #[arg(long)]
        id: Option<Uuid>,
        #[arg(long, default_value = "{}")]
        data: String,
    },
    /// Start existing canvas training. Reuse the key only with exactly the same request.
    Train {
        #[arg(long)]
        data: String,
        #[arg(long)]
        idempotency_key: String,
    },
    Get {
        id: Uuid,
    },
    Watch {
        id: Uuid,
        #[arg(long, default_value_t = 600)]
        timeout: u64,
    },
    Cancel {
        id: Uuid,
    },
    Resume {
        id: Uuid,
    },
}
fn body(raw: &str) -> Result<Value> {
    let value: Value = serde_json::from_str(raw)
        .map_err(|_| Error::new("INVALID_INPUT", "Expected a JSON object"))?;
    if !value.is_object() {
        return Err(Error::new("INVALID_INPUT", "Expected a JSON object"));
    }
    Ok(value)
}
pub async fn run(
    command: &Aesthetic,
    client: &Client,
    org: Option<Uuid>,
    canvas: Option<Uuid>,
    json_output: bool,
) -> Result<u8> {
    let value = match command {
        Aesthetic::Call { action, id, data } => {
            client
                .aesthetic(action.operation(), *id, body(data)?)
                .await?
        }
        Aesthetic::Train {
            data,
            idempotency_key,
        } => {
            client
                .start_aesthetic_training(
                    crate::required(canvas, "canvas")?,
                    crate::required(org, "org")?,
                    body(data)?,
                    idempotency_key,
                )
                .await?
        }
        Aesthetic::Get { id } => json!(
            client
                .aesthetic_training(
                    crate::required(canvas, "canvas")?,
                    crate::required(org, "org")?,
                    *id
                )
                .await?
        ),
        Aesthetic::Cancel { id } | Aesthetic::Resume { id } => json!(
            client
                .aesthetic_training_action(
                    crate::required(canvas, "canvas")?,
                    crate::required(org, "org")?,
                    *id,
                    if matches!(command, Aesthetic::Cancel { .. }) {
                        ToolRunAction::Cancel
                    } else {
                        ToolRunAction::Resume
                    }
                )
                .await?
        ),
        Aesthetic::Watch { id, timeout } => {
            let stream = client.watch_aesthetic_training(
                crate::required(canvas, "canvas")?,
                crate::required(org, "org")?,
                *id,
            );
            futures_util::pin_mut!(stream);
            let deadline = tokio::time::sleep(std::time::Duration::from_secs(*timeout));
            tokio::pin!(deadline);
            loop {
                tokio::select! {
                    _=tokio::signal::ctrl_c()=>return Ok(130),
                    _=&mut deadline=>return Ok(8),
                    next=stream.next()=>{let Some(value)=next else{return Ok(0)};let run=value?;crate::print(&run,json_output)?;
                        if run.stop_waiting(){return Ok(match run.status.as_str(){"succeeded"=>0,"needs_attention"=>7,_=>6});}}
                }
            }
        }
    };
    crate::print(&value, json_output)?;
    Ok(0)
}
