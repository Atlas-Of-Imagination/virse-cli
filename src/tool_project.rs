//! Offline, bounded package authoring. Imported scripts are data, never executed.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Cursor, Read, Write},
    path::{Component, Path},
};
use virse_api::{Error, Result};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

pub const MAX_ZIP: usize = 20 * 1024 * 1024;
pub const MAX_EXPANDED: usize = 100 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 2048;

#[derive(Debug, Clone)]
pub struct Package {
    pub bytes: Vec<u8>,
    pub checksum: String,
    pub metadata: Value,
    pub config: Value,
    pub explicit_fields: Vec<String>,
    pub files: Vec<String>,
    pub excluded: Vec<String>,
}

impl Package {
    /// Includes local-only publishing metadata as well as immutable ZIP bytes.
    pub fn fingerprint(&self) -> String {
        digest(
            &serde_json::to_vec(
                &json!({"zip":self.checksum,"metadata":self.metadata,"config":self.config,"explicitFields":self.explicit_fields}),
            )
            .expect("JSON values serialize"),
        )
    }
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn io_error(error: std::io::Error) -> Error {
    Error::new(
        "LOCAL_IO_ERROR",
        format!("Local package I/O failed: {}", error.kind()),
    )
}
fn invalid(message: impl Into<String>) -> Error {
    Error::new("INVALID_TOOL_PACKAGE", message)
}
fn zip_error(_: zip::result::ZipError) -> Error {
    invalid("Invalid or unsupported ZIP archive")
}

/// Portable ZIP names: refuse aliases instead of normalizing untrusted input.
pub fn safe_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 240
        || name.contains(['\\', ':'])
        || name.chars().any(char::is_control)
        || name.split('/').any(|part| {
            part.is_empty() || part == "." || part == ".." || part.ends_with(['.', ' '])
        })
    {
        return Err(invalid(format!("Unsafe package path: {name:?}")));
    }
    for part in name.split('/') {
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.as_bytes()[3].is_ascii_digit())
        {
            return Err(invalid(format!("Non-portable package path: {name:?}")));
        }
    }
    Ok(())
}
fn excluded(name: &str) -> bool {
    name.split('/').any(|part| {
        let lower = part.to_ascii_lowercase();
        lower.starts_with('.')
            || matches!(
                lower.as_str(),
                "node_modules"
                    | "__pycache__"
                    | "__macosx"
                    | "target"
                    | "dist"
                    | "build"
                    | "venv"
                    | "env"
                    | "coverage"
                    | "secrets"
                    | "credentials"
                    | "credentials.json"
                    | "service-account.json"
                    | "service_account.json"
                    | "id_rsa"
                    | "id_ed25519"
            )
            || lower.ends_with(".pem")
            || lower.ends_with(".key")
            || lower.ends_with(".p12")
            || lower.ends_with(".pfx")
            || lower.ends_with(".pyc")
            || lower.ends_with(".log")
            || lower.ends_with(".zip")
    })
}
fn check_content(name: &str, data: &[u8]) -> Result<()> {
    // Detect common accidentally embedded credentials without printing their contents.
    let text = String::from_utf8_lossy(data);
    if text.contains("PRIVATE KEY-----")
        || text
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .any(|s| s.starts_with("virse_sk_") && s.len() > 24)
    {
        return Err(Error::new(
            "PACKAGE_SECRET_DETECTED",
            format!("Possible credential in {name}; remove it before publishing"),
        ));
    }
    Ok(())
}

/// Open every component relative to an already opened directory. O_NOFOLLOW and
/// O_NONBLOCK also protect against directory swaps, FIFO hangs and symlink races.
#[cfg(unix)]
pub fn open_directory(path: &Path) -> Result<File> {
    use rustix::fs::{Mode, OFlags, open, openat};
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir().map_err(io_error)?.join(path)
    };
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut directory =
        File::from(open("/", flags, Mode::empty()).map_err(|e| io_error(e.into()))?);
    for part in absolute.components() {
        match part {
            Component::Normal(name) => {
                directory =
                    File::from(openat(&directory, name, flags, Mode::empty()).map_err(|_| {
                        invalid(
                            "Package directory must be readable and contain no symlink components",
                        )
                    })?)
            }
            Component::ParentDir => {
                return Err(invalid(
                    "Resolve '..' in the package directory path before using it",
                ));
            }
            Component::RootDir | Component::CurDir => {}
            _ => return Err(invalid("Unsupported package directory")),
        }
    }
    Ok(directory)
}
#[cfg(not(unix))]
pub fn open_directory(path: &Path) -> Result<File> {
    let mut prefix = std::path::PathBuf::new();
    for part in path.components() {
        prefix.push(part);
        if fs::symlink_metadata(&prefix)
            .map_err(io_error)?
            .file_type()
            .is_symlink()
        {
            return Err(invalid("Symlink directory is not allowed"));
        }
    }
    File::open(path).map_err(io_error)
}

#[cfg(unix)]
fn collect(
    directory: &File,
    prefix: &str,
    files: &mut BTreeMap<String, Vec<u8>>,
    skipped: &mut Vec<String>,
    budget: &mut usize,
) -> Result<()> {
    use rustix::fs::{Dir, Mode, OFlags, openat};
    use std::os::unix::fs::MetadataExt;
    for entry in Dir::read_from(directory).map_err(|e| io_error(e.into()))? {
        let entry = entry.map_err(|e| io_error(e.into()))?;
        let name = entry
            .file_name()
            .to_str()
            .map_err(|_| invalid("Package filenames must be UTF-8"))?;
        if matches!(name, "." | "..") {
            continue;
        }
        let relative = if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}/{name}")
        };
        if excluded(&relative) {
            skipped.push(relative);
            continue;
        }
        safe_name(&relative)?;
        let mut file = File::from(
            openat(
                directory,
                name,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|_| {
                invalid(format!(
                    "Cannot safely read {relative}; symlinks are not allowed"
                ))
            })?,
        );
        let metadata = file.metadata().map_err(io_error)?;
        if metadata.is_dir() {
            if relative.split('/').count() >= 32 {
                return Err(invalid("Package nesting exceeds 32 directories"));
            }
            collect(&file, &relative, files, skipped, budget)?;
        } else if metadata.is_file() {
            if metadata.nlink() != 1 {
                return Err(invalid(format!(
                    "Hard-linked file is not allowed: {relative}"
                )));
            }
            if files.len() >= MAX_ENTRIES || metadata.len() > (MAX_EXPANDED - *budget) as u64 {
                return Err(invalid("Package exceeds 2048 files or 100 MiB expanded"));
            }
            let mut bytes = Vec::new();
            (&mut file)
                .take((MAX_EXPANDED - *budget + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(io_error)?;
            if bytes.len() > MAX_EXPANDED - *budget || bytes.len() as u64 != metadata.len() {
                return Err(invalid(format!(
                    "File changed while packing or exceeds size limit: {relative}"
                )));
            }
            check_content(&relative, &bytes)?;
            *budget += bytes.len();
            files.insert(relative, bytes);
        } else {
            return Err(invalid(format!(
                "Only regular files and directories are allowed: {relative}"
            )));
        }
    }
    Ok(())
}
#[cfg(not(unix))]
fn collect_path(
    path: &Path,
    prefix: &str,
    files: &mut BTreeMap<String, Vec<u8>>,
    skipped: &mut Vec<String>,
    budget: &mut usize,
) -> Result<()> {
    for entry in fs::read_dir(path).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| invalid("Filename is not UTF-8"))?;
        let relative = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if excluded(&relative) {
            skipped.push(relative);
            continue;
        }
        safe_name(&relative)?;
        let meta = fs::symlink_metadata(entry.path()).map_err(io_error)?;
        if meta.file_type().is_symlink() {
            return Err(invalid("Symlinks are not allowed"));
        }
        if meta.is_dir() {
            collect_path(&entry.path(), &relative, files, skipped, budget)?;
        } else if meta.is_file() {
            if meta.len() > (MAX_EXPANDED - *budget) as u64 || files.len() >= MAX_ENTRIES {
                return Err(invalid("Package size limit exceeded"));
            }
            let mut data = Vec::new();
            File::open(entry.path())
                .map_err(io_error)?
                .take((MAX_EXPANDED - *budget + 1) as u64)
                .read_to_end(&mut data)
                .map_err(io_error)?;
            if data.len() > MAX_EXPANDED - *budget {
                return Err(invalid("Package size limit exceeded"));
            }
            check_content(&relative, &data)?;
            *budget += data.len();
            files.insert(relative, data);
        } else {
            return Err(invalid("Only regular files are allowed"));
        }
    }
    Ok(())
}

fn parse_json(bytes: &[u8], name: &str) -> Result<Value> {
    // Deserialize recursively with duplicate-key rejection, including nested objects.
    struct Unique(Value);
    impl<'de> serde::Deserialize<'de> for Unique {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
            struct Visitor;
            impl<'de> serde::de::Visitor<'de> for Visitor {
                type Value = Unique;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    f.write_str("JSON with unique object keys")
                }
                fn visit_bool<E: serde::de::Error>(
                    self,
                    v: bool,
                ) -> std::result::Result<Unique, E> {
                    Ok(Unique(json!(v)))
                }
                fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Unique, E> {
                    Ok(Unique(json!(v)))
                }
                fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Unique, E> {
                    Ok(Unique(json!(v)))
                }
                fn visit_f64<E: serde::de::Error>(self, v: f64) -> std::result::Result<Unique, E> {
                    Ok(Unique(json!(v)))
                }
                fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<Unique, E> {
                    Ok(Unique(json!(v)))
                }
                fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Unique, E> {
                    Ok(Unique(Value::Null))
                }
                fn visit_seq<A: serde::de::SeqAccess<'de>>(
                    self,
                    mut seq: A,
                ) -> std::result::Result<Unique, A::Error> {
                    let mut out = Vec::new();
                    while let Some(Unique(v)) = seq.next_element()? {
                        out.push(v);
                    }
                    Ok(Unique(Value::Array(out)))
                }
                fn visit_map<A: serde::de::MapAccess<'de>>(
                    self,
                    mut map: A,
                ) -> std::result::Result<Unique, A::Error> {
                    let mut out = serde_json::Map::new();
                    while let Some((k, Unique(v))) = map.next_entry::<String, Unique>()? {
                        if out.insert(k, v).is_some() {
                            return Err(serde::de::Error::custom("duplicate object key"));
                        }
                    }
                    Ok(Unique(Value::Object(out)))
                }
            }
            d.deserialize_any(Visitor)
        }
    }
    serde_json::from_slice::<Unique>(bytes)
        .map(|v| v.0)
        .map_err(|e| {
            invalid(format!(
                "{name}: invalid JSON at line {}, column {}",
                e.line(),
                e.column()
            ))
        })
}
fn frontmatter(bytes: &[u8]) -> Result<Value> {
    if bytes.len() > 64 * 1024 {
        return Err(invalid("SKILL.md exceeds 64 KiB"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("SKILL.md must be UTF-8"))?;
    if text.contains('\0') {
        return Err(invalid("SKILL.md contains NUL"));
    }
    let normalized = text.replace("\r\n", "\n");
    let rest = normalized
        .strip_prefix("---\n")
        .ok_or_else(|| invalid("SKILL.md must start with --- frontmatter"))?;
    let (header, body) = rest
        .split_once("\n---\n")
        .ok_or_else(|| invalid("SKILL.md must close frontmatter with --- and contain a body"))?;
    if body.trim().is_empty() {
        return Err(invalid("SKILL.md body must not be empty"));
    }
    let mut keys = BTreeSet::new();
    for line in header.lines() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with(char::is_whitespace) {
            return Err(invalid(
                "SKILL.md supports only one-line key: value frontmatter, no nested/multiline YAML",
            ));
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| invalid("SKILL.md frontmatter requires key: value lines"))?;
        if !keys.insert(key.trim())
            || matches!(
                value.trim().chars().next(),
                Some('|' | '>' | '&' | '*' | '!' | '[' | '{')
            )
        {
            return Err(invalid(
                "Duplicate frontmatter key or unsupported YAML syntax; use one-line scalar values",
            ));
        }
    }
    let fm: Value = yaml_serde::from_str(header)
        .map_err(|_| invalid("SKILL.md has invalid YAML scalar frontmatter"))?;
    for key in ["name", "description"] {
        if fm
            .get(key)
            .and_then(Value::as_str)
            .is_none_or(|s| s.trim().is_empty())
        {
            return Err(invalid(format!(
                "SKILL.md requires non-empty string frontmatter '{key}'"
            )));
        }
    }
    Ok(fm)
}
fn valid_identifier(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .enumerate()
            .all(|(i, c)| c == b'_' || c.is_ascii_alphabetic() || i > 0 && c.is_ascii_digit())
}
fn text_controls(value: &Value) -> Result<()> {
    match value {
        Value::String(s)
            if s.chars().any(|c| {
                (c <= '\u{001f}' || c == '\u{007f}') && !matches!(c, '\t' | '\n' | '\r')
            }) =>
        {
            return Err(invalid(
                "interface.json contains a forbidden control character",
            ));
        }
        Value::Array(values) => {
            for v in values {
                text_controls(v)?;
            }
        }
        Value::Object(values) => {
            for v in values.values() {
                text_controls(v)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn references(text: &str, ids: &BTreeSet<&str>, location: &str) -> Result<()> {
    let mut rest = text;
    while let Some((_, tail)) = rest.split_once("{{") {
        let (id, next) = tail
            .split_once("}}")
            .ok_or_else(|| invalid(format!("{location}: unclosed field reference")))?;
        if !ids.contains(id.trim()) {
            return Err(invalid(format!("{location}: unknown field reference {id}")));
        }
        rest = next;
    }
    let mut rest = text;
    while let Some((_, tail)) = rest.split_once("${") {
        let Some((id, next)) = tail.split_once('}') else {
            break;
        };
        if ids.contains(id.trim()) {
            return Err(invalid(format!(
                "{location}: use {{{{{}}}}} rather than JavaScript field references",
                id.trim()
            )));
        }
        rest = next;
    }
    Ok(())
}
fn interface(value: &Value) -> Result<()> {
    text_controls(value)?;
    let obj = value
        .as_object()
        .ok_or_else(|| invalid("interface.json must be an object"))?;
    if value.to_string().len() > 64 * 1024 {
        return Err(invalid("interface.json exceeds 64 KiB"));
    }
    if obj
        .get("version")
        .is_some_and(|v| !v.is_null() && v.as_u64() != Some(1))
    {
        return Err(invalid("interface.json version must be 1"));
    }
    let fields = value["fields"]
        .as_array()
        .ok_or_else(|| invalid("interface.json fields must be an array"))?;
    if fields.len() > 30 {
        return Err(invalid("interface.json permits at most 30 fields"));
    }
    let mut ids = BTreeSet::new();
    for (i, field) in fields.iter().enumerate() {
        let id = field["id"]
            .as_str()
            .ok_or_else(|| invalid(format!("interface.json fields[{i}].id is required")))?;
        if id.is_empty()
            || id.len() > 64
            || id == "module"
            || !id
                .bytes()
                .enumerate()
                .all(|(n, c)| c == b'_' || c.is_ascii_alphabetic() || n > 0 && c.is_ascii_digit())
            || !ids.insert(id)
        {
            return Err(invalid(format!(
                "interface.json invalid or duplicate field id: {id}"
            )));
        }
        let kind = field["type"].as_str().unwrap_or("");
        if !matches!(
            kind,
            "text"
                | "textarea"
                | "image"
                | "file"
                | "style"
                | "select"
                | "video"
                | "multiselect"
                | "number"
                | "color"
        ) {
            return Err(invalid(format!(
                "interface.json unsupported field type for {id}"
            )));
        }
        for key in ["label", "labelEn", "placeholder", "placeholderEn"] {
            if field
                .get(key)
                .is_some_and(|v| !v.is_null() && !v.is_string())
            {
                return Err(invalid(format!(
                    "interface.json {id}.{key} must be a string"
                )));
            }
        }
        if field
            .get("required")
            .is_some_and(|v| !v.is_null() && !v.is_boolean())
            || field
                .get("maxCount")
                .is_some_and(|v| !v.is_null() && v.as_u64().is_none_or(|n| n < 1))
        {
            return Err(invalid(format!(
                "interface.json {id}: required must be boolean and maxCount positive integer"
            )));
        }
        if matches!(kind, "select" | "multiselect") {
            let options = field["options"]
                .as_array()
                .filter(|v| !v.is_empty())
                .ok_or_else(|| {
                    invalid(format!("interface.json {id} requires non-empty options"))
                })?;
            let mut values = BTreeSet::new();
            for option in options {
                let val = option["value"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        invalid(format!(
                            "interface.json {id}: option value must be non-empty string"
                        ))
                    })?;
                if ["label", "labelEn"].iter().any(|k| {
                    option
                        .get(*k)
                        .is_some_and(|v| !v.is_null() && !v.is_string())
                }) || !values.insert(val)
                {
                    return Err(invalid(format!(
                        "interface.json {id}: invalid or duplicate options"
                    )));
                }
            }
        }
    }
    if let Some(template) = obj.get("promptTemplate").filter(|v| !v.is_null()) {
        let text = template
            .as_str()
            .ok_or_else(|| invalid("interface.json promptTemplate must be string"))?;
        references(text, &ids, "interface.json promptTemplate")?;
    }
    for key in ["steps", "outputs"] {
        if obj.get(key).is_some_and(|v| !v.is_null() && !v.is_array()) {
            return Err(invalid(format!("interface.json {key} must be an array")));
        }
    }
    if let Some(steps) = value.get("steps").and_then(Value::as_array) {
        if steps.len() > 20 {
            return Err(invalid("interface.json permits at most 20 steps"));
        }
        let mut seen = BTreeSet::new();
        for (i, step) in steps.iter().enumerate() {
            if !step.is_object() {
                return Err(invalid(format!(
                    "interface.json steps[{i}] must be an object"
                )));
            }
            let fallback = format!("step_{}", i + 1);
            let id = match step.get("id").filter(|v| !v.is_null()) {
                Some(v) => v
                    .as_str()
                    .ok_or_else(|| invalid("Step id must be a string"))?,
                None => &fallback,
            };
            if !valid_identifier(id) || !seen.insert(id.to_owned()) {
                return Err(invalid(format!(
                    "interface.json steps[{i}] has invalid/duplicate id"
                )));
            }
            for (key, max) in [
                ("title", 80),
                ("titleEn", 80),
                ("body", 2000),
                ("bodyEn", 2000),
            ] {
                if let Some(value) = step.get(key).filter(|v| !v.is_null()) {
                    let text = value.as_str().ok_or_else(|| {
                        invalid(format!("interface.json steps[{i}].{key} must be text"))
                    })?;
                    if text.chars().count() > max {
                        return Err(invalid(format!(
                            "interface.json steps[{i}].{key} exceeds {max} characters"
                        )));
                    }
                    if key.starts_with("body") {
                        references(text, &ids, &format!("interface.json steps[{i}].{key}"))?;
                    }
                }
            }
        }
    }
    if let Some(outputs) = value.get("outputs").and_then(Value::as_array) {
        for (i, output) in outputs.iter().enumerate() {
            if output
                .get("type")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            {
                return Err(invalid(format!(
                    "interface.json outputs[{i}] requires a non-empty type string"
                )));
            }
            for key in ["label", "labelEn"] {
                if output
                    .get(key)
                    .is_some_and(|v| !v.is_null() && !v.is_string())
                {
                    return Err(invalid(format!(
                        "interface.json outputs[{i}].{key} must be text"
                    )));
                }
            }
            if output
                .get("count")
                .is_some_and(|v| !v.is_null() && v.as_u64().is_none_or(|n| n == 0))
            {
                return Err(invalid(format!(
                    "interface.json outputs[{i}].count must be a positive integer"
                )));
            }
        }
    }
    Ok(())
}
fn metadata(files: &BTreeMap<String, Vec<u8>>) -> Result<(Value, Value, Vec<String>)> {
    if files
        .keys()
        .filter(|n| n.as_str() == "SKILL.md" || n.ends_with("/SKILL.md"))
        .count()
        != 1
    {
        return Err(invalid("Package must contain exactly one root SKILL.md"));
    }
    let fm = frontmatter(
        files
            .get("SKILL.md")
            .ok_or_else(|| invalid("Missing root SKILL.md"))?,
    )?;
    if let Some(bytes) = files.get("interface.json") {
        interface(&parse_json(bytes, "interface.json")?)?;
    }
    let meta = match files.get("meta.json") {
        Some(bytes) => parse_json(bytes, "meta.json")?,
        None => json!({}),
    };
    if !meta.is_object() {
        return Err(invalid("meta.json must be an object"));
    }
    let mut explicit_fields = Vec::new();
    let mut data = json!({"name":fm["name"],"description":fm["description"],"detail":fm["description"],"scenario":fm.get("when_to_use").and_then(Value::as_str).unwrap_or(""),"version":"1.0.0"});
    for (local, wire) in [
        ("name", "name"),
        ("description", "description"),
        ("detail", "detail"),
        ("scenario", "scenario"),
        ("nameEn", "name_en"),
        ("descriptionEn", "description_en"),
        ("detailEn", "detail_en"),
        ("coverUrl", "cover_url"),
        ("coverInputUrl", "cover_input_url"),
        ("casePublicLink", "case_public_link"),
        ("version", "version"),
    ] {
        if let Some(value) = meta.get(local) {
            explicit_fields.push(wire.to_owned());
            if !value.is_string() {
                return Err(invalid(format!("meta.json {local} must be a string")));
            }
            data[wire] = value.clone();
        }
    }
    for (local, wire) in [
        ("tags", "tags"),
        ("scenes", "scenes"),
        ("categoryIds", "category_ids"),
    ] {
        if let Some(value) = meta.get(local) {
            explicit_fields.push(wire.to_owned());
            if value
                .as_array()
                .is_none_or(|v| v.iter().any(|x| !x.is_string()))
            {
                return Err(invalid(format!(
                    "meta.json {local} must be an array of strings"
                )));
            }
            data[wire] = value.clone();
        }
    }
    for key in ["name", "description", "detail", "scenario"] {
        if data[key].as_str().is_none_or(|s| s.trim().is_empty()) {
            return Err(invalid(format!("meta.json {key} must be non-empty")));
        }
    }
    let mut config =
        json!({"name":data["name"],"description":data["description"],"detail":data["detail"]});
    for (wire, local) in [
        ("cover_url", "coverUrl"),
        ("cover_input_url", "coverInputUrl"),
    ] {
        if let Some(value) = data.get(wire) {
            config[local] = value.clone();
        }
    }
    Ok((data, config, explicit_fields))
}
fn verify_paths(files: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    let mut names = BTreeSet::new();
    for name in files.keys() {
        safe_name(name)?;
        if !names.insert(name.to_lowercase()) {
            return Err(invalid("Duplicate case-insensitive package path"));
        }
    }
    for name in &names {
        for (i, _) in name.match_indices('/') {
            if names.contains(&name[..i]) {
                return Err(invalid("Package contains file/directory collision"));
            }
        }
    }
    Ok(())
}
pub fn pack(directory: &Path) -> Result<Package> {
    let root = open_directory(directory)?;
    let mut files = BTreeMap::new();
    let mut skipped = Vec::new();
    let mut budget = 0;
    #[cfg(unix)]
    collect(&root, "", &mut files, &mut skipped, &mut budget)?;
    #[cfg(not(unix))]
    {
        drop(root);
        collect_path(directory, "", &mut files, &mut skipped, &mut budget)?;
    }
    verify_paths(&files)?;
    let (metadata, config, explicit_fields) = metadata(&files)?;
    // Publishing metadata is sent separately; never make cosmetic meta.json
    // formatting part of the backend's package-content/no-op comparison.
    files.remove("meta.json");
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o644);
    for (name, bytes) in &files {
        zip.start_file(name, options).map_err(zip_error)?;
        zip.write_all(bytes).map_err(io_error)?;
    }
    let bytes = zip.finish().map_err(zip_error)?.into_inner();
    if bytes.len() > MAX_ZIP {
        return Err(invalid("Compressed package exceeds 20 MiB"));
    }
    skipped.sort();
    Ok(Package {
        checksum: digest(&bytes),
        bytes,
        metadata,
        config,
        explicit_fields,
        files: files.into_keys().collect(),
        excluded: skipped,
    })
}
pub fn validate(directory: &Path) -> Result<Value> {
    let package = pack(directory)?;
    Ok(
        json!({"valid":true,"offline":true,"files":package.files,"excluded":package.excluded,"packageChecksum":package.checksum,"packageBytes":package.bytes.len(),"metadata":package.metadata}),
    )
}
pub fn init(directory: &Path, name: Option<&str>) -> Result<Value> {
    let name = name
        .or_else(|| directory.file_name().and_then(|n| n.to_str()))
        .filter(|n| !n.trim().is_empty())
        .ok_or_else(|| invalid("Provide a tool name"))?;
    if name.chars().any(char::is_control) {
        return Err(invalid("Tool name must be a single line"));
    }
    let quoted = serde_json::to_string(name).map_err(|_| invalid("Invalid name"))?;
    let mut files = BTreeMap::new();
    files.insert("SKILL.md".into(),format!("---\nname: {quoted}\ndescription: Describe the tool's purpose and output.\nwhen_to_use: Use this tool when the user requests the specific task described below.\n---\n\n# {name}\n\n## Instructions\n\nDescribe the inputs, execution steps, and expected output.\n").into_bytes());
    files.insert(
        "interface.json".into(),
        b"{\n  \"version\": 1,\n  \"fields\": []\n}\n".to_vec(),
    );
    files.insert("meta.json".into(),serde_json::to_vec_pretty(&json!({"name":name,"description":"Describe the tool's purpose and output.","version":"1.0.0"})).unwrap());
    install_files(directory, &files)?;
    Ok(json!({"directory":directory,"created":["SKILL.md","interface.json","meta.json"]}))
}

// Check central directory before ZipArchive: ZIP libraries may collapse duplicate
// names into a map. Reject aliases and inconsistent local names before extraction.
fn zip_directory(bytes: &[u8]) -> Result<usize> {
    fn u16at(b: &[u8], p: usize) -> Result<usize> {
        let v = b.get(p..p + 2).ok_or_else(|| invalid("Truncated ZIP"))?;
        Ok(u16::from_le_bytes([v[0], v[1]]) as usize)
    }
    fn u32at(b: &[u8], p: usize) -> Result<usize> {
        let v = b.get(p..p + 4).ok_or_else(|| invalid("Truncated ZIP"))?;
        Ok(u32::from_le_bytes(v.try_into().unwrap()) as usize)
    }
    let end = bytes
        .windows(4)
        .rposition(|s| s == b"PK\x05\x06")
        .ok_or_else(|| invalid("ZIP end record missing"))?;
    if end + 22 + u16at(bytes, end + 20)? != bytes.len()
        || u16at(bytes, end + 4)? != 0
        || u16at(bytes, end + 6)? != 0
    {
        return Err(invalid("Multi-volume or trailing ZIP data is not allowed"));
    }
    let count = u16at(bytes, end + 10)?;
    let size = u32at(bytes, end + 12)?;
    let mut pos = u32at(bytes, end + 16)?;
    if count == 0
        || count > MAX_ENTRIES
        || count != u16at(bytes, end + 8)?
        || pos.checked_add(size) != Some(end)
    {
        return Err(invalid("ZIP entry count or directory bounds invalid"));
    }
    let mut names = BTreeMap::new();
    let mut offsets = BTreeSet::new();
    let mut expanded = 0usize;
    for _ in 0..count {
        if bytes.get(pos..pos + 4) != Some(b"PK\x01\x02") {
            return Err(invalid("Invalid ZIP central directory"));
        }
        let flags = u16at(bytes, pos + 8)?;
        let method = u16at(bytes, pos + 10)?;
        if flags & 1 != 0 || !matches!(method, 0 | 8) {
            return Err(invalid("Encrypted or unsupported compressed ZIP entry"));
        }
        expanded = expanded
            .checked_add(u32at(bytes, pos + 24)?)
            .ok_or_else(|| invalid("ZIP expansion overflow"))?;
        if expanded > MAX_EXPANDED {
            return Err(invalid("ZIP expands beyond 100 MiB"));
        }
        let len = u16at(bytes, pos + 28)?;
        let extra = u16at(bytes, pos + 30)?;
        let comment = u16at(bytes, pos + 32)?;
        let raw = bytes
            .get(pos + 46..pos + 46 + len)
            .ok_or_else(|| invalid("Truncated ZIP filename"))?;
        let name = std::str::from_utf8(raw).map_err(|_| invalid("ZIP filenames must be UTF-8"))?;
        let is_dir = name.ends_with('/');
        let canonical = name.strip_suffix('/').unwrap_or(name);
        safe_name(canonical)?;
        if excluded(canonical) {
            return Err(invalid(format!(
                "Export contains excluded or secret path: {canonical}"
            )));
        }
        if names.insert(canonical.to_lowercase(), is_dir).is_some() {
            return Err(invalid("Duplicate ZIP path"));
        }
        let mode = (u32at(bytes, pos + 38)? >> 16) & 0o170000;
        if !matches!(mode, 0 | 0o100000 | 0o040000) || (mode == 0o040000 && !is_dir) {
            return Err(invalid("ZIP symlinks or special files are not allowed"));
        }
        let local = u32at(bytes, pos + 42)?;
        if !offsets.insert(local)
            || bytes.get(local..local + 4) != Some(b"PK\x03\x04")
            || flags != u16at(bytes, local + 6)?
            || method != u16at(bytes, local + 8)?
            || len != u16at(bytes, local + 26)?
            || bytes.get(local + 30..local + 30 + len) != Some(raw)
        {
            return Err(invalid("ZIP local/central headers disagree"));
        }
        let start = local + 30 + len + u16at(bytes, local + 28)?;
        if start
            .checked_add(u32at(bytes, pos + 20)?)
            .is_none_or(|n| n > end - size)
        {
            return Err(invalid("ZIP file extends into directory"));
        }
        pos = pos
            .checked_add(46 + len + extra + comment)
            .ok_or_else(|| invalid("ZIP directory overflow"))?;
    }
    if pos != end {
        return Err(invalid("ZIP directory size mismatch"));
    }
    for name in names.keys() {
        for (i, _) in name.match_indices('/') {
            if names.get(&name[..i]) == Some(&false) {
                return Err(invalid("ZIP file/directory collision"));
            }
        }
    }
    Ok(count)
}
pub fn unpack(bytes: &[u8], checksum: &str) -> Result<BTreeMap<String, Vec<u8>>> {
    if bytes.is_empty() || bytes.len() > MAX_ZIP {
        return Err(invalid("ZIP exceeds 20 MiB or is empty"));
    }
    if checksum != digest(bytes) {
        return Err(Error::new(
            "PACKAGE_CHECKSUM_MISMATCH",
            "Source ZIP checksum mismatch; no files written",
        ));
    }
    let count = zip_directory(bytes)?;
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(zip_error)?;
    if archive.len() != count {
        return Err(invalid("ZIP contains duplicate entries"));
    }
    let mut files = BTreeMap::new();
    let mut size = 0;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(zip_error)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_owned();
        let expected = entry.size();
        let mut data = Vec::new();
        (&mut entry)
            .take((MAX_EXPANDED - size + 1) as u64)
            .read_to_end(&mut data)
            .map_err(io_error)?;
        if data.len() > MAX_EXPANDED - size || data.len() as u64 != expected {
            return Err(invalid("ZIP expanded size mismatch or limit exceeded"));
        }
        size += data.len();
        check_content(&name, &data)?;
        files.insert(name, data);
    }
    verify_paths(&files)?;
    let roots: Vec<_> = files
        .keys()
        .filter(|n| n.as_str() == "SKILL.md" || n.ends_with("/SKILL.md"))
        .cloned()
        .collect();
    if roots.len() != 1 {
        return Err(invalid("Export must contain exactly one SKILL.md"));
    }
    let prefix = roots[0].strip_suffix("SKILL.md").unwrap();
    if !prefix.is_empty() {
        if files.keys().any(|name| !name.starts_with(prefix)) {
            return Err(invalid("Export contains files outside its package root"));
        }
        files = files
            .into_iter()
            .map(|(name, data)| (name[prefix.len()..].to_owned(), data))
            .collect();
    }
    metadata(&files)?;
    Ok(files)
}

/// Write a complete staging directory, then install it without replacing even an
/// empty existing destination. No archive-supplied path is passed to extract().
pub fn install_files(destination: &Path, files: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    verify_paths(files)?;
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = destination
        .file_name()
        .ok_or_else(|| invalid("Destination must name a new directory"))?;
    let _parent = open_directory(parent)?;
    if fs::symlink_metadata(destination).is_ok() {
        return Err(Error::new(
            "DESTINATION_EXISTS",
            "Destination already exists; choose a new directory",
        ));
    }
    let stage = tempfile::Builder::new()
        .prefix(".virse-tool-stage-")
        .tempdir_in(parent)
        .map_err(io_error)?;
    for (name, data) in files {
        let path = stage.path().join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut f = options.open(path).map_err(io_error)?;
        f.write_all(data).map_err(io_error)?;
        f.sync_all().map_err(io_error)?;
    }
    #[cfg(target_os = "linux")]
    rustix::fs::renameat_with(
        &_parent,
        stage.path().file_name().unwrap(),
        &_parent,
        name,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(|e| {
        if e == rustix::io::Errno::EXIST {
            Error::new("DESTINATION_EXISTS", "Destination already exists")
        } else {
            io_error(e.into())
        }
    })?;
    #[cfg(not(target_os = "linux"))]
    {
        // Reserve destination exclusively. Per-file create_new still forbids overwrite.
        fs::create_dir(destination).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Error::new("DESTINATION_EXISTS", "Destination already exists")
            } else {
                io_error(e)
            }
        })?;
        for name in files.keys() {
            let dst = destination.join(name);
            if let Some(p) = dst.parent() {
                fs::create_dir_all(p).map_err(io_error)?;
            }
            let mut target = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&dst)
                .map_err(io_error)?;
            let mut source = File::open(stage.path().join(name)).map_err(io_error)?;
            std::io::copy(&mut source, &mut target).map_err(io_error)?;
            target.sync_all().map_err(io_error)?;
        }
    }
    #[cfg(unix)]
    _parent.sync_all().map_err(io_error)?;
    Ok(())
}
