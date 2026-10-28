use crate::{
    print,
    resource_paths::{Resolver, resource_id},
    storage::{Store, digest},
    workflow_state as state,
};
use clap::Subcommand;
use serde_json::{Value, json};
use std::{collections::HashSet, path::PathBuf};
use uuid::Uuid;
use virse_api::{
    AccessContext, Client, EntityKind, Error, ResourceMutation, ResourceOperation, Result,
};

#[derive(Subcommand)]
pub enum ResourceExtra {
    Pin {
        target: String,
    },
    Unpin {
        target: String,
    },
    Star {
        target: String,
    },
    Unstar {
        target: String,
    },
    /// List personal quick-access entries (including tools where returned by the server).
    Favorites,
    /// Set the visible order in one directory. Server validates membership and permissions.
    Reorder {
        parent: String,
        #[arg(required=true,num_args=1..)]
        ordered: Vec<String>,
    },
    /// Hide an external share entry without changing the original resource's access rules.
    HideShared {
        target: String,
    },
    /// Copy canvas media into a resource directory; does not remove canvas content.
    SaveCanvas {
        #[arg(long, required_unless_present = "group", conflicts_with = "group")]
        element: Vec<Uuid>,
        #[arg(long)]
        group: Option<Uuid>,
        #[arg(long, default_value = "/")]
        parent: String,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    /// Permanently remove specified recycle-bin roots. Requires IDs and explicit --yes.
    Purge {
        #[arg(required=true,num_args=1..)]
        ids: Vec<String>,
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    /// Drain authorized recycle-bin candidates in bounded, resumable pages.
    EmptyTrash {
        #[arg(long)]
        yes: bool,
        #[arg(long,default_value_t=20,value_parser=clap::value_parser!(u16).range(1..=1000))]
        max_batches: u16,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    /// Download selected resources into a ZIP with a per-file result manifest.
    Pack {
        #[arg(required=true,num_args=1..)]
        targets: Vec<String>,
        #[arg(long)]
        output: PathBuf,
    },
}
pub async fn run(
    command: &ResourceExtra,
    client: &Client,
    store: &Store,
    backend: &str,
    org: Uuid,
    canvas: Option<Uuid>,
    compact: bool,
) -> Result<u8> {
    let mut resolver = Resolver::new(client, org);
    match command {
        ResourceExtra::Pin { target }
        | ResourceExtra::Unpin { target }
        | ResourceExtra::Star { target }
        | ResourceExtra::Unstar { target } => {
            let id = resolver.item(target).await?.id;
            let mark = if matches!(
                command,
                ResourceExtra::Pin { .. } | ResourceExtra::Unpin { .. }
            ) {
                "pinned"
            } else {
                "quick_access"
            };
            let on = matches!(
                command,
                ResourceExtra::Pin { .. } | ResourceExtra::Star { .. }
            );
            print(
                &client.mark_resource(org, id, mark, on).await.map_err(|e| {
                    state::uncertain(
                        e,
                        "Mark result unknown; reread resource details before retrying",
                        None,
                    )
                })?,
                compact,
            )?;
        }
        ResourceExtra::Favorites => print(&client.resource_favorites(org).await?, compact)?,
        ResourceExtra::Reorder { parent, ordered } => {
            if ordered.len() > 500 {
                return Err(Error::new(
                    "INVALID_INPUT",
                    "Order supports at most 500 items",
                ));
            }
            let parent = resolver.folder(parent).await?;
            let mut ids = vec![];
            for value in ordered {
                ids.push(resolver.item(value).await?.id);
            }
            print(
                &client
                    .reorder_resources(org, parent, &ids)
                    .await
                    .map_err(|e| {
                        state::uncertain(
                            e,
                            "Order result unknown; reread the directory before retrying",
                            None,
                        )
                    })?,
                compact,
            )?;
        }
        ResourceExtra::HideShared { target } => {
            let id = resolver.item(target).await?.id;
            print(
                &client.hide_shared_resource(org, id).await.map_err(|e| {
                    state::uncertain(
                        e,
                        "Shared-entry removal result unknown; reread your external list",
                        None,
                    )
                })?,
                compact,
            )?;
        }
        ResourceExtra::SaveCanvas {
            element,
            group,
            parent,
            idempotency_key,
        } => {
            let canvas = crate::required(canvas, "canvas")?;
            let user = state::user(client).await?;
            let key = state::key(idempotency_key)?;
            let scope = json!([backend, user, org, canvas, "save-canvas", key]);
            let intent = json!({"elements":element,"group":group,"parent":parent});
            let request = match state::freeze(store, &scope, &intent, None)? {
                Some(value) => value,
                None => {
                    let ids = if let Some(group) = group {
                        group_elements(
                            &client
                                .canvas_entity(canvas, org, EntityKind::Group, *group)
                                .await?,
                        )?
                    } else {
                        element.clone()
                    };
                    if ids.is_empty()
                        || ids.len() > 200
                        || ids.iter().collect::<HashSet<_>>().len() != ids.len()
                    {
                        return Err(Error::new(
                            "INVALID_INPUT",
                            "Choose 1..200 distinct elements",
                        ));
                    }
                    let parent = resolver.folder(parent).await?;
                    state::freeze(
                        store,
                        &scope,
                        &intent,
                        Some(json!({"elements":ids,"parent":parent})),
                    )?
                    .unwrap()
                }
            };
            let ids: Vec<Uuid> = serde_json::from_value(request["elements"].clone())
                .map_err(|_| Error::new("CONFIG_ERROR", "Invalid saved elements"))?;
            let parent: Option<Uuid> = serde_json::from_value(request["parent"].clone())
                .map_err(|_| Error::new("CONFIG_ERROR", "Invalid saved destination"))?;
            eprintln!("Operation key: {key}");
            let mut outcomes = vec![];
            let mut unknown = false;
            for id in ids {
                let item_key = digest(&json!([key, id]).to_string());
                match client
                    .save_canvas_resource(org, canvas, id, parent, &item_key, user)
                    .await
                {
                    Ok(value) => {
                        outcomes.push(json!({"element_id":id,"outcome":"succeeded","data":value}))
                    }
                    Err(e) => {
                        let e = state::uncertain(
                            e,
                            "Save result unknown; replay original command/key",
                            Some(&key),
                        );
                        unknown |= e.code == "WORKFLOW_UNCERTAIN";
                        outcomes.push(json!({"element_id":id,"outcome":if e.code=="WORKFLOW_UNCERTAIN"{"unknown"}else{"failed"},"error":e}));
                    }
                }
            }
            let ok = outcomes.iter().all(|v| v["outcome"] == "succeeded");
            print(&json!({"idempotency_key":key,"items":outcomes}), compact)?;
            return Ok(if unknown {
                7
            } else if ok {
                0
            } else {
                6
            });
        }
        ResourceExtra::Purge {
            ids,
            yes,
            idempotency_key,
        } => {
            if !yes {
                return Err(Error::new(
                    "INVALID_INPUT",
                    "Permanent deletion requires --yes",
                ));
            }
            let user = state::user(client).await?;
            let key = state::key(idempotency_key)?;
            let scope = json!([backend, user, org, "purge", key]);
            let intent = json!(ids);
            let body = match state::freeze(store, &scope, &intent, None)? {
                Some(v) => v,
                None => {
                    let ids = ids
                        .iter()
                        .map(|s| {
                            resource_id(s).ok_or_else(|| {
                                Error::new(
                                    "INVALID_INPUT",
                                    "Purge requires resource IDs from trash",
                                )
                            })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    let request = ResourceMutation {
                        context: AccessContext::organization(org),
                        op: ResourceOperation::Purge,
                        ids,
                        name: None,
                        destination: None,
                        confirm_widen: false,
                        confirm_narrow: false,
                    };
                    request.validate()?;
                    state::freeze(store, &scope, &intent, Some(json!(request)))?.unwrap()
                }
            };
            let request = serde_json::from_value(body)
                .map_err(|_| Error::new("CONFIG_ERROR", "Invalid purge record"))?;
            let result = client
                .mutate_resources(&request, &key, user)
                .await
                .map_err(|e| {
                    state::uncertain(
                        e,
                        "Purge result unknown; repeat the same command/key",
                        Some(&key),
                    )
                })?;
            let ok = result.succeeded();
            print(
                &json!({"idempotency_key":key,"items":result.items}),
                compact,
            )?;
            return Ok(if ok { 0 } else { 6 });
        }
        ResourceExtra::EmptyTrash {
            yes,
            max_batches,
            idempotency_key,
        } => {
            if !yes {
                return Err(Error::new(
                    "INVALID_INPUT",
                    "Emptying the recycle bin requires --yes",
                ));
            }
            let user = state::user(client).await?;
            let key = state::key(idempotency_key)?;
            let name = format!(
                "empty-trash-{}.json",
                digest(&json!([backend, user, org, key]).to_string())
            );
            let _guard = state::exclusive(store, &json!([backend, user, org, "empty-trash", key]))?;
            let mut progress: Value = store.read(&name)?;
            if progress.is_null() {
                progress = json!({"next_batch":0,"pages":[],"operation_complete":false});
            }
            eprintln!("Operation key: {key}");
            for _ in 0..*max_batches {
                if progress["operation_complete"] == true {
                    break;
                }
                let index = progress["next_batch"]
                    .as_u64()
                    .ok_or_else(|| Error::new("CONFIG_ERROR", "Invalid empty-trash progress"))?;
                let page = client
                    .empty_trash_page(org, &digest(&json!([key, index]).to_string()), user)
                    .await
                    .map_err(|e| {
                        state::uncertain(
                            e,
                            "Empty-trash result unknown; repeat original key to resume",
                            Some(&key),
                        )
                    })?;
                let failed = !page.failed.is_empty();
                let empty = page.total == 0 && !page.truncated;
                progress["pages"]
                    .as_array_mut()
                    .ok_or_else(|| Error::new("CONFIG_ERROR", "Invalid empty-trash pages"))?
                    .push(json!(page));
                progress["next_batch"] = json!(index + 1);
                progress["operation_complete"] = json!(empty);
                {
                    let _lock = store.lock()?;
                    store.save(&name, &progress)?;
                }
                if failed {
                    print(&json!({"idempotency_key":key,"progress":progress}), compact)?;
                    return Ok(6);
                }
                if page.total == 0 && !empty {
                    return Err(Error::new(
                        "INVALID_RESPONSE",
                        "Empty-trash made no progress",
                    ));
                }
            }
            print(&json!({"idempotency_key":key,"progress":progress}), compact)?;
            return Ok(if progress["operation_complete"] == true {
                0
            } else {
                6
            });
        }
        ResourceExtra::Pack { targets, output } => {
            return crate::delivery::pack_resources(
                client, store, backend, org, targets, output, compact,
            )
            .await;
        }
    }
    Ok(0)
}
pub fn group_elements(group: &Value) -> Result<Vec<Uuid>> {
    let ids = group
        .get("elements")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::new("INVALID_RESPONSE", "Group has no member list"))?;
    ids.iter()
        .map(|v| {
            v.as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
                .ok_or_else(|| Error::new("INVALID_RESPONSE", "Invalid group member"))
        })
        .collect()
}
