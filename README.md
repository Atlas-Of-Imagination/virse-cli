# Virse CLI

A Rust command-line client for [Virse](https://virse.ai), built for interactive use, scripts, and AI agents.

Manage assets, work with canvases, generate images and video, create reusable tools, and interact with canvas agents from your terminal. Commands support structured JSON output and resumable workflows.

[Installation](#installation) · [Quick start](#quick-start) · [Workflows](#common-workflows) · [Agent skills](#agent-skills) · [Rust SDK](#rust-sdk) · [Development](#development-and-releases)

This repository contains two crates:

- **`virse-cli`** — the `virse` executable, including command parsing, local configuration, credentials, and recovery records.
- **[`virse-api`](virse-api/)** — an asynchronous Rust client for Virse HTTP and WebSocket APIs. It accepts explicit configuration and does not manage user files or environment variables.

## Installation

### Prebuilt binaries

Download an archive and its SHA-256 checksum from [GitHub Releases](https://github.com/Atlas-Of-Imagination/virse-cli/releases), or use the installer:

```sh
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/Atlas-Of-Imagination/virse-cli/main/install.sh -o install.sh
sh install.sh
```

```powershell
# Windows PowerShell
Invoke-WebRequest https://raw.githubusercontent.com/Atlas-Of-Imagination/virse-cli/main/install.ps1 -OutFile install.ps1
.\install.ps1
```

Installers require a published binary release. If a release is not available, install from source below. They verify archive checksums and install only the executable; skills are distributed separately in this repository.

| Platform | Architectures |
| --- | --- |
| Linux | x64 and ARM64, musl; x64, glibc |
| macOS | Apple Silicon and Intel |
| Windows | x64 and ARM64 |

The Unix installer defaults to musl on Linux. It selects a writable directory on `PATH`, or falls back to `~/.local/bin`. Windows defaults to `%LOCALAPPDATA%\Programs\virse\bin` and updates the user `PATH`.

Rerun the installer to update. Use `sh install.sh --help` or `Get-Help .\install.ps1` for options. Both installers accept `VIRSE_VERSION`, `VIRSE_INSTALL_DIR`, `VIRSE_TARGET`, and `VIRSE_REPO`; explicit arguments take precedence. There is no automatic updater.

### From source

Install a current stable Rust toolchain, then:

```sh
git clone https://github.com/Atlas-Of-Imagination/virse-cli.git
cd virse-cli
cargo install --path . --locked --jobs 2
virse --help
```

## Quick start

Sign in with your Virse account through the browser, or use an API token for scripts and CI.

```sh
# Open the browser, sign in, and authorize this CLI.
virse auth login
virse auth status

# Find your personal or team workspace and set it as the default.
virse space list --json
virse config set org <org-id>

# Browse resources, create a canvas, and select it.
virse resource ls /
virse canvas create 'My canvas' --json
virse config set canvas <canvas-space-id>

# Discover models available to this workspace.
virse model list --mode image --json
virse balance --json

# Use a model key from the model list.
virse generation image --model <model-key> --prompt 'A ceramic cup in soft daylight' --wait
```

Use the `canvas_space_id` returned by `canvas create` when setting the default canvas. A canvas's resource ID is a separate identifier used by resource-library commands.

API access, model availability, permissions, and generation charges are enforced by the server. Model keys and parameters depend on your workspace; examples use placeholders rather than assuming a particular model is available.

## Authentication and configuration

Browser login prints an authorization link and attempts to open your default browser. Confirm the account, environment, and verification code before approving access. Production authorization requires human verification, including when you are already signed in.

```sh
virse auth                          # Same as auth login
virse auth login --no-browser        # Print the link; keep the local callback
virse auth login --device            # Authorize an SSH or headless session
virse auth status
virse auth logout
```

Local login uses a temporary loopback callback. `--no-browser` still requires a browser that can reach the CLI's local callback port; use `--device` when the browser is on another machine. Device login polls only while waiting for authorization.

Each browser authorization creates its own API token. The token remains valid until revoked; it is not a short-lived browser session. If login is interrupted, rerun `auth login` with the same profile and backend to resume. Expired pending requests can be cleared with `auth logout` before starting again.

For automation, set `VIRSE_API_TOKEN` through your secret manager or import a token through stdin:

```sh
secret-manager-command | virse auth login --token-stdin
virse --profile work auth status
virse config get
```

`auth status` identifies whether credentials come from the environment, browser authorization, or manual import. Environment credentials take precedence over stored credentials.

- **Browser authorization:** `auth logout` revokes that authorization's token, then clears local credentials. If revocation fails, records are retained for retry. Unset `VIRSE_API_TOKEN` first if it overrides a stored browser authorization.
- **Manual import:** logout clears the selected profile's local credential without revoking the server-side token.
- **Environment token:** logout does not revoke or unset it.

Do not pass tokens as command-line arguments. Stored credentials and authorization recovery records are local files, not an OS keychain; on Unix, configuration directories use permissions `0700` and files use `0600`. Keep these files private.

| Variable | Purpose |
| --- | --- |
| `VIRSE_API_TOKEN` | API token for this process; overrides stored credentials. |
| `VIRSE_BACKEND_URL` | Backend origin; defaults to `https://api.virse.ai`. |
| `VIRSE_CONFIG_DIR` | Override the local configuration and recovery-state directory. |

The backend can only be changed through `VIRSE_BACKEND_URL`, not a flag or saved profile setting. It must be an HTTP(S) origin without credentials, a path, or a query string. HTTP and WebSocket connections use the same backend.

Profiles are selected with `--profile` and default to `default`. Stored credentials and default organization/canvas settings are isolated by profile and backend. Configuration normally lives in your OS user configuration directory, such as `~/.config/virse` on Linux.

Global flags include `--profile`, `--org`, `--canvas`, and `--json`. Explicit organization and canvas flags override profile defaults.

## Common workflows

Use `virse <command> --help` and nested `--help` for the complete command reference.

### Assets and folders

```sh
virse resource mkdir '/Assets'
virse resource upload ./photo.png '/Assets' --conflict rename --json
virse resource tree '/Assets' --ids
virse resource search 'photo' '/Assets' --kind image --json
virse resource get '/Assets/photo.png' --json
virse resource preview '/Assets/photo.png' --output preview.jpg
virse resource download '/Assets/photo.png' --output photo.png
virse resource rename '/Assets/photo.png' 'product.png'

# Read asset.content.revision before replacing content.
virse resource replace '/Assets/product.png' ./edited.png --expected-revision <revision>

# Move to the recycle bin, inspect it, and restore by ID.
virse resource delete '/Assets/product.png'
virse resource trash --json
virse resource restore @<resource-id>
```

Resources accept absolute paths or `@<resource-id>`; `/` is the selected workspace's root. Paths are case-sensitive. Duplicate names produce an ambiguity error with candidates instead of selecting an arbitrary resource. Use IDs when a path is ambiguous. Relative paths, wildcards, and an implicit remote working directory are not supported.

Uploads support server-validated images, video, DOCX, PDF, and GLB, up to **100 MiB per file**. Content replacement preserves the asset ID and requires a previously read revision. Conflicts are never resolved by silently overwriting a newer version. `--conflict skip` only means a name already exists, not that the bytes match.

List and search results are paginated; use the returned cursor with the same filters. `tree` reports when its depth or item limit leaves part of the tree unexpanded.

### Imports, exports, and live changes

```sh
# Snapshot files and directories into a persistent local batch.
virse resource import ./photos ./brief.pdf --parent '/Assets' --defer
virse batch list
virse batch run <batch-id>
virse batch pause <batch-id>
virse batch resume <batch-id>
virse batch retry <batch-id>

virse resource pack '/Assets/product.png' '/Assets/brief.pdf' --output delivery.zip
virse resource download '/Assets/video.mp4' --output video.mp4 --resume

virse resource watch --json
virse resource watch --cursor '<changes-cursor>' --json
```

Import batches preserve directory structure and snapshot source bytes before sending. A batch runs in the foreground and can resume after the process exits. Pause/cancel stops further dispatch; it does not undo completed uploads or guarantee cancellation of an in-flight request. Resolve uncertain items before forgetting a batch.

ZIP exports include downloaded files and a manifest with per-item results. Downloads do not overwrite existing output files; resumable downloads verify content identity and checkpoints before continuing. File extensions do not convert media formats.

Resource watching uses WebSocket notifications to trigger cursor-based catch-up, without periodic HTTP polling. Without a cursor it observes changes from a new baseline, not an initial full listing. For snapshot-to-live handoff, use the listing's `sync_cursor`, not its pagination `next_cursor`. Expired cursors require a fresh snapshot; permission invalidations require consumers to refresh their cached view.

### Permissions and sharing

```sh
virse resource permissions get '/Assets' --json
virse resource permissions set '/Assets' --email teammate@example.com --role view --dry-run
virse resource permissions set '/Assets' --email teammate@example.com --role view
virse resource share link '/Assets' --json
```

Permission changes follow the server's roles and inheritance rules. Preview access changes before applying them. Removing a shared entry is different from revoking access; clearing local permission rules does not remove inherited access.

### Canvases

```sh
virse canvas list --json
virse canvas add --resource '/Assets/product.png'
virse canvas element list --json
virse canvas element get <element-id>
virse canvas element update <element-id> --file patch.json
virse canvas layout --element <element-id-1> --element <element-id-2> --mode row --gap 40
virse canvas contents '/My canvas' --limit 50 --offset 0
virse canvas pack --element <element-id> --output canvas-assets.zip
```

Canvas commands also manage groups, edges, transactions, copied elements, recent deletions, and discussions. Layout changes affect canvas display geometry, not image pixels. `canvas contents` uses a resource path or resource ID; node operations use `--canvas` with the canvas Space ID.

### Generation and image editing

```sh
virse generation video --model <video-model> --prompt 'Slow camera movement around a ceramic cup' --wait
virse generation 3d --model <3d-model> --reference <image-element-id> --wait
virse generation text --model <text-model> --prompt 'Write a short product description' --wait

virse generation edit --source-element <image-element-id> --model <edit-model> \
  --prompt 'Make the cup blue' --wait
virse generation edit --source-element <image-element-id> --model <inpaint-model> \
  --mask-file ./mask.png --prompt 'Replace the selected object with a glass cup' --wait

virse generation get <task-id> --json
virse generation watch <task-id> --timeout 600 --json
virse generation cancel <task-id>
```

Generation creates an output element by default, or targets an existing element when supported. Model-specific options can be supplied through `--parameters` or `--parameters-file`; `generation submit --file` supports structured requests. Additional commands include background removal, upscaling, outpainting, task listing, and retrying failed outputs.

Image-edit masks are black-and-white PNGs: white selects the area to edit and black preserves the rest. Model behavior can still affect details outside the mask. Editing creates a new output by default; it is distinct from replacing an asset's original file.

`--wait` observes asynchronous execution over WebSocket. Ctrl+C or a waiting timeout stops observation, not the server task. Cancellation is limited to what the server actually supports.

### Composer conversations

```sh
virse composer create --name 'Design assistant' --json
virse composer send <element-id> --input 'Suggest three directions for this canvas' --wait
virse composer send <element-id> --input-file brief.md --wait
virse composer history <element-id> --limit 20
virse composer watch <element-id> --timeout 600
virse composer stop <element-id> --wait
```

Send another message to the same Composer element to continue a conversation. References can be supplied explicitly with `--reference`. Complex messages use `--request-file`.

Message submission is attempted once: repeating `send` creates a new message and may repeat work or charges. If acceptance is uncertain, inspect history and execution state before sending again. `stop` requests cooperative interruption and does not undo completed canvas changes. History and live observation can overlap; they do not promise exactly-once event delivery.

### Tools and aesthetics

```sh
virse tool init ./my-tool --name 'My tool'
virse tool validate ./my-tool
virse tool build ./my-tool --json
virse tool run <tool-id> --artifact-id <artifact-id> --inputs inputs.json --wait
virse tool publish --tool-id <tool-id> --artifact-id <artifact-id>
virse tool versions <tool-id> --json

virse aesthetic train --data '{"name":"Soft light","image_ids":["<image-asset-id>"],"defer_dentry":true}'
virse aesthetic watch <run-id>
virse aesthetic call context --id <session-id>
```

Tools support local authoring, immutable builds, publication, source retrieval, version selection, and canvas execution. Aesthetics expose training and existing authoring operations through `aesthetic call`; use the bundled skill for request examples and draft/save workflows.

## Scripting and recovery

`--json` produces compact JSON. Watch commands and some conversational output produce newline-delimited JSON; stdout contains results, while progress and errors go to stderr.

Commands that support `--idempotency-key` keep the original identity, targets, and request for retries. Reuse the **same command and key** after an uncertain result; do not substitute a new key or re-resolve a changed path. Upload recovery also preserves source bytes; use `--resume` to explicitly reuse that snapshot if the original file changed or disappeared.

Not every write has an idempotency contract. Commands such as folder/canvas creation, permission changes, comments, and Composer messages may require inspecting server state before retrying. Keep recovery records until uncertainty is resolved.

Destructive actions are explicit: `resource delete` uses the recycle bin, while `resource purge` and `resource empty-trash` require `--yes`. Preview access changes before confirming a move that changes visibility.

| Exit code | Meaning |
| --- | --- |
| `0` | Command completed successfully. |
| `1` | Business, configuration, or local file error. |
| `2` | Invalid arguments. |
| `3` / `4` | Authentication / permission failure. |
| `5` | Network, service, or observation failure. |
| `6` | Task failure/cancellation, partial success, or incomplete batch. |
| `7` | Outcome uncertain or requiring reconciliation. |
| `8` | Waiting timed out. |
| `130` | Observation interrupted with Ctrl+C. |

A successful query can return a failed task; use task status as well as the process exit code.

## Agent skills

The [skills directory](skills/) contains task-oriented instructions and command examples for AI agents. Start with [Virse](skills/virse/SKILL.md), then load the relevant guide:

- [Assets](skills/virse-assets/SKILL.md) and [sharing](skills/virse-sharing/SKILL.md)
- [Canvas editing](skills/virse-canvas/SKILL.md) and [generation](skills/virse-creation/SKILL.md)
- [Composer](skills/virse-composer/SKILL.md)
- [Tool use](skills/virse-tools/SKILL.md) and [tool creation](skills/smart-tool-creator/SKILL.md)
- [Aesthetic creation](skills/aesthetic-creator/SKILL.md)

Skills are optional and are not installed by the binary installers. Load or copy them using your agent's supported skill mechanism.

## Rust SDK

The SDK is available as the local workspace crate [`virse-api`](virse-api/):

```rust,no_run
use virse_api::Client;

async fn example(token: &str) -> virse_api::Result<()> {
    let client = Client::new("https://api.virse.ai", token)?;
    let account = client.me().await?;
    println!("{}", account);
    Ok(())
}
```

It provides typed requests, results, HTTP operations, safe streaming downloads, and WebSocket observation. `CliAuthClient` handles browser/device authorization requests without requiring an existing API token. Callers own credential storage, files, retry decisions, and idempotency keys. CLI argument parsing and local recovery state remain in the binary crate.

## Development and releases

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -j 2 -- -D warnings
cargo test --workspace -j 2 -- --test-threads=2
```

Tests use local HTTP/WebSocket fixtures rather than a live Virse account. For integration work, select your backend explicitly and use a dedicated test account. Never commit credentials or local recovery files.

The [release workflow](.github/workflows/release.yml) builds seven platform targets, packages only the executable, and generates SHA-256 checksums. Pull requests and manual workflow runs produce build artifacts. Publishing requires a version tag matching the root `Cargo.toml` package version, for example `v0.1.0` for version `0.1.0`. Update the version and lockfile first; all build jobs must succeed before the draft release is published. Installers are separate release assets.

For bug reports, include the CLI version, platform, command with secrets removed, and the error code or request ID. Use [GitHub Issues](https://github.com/Atlas-Of-Imagination/virse-cli/issues) for reports and feature requests.

## Client compatibility and updates

Network business commands check `/client-compatibility` before sending requests. An older API contract exits with code **9** (`CLIENT_UPGRADE_REQUIRED`); a failed, invalid or timed-out check exits with code **5** without sending business requests. Local commands and authentication recovery remain available. Upgrade rejections stop observation/reconnect attempts and keep recovery records; already accepted server tasks are not cancelled.

Compatible newer CLI releases only produce a stderr notice. The public GitHub latest-release check has a two-second timeout and a 24-hour cache in the CLI config directory; unavailable release information never blocks commands. Upgrade messages contain an OS-specific command using `https://virse.ai/install.sh` or `https://virse.ai/install.ps1`. They do not install automatically. These website endpoints must be published before releasing this CLI.

Deploy the matching backend CLI compatibility support first, keeping the minimum contract at 1. Publish a compatible downloadable CLI before raising the backend minimum. CLI releases and SDK/API contract versions are separate; unmarked historical API clients are not retroactively blocked.
