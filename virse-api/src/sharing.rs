use crate::{
    ClearedShareSettings, Client, Error, RemovedShareRule, Result, ShareAddress, ShareChange,
    ShareClaimTarget, ShareClaimed, ShareImpact, ShareResolution, ShareRuleInput, ShareSettings,
};
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use uuid::Uuid;

fn parse<T: DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| Error::protocol())
}
fn settings(value: Value, resource: Uuid) -> Result<ShareSettings> {
    let value: ShareSettings = parse(value)?;
    if value.dentry_id != resource {
        return Err(Error::protocol());
    }
    Ok(value)
}
impl Client {
    pub async fn share_settings(&self, org: Uuid, resource: Uuid) -> Result<ShareSettings> {
        settings(
            self.request(
                Method::POST,
                "versa/share/settings",
                &[],
                Some(json!({"org_id":org,"dentry_id":resource})),
                None,
            )
            .await?,
            resource,
        )
    }
    /// Single attempt: these legacy endpoints do not support request-key replay.
    pub async fn set_share_rule(
        &self,
        org: Uuid,
        resource: Uuid,
        rule: &ShareRuleInput,
    ) -> Result<ShareSettings> {
        rule.subject.validate(rule.role)?;
        let mut body = json!(rule);
        body["org_id"] = json!(org);
        body["dentry_id"] = json!(resource);
        settings(
            self.request(Method::POST, "versa/share/rule/set", &[], Some(body), None)
                .await?,
            resource,
        )
    }
    pub async fn remove_share_rule(
        &self,
        org: Uuid,
        resource: Uuid,
        rule: Uuid,
    ) -> Result<RemovedShareRule> {
        let result: RemovedShareRule = parse(
            self.request(
                Method::POST,
                "versa/share/rule/remove",
                &[],
                Some(json!({"org_id":org,"dentry_id":resource,"rule_id":rule})),
                None,
            )
            .await?,
        )?;
        if result.removed > 1 {
            return Err(Error::protocol());
        }
        Ok(result)
    }
    pub async fn share_impact(
        &self,
        org: Uuid,
        resource: Uuid,
        change: &ShareChange,
    ) -> Result<ShareImpact> {
        change.validate()?;
        parse(
            self.request(
                Method::POST,
                "versa/share/impact",
                &[],
                Some(json!({"org_id":org,"dentry_id":resource,"change":change})),
                None,
            )
            .await?,
        )
    }
    pub async fn clear_share_settings(
        &self,
        org: Uuid,
        resource: Uuid,
    ) -> Result<ClearedShareSettings> {
        parse(
            self.request(
                Method::POST,
                "versa/share/settings/delete",
                &[],
                Some(json!({"org_id":org,"dentry_id":resource})),
                None,
            )
            .await?,
        )
    }
    pub async fn resolve_share(
        &self,
        org: Uuid,
        address: &ShareAddress,
    ) -> Result<ShareResolution> {
        let (path, body) = match address {
            ShareAddress::Token(token) => {
                crate::share_address::validate_share_token(token)?;
                ("versa/resolve", json!({"org_id":org,"token":token}))
            }
            ShareAddress::Canvas { canvas_id, token } => {
                if let Some(token) = token {
                    crate::share_address::validate_share_token(token)?;
                }
                (
                    "versa/canvas/resolve",
                    json!({"org_id":org,"canvas_space_id":canvas_id,"token":token}),
                )
            }
        };
        let result: ShareResolution = parse(
            self.request(Method::POST, path, &[], Some(body), None)
                .await?,
        )?;
        let valid = if result.claimable {
            result
                .data
                .get("dentry_id")
                .and_then(Value::as_str)
                .is_some()
        } else {
            result
                .data
                .get("dentry")
                .and_then(|v| v.get("id"))
                .and_then(Value::as_str)
                .is_some()
        };
        if !valid {
            return Err(Error::protocol());
        }
        Ok(result)
    }
    pub async fn claim_share(
        &self,
        chosen_org: Uuid,
        target: &ShareClaimTarget,
    ) -> Result<ShareClaimed> {
        let mut body = json!({"chosen_org_id":chosen_org});
        match target {
            ShareClaimTarget::Token(token) => {
                crate::share_address::validate_share_token(token)?;
                body["token"] = json!(token);
            }
            ShareClaimTarget::Resource(resource) => {
                body["dentry_id"] = json!(resource);
            }
        }
        let result: ShareClaimed = parse(
            self.request(Method::POST, "versa/share/claim", &[], Some(body), None)
                .await?,
        )?;
        if !result.claimed || result.dentry.get("id").and_then(Value::as_str).is_none() {
            return Err(Error::protocol());
        }
        Ok(result)
    }
}
