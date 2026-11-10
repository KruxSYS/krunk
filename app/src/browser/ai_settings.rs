//! Settings → AI (0.12.0, WP-A): providers, keys, the key test and model lists.
//!
//! Providers and keys can be set up before or after AI is turned on; only the commands that
//! contact the provider (`aiTest`, `aiModels`) need it on. Keys only go in: no reply carries one.

use super::Browser;
use lb_core::agent::provider as prov;
use lb_core::agent::ProviderKind;
use lb_core::protocol::{Event, Request};
use lb_core::settings::AiProvider;
use lb_core::tabs::TabId;
use serde_json::json;

impl Browser {
    pub(super) fn ai_settings_request(&self, tab: Option<TabId>, req_id: Option<u32>, request: Request) {
        match request {
            Request::AiStatus => {
                let stored = self.store.borrow().ai_key_providers().unwrap_or_default();
                let keys: serde_json::Map<String, serde_json::Value> = {
                    let s = self.settings.borrow();
                    stored.into_iter().filter(|id| s.ai.provider(id).is_some()).map(|id| (id, true.into())).collect()
                };
                self.reply(tab, Event::ok(req_id, json!({ "keys": keys })));
            }
            Request::AiProviderSave { provider } => self.ai_provider_save(tab, req_id, provider),
            Request::AiProviderRemove { id } => {
                if self.settings.borrow().ai.provider(&id).is_none() {
                    self.reply(tab, Event::err(req_id, "This provider does not exist"));
                    return;
                }
                let r = self.try_change_settings(|s| {
                    s.ai.providers.retain(|p| p.id != id);
                    if s.ai.active_provider == id {
                        s.ai.active_provider.clear();
                        s.ai.active_model.clear();
                    }
                });
                match r {
                    Ok(()) => {
                        self.ai_key_remove(&id);
                        let providers = self.settings.borrow().ai.providers.clone();
                        self.reply(tab, Event::ok(req_id, json!({ "providers": providers })));
                    }
                    Err(e) => self.reply(tab, Event::err(req_id, e)),
                }
            }
            Request::AiKeySet { id, key } => {
                if self.settings.borrow().ai.provider(&id).is_none() {
                    self.reply(tab, Event::err(req_id, "Save the provider first"));
                    return;
                }
                // `key` is a Secret: wiped when it is dropped at the end of this arm.
                match self.ai_key_set(&id, &key) {
                    Ok(()) => self.reply(tab, Event::ok(req_id, json!({ "hasKey": true }))),
                    Err(e) => self.reply(tab, Event::err(req_id, e)),
                }
            }
            Request::AiKeyRemove { id } => {
                self.ai_key_remove(&id);
                self.reply(tab, Event::ok(req_id, json!({ "hasKey": false })));
            }
            Request::AiTest { id } => {
                let weak = self.me.clone();
                self.provider_models(&id, true, move |r| {
                    let Some(b) = weak.upgrade() else { return };
                    let event = match r {
                        Ok(models) => {
                            let n = models.len();
                            let message = if n == 1 {
                                "Connected: 1 model".to_string()
                            } else {
                                format!("Connected: {n} models")
                            };
                            Event::ok(req_id, json!({ "ok": true, "models": n, "message": message }))
                        }
                        Err(e) => Event::err(req_id, e),
                    };
                    b.reply(tab, event);
                });
            }
            Request::AiModels { id, refresh } => {
                let weak = self.me.clone();
                self.provider_models(&id, refresh, move |r| {
                    let Some(b) = weak.upgrade() else { return };
                    let event = match r {
                        Ok(models) => Event::ok(req_id, json!({ "models": models })),
                        Err(e) => Event::err(req_id, e),
                    };
                    b.reply(tab, event);
                });
            }
            _ => {}
        }
    }

    /// Adds (empty id) or changes a provider. A provider keeps its kind; only a custom one has
    /// its own address. Replies with the providers and the saved one's id.
    fn ai_provider_save(&self, tab: Option<TabId>, req_id: Option<u32>, sent: AiProvider) {
        let existing = if sent.id.is_empty() {
            None
        } else {
            match self.settings.borrow().ai.provider(&sent.id) {
                Some(p) => Some(p.clone()),
                None => {
                    self.reply(tab, Event::err(req_id, "This provider does not exist"));
                    return;
                }
            }
        };
        let kind = existing.as_ref().map_or(sent.kind, |p| p.kind);
        let id = match &existing {
            Some(p) => p.id.clone(),
            None => {
                let s = self.settings.borrow();
                loop {
                    let id = lb_core::scripts_store::new_id()[..8].to_string();
                    if s.ai.provider(&id).is_none() {
                        break id;
                    }
                }
            }
        };
        let name = sent.name.trim();
        let provider = AiProvider {
            id: id.clone(),
            kind,
            name: if name.is_empty() { kind.label().to_string() } else { name.to_string() },
            base_url: if kind == ProviderKind::Custom { prov::base_url(kind, &sent.base_url) } else { String::new() },
            vision: sent.vision,
            max_output_tokens: existing
                .as_ref()
                .map_or(lb_core::settings::DEFAULT_MAX_OUTPUT_TOKENS, |p| p.max_output_tokens),
        };
        let address_changed = existing.as_ref().is_some_and(|p| p.base_url != provider.base_url);
        let r = self.try_change_settings(|s| match s.ai.providers.iter_mut().find(|p| p.id == id) {
            Some(p) => *p = provider,
            None => s.ai.providers.push(provider),
        });
        match r {
            Ok(()) => {
                if address_changed {
                    self.provider_models_forget(&id);
                }
                let providers = self.settings.borrow().ai.providers.clone();
                self.reply(tab, Event::ok(req_id, json!({ "providers": providers, "id": id })));
            }
            Err(e) => self.reply(tab, Event::err(req_id, e)),
        }
    }
}
