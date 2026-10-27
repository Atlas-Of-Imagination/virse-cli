use crate::resource_paths::{Resolver, label};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;
use virse_api::{Client, Error, ResourceItem, ResourceQuery, Result};

#[derive(Serialize)]
pub struct Node {
    pub resource: ResourceItem,
    pub children: Vec<Node>,
    pub collapsed: bool,
}
#[derive(Serialize)]
pub struct Tree {
    pub root: String,
    pub root_id: Option<Uuid>,
    pub nodes: Vec<Node>,
    pub unplaced: Vec<ResourceItem>,
    pub fetched: usize,
    pub incomplete: bool,
}
impl Tree {
    pub fn display(&self, show_ids: bool) {
        println!("{}", label(&self.root));
        fn lines(nodes: &[Node], prefix: &str, show_ids: bool) {
            for (index, node) in nodes.iter().enumerate() {
                let last = index + 1 == nodes.len();
                println!(
                    "{}{} {}{}{}{}",
                    prefix,
                    if last { "└──" } else { "├──" },
                    label(&node.resource.name),
                    if node.resource.is_folder() { "/" } else { "" },
                    if show_ids {
                        format!("  @{}", node.resource.id)
                    } else {
                        String::new()
                    },
                    if node.collapsed { " …" } else { "" }
                );
                lines(
                    &node.children,
                    &format!("{prefix}{}", if last { "    " } else { "│   " }),
                    show_ids,
                );
            }
        }
        lines(&self.nodes, "", show_ids);
        if !self.unplaced.is_empty() {
            println!("[Resources whose parent is not included in this view]");
            for item in &self.unplaced {
                println!("  {}  @{}", label(&item.name), item.id);
            }
        }
        if self.incomplete {
            eprintln!(
                "Tree is partially expanded; increase --depth/--max-items or browse a smaller folder"
            );
        }
    }
}

pub async fn load(
    client: &Client,
    org: Uuid,
    path: &str,
    depth: u16,
    max_items: usize,
) -> Result<Tree> {
    let root_id = Resolver::new(client, org).folder(path).await?;
    let mut query = ResourceQuery::new(org);
    query.parent_id = root_id;
    query.recursive = true;
    let mut items = vec![];
    let mut cursors = HashSet::new();
    let mut incomplete = false;
    loop {
        let page = client.resources(&query).await?;
        let available = max_items - items.len();
        incomplete |= page.truncated || page.items.len() > available;
        items.extend(page.items.into_iter().take(available));
        if items.len() == max_items {
            incomplete |= page.next_cursor.is_some();
            break;
        }
        match page.next_cursor {
            Some(cursor) if cursors.insert(cursor.clone()) => query.cursor = Some(cursor),
            Some(_) => {
                return Err(Error::new(
                    "INVALID_RESPONSE",
                    "Tree cursor did not advance",
                ));
            }
            None => break,
        }
    }
    assemble(path, root_id, items, depth, incomplete)
}
fn assemble(
    path: &str,
    root_id: Option<Uuid>,
    items: Vec<ResourceItem>,
    depth: u16,
    mut incomplete: bool,
) -> Result<Tree> {
    let by_id: HashMap<_, _> = items.iter().map(|r| (r.id, r)).collect();
    if by_id.len() != items.len() {
        return Err(Error::new(
            "INVALID_RESPONSE",
            "Duplicate resources in a tree snapshot",
        ));
    }
    let mut checked = HashSet::new();
    for item in &items {
        let mut chain = HashSet::new();
        let mut at = Some(item.id);
        while let Some(id) = at {
            if checked.contains(&id) {
                break;
            }
            if !chain.insert(id) {
                return Err(Error::new(
                    "INVALID_RESPONSE",
                    "Cycle in resource hierarchy",
                ));
            }
            at = by_id.get(&id).and_then(|r| r.parent_id);
        }
        checked.extend(chain);
    }
    let mut children: HashMap<Option<Uuid>, Vec<ResourceItem>> = HashMap::new();
    let mut unplaced = vec![];
    for item in &items {
        if item.parent_id != root_id && item.parent_id.is_some_and(|id| !by_id.contains_key(&id)) {
            unplaced.push(item.clone());
            incomplete = true;
        } else {
            children
                .entry(item.parent_id)
                .or_default()
                .push(item.clone());
        }
    }
    fn build(
        parent: Option<Uuid>,
        remaining: u16,
        children: &HashMap<Option<Uuid>, Vec<ResourceItem>>,
        incomplete: &mut bool,
    ) -> Vec<Node> {
        children
            .get(&parent)
            .into_iter()
            .flatten()
            .map(|r| {
                let has_children = children.get(&Some(r.id)).is_some_and(|v| !v.is_empty());
                let collapsed = remaining <= 1 && has_children;
                *incomplete |= collapsed;
                Node {
                    resource: r.clone(),
                    children: if remaining > 1 {
                        build(Some(r.id), remaining - 1, children, incomplete)
                    } else {
                        vec![]
                    },
                    collapsed,
                }
            })
            .collect()
    }
    let nodes = build(root_id, depth, &children, &mut incomplete);
    Ok(Tree {
        root: path.into(),
        root_id,
        nodes,
        unplaced,
        fetched: items.len(),
        incomplete,
    })
}
