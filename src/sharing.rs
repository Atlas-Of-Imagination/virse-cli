use crate::{
    print,
    resource_paths::{Resolver, resource_id},
    share_args::{Permissions, Share},
};
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;
use virse_api::{
    Client, Error, Result, ShareAddress, ShareChange, ShareClaimTarget, ShareRule, ShareRuleInput,
    ShareSettings, ShareSubject, absolute_share_url,
};

async fn target_id(client: &Client, org: Uuid, target: &str) -> Result<Uuid> {
    match resource_id(target) {
        Some(id) => Ok(id),
        None => Ok(Resolver::new(client, org).item(target).await?.id),
    }
}
fn management(settings: &ShareSettings) -> Result<()> {
    if !settings.can_manage {
        return Err(Error {
            status: Some(403),
            ..Error::new(
                "PERMISSION_DENIED",
                "You can view these rules but cannot manage them",
            )
        });
    }
    Ok(())
}
fn existing_rule<'a>(settings: &'a ShareSettings, input: &ShareRuleInput) -> Option<&'a ShareRule> {
    settings.rules.iter().find(|row| {
        if row.inherited_from.is_some()
            || matches!(
                row.row_type.as_str(),
                "creator" | "ancestor_creator" | "inherited"
            )
        {
            return false;
        }
        match &input.subject {
            ShareSubject::Org => row.row_type == "org_all",
            ShareSubject::Link => row.row_type == "link",
            ShareSubject::Member(id) => {
                row.subject_type.as_deref() == Some("member")
                    && row.subject_id.as_deref() == Some(id)
            }
            ShareSubject::Group(id) => {
                row.subject_type.as_deref() == Some("group")
                    && row.subject_id.as_deref() == Some(id.to_string().as_str())
            }
            ShareSubject::Email(email) => {
                row.subject_type.as_deref() == Some("email")
                    && row
                        .subject_id
                        .as_deref()
                        .is_some_and(|s| s.eq_ignore_ascii_case(email))
            }
        }
    })
}
fn uncertain(error: Error, resource: Option<Uuid>, operation: &str) -> Error {
    if matches!(
        error.code.as_str(),
        "TRANSPORT_ERROR" | "INVALID_RESPONSE" | "internal"
    ) || error.status.is_some_and(|s| s >= 500)
    {
        Error {
            status: error.status,
            request_id: error.request_id,
            details: Some(Box::new(
                json!({"resource_id":resource,"operation":operation,"cause":error.code}),
            )),
            ..Error::new(
                "SHARE_MUTATION_UNCERTAIN",
                "The permission operation may have succeeded. Inspect current permissions before repeating it; it was not automatically retried",
            )
        }
    } else {
        error
    }
}
async fn refreshed(
    client: &Client,
    org: Uuid,
    id: Uuid,
    result: impl Serialize,
    json_output: bool,
) -> Result<()> {
    // A failed follow-up read must not turn a confirmed write into a reported write failure.
    match client.share_settings(org, id).await {
        Ok(settings) => print(
            &json!({"applied":true,"result":result,"settings":settings}),
            json_output,
        ),
        Err(error) => {
            eprintln!(
                "Permission change succeeded; refreshing the rule list failed ({})",
                error.code
            );
            print(
                &json!({"applied":true,"result":result,"settings":null,"refresh_error":error}),
                json_output,
            )
        }
    }
}

pub async fn permissions(
    command: &Permissions,
    client: &Client,
    org: Uuid,
    json_output: bool,
) -> Result<u8> {
    let target = match command {
        Permissions::Get { target }
        | Permissions::Set { target, .. }
        | Permissions::Remove { target, .. }
        | Permissions::Clear { target, .. } => target,
    };
    // Freeze the concrete resource ID once; no path re-resolution between preview and write.
    let id = target_id(client, org, target).await?;
    let settings = client.share_settings(org, id).await?;
    match command {
        Permissions::Get { .. } => print(&settings, json_output)?,
        Permissions::Set {
            audience,
            role,
            dry_run,
            ..
        } => {
            management(&settings)?;
            let rule = audience.rule(org, *role)?;
            if let Some(row) = existing_rule(&settings, &rule)
                && (row.max_role == 0 || rule.role > row.max_role)
            {
                return Err(Error {
                    status: Some(403),
                    ..Error::new(
                        "PERMISSION_DENIED",
                        "The server does not allow this role change",
                    )
                });
            }
            let change = ShareChange::RuleSet { rule: rule.clone() };
            let impact = client.share_impact(org, id, &change).await?;
            if *dry_run {
                print(
                    &json!({"dry_run":true,"resource_id":id,"change":change,"impact":impact}),
                    json_output,
                )?;
            } else {
                let updated = client
                    .set_share_rule(org, id, &rule)
                    .await
                    .map_err(|e| uncertain(e, Some(id), "rule_set"))?;
                print(
                    &json!({"applied":true,"impact":impact,"settings":updated}),
                    json_output,
                )?;
            }
        }
        Permissions::Remove {
            rule_id, dry_run, ..
        } => {
            management(&settings)?;
            if let Some(row) = settings
                .rules
                .iter()
                .find(|row| row.rule_id.as_deref() == Some(rule_id.to_string().as_str()))
                && !row.removable
            {
                return Err(Error {
                    status: Some(403),
                    ..Error::new(
                        "RULE_NOT_REMOVABLE",
                        "This row cannot be removed; inherited/creator rows are read-only and permanent rows use role none",
                    )
                });
            }
            let change = ShareChange::RuleRemove { rule_id: *rule_id };
            let impact = client.share_impact(org, id, &change).await?;
            if *dry_run {
                print(
                    &json!({"dry_run":true,"resource_id":id,"change":change,"impact":impact}),
                    json_output,
                )?;
            } else {
                let result = client
                    .remove_share_rule(org, id, *rule_id)
                    .await
                    .map_err(|e| uncertain(e, Some(id), "rule_remove"))?;
                refreshed(client, org, id, result, json_output).await?;
            }
        }
        Permissions::Clear { dry_run, .. } => {
            management(&settings)?;
            if *dry_run {
                print(
                    &json!({"dry_run":true,"resource_id":id,"operation":"clear_local_rules","current_settings":settings,"message":"Clear this resource's local rules and local link role only; inherited access remains. Affected-user counts are unavailable for clearing all rules."}),
                    json_output,
                )?;
            } else {
                let result = client
                    .clear_share_settings(org, id)
                    .await
                    .map_err(|e| uncertain(e, Some(id), "settings_clear"))?;
                refreshed(client, org, id, result, json_output).await?;
            }
        }
    }
    Ok(0)
}

pub async fn share(command: &Share, client: &Client, org: Uuid, json_output: bool) -> Result<u8> {
    match command {
        Share::Link { target, web_origin } => {
            let id = target_id(client, org, target).await?;
            let settings = client.share_settings(org, id).await?;
            let relative = settings.link_url.as_deref().ok_or_else(|| {
                Error::new(
                    "SHARE_LINK_UNAVAILABLE",
                    "No share address is available for this resource",
                )
            })?;
            let link = match web_origin {
                Some(origin) => absolute_share_url(relative, origin)?,
                None => relative.to_owned(),
            };
            if json_output {
                print(
                    &json!({"resource_id":id,"link_url":link,"my_access":settings.my_access,"external_sharing":settings.external_sharing}),
                    true,
                )?;
            } else {
                println!("{link}");
            }
        }
        Share::Resolve { link } => print(
            &client
                .resolve_share(org, &ShareAddress::parse(link)?)
                .await?,
            json_output,
        )?,
        Share::Claim { link, resource } => {
            let target = match (link, resource) {
                (Some(link), None) => ShareAddress::parse(link)?.claim_target()?,
                (None, Some(id)) => ShareClaimTarget::Resource(*id),
                _ => {
                    return Err(Error::new(
                        "INVALID_INPUT",
                        "Choose a share address or resource ID",
                    ));
                }
            };
            let result = client
                .claim_share(org, &target)
                .await
                .map_err(|e| uncertain(e, *resource, "claim"))?;
            print(&result, json_output)?;
        }
    }
    Ok(0)
}
