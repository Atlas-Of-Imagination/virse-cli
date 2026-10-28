use crate::{print, workflow_state};
use clap::Subcommand;
use serde_json::json;
use uuid::Uuid;
use virse_api::{Client, Error, Result};
#[derive(Subcommand)]
pub enum Discussion {
    List {
        #[arg(long,default_value="all",value_parser=["all","open","resolved"])]
        status: String,
    },
    Get {
        id: Uuid,
    },
    Create {
        #[arg(long)]
        content: String,
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        x: f64,
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        y: f64,
        #[arg(long, conflicts_with = "group")]
        element: Option<Uuid>,
        #[arg(long)]
        group: Option<Uuid>,
    },
    Reply {
        id: Uuid,
        #[arg(long)]
        content: String,
    },
    Edit {
        id: Uuid,
        comment: Uuid,
        #[arg(long)]
        content: String,
    },
    /// Delete a reply, or the entire discussion when --comment is omitted.
    Delete {
        id: Uuid,
        #[arg(long)]
        comment: Option<Uuid>,
    },
    Resolve {
        id: Uuid,
        #[arg(long)]
        reopen: bool,
    },
    Read {
        id: Uuid,
    },
    Move {
        id: Uuid,
        #[arg(long, allow_hyphen_values = true)]
        x: f64,
        #[arg(long, allow_hyphen_values = true)]
        y: f64,
    },
    Anchor {
        id: Uuid,
        #[arg(long,conflicts_with_all=["group","detach"],required_unless_present_any=["group","detach"])]
        element: Option<Uuid>,
        #[arg(long, conflicts_with = "detach")]
        group: Option<Uuid>,
        #[arg(long)]
        detach: bool,
    },
}
fn content(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.trim().chars().count() > 200 {
        Err(Error::new(
            "INVALID_INPUT",
            "Comment must contain 1..200 characters",
        ))
    } else {
        Ok(())
    }
}
fn point(x: f64, y: f64) -> Result<()> {
    if !x.is_finite() || !y.is_finite() {
        Err(Error::new("INVALID_INPUT", "Coordinates must be finite"))
    } else {
        Ok(())
    }
}
pub async fn run(command: &Discussion, client: &Client, canvas: Uuid, compact: bool) -> Result<u8> {
    let result=match command{
        Discussion::List{status}=>return print(&client.discussions(canvas,status).await?,compact).map(|_|0),
        Discussion::Get{id}=>return print(&client.discussion(canvas,*id).await?,compact).map(|_|0),
        Discussion::Create{content:text,x,y,element,group}=>{
            content(text)?;point(*x,*y)?;let mut body=json!({"content":text,"position":{"x":x,"y":y}});
            if let Some(id)=element.or(*group){body["anchor_id"]=json!(id);body["anchor_type"]=json!(if element.is_some(){"element"}else{"group"});}
            client.create_discussion(canvas,&body).await
        }
        Discussion::Reply{id,content:text}=>{content(text)?;client.write_comment(canvas,*id,None,text).await}
        Discussion::Edit{id,comment,content:text}=>{content(text)?;client.write_comment(canvas,*id,Some(*comment),text).await}
        Discussion::Delete{id,comment}=>if let Some(comment)=comment{client.delete_comment(canvas,*id,*comment).await}else{client.delete_discussion(canvas,*id).await},
        Discussion::Resolve{id,reopen}=>client.update_discussion(canvas,*id,&json!({"resolved":!reopen})).await,
        Discussion::Read{id}=>client.mark_discussion_read(canvas,*id).await,
        Discussion::Move{id,x,y}=>{point(*x,*y)?;client.update_discussion(canvas,*id,&json!({"position":{"x":x,"y":y}})).await}
        Discussion::Anchor{id,element,group,detach}=>{
            let anchor=if *detach{serde_json::Value::Null}else{json!({"anchor_type":if element.is_some(){"element"}else{"group"},"anchor_id":element.or(*group)})};client.update_discussion(canvas,*id,&json!({"anchor":anchor})).await
        }
    }.map_err(|e|workflow_state::uncertain(e,"Discussion write result unknown; reread the discussion before retrying",None))?;
    print(&result, compact)?;
    Ok(0)
}
