use std::collections::{HashMap, HashSet};
use uuid::Uuid;
use virse_api::{Client, Error, ResourceItem, ResourceQuery, Result};

pub fn resource_id(value: &str) -> Option<Uuid> {
    Uuid::parse_str(value.strip_prefix('@').unwrap_or(value)).ok()
}

pub fn components(path: &str) -> Result<Vec<&str>> {
    if !path.starts_with('/') {
        return Err(Error::new(
            "INVALID_INPUT",
            "Use an absolute resource path (/folder/name) or @UUID",
        ));
    }
    // No hidden current directory, shell expansion, or speculative traversal through '..'.
    let parts: Vec<_> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.iter().any(|p| matches!(*p, "." | "..")) {
        return Err(Error::new(
            "INVALID_INPUT",
            "Use an absolute path without . or ..; special names can be addressed by @UUID",
        ));
    }
    Ok(parts)
}

pub struct Resolver<'a> {
    pub client: &'a Client,
    pub org: Uuid,
    children: HashMap<Option<Uuid>, Vec<ResourceItem>>,
}
impl<'a> Resolver<'a> {
    pub fn new(client: &'a Client, org: Uuid) -> Self {
        Self {
            client,
            org,
            children: HashMap::new(),
        }
    }
    pub async fn children(&mut self, parent: Option<Uuid>) -> Result<&[ResourceItem]> {
        if !self.children.contains_key(&parent) {
            let mut query = ResourceQuery::new(self.org);
            query.parent_id = parent;
            let mut items = vec![];
            let mut cursors = HashSet::new();
            loop {
                let page = self.client.resources(&query).await?;
                if page.truncated {
                    return Err(Error::new(
                        "INCOMPLETE_DIRECTORY",
                        "Cannot resolve a path from an incomplete directory",
                    ));
                }
                items.extend(page.items);
                match page.next_cursor {
                    Some(cursor) if cursors.insert(cursor.clone()) => query.cursor = Some(cursor),
                    Some(_) => {
                        return Err(Error::new(
                            "INVALID_RESPONSE",
                            "Directory cursor did not advance",
                        ));
                    }
                    None => break,
                }
            }
            self.children.insert(parent, items);
        }
        Ok(self.children.get(&parent).unwrap())
    }
    pub async fn resolve(&mut self, target: &str) -> Result<Option<ResourceItem>> {
        if let Some(id) = resource_id(target) {
            return Ok(Some(self.client.resource(self.org, id).await?.resource));
        }
        let parts = components(target)?;
        let mut parent = None;
        let mut current = None;
        for (index, name) in parts.iter().enumerate() {
            let mut matches: Vec<_> = self
                .children(parent)
                .await?
                .iter()
                .filter(|r| r.name == *name)
                .cloned()
                .collect();
            matches.sort_by_key(|r| r.id);
            matches.dedup_by_key(|r| r.id);
            if matches.is_empty() {
                return Err(Error::new(
                    "RESOURCE_NOT_FOUND",
                    format!("Resource path not found: {target}"),
                ));
            }
            if matches.len() > 1 {
                let mut error = Error::new(
                    "AMBIGUOUS_PATH",
                    format!("Multiple resources named {name}; use @UUID to select one"),
                );
                error.details = Some(Box::new(
                    serde_json::json!({"candidates":matches.iter().map(|r|serde_json::json!({"id":r.id,"name":r.name,"kind":r.kind})).collect::<Vec<_>>()}),
                ));
                return Err(error);
            }
            let item = matches.remove(0);
            if index + 1 < parts.len() && !item.is_folder() {
                return Err(Error::new(
                    "NOT_A_FOLDER",
                    format!("{} is not a folder", item.name),
                ));
            }
            parent = Some(item.id);
            current = Some(item);
        }
        Ok(current)
    }
    pub async fn item(&mut self, target: &str) -> Result<ResourceItem> {
        self.resolve(target).await?.ok_or_else(|| {
            Error::new(
                "INVALID_INPUT",
                "This operation requires a resource, not the root",
            )
        })
    }
    pub async fn folder(&mut self, target: &str) -> Result<Option<Uuid>> {
        match self.resolve(target).await? {
            Some(item) if item.is_folder() => Ok(Some(item.id)),
            Some(_) => Err(Error::new("NOT_A_FOLDER", "The target must be a folder")),
            None => Ok(None),
        }
    }
}

/// Render untrusted names without interpreting terminal control sequences.
pub fn label(value: &str) -> String {
    value
        .chars()
        .flat_map(|c| {
            if c.is_control() {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}
