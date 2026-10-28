use crate::{Error, Result, ShareClaimTarget, normalize_backend};
use url::Url;
use uuid::Uuid;

/// Parsed addresses are sent to the configured backend, never fetched from the pasted host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareAddress {
    Token(String),
    Canvas {
        canvas_id: Uuid,
        token: Option<String>,
    },
}
fn invalid() -> Error {
    Error::new(
        "INVALID_INPUT",
        "Expected a share token, /s/<token>, or /canvas/<UUID>?share=<token> URL",
    )
}
pub(crate) fn validate_share_token(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 512
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
    {
        return Err(invalid());
    }
    Ok(())
}
impl ShareAddress {
    pub fn parse(input: &str) -> Result<Self> {
        if !input.contains('/')
            && !input.contains(':')
            && !input.contains('?')
            && !input.contains('#')
        {
            validate_share_token(input)?;
            return Ok(Self::Token(input.into()));
        }
        if input.starts_with("//") {
            return Err(invalid());
        }
        let url = if input.starts_with('/') {
            Url::parse("https://share.invalid")
                .unwrap()
                .join(input)
                .map_err(|_| invalid())?
        } else {
            Url::parse(input).map_err(|_| invalid())?
        };
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(invalid());
        }
        let parts: Vec<_> = url.path().trim_end_matches('/').split('/').collect();
        match parts.as_slice() {
            ["", "s", token] => {
                validate_share_token(token)?;
                Ok(Self::Token((*token).into()))
            }
            ["", "canvas", canvas] => {
                let canvas_id = Uuid::parse_str(canvas).map_err(|_| invalid())?;
                let tokens: Vec<_> = url
                    .query_pairs()
                    .filter(|(key, _)| key == "share")
                    .map(|(_, value)| value.into_owned())
                    .collect();
                if tokens.len() > 1 {
                    return Err(invalid());
                }
                let token = tokens.into_iter().next();
                if let Some(token) = &token {
                    validate_share_token(token)?;
                }
                Ok(Self::Canvas { canvas_id, token })
            }
            _ => Err(invalid()),
        }
    }
    pub fn claim_target(&self) -> Result<ShareClaimTarget> {
        match self {
            Self::Token(token)
            | Self::Canvas {
                token: Some(token), ..
            } => Ok(ShareClaimTarget::Token(token.clone())),
            _ => Err(Error::new(
                "INVALID_INPUT",
                "Claim requires a share token or an explicit resource ID",
            )),
        }
    }
}
/// Compose only when the caller explicitly provides the browser origin; backend != frontend.
pub fn absolute_share_url(relative: &str, web_origin: &str) -> Result<String> {
    if !relative.starts_with('/') || relative.starts_with("//") {
        return Err(invalid());
    }
    ShareAddress::parse(relative)?;
    let base = Url::parse(&normalize_backend(web_origin)?).map_err(|_| invalid())?;
    let url = base.join(relative).map_err(|_| invalid())?;
    if url.origin() != base.origin() {
        return Err(invalid());
    }
    Ok(url.into())
}
