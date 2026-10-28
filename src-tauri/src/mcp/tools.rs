//! The MCP tools: one per `AgentApi` method. Thin on purpose: parse the arguments, call the service, and wrap
//! the answer. Access decisions, redaction and size caps all happen inside `hopper-core`.

use hopper_core::format::Stage;
use hopper_core::service::{
    AgentApi, HopperError,
    agent::{
        AGENT_INSTRUCTIONS, CreateRequestArgs, EnvUpsert, ListRequestsArgs, ResponsePage,
        SearchArgs, SendArgs, SetVariablesArgs, UpdateCollectionArgs, UpdateRequestArgs,
    },
    dto::SpecSource,
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

/// Server instructions come from hopper-core, so the wording lives in one place.
pub const INSTRUCTIONS: &str = AGENT_INSTRUCTIONS;

fn schema(properties: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": properties, "required": required, "additionalProperties": false})
}

fn s(description: &str) -> Value {
    json!({"type": "string", "description": description})
}

fn n(description: &str) -> Value {
    json!({"type": "integer", "minimum": 0, "description": description})
}

fn strings(description: &str) -> Value {
    json!({"type": "array", "items": {"type": "string"}, "description": description})
}

fn method() -> Value {
    json!({"type": "string", "enum": ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"]})
}

fn key_values() -> Value {
    json!({"type": "array", "items": {"type": "object",
        "properties": {"name": {"type": "string"}, "value": {"type": "string"}, "enabled": {"type": "boolean"}},
        "required": ["name", "value"]}})
}

fn request_fields() -> Value {
    json!({
        "name": s("Display name"),
        "method": method(),
        "url": s("URL template; may use {{variables}} and {pathParams}"),
        "pathParams": {"type": "object", "additionalProperties": {"type": "string"}},
        "query": key_values(),
        "headers": key_values(),
        "body": {"type": "object", "description": "{type: none|json|text|xml|form|multipart|binary, content|fields|parts|file}"},
        "auth": {"type": "object", "description": "{type: inherit|none|bearer|basic|apiKey, ...}; use {{secretName}} references, never literal secrets"},
        "timeoutMs": n("Request timeout"),
        "followRedirects": {"type": "boolean"},
        "notes": s("Free text")
    })
}

fn tool(name: &str, description: &str, input: Value) -> Value {
    json!({"name": name, "description": description, "inputSchema": input})
}

pub fn definitions() -> Vec<Value> {
    let patch = request_fields();
    let inline = request_fields();
    vec![
        tool(
            "hopper_workspace",
            "Orient: workspace, collections, environments (names), specs, active environment, format version and usage hints.",
            schema(json!({}), &[]),
        ),
        tool(
            "hopper_describe_format",
            "JSON Schemas and rules for request and environment files.",
            schema(json!({}), &[]),
        ),
        tool(
            "hopper_search_routes",
            "Fuzzy search over spec operations and saved requests. Syntax: `post users`, `tag:Admin`, `#GetUserById`, `path:/api/users/{id}`.",
            schema(
                json!({"query": s("Search text"), "collection": s("Collection slug"), "tag": s("Tag"), "method": method(), "limit": n("Max hits (default 20)")}),
                &["query"],
            ),
        ),
        tool(
            "hopper_get_operation",
            "Full details of one spec operation: parameters, body schema with an example, responses.",
            schema(
                json!({"collection": s("Collection slug"), "spec": s("Spec name"), "key": s("Operation key from a search hit")}),
                &["collection", "spec", "key"],
            ),
        ),
        tool(
            "hopper_list_requests",
            "List saved requests by collection, folder or query.",
            schema(
                json!({"collection": s("Collection slug"), "folder": strings("Folder slugs from the collection root"), "query": s("Filter text"), "limit": n("Max items")}),
                &[],
            ),
        ),
        tool(
            "hopper_get_request",
            "One saved request with its `revision` (needed for hopper_update_request).",
            schema(json!({"id": s("Request id")}), &["id"]),
        ),
        tool(
            "hopper_import_spec",
            "Import an OpenAPI/Swagger spec into a collection from a URL or a local file.",
            schema(
                json!({"collection": s("Collection slug"),
                    "source": {"type": "object", "description": "{url: string} or {file: string}"},
                    "name": s("Optional spec name")}),
                &["collection", "source"],
            ),
        ),
        tool(
            "hopper_create_request",
            "Create a saved request, from `fromOperation` (preferred) or explicit `request` fields. Returns id and path.",
            schema(
                json!({"collection": s("Collection slug"), "folder": strings("Folder slugs"),
                    "fromOperation": {"type": "object", "properties": {"spec": {"type": "string"}, "key": {"type": "string"}}, "required": ["spec", "key"]},
                    "request": {"type": "object", "properties": inline, "required": ["method", "url"]},
                    "name": s("Override the name")}),
                &["collection"],
            ),
        ),
        tool(
            "hopper_update_request",
            "Patch a saved request. Pass `expectedRevision` from hopper_get_request; a stale revision is rejected. Editing suspends the user's Auto grant for that request.",
            schema(
                json!({"id": s("Request id"), "expectedRevision": s("Revision read from hopper_get_request"),
                    "patch": {"type": "object", "properties": patch}}),
                &["id", "expectedRevision", "patch"],
            ),
        ),
        tool(
            "hopper_move_request",
            "Move a saved request to another collection or folder.",
            schema(
                json!({"id": s("Request id"), "collection": s("Target collection slug"), "folder": strings("Target folder slugs")}),
                &["id", "collection"],
            ),
        ),
        tool(
            "hopper_create_folder",
            "Create a folder inside a collection.",
            schema(
                json!({"collection": s("Collection slug"), "parent": strings("Parent folder slugs"), "name": s("Folder name")}),
                &["collection", "name"],
            ),
        ),
        tool(
            "hopper_delete_request",
            "Move a saved request to the in-app trash; the user can undo it.",
            schema(json!({"id": s("Request id")}), &["id"]),
        ),
        tool(
            "hopper_upsert_environment",
            "Set one application's variables and declared secret references in a stage. You cannot set secret values, TLS or allowed hosts.",
            schema(
                json!({"stage": s("local, dev, rc or production"),
                    "application": s("Collection slug"),
                    "variables": {"type": "object", "additionalProperties": {"type": "string"}},
                    "removeVariables": strings("Variable names to remove"),
                    "secrets": {"type": "object", "additionalProperties": {"type": "string"}, "description": "secret name -> provider (\"keyring\")"},
                    "expectedRevision": s("Revision when updating")}),
                &["stage", "application"],
            ),
        ),
        tool(
            "hopper_create_collection",
            "Create a collection. Returns its slug; add folders and requests to it next.",
            schema(json!({"name": s("Collection name")}), &["name"]),
        ),
        tool(
            "hopper_get_collection",
            "One collection: name, revision (needed for hopper_update_collection), folder paths, spec links, request count, header names and auth type. Header values and auth details are never shown.",
            schema(json!({"collection": s("Collection slug")}), &["collection"]),
        ),
        tool(
            "hopper_update_collection",
            "Rename a collection or remove spec links from it. Pass `expectedRevision` from hopper_get_collection; a stale revision is rejected.",
            schema(
                json!({"collection": s("Collection slug"),
                    "expectedRevision": s("Revision read from hopper_get_collection"),
                    "patch": {"type": "object", "properties": {
                        "name": s("New display name"),
                        "removeSpecs": strings("Spec names to unlink; their operations leave search")}}}),
                &["collection", "expectedRevision", "patch"],
            ),
        ),
        tool(
            "hopper_delete_collection",
            "Delete a collection. Refused unless it has no requests, folders or spec links.",
            schema(json!({"collection": s("Collection slug")}), &["collection"]),
        ),
        tool(
            "hopper_delete_folder",
            "Delete a folder. Refused unless it has no requests or subfolders.",
            schema(
                json!({"collection": s("Collection slug"), "folder": strings("Folder slugs from the collection root")}),
                &["collection", "folder"],
            ),
        ),
        tool(
            "hopper_duplicate_request",
            "Copy a saved request into the same folder as `<name> copy`. The copy has no agent grant.",
            schema(json!({"id": s("Request id")}), &["id"]),
        ),
        tool(
            "hopper_restore_request",
            "Restore a request deleted with hopper_delete_request, using its `undoToken`.",
            schema(
                json!({"undoToken": s("Undo token from hopper_delete_request")}),
                &["undoToken"],
            ),
        ),
        tool(
            "hopper_set_active_environment",
            "Make a stage the active one, or pass null to clear it.",
            schema(
                json!({"stage": {"type": ["string", "null"], "description": "local, dev, rc, production, or null"}}),
                &["stage"],
            ),
        ),
        tool(
            "hopper_list_environments",
            "The four stages with per-application non-secret variables, local variable names, secrets as {provider, status}, allowed hosts and revision.",
            schema(json!({}), &[]),
        ),
        tool(
            "hopper_get_environment",
            "One stage; secrets show only {provider, status: set|missing}, never a value.",
            schema(
                json!({"stage": s("local, dev, rc or production")}),
                &["stage"],
            ),
        ),
        tool(
            "hopper_set_variables",
            "Set or remove variables at workspace, collection, folder or request scope. Precedence: request > folder > collection > stage+application > workspace.",
            schema(
                json!({"scope": s("workspace, collection, folder or request"),
                    "collection": s("Collection slug (collection and folder scopes)"),
                    "folder": strings("Folder slug path below the collection (folder scope)"),
                    "requestId": s("Request id (request scope)"),
                    "variables": {"type": "object", "additionalProperties": {"type": "string"}},
                    "removeVariables": strings("Variable names to remove"),
                    "expectedRevision": s("Revision when updating")}),
                &["scope"],
            ),
        ),
        tool(
            "hopper_send_request",
            "Send a saved request (`id`) or an inline `request` in an environment. May wait for the user's approval in the panel; returns status, redacted headers, timing, size, a truncated body and a `responseId`.",
            schema(
                json!({"id": s("Saved request id"),
                    "request": {"type": "object", "properties": inline, "required": ["method", "url"]},
                    "collection": s("Collection whose inherited headers/auth an inline request uses"),
                    "folder": strings("Folder slugs"),
                    "environment": s("Environment name (default: the active one)"),
                    "overrides": {"type": "object", "additionalProperties": {"type": "string"}}}),
                &[],
            ),
        ),
        tool(
            "hopper_get_response",
            "Page through a stored response by `responseId`, optionally selecting a JSON pointer.",
            schema(
                json!({"responseId": s("From hopper_send_request"), "offset": n("Character offset"), "limit": n("Characters per page"), "path": s("JSON pointer, e.g. /items/0/id")}),
                &["responseId"],
            ),
        ),
        tool(
            "hopper_list_history",
            "Recent runs of a request.",
            schema(
                json!({"requestId": s("Request id"), "limit": n("Max runs (up to 20)")}),
                &["requestId"],
            ),
        ),
        tool(
            "hopper_open_in_app",
            "Show a saved request in the Hopper panel so the user sees what you built.",
            schema(json!({"id": s("Request id")}), &["id"]),
        ),
    ]
}

fn parse<T: DeserializeOwned>(args: Value) -> Result<T, HopperError> {
    serde_json::from_value(args)
        .map_err(|e| HopperError::invalid(format!("invalid arguments: {e}")))
}

#[derive(Deserialize)]
struct Id {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OperationArgs {
    collection: String,
    spec: String,
    key: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportArgs {
    collection: String,
    source: SpecSource,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct MoveArgs {
    id: String,
    collection: String,
    #[serde(default)]
    folder: Vec<String>,
}

#[derive(Deserialize)]
struct FolderArgs {
    collection: String,
    #[serde(default)]
    parent: Vec<String>,
    name: String,
}

#[derive(Deserialize)]
struct Name {
    name: String,
}

#[derive(Deserialize)]
struct CollectionRef {
    collection: String,
}

#[derive(Deserialize)]
struct FolderRef {
    collection: String,
    folder: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UndoArgs {
    undo_token: String,
}

#[derive(Deserialize)]
struct ActiveEnvironment {
    stage: Option<Stage>,
}

#[derive(Deserialize)]
struct StageRef {
    stage: Stage,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryArgs {
    request_id: String,
    #[serde(default)]
    limit: Option<usize>,
}

/// Runs a tool. `None` when the tool does not exist.
pub async fn call(api: &AgentApi, name: &str, args: Value) -> Option<Result<Value, HopperError>> {
    let args = if args.is_null() { json!({}) } else { args };
    Some(match name {
        "hopper_workspace" => api.workspace_overview().await,
        "hopper_describe_format" => api.describe_format().await,
        "hopper_search_routes" => match parse::<SearchArgs>(args) {
            Ok(a) => api.search_routes(a).await,
            Err(e) => Err(e),
        },
        "hopper_get_operation" => match parse::<OperationArgs>(args) {
            Ok(a) => api.get_operation(&a.collection, &a.spec, &a.key).await,
            Err(e) => Err(e),
        },
        "hopper_list_requests" => match parse::<ListRequestsArgs>(args) {
            Ok(a) => api.list_requests(a).await,
            Err(e) => Err(e),
        },
        "hopper_get_request" => match parse::<Id>(args) {
            Ok(a) => api.get_request(&a.id).await,
            Err(e) => Err(e),
        },
        "hopper_import_spec" => match parse::<ImportArgs>(args) {
            Ok(a) => {
                api.import_spec(&a.collection, a.source, a.name.as_deref())
                    .await
            }
            Err(e) => Err(e),
        },
        "hopper_create_request" => match parse::<CreateRequestArgs>(args) {
            Ok(a) => api.create_request(a).await,
            Err(e) => Err(e),
        },
        "hopper_update_request" => match parse::<UpdateRequestArgs>(args) {
            Ok(a) => api.update_request(a).await,
            Err(e) => Err(e),
        },
        "hopper_move_request" => match parse::<MoveArgs>(args) {
            Ok(a) => api.move_request(&a.id, &a.collection, &a.folder).await,
            Err(e) => Err(e),
        },
        "hopper_create_folder" => match parse::<FolderArgs>(args) {
            Ok(a) => api.create_folder(&a.collection, &a.parent, &a.name).await,
            Err(e) => Err(e),
        },
        "hopper_delete_request" => match parse::<Id>(args) {
            Ok(a) => api.delete_request(&a.id).await,
            Err(e) => Err(e),
        },
        "hopper_upsert_environment" => match parse::<EnvUpsert>(args) {
            Ok(a) => api.upsert_environment(a).await,
            Err(e) => Err(e),
        },
        "hopper_create_collection" => match parse::<Name>(args) {
            Ok(a) => api.create_collection(&a.name).await,
            Err(e) => Err(e),
        },
        "hopper_get_collection" => match parse::<CollectionRef>(args) {
            Ok(a) => api.get_collection(&a.collection).await,
            Err(e) => Err(e),
        },
        "hopper_update_collection" => match parse::<UpdateCollectionArgs>(args) {
            Ok(a) => api.update_collection(a).await,
            Err(e) => Err(e),
        },
        "hopper_delete_collection" => match parse::<CollectionRef>(args) {
            Ok(a) => api.delete_collection(&a.collection).await,
            Err(e) => Err(e),
        },
        "hopper_delete_folder" => match parse::<FolderRef>(args) {
            Ok(a) => api.delete_folder(&a.collection, &a.folder).await,
            Err(e) => Err(e),
        },
        "hopper_duplicate_request" => match parse::<Id>(args) {
            Ok(a) => api.duplicate_request(&a.id).await,
            Err(e) => Err(e),
        },
        "hopper_restore_request" => match parse::<UndoArgs>(args) {
            Ok(a) => api.restore_request(&a.undo_token).await,
            Err(e) => Err(e),
        },
        "hopper_set_active_environment" => match parse::<ActiveEnvironment>(args) {
            Ok(a) => api.set_active_environment(a.stage).await,
            Err(e) => Err(e),
        },
        "hopper_list_environments" => api.list_environments().await,
        "hopper_get_environment" => match parse::<StageRef>(args) {
            Ok(a) => api.get_environment(a.stage).await,
            Err(e) => Err(e),
        },
        "hopper_set_variables" => match parse::<SetVariablesArgs>(args) {
            Ok(a) => api.set_variables(a).await,
            Err(e) => Err(e),
        },
        "hopper_send_request" => match parse::<SendArgs>(args) {
            Ok(a) => api.send_request(a).await,
            Err(e) => Err(e),
        },
        "hopper_get_response" => match parse::<ResponsePage>(args) {
            Ok(a) => api.get_response(a).await,
            Err(e) => Err(e),
        },
        "hopper_list_history" => match parse::<HistoryArgs>(args) {
            Ok(a) => api.list_history(&a.request_id, a.limit).await,
            Err(e) => Err(e),
        },
        "hopper_open_in_app" => match parse::<Id>(args) {
            Ok(a) => api.open_in_app(&a.id).await,
            Err(e) => Err(e),
        },
        _ => return None,
    })
}
