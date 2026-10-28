//! In-app approvals for agent sends that policy answers with `Ask`.
//!
//! The agent call parks on a oneshot for up to the approval timeout (60 s by
//! default). The user's answer is applied by [`Open::respond_approval`]: grants
//! and session entries are written there, with the fingerprint recorded when the
//! question was asked, so an edit in between cannot ride on the answer.

use std::{sync::Arc, time::Duration};

use tokio::sync::oneshot;
use ulid::Ulid;

use super::{Open, blocking, canon, dto::*, events::Event, lock, now_rfc3339};
use crate::{
    format::Stage,
    policy::{AgentAccess, Grant},
};

const ARM_AFTER_MS: u64 = 500;

pub(crate) struct Pending {
    pub dto: ApprovalRequest,
    pub tx: oneshot::Sender<ApprovalDecision>,
    pub request_id: Option<String>,
    pub fingerprint: String,
    pub binding: String,
    pub session_key: String,
}

pub(crate) struct AskInfo {
    pub client: String,
    pub request_name: String,
    pub request_id: Option<String>,
    pub stage: Stage,
    pub application: String,
    pub masked: MaskedRequest,
    pub reason: String,
    pub fingerprint: String,
    pub binding: String,
    pub session_key: String,
}

/// Prompts that may wait at once. More would let an agent bury the one that matters.
const MAX_PENDING: usize = 5;

/// Removes the pending entry and tells the UI when the wait ends for any
/// reason: answered, timed out, or the agent call was dropped.
struct PendingGuard {
    open: Arc<Open>,
    id: String,
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        lock(&self.open.pending).remove(&self.id);
        self.open.emit(Event::ApprovalResolved {
            id: self.id.clone(),
        });
    }
}

impl Open {
    pub(crate) async fn ask_approval(self: &Arc<Self>, info: AskInfo) -> Result<(), HopperError> {
        if lock(&self.pending).len() >= MAX_PENDING {
            return Err(HopperError::new(
                "rate_limited",
                "too many approval prompts are already waiting for the user",
            )
            .hint("wait for the user to answer the pending prompts"));
        }
        let id = Ulid::generate().to_string();
        let (tx, rx) = oneshot::channel();
        let dto = ApprovalRequest {
            id: id.clone(),
            client: info.client,
            request_name: info.request_name,
            request_id: info.request_id.clone(),
            stage: info.stage,
            application: info.application,
            request: info.masked,
            reason: info.reason,
            created_at: now_rfc3339(),
            arm_after_ms: ARM_AFTER_MS,
        };
        lock(&self.pending).insert(
            id.clone(),
            Pending {
                dto: dto.clone(),
                tx,
                request_id: info.request_id,
                fingerprint: info.fingerprint,
                binding: info.binding,
                session_key: info.session_key,
            },
        );
        let _guard = PendingGuard {
            open: self.clone(),
            id,
        };
        self.emit(Event::ApprovalRequest(dto));

        let wait = Duration::from_millis(
            self.tun
                .approval_timeout_ms
                .load(std::sync::atomic::Ordering::Relaxed),
        );
        match tokio::time::timeout(wait, rx).await {
            Ok(Ok(ApprovalDecision::AllowOnce))
            | Ok(Ok(ApprovalDecision::AllowSession))
            | Ok(Ok(ApprovalDecision::AlwaysAllow)) => Ok(()),
            Ok(Ok(ApprovalDecision::Block)) => Err(HopperError::denied(
                "the user blocked this request for agents",
            )
            .detail("reason", "request_blocked".into())),
            Ok(Ok(ApprovalDecision::Deny)) => Err(HopperError::new(
                "not_approved",
                "the user denied this request",
            )),
            Ok(Err(_)) => Err(HopperError::new(
                "not_approved",
                "the approval was cancelled",
            )),
            Err(_) => Err(HopperError::new(
                "not_approved",
                format!(
                    "the user did not respond within {} s",
                    wait.as_secs().max(1)
                ),
            )
            .hint("ask the user to watch for the Hopper approval prompt, then try again")),
        }
    }

    pub(crate) async fn respond_approval(
        self: &Arc<Self>,
        id: &str,
        decision: ApprovalDecision,
    ) -> Result<(), HopperError> {
        let expired = || {
            HopperError::new(
                "unknown_approval",
                "this approval request does not exist or has expired",
            )
        };
        let pending = lock(&self.pending).remove(id).ok_or_else(expired)?;
        if pending.tx.is_closed() {
            return Err(expired());
        }
        match decision {
            ApprovalDecision::AllowOnce | ApprovalDecision::Deny => {}
            ApprovalDecision::AllowSession => {
                lock(&self.session).insert(pending.session_key.clone());
            }
            ApprovalDecision::AlwaysAllow => match &pending.request_id {
                Some(rid) => {
                    self.write_grant(
                        rid,
                        AgentAccess::Auto,
                        &pending.fingerprint,
                        &pending.binding,
                    )
                    .await?
                }
                None => {
                    lock(&self.session).insert(pending.session_key.clone());
                }
            },
            ApprovalDecision::Block => {
                if let Some(rid) = &pending.request_id {
                    self.write_grant(
                        rid,
                        AgentAccess::Blocked,
                        &pending.fingerprint,
                        &pending.binding,
                    )
                    .await?;
                }
            }
        }
        pending.tx.send(decision).map_err(|_| expired())
    }

    async fn write_grant(
        self: &Arc<Self>,
        request_id: &str,
        access: AgentAccess,
        fingerprint: &str,
        binding: &str,
    ) -> Result<(), HopperError> {
        let (data, rid, fp, binding) = (
            self.data.clone(),
            canon(request_id),
            fingerprint.to_owned(),
            binding.to_owned(),
        );
        blocking(move || {
            data.set_grant(
                &rid,
                Grant {
                    access,
                    fingerprint: fp,
                    binding,
                },
            )
        })
        .await??;
        let o = self.clone();
        blocking(move || o.refresh_index()).await?;
        Ok(())
    }
}
