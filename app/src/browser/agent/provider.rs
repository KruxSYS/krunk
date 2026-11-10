//! Provider calls from the browser process (0.12.0, WP-A): builds the request with
//! `lb_core::agent::provider`, adds the key (unwrapped from DPAPI only for this call), sends it
//! with `fetch::request` and decodes the streamed answer.

use super::super::Browser;
use crate::ffi::cef::fetch::JobId;
use lb_core::agent::{ChatRequest, ModelInfo, StreamEvent};

impl Browser {
    /// Streams a chat request to provider `provider_id` (from `settings.ai.providers`).
    /// `on_event` gets the decoded events; `done` runs once: `Ok` at the normal end, `Err` with a
    /// message for the panel (HTTP status and the provider's message, network error, timeout,
    /// cancelled). Nothing is sent while AI is off. Returns the job for `provider_cancel`.
    pub(in crate::browser) fn provider_stream(
        &self,
        provider_id: &str,
        request: &ChatRequest,
        on_event: impl FnMut(StreamEvent) + 'static,
        done: impl FnOnce(Result<(), String>) + 'static,
    ) -> Option<JobId> {
        let _ = (provider_id, request, on_event);
        done(Err("not implemented (WP-A)".into()));
        None
    }

    pub(in crate::browser) fn provider_cancel(&self, job: JobId) {
        crate::ffi::cef::fetch::cancel(job);
    }

    /// The provider's model list (cached per provider for 10 minutes unless `refresh`).
    pub(in crate::browser) fn provider_models(
        &self,
        provider_id: &str,
        refresh: bool,
        done: impl FnOnce(Result<Vec<ModelInfo>, String>) + 'static,
    ) {
        let _ = (provider_id, refresh);
        done(Err("not implemented (WP-A)".into()));
    }
}
