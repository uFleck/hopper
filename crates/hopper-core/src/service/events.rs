//! Events the services emit. The Tauri adapter forwards them to the panel
//! (names and payloads mirror `EventMap` in `src/ipc/types.ts`).

use std::sync::Mutex;

use serde_json::{Value, json};

use super::dto::{ActivityEntry, ApprovalRequest, EndpointsAdded, McpStatus, Phase};

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    WorkspaceChanged,
    RunProgress {
        run_id: String,
        phase: Phase,
    },
    SecretsStatus {
        stage: crate::format::Stage,
        application: String,
    },
    AgentActivity(ActivityEntry),
    EndpointsAdded(EndpointsAdded),
    ApprovalRequest(ApprovalRequest),
    ApprovalResolved {
        id: String,
    },
    ShowRequest {
        id: String,
    },
    McpStatusChanged(McpStatus),
}

impl Event {
    /// The event name on the wire.
    pub fn name(&self) -> &'static str {
        match self {
            Event::WorkspaceChanged => "workspace://changed",
            Event::RunProgress { .. } => "run://progress",
            Event::SecretsStatus { .. } => "secrets://status",
            Event::AgentActivity(_) => "agent://activity",
            Event::EndpointsAdded(_) => "agent://endpoints-added",
            Event::ApprovalRequest(_) => "agent://approval-request",
            Event::ApprovalResolved { .. } => "agent://approval-resolved",
            Event::ShowRequest { .. } => "panel://show-request",
            Event::McpStatusChanged(_) => "mcp://status",
        }
    }

    /// The JSON payload on the wire (`Null` for payload-less events).
    pub fn payload(&self) -> Value {
        let ser = |v: &dyn erased::Ser| v.value();
        match self {
            Event::WorkspaceChanged => Value::Null,
            Event::RunProgress { run_id, phase } => json!({ "runId": run_id, "phase": phase }),
            Event::SecretsStatus { stage, application } => {
                json!({ "stage": stage, "application": application })
            }
            Event::AgentActivity(a) => ser(a),
            Event::EndpointsAdded(a) => ser(a),
            Event::ApprovalRequest(a) => ser(a),
            Event::ApprovalResolved { id } => json!({ "id": id }),
            Event::ShowRequest { id } => json!({ "id": id }),
            Event::McpStatusChanged(s) => ser(s),
        }
    }
}

mod erased {
    use serde::Serialize;
    use serde_json::Value;

    pub trait Ser {
        fn value(&self) -> Value;
    }

    impl<T: Serialize> Ser for T {
        fn value(&self) -> Value {
            serde_json::to_value(self).unwrap_or(Value::Null)
        }
    }
}

pub trait EventSink: Send + Sync {
    fn emit(&self, e: Event);
}

/// Collects events; for tests and headless use.
#[derive(Default)]
pub struct RecordingSink {
    events: Mutex<Vec<Event>>,
}

impl RecordingSink {
    pub fn events(&self) -> Vec<Event> {
        self.events
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn take(&self) -> Vec<Event> {
        std::mem::take(&mut *self.events.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

impl EventSink for RecordingSink {
    fn emit(&self, e: Event) {
        self.events
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(e);
    }
}
