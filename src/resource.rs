use crate::{
    download, print,
    resource_args::{Listing, MutationOptions, Resource},
    resource_paths::{Resolver, components, label, resource_id},
    resource_tree,
    storage::Store,
};
use serde_json::json;
use std::time::Duration;
use uuid::Uuid;
use virse_api::{
    AccessContext, Client, Error, ResourceDestination, ResourceItem, ResourceMutation,
    ResourceOperation, ResourceQuery, Result,
};

pub async fn run(
    command: &Resource,
    client: &Client,
    store: &Store,
    backend: &str,
    org: Uuid,
    json_output: bool,
) -> Result<u8> {
    let mut resolver = Resolver::new(client, org);
    match command {
        Resource::Ls(options) => listing(client, &mut resolver, options, None, json_output).await?,
        Resource::Search { query, options } => {
            listing(client, &mut resolver, options, Some(query), json_output).await?
        }
        Resource::Tree {
            path,
            depth,
            max_items,
            ids,
        } => {
            let tree = resource_tree::load(client, org, path, *depth, *max_items as usize).await?;
            if json_output {
                print(&tree, true)?;
            } else {
                tree.display(*ids);
            }
        }
        Resource::Get { target } => match resolver.resolve(target).await? {
            Some(item) => print(&client.resource(org, item.id).await?, json_output)?,
            None => {
                let mut query = ResourceQuery::new(org);
                query.limit = 1;
                client.resources(&query).await?;
                print(
                    &json!({"name":"/","kind":"folder","org_id":org}),
                    json_output,
                )?;
            }
        },
        Resource::Download { target, output } => {
            if output.try_exists().map_err(crate::storage::io_error)? {
                return Err(Error::new(
                    "OUTPUT_EXISTS",
                    "Output already exists; choose another path",
                ));
            }
            let item = resolver.item(target).await?;
            let info = client.resource_download_info(org, item.id).await?;
            let size = download::save_info(client, &info, output).await?;
            print(
                &json!({"resource_id":item.id,"path":output,"bytes":size}),
                json_output,
            )?;
        }
        Resource::Mkdir { path, parent } => {
            let (parent_path, name) = match parent {
                Some(parent) => (parent.clone(), path.trim().to_owned()),
                None => {
                    let parts = components(path)?;
                    let name = parts
                        .last()
                        .ok_or_else(|| Error::new("INVALID_INPUT", "Root already exists"))?;
                    (
                        format!("/{}", parts[..parts.len() - 1].join("/")),
                        name.trim().to_owned(),
                    )
                }
            };
            let parent = resolver.folder(&parent_path).await?;
            if resolver
                .children(parent)
                .await?
                .iter()
                .any(|item| item.name == name)
            {
                return Err(Error::new(
                    "RESOURCE_EXISTS",
                    "A resource with this name already exists",
                ));
            }
            let result = client.create_folder(org, parent, &name).await;
            let folder = match result {
                Err(error) if error.retryable || error.code == "INVALID_RESPONSE" => {
                    return Err(Error::new(
                        "FOLDER_CREATION_UNCERTAIN",
                        "Folder creation may have succeeded. List the parent folder before trying again",
                    ));
                }
                result => result?,
            };
            print(&folder, json_output)?;
        }
        Resource::Trash { scope } => {
            let scope = match scope {
                Some(target) => Some(match resource_id(target) {
                    Some(id) => id,
                    None => resolver.item(target).await?.id,
                }),
                None => None,
            };
            let items = client.resource_trash(org, scope).await?;
            if json_output {
                print(&json!({"items":items}), true)?;
            } else {
                show_items(&items);
            }
        }
        _ => return mutate(command, client, store, backend, org, json_output).await,
    }
    Ok(0)
}

async fn listing(
    client: &Client,
    resolver: &mut Resolver<'_>,
    options: &Listing,
    search: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let parent = resolver.folder(&options.path).await?;
    let mut query = ResourceQuery::new(resolver.org);
    query.parent_id = parent;
    query.recursive = options.recursive || search.is_some();
    query.query = search.map(str::to_owned).or_else(|| options.search.clone());
    query.kinds = options.kind.iter().map(|k| k.wire().into()).collect();
    query.tab = options.view.wire().into();
    query.entry_points = parent.is_none()
        && matches!(query.tab.as_str(), "shared" | "external")
        && !query.recursive
        && query.query.is_none();
    query.limit = options.limit;
    query.cursor = options.cursor.clone();
    let page = client.resources(&query).await?;
    if json_output {
        print(&page, true)?;
    } else {
        show_items(&page.items);
        if let Some(cursor) = page.next_cursor {
            eprintln!("More resources available; repeat with --cursor {cursor}");
        }
    }
    Ok(())
}
fn show_items(items: &[ResourceItem]) {
    for item in items {
        println!(
            "{:<14} {}{}  @{}",
            label(&item.kind),
            label(&item.name),
            if item.is_folder() { "/" } else { "" },
            item.id
        );
    }
}

struct Intent<'a> {
    op: ResourceOperation,
    targets: Vec<&'a str>,
    name: Option<&'a str>,
    destination: Option<&'a str>,
    confirm: bool,
    options: &'a MutationOptions,
}
fn intent(command: &Resource) -> Result<Intent<'_>> {
    let value = match command {
        Resource::Rename {
            target,
            name,
            options,
        } => Intent {
            op: ResourceOperation::Rename,
            targets: vec![target],
            name: Some(name),
            destination: None,
            confirm: false,
            options,
        },
        Resource::Move {
            source,
            destination,
            confirm_access_change,
            options,
        } => Intent {
            op: ResourceOperation::Move,
            targets: vec![source],
            name: None,
            destination: Some(destination),
            confirm: *confirm_access_change,
            options,
        },
        Resource::Copy {
            source,
            destination,
            options,
        } => Intent {
            op: ResourceOperation::Copy,
            targets: vec![source],
            name: None,
            destination: Some(destination),
            confirm: false,
            options,
        },
        Resource::Delete { targets, options } => Intent {
            op: ResourceOperation::Trash,
            targets: targets.iter().map(String::as_str).collect(),
            name: None,
            destination: None,
            confirm: false,
            options,
        },
        Resource::Restore { ids, options } => Intent {
            op: ResourceOperation::Restore,
            targets: ids.iter().map(String::as_str).collect(),
            name: None,
            destination: None,
            confirm: false,
            options,
        },
        _ => {
            return Err(Error::new(
                "INVALID_INPUT",
                "Unsupported resource operation",
            ));
        }
    };
    Ok(value)
}
async fn mutate(
    command: &Resource,
    client: &Client,
    store: &Store,
    backend: &str,
    org: Uuid,
    json_output: bool,
) -> Result<u8> {
    let intent = intent(command)?;
    let key = intent
        .options
        .idempotency_key
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    if key.is_empty() || key.len() > 128 || !key.bytes().all(|b| (33..=126).contains(&b)) {
        return Err(Error::new(
            "INVALID_INPUT",
            "Idempotency key must contain 1..128 printable ASCII characters without spaces",
        ));
    }
    let user = client.me().await?["id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| Error::new("INVALID_RESPONSE", "Identity has no user ID"))?;
    let logical = json!({"op":intent.op,"targets":intent.targets,"name":intent.name,"destination":intent.destination,"confirm":intent.confirm});
    let frozen = store.resource_command(backend, user, org, &key, &logical, None)?;
    let mutation = match frozen {
        Some(request) => request,
        None => {
            let mut resolver = Resolver::new(client, org);
            let mut ids = vec![];
            for target in &intent.targets {
                let id = if intent.op == ResourceOperation::Restore {
                    resource_id(target).ok_or_else(|| {
                        Error::new(
                            "INVALID_INPUT",
                            "Restore requires a resource ID from resource trash",
                        )
                    })?
                } else {
                    resolver.item(target).await?.id
                };
                ids.push(id);
            }
            let destination = match intent.destination {
                Some(path) => Some(ResourceDestination::folder(
                    org,
                    resolver.folder(path).await?,
                )),
                None => None,
            };
            let request = ResourceMutation {
                context: AccessContext::organization(org),
                op: intent.op,
                ids,
                name: intent.name.map(str::to_owned),
                destination,
                confirm_widen: intent.confirm,
                confirm_narrow: intent.confirm,
            };
            request.validate()?;
            store
                .resource_command(backend, user, org, &key, &logical, Some(&request))?
                .ok_or_else(|| Error::new("CONFIG_ERROR", "Cannot save resource operation"))?
        }
    };
    eprintln!("Operation key: {key}");
    for attempt in 0..4 {
        match client.mutate_resources(&mutation, &key, user).await {
            Ok(batch) => {
                let succeeded = batch.succeeded();
                let confirmation = batch.items.iter().any(|item| {
                    item.error.as_ref().is_some_and(|e| {
                        matches!(
                            e.code.as_str(),
                            "visibility_widened" | "visibility_narrowed"
                        )
                    })
                });
                if json_output {
                    print(&json!({"idempotency_key":key,"items":batch.items}), true)?;
                } else {
                    for item in &batch.items {
                        if let Some(error) = &item.error {
                            println!(
                                "{}: {} — {}",
                                item.id,
                                label(&error.code),
                                label(&error.message)
                            );
                            if !error.details.is_null() {
                                print(&error.details, false)?;
                            }
                        } else {
                            println!("{}: succeeded", item.id);
                            if let Some(data) = &item.data {
                                print(data, false)?;
                            }
                        }
                    }
                }
                if confirmation {
                    eprintln!(
                        "Moving changes who can access this resource. Review the returned details; to accept, rerun with --confirm-access-change and a new idempotency key"
                    );
                }
                return Ok(if succeeded { 0 } else { 6 });
            }
            Err(error) if error.retryable || error.code == "INVALID_RESPONSE" => {
                if attempt == 3 {
                    return Err(Error {
                        idempotency_key: Some(key.into_boxed_str()),
                        retryable: true,
                        ..Error::new(
                            "RESOURCE_MUTATION_UNCERTAIN",
                            "Result unknown. Rerun the original command with the same --idempotency-key; do not change its paths or options",
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
