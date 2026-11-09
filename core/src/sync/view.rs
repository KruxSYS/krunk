//! What `krunk://sync` and the Settings → Sync (beta) card show (0.9.0). The host builds a
//! [`SyncView`] from its state and pushes it as the `sync` event; the pages only render it.
//! The passphrase and the recovery key appear here only during setup, and only towards
//! `krunk://sync` (the trusted page): never towards Settings, never in a private window.

use serde::{Deserialize, Serialize};

/// A device as `krunk://sync` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRow {
    pub id: String,
    pub name: String,
    pub platform: String,
    /// It holds the sync keys (listed in the account directory).
    pub keys: bool,
    /// This device.
    pub me: bool,
    pub last_sync: Option<String>,
}

/// A device that asks to join (an open approval).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRow {
    pub id: String,
    pub label: String,
    pub platform: String,
    pub country: Option<String>,
}

/// The approval this device is answering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Approving {
    pub id: String,
    pub label: String,
    /// The 6 digits to type on the new device, once both sides have them.
    pub code: Option<String>,
    pub done: bool,
}

/// A collection and whether this device syncs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionRow {
    pub name: String,
    pub label: String,
    pub on: bool,
}

/// Sync is on: the status and what the user can do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OnView {
    /// One short sentence ("Up to date", "Offline — will retry", …).
    pub status: String,
    pub last_sync_ms: Option<i64>,
    /// Passwords take part (this device holds the passwords key in its vault).
    pub passwords: bool,
    /// The vault exists but is locked: passwords wait for it.
    pub passwords_locked: bool,
    pub devices: Vec<DeviceRow>,
    pub approvals: Vec<ApprovalRow>,
    pub approving: Option<Approving>,
    /// A device was removed and the keys are being rotated.
    pub rotation_pending: bool,
    pub collections: Vec<CollectionRow>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SyncView {
    /// A private window: nothing of sync.
    Private,
    /// Not signed in to a Krunk account: sign in first (Settings → Account).
    NoAccount,
    /// Signed in; sync is off on this device. Nothing has been sent for sync.
    Off {
        message: Option<String>,
    },
    Working {
        message: String,
    },
    /// The first device: the passphrase and the recovery key, to be saved and confirmed.
    Setup {
        passphrase: String,
        recovery_key: String,
        passwords: bool,
        message: Option<String>,
    },
    /// The account already syncs: restore with the passphrase or recovery key, or ask a device.
    Join {
        message: Option<String>,
        passwords: bool,
    },
    /// A restore found these devices: they keep access (the user confirms).
    RestoreConfirm {
        devices: Vec<String>,
        revoked: usize,
    },
    /// Waiting for a device to answer, then for the 6 digits it shows.
    Joining {
        enter_code: bool,
        message: Option<String>,
    },
    On(OnView),
    /// A security alarm: writes stopped (PLAN §5.6).
    Alarm {
        message: String,
    },
}

impl SyncView {
    /// The Settings card gets no secret: the setup view loses the passphrase and the key.
    pub fn for_settings(&self) -> SyncView {
        match self {
            SyncView::Setup { passwords, .. } => SyncView::Working {
                message: format!("Setting up sync{} in krunk://sync…", if *passwords { " with passwords" } else { "" }),
            },
            v => v.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_serialize_for_the_pages_and_settings_never_sees_secrets() {
        let v =
            SyncView::Setup { passphrase: "a-b".into(), recovery_key: "KRNK-X".into(), passwords: true, message: None };
        let j = serde_json::to_value(&v).unwrap();
        assert_eq!(j["state"], "setup");
        assert_eq!(j["recoveryKey"], "KRNK-X");
        let s = serde_json::to_string(&v.for_settings()).unwrap();
        assert!(!s.contains("a-b") && !s.contains("KRNK"), "{s}");
        let on = SyncView::On(OnView { status: "Up to date".into(), last_sync_ms: None, ..Default::default() });
        let j = serde_json::to_value(&on).unwrap();
        assert_eq!((j["state"].as_str(), j["status"].as_str()), (Some("on"), Some("Up to date")));
        assert!(j.get("lastSyncMs").is_some());
    }
}
