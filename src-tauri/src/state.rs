//! Shared app state and the sink that forwards service events to the panel, the OS and the panel state machine.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use hopper_core::{
    panel::{Event as PanelEvent, Mode},
    service::{AppService, Deps, Event, EventSink, HopperError, dto::Phase},
    userdata::UserData,
};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::{mcp::McpState, panel};

/// How long agent activity keeps the panel on screen after the last call.
const ACTIVITY_LINGER: Duration = Duration::from_secs(4);
/// How long the floating Hopper keeps a "new endpoint" notice on screen.
const NOTICE_LINGER: Duration = Duration::from_secs(8);

pub struct AppState {
    pub svc: AppService,
    pub userdata: UserData,
    pub mcp: Arc<McpState>,
}

/// Command result type: serializes as the `HopperError` object from `src/ipc/types.ts`.
pub type CmdResult<T> = Result<T, HopperError>;

pub struct TauriSink {
    app: AppHandle,
    generation: Arc<AtomicU64>,
}

impl TauriSink {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// A passive show that fades back after a quiet period.
    fn pulse(&self) {
        self.pulse_for(ACTIVITY_LINGER);
    }

    fn pulse_for(&self, linger: Duration) {
        panel::dispatch(&self.app, PanelEvent::Activity);
        let mine = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let (app, generation) = (self.app.clone(), self.generation.clone());
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(linger).await;
            if generation.load(Ordering::SeqCst) == mine {
                panel::dispatch(&app, PanelEvent::Idle);
            }
        });
    }

    fn notify(&self, title: &str, body: String) {
        let _ = self
            .app
            .notification()
            .builder()
            .title(title)
            .body(body)
            .show();
    }
}

impl EventSink for TauriSink {
    fn emit(&self, e: Event) {
        let _ = self.app.emit(e.name(), e.payload());
        match &e {
            Event::ApprovalRequest(a) => {
                // Approvals never take focus: show passively, and tell the user if the panel was not open.
                if panel::mode(&self.app) != Mode::Open {
                    self.notify(
                        "Hopper needs your approval",
                        format!(
                            "{} wants to send {} to {}",
                            a.client,
                            a.request_name,
                            a.stage.as_str()
                        ),
                    );
                }
                panel::dispatch(&self.app, PanelEvent::ApprovalNeeded);
            }
            Event::ApprovalResolved { .. } => {
                panel::dispatch(&self.app, PanelEvent::ApprovalResolved)
            }
            Event::EndpointsAdded(a) => {
                // The floating Hopper shows the notice; the OS toast is only for when it is not on screen.
                if panel::mode(&self.app) == Mode::Hidden {
                    self.notify(
                        "Hopper: new endpoint",
                        format!("{} added {}", a.client, a.label),
                    );
                }
                self.pulse_for(NOTICE_LINGER);
            }
            Event::AgentActivity(_) | Event::ShowRequest { .. } => self.pulse(),
            Event::RunProgress { phase, .. } => match phase {
                Phase::Done => panel::dispatch(&self.app, PanelEvent::Idle),
                _ => panel::dispatch(&self.app, PanelEvent::Activity),
            },
            _ => {}
        }
    }
}

/// Builds the service at startup: user data in the app data dir, the OS credential store, the HTTP engine.
pub fn build(app: &AppHandle) -> Result<AppState, Box<dyn std::error::Error>> {
    let userdata = UserData::open(app.path().app_data_dir()?)?;
    let svc = AppService::new(Deps {
        userdata: userdata.clone(),
        secrets: Arc::new(hopper_core::secrets::KeyringProvider),
        engine: Arc::new(hopper_core::engine::Engine::new()),
        events: Arc::new(TauriSink::new(app.clone())),
    });
    Ok(AppState {
        svc,
        userdata,
        mcp: Arc::new(McpState::default()),
    })
}
