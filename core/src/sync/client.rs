//! The key steps over the API (PROTOCOL §6, §11.4–§11.6): set up, restore, approve a new device
//! (both sides) and follow the directories. Platform-free: the caller passes a transport
//! ([`HttpClient`]) and a fresh access token, and runs these on a worker thread. Interactive
//! flows (the approval) are small state machines the UI polls.

use super::api::{classify, ApiError, HttpClient};
use super::dir::{self, Blob, Opener};
use super::envelope::{b64, unb64};
use super::keys::{
    self, ApprovalCtx, Commit, EscrowWire, Joined, KeyError, Me, PakState, RestorePlan, Secret, SetupPlan,
};
use super::state::{AkPin, SyncState};
use super::AccountId;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncFail {
    Api(ApiError),
    Keys(KeyError),
    /// The server answered something that doesn't parse.
    BadAnswer(&'static str),
    /// The local database failed.
    Local(String),
    /// Sync was reset on the website (a new `keyset_gen`, PROTOCOL §4.4 rule 4a): ask the user.
    Reset,
}

impl std::fmt::Display for SyncFail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncFail::Api(e) => write!(f, "{e}"),
            SyncFail::Keys(e) => write!(f, "{e}"),
            SyncFail::BadAnswer(w) => write!(f, "unexpected answer ({w})"),
            SyncFail::Local(e) => write!(f, "local database: {e}"),
            SyncFail::Reset => f.write_str("sync was reset on the website"),
        }
    }
}

impl std::error::Error for SyncFail {}

impl From<ApiError> for SyncFail {
    fn from(e: ApiError) -> SyncFail {
        SyncFail::Api(e)
    }
}

impl From<KeyError> for SyncFail {
    fn from(e: KeyError) -> SyncFail {
        SyncFail::Keys(e)
    }
}

impl From<dir::DirError> for SyncFail {
    fn from(e: dir::DirError) -> SyncFail {
        SyncFail::Keys(KeyError::Dir(e))
    }
}

impl From<rusqlite::Error> for SyncFail {
    fn from(e: rusqlite::Error) -> SyncFail {
        SyncFail::Local(e.to_string())
    }
}

impl From<super::SyncError> for SyncFail {
    fn from(e: super::SyncError) -> SyncFail {
        SyncFail::Keys(KeyError::Crypto(e))
    }
}

pub type SResult<T> = Result<T, SyncFail>;

/// One authenticated device's view of the API.
pub struct Api<'a> {
    pub http: &'a dyn HttpClient,
    pub token: &'a str,
    pub now_ms: i64,
}

impl Api<'_> {
    pub fn raw(&self, method: &str, path: &str, body: Option<&Value>) -> SResult<Vec<u8>> {
        let bytes = body.map(|b| serde_json::to_vec(b).unwrap_or_default());
        let r = self.http.call(method, path, Some(self.token), bytes.as_deref()).map_err(ApiError::Offline)?;
        Ok(classify(r, self.now_ms)?)
    }

    pub fn get<R: DeserializeOwned>(&self, path: &str) -> SResult<R> {
        let b = self.raw("GET", path, None)?;
        serde_json::from_slice(&b).map_err(|_| SyncFail::BadAnswer("json"))
    }

    pub fn post<R: DeserializeOwned>(&self, path: &str, body: &Value) -> SResult<R> {
        let b = self.raw("POST", path, Some(body))?;
        if b.is_empty() {
            return serde_json::from_value(Value::Null).map_err(|_| SyncFail::BadAnswer("json"));
        }
        serde_json::from_slice(&b).map_err(|_| SyncFail::BadAnswer("json"))
    }

    pub fn post_empty(&self, path: &str, body: &Value) -> SResult<()> {
        self.raw("POST", path, Some(body)).map(|_| ())
    }
}

// ---- Wire types ---------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    pub bytes: String,
    pub mac: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escrow_mac: Option<String>,
}

impl BlobWire {
    pub fn from_blob(b: &Blob) -> BlobWire {
        BlobWire {
            seq: None,
            bytes: b64(&b.bytes),
            mac: b64(&b.mac),
            chain: b.chain.map(|c| b64(&c)),
            escrow_mac: b.escrow_mac.map(|c| b64(&c)),
        }
    }

    pub fn to_blob(&self) -> SResult<Blob> {
        let t = |s: &str| -> SResult<[u8; 32]> { unb64(s)?.try_into().map_err(|_| SyncFail::BadAnswer("tag")) };
        Ok(Blob {
            bytes: unb64(&self.bytes)?,
            mac: t(&self.mac)?,
            chain: self.chain.as_deref().map(t).transpose()?,
            escrow_mac: self.escrow_mac.as_deref().map(t).transpose()?,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct HeadWire {
    pub ak_dir: BlobWire,
    pub pak_dir: BlobWire,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChainWire {
    pub ak: Vec<BlobWire>,
    pub pak: Vec<BlobWire>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Rotation {
    pub ak: bool,
    pub pak: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Rekey {
    pub remaining_old: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct QuotaWire {
    pub records: i64,
    pub bytes: i64,
    pub history: i64,
    pub max_records: i64,
    pub max_bytes: i64,
    pub max_history: i64,
}

/// `GET sync/state`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct StateWire {
    pub keyset_gen: Option<String>,
    pub current_epoch: u32,
    pub pak_epoch: u32,
    #[serde(default)]
    pub rr_gen: u32,
    pub rotation: Rotation,
    pub rekey: Rekey,
    pub head_seq: i64,
    pub pending_approvals: i64,
    pub quota: QuotaWire,
}

/// One entry of `GET devices`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeviceWire {
    pub id: String,
    pub short: u16,
    pub platform: String,
    pub label: String,
    pub client_version: String,
    #[serde(default)]
    pub last_used: Option<String>,
    #[serde(default)]
    pub last_sync: Option<String>,
    #[serde(default)]
    pub last_country: Option<String>,
    pub has_keys: bool,
    #[serde(default)]
    pub revoked_at: Option<String>,
    #[serde(rename = "self", default)]
    pub is_self: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApprovalDevice {
    pub id: String,
    pub label: String,
    pub platform: String,
    #[serde(default)]
    pub country: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApprovalItem {
    pub approval_id: String,
    pub state: String,
    pub device: ApprovalDevice,
    pub expires_in: i64,
    /// `"join"` (a new device) or `"pak"` (a device that syncs asks for the passwords key only;
    /// servers before 0.21.1 don't send it).
    #[serde(default)]
    pub kind: Option<String>,
}

impl ApprovalItem {
    pub fn wants_pak(&self) -> bool {
        self.kind.as_deref() == Some(keys::WANT_PAK)
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ApprovalView {
    state: String,
    device: ApprovalDevice,
    #[serde(default)]
    approver: Option<String>,
    #[serde(default, rename = "epk_A")]
    epk_a: Option<String>,
    #[serde(default, rename = "nonce_A")]
    nonce_a: Option<String>,
    #[serde(default, rename = "box_A")]
    box_a: Option<String>,
    #[serde(default, rename = "commit_N")]
    commit_n: Option<String>,
    #[serde(default, rename = "epk_N")]
    epk_n: Option<String>,
    #[serde(default, rename = "nonce_N")]
    nonce_n: Option<String>,
    #[serde(default, rename = "box_N")]
    box_n: Option<String>,
    #[serde(default, rename = "tag_N")]
    tag_n: Option<String>,
}

fn d32(s: &Option<String>) -> SResult<[u8; 32]> {
    let s = s.as_deref().ok_or(SyncFail::BadAnswer("missing field"))?;
    unb64(s)?.try_into().map_err(|_| SyncFail::BadAnswer("32 bytes"))
}

// ---- Simple calls -------------------------------------------------------------------------------

pub fn state(api: &Api) -> SResult<StateWire> {
    api.get("sync/state")
}

/// The head directories, or `None` when sync isn't set up on the account.
pub fn head(api: &Api) -> SResult<Option<(Blob, Blob)>> {
    match api.get::<HeadWire>("keys/head") {
        Ok(h) => Ok(Some((h.ak_dir.to_blob()?, h.pak_dir.to_blob()?))),
        Err(SyncFail::Api(ApiError::NotFound(c))) if c == "no_keyset" => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn devices(api: &Api) -> SResult<Vec<DeviceWire>> {
    api.get("devices")
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommitOk {
    pub ak_seq: Option<u64>,
    pub pak_seq: Option<u64>,
}

pub fn commit(api: &Api, c: &Commit) -> SResult<CommitOk> {
    let mut body = json!({});
    if let Some(b) = &c.ak_dir {
        body["ak_dir"] = serde_json::to_value(BlobWire::from_blob(b)).unwrap_or_default();
    }
    if let Some(b) = &c.pak_dir {
        body["pak_dir"] = serde_json::to_value(BlobWire::from_blob(b)).unwrap_or_default();
    }
    if c.if_absent {
        body["if_absent"] = true.into();
    }
    if let Some(h) = c.if_ak_prev_hash {
        body["if_ak_prev_hash"] = b64(&h).into();
    }
    if let Some(h) = c.if_pak_prev_hash {
        body["if_pak_prev_hash"] = b64(&h).into();
    }
    if let Some(e) = &c.escrow {
        body["escrow"] = serde_json::to_value(e).unwrap_or_default();
    }
    api.post("keys/commit", &body)
}

/// The state file of a device that just got its keys.
pub fn new_state(account_id: &str, me: &Me, j: &Joined, keyset_gen: String) -> SyncState {
    SyncState {
        account_id: account_id.into(),
        device_id: me.device_id.clone(),
        dev_short: me.short,
        device_sk: j.device_sk.clone(),
        ak: Some(j.ak.clone()),
        ak_old: j.ak_old.clone(),
        keyset_gen,
        esc_pk: Some(j.esc_pk),
        ..Default::default()
    }
}

/// This device's `short` as the server allocated it.
pub fn my_short(api: &Api, my_id: &str) -> SResult<u16> {
    devices(api)?
        .into_iter()
        .find(|d| d.id == my_id)
        .map(|d| d.short)
        .ok_or(SyncFail::BadAnswer("this device isn't listed"))
}

// ---- Set up, restore ----------------------------------------------------------------------------

/// The first device: commits the plan (the user has confirmed the passphrase and the recovery
/// key). A 409 means another device set up sync meanwhile: restore instead.
pub fn setup(api: &Api, plan: &SetupPlan) -> SResult<(Joined, String)> {
    let (c, j) = keys::setup_commit(plan, api.now_ms as u64)?;
    commit(api, &c)?;
    let gen = state(api)?.keyset_gen.ok_or(SyncFail::BadAnswer("keyset_gen"))?;
    Ok((j, gen))
}

/// A restore, up to the device list the user confirms: the escrow, the passphrase or recovery
/// key, the anchor and the chain to the heads.
pub fn restore_begin(api: &Api, secret: &Secret, account_id: AccountId) -> SResult<RestorePlan> {
    let escrow = fetch_escrow(api)?;
    restore_with(api, &escrow, secret, account_id)
}

/// `GET keys/escrow` on its own (rate-limited to 5 a day and mailed), so a caller can try
/// several secrets against one answer ([`restore_with`]; Android 0.6.0).
pub fn fetch_escrow(api: &Api) -> SResult<EscrowWire> {
    api.get("keys/escrow")
}

/// [`restore_begin`] with an escrow fetched before: a wrong secret fails here without a
/// request (`KeyError` from the commit check); the right one fetches the chain and opens it.
pub fn restore_with(api: &Api, escrow: &EscrowWire, secret: &Secret, account_id: AccountId) -> SResult<RestorePlan> {
    let rr = keys::unwrap_rr(escrow, secret, &account_id)?;
    let chain: ChainWire = api.get("keys/chain?from=anchor")?;
    let ak: Vec<Blob> = chain.ak.iter().map(BlobWire::to_blob).collect::<SResult<_>>()?;
    let pak: Vec<Blob> = chain.pak.iter().map(BlobWire::to_blob).collect::<SResult<_>>()?;
    Ok(keys::restore_open(rr, account_id, &ak, &pak)?)
}

pub fn restore_finish(api: &Api, plan: &RestorePlan, me: &Me, with_vault: bool) -> SResult<(Joined, String)> {
    let st = state(api)?;
    let j = if st.rotation.ak {
        // A removal waits for its rotation: this device joins and rotates in one commit
        // (PROTOCOL §7 step 8); the records are rekeyed by its first sync run.
        let active: Vec<String> = devices(api)?.into_iter().filter(|d| d.revoked_at.is_none()).map(|d| d.id).collect();
        let (c, j, _removed) =
            keys::restore_rotate_commit(plan, me, with_vault, &active, st.rotation.pak, api.now_ms as u64)?;
        commit(api, &c)?;
        j
    } else {
        let (c, j) = keys::restore_commit(plan, me, with_vault, api.now_ms as u64)?;
        commit(api, &c)?;
        j
    };
    let gen = state(api)?.keyset_gen.ok_or(SyncFail::BadAnswer("keyset_gen"))?;
    Ok((j, gen))
}

/// "Add passwords" on a device that syncs without the passwords key (Android 0.6.1): the
/// passphrase or recovery key opens the escrow fetched before (`fetch_escrow`), the account
/// directory found from the anchor must be the one this device pinned (anything else is a
/// fork), and this device's vault key joins `pak_dir`. The caller stores the returned state in
/// the unlocked vault. A wrong secret fails before any request.
pub fn add_passwords(
    api: &Api,
    st: &mut SyncState,
    escrow: &keys::EscrowWire,
    secret: &Secret,
) -> SResult<keys::PakState> {
    let acc = st.account_bytes().map_err(|_| SyncFail::BadAnswer("account id"))?;
    // `restore_with` checks the secret first: a wrong one costs no request and changes nothing.
    let plan = restore_with(api, escrow, secret, acc)?;
    if refresh(api, st, None)? == Refresh::SelfRevoked {
        return Err(SyncFail::Api(ApiError::Unauthorized));
    }
    let pin = st.ak.clone().ok_or(SyncFail::BadAnswer("no keys"))?;
    if plan.ak_head.hash != pin.hash || plan.ak_head.epoch != pin.epoch {
        return Err(dir::DirError::Fork.into());
    }
    let (c, pak) = keys::add_vault_from_restore(&plan, &st.device_id, api.now_ms as u64)?;
    commit(api, &c)?;
    Ok(pak)
}

/// "Finish securing" after a rotation: the passphrase or recovery key gives RR, the chain from
/// the last anchor must arrive at the heads this device pinned (anything else is a fork), and
/// both heads are committed again as a new anchor.
pub fn finish_securing(
    api: &Api,
    st: &mut SyncState,
    pak: Option<&mut keys::PakState>,
    secret: &Secret,
) -> SResult<()> {
    if refresh(api, st, None)? == Refresh::SelfRevoked {
        return Err(SyncFail::Api(ApiError::Unauthorized));
    }
    let acc = st.account_bytes().map_err(|_| SyncFail::BadAnswer("account id"))?;
    let plan = restore_begin(api, secret, acc)?;
    let pin = st.ak.clone().ok_or(SyncFail::BadAnswer("no keys"))?;
    if plan.ak_head.hash != pin.hash || plan.ak_head.epoch != pin.epoch {
        return Err(dir::DirError::Fork.into());
    }
    if let Some(p) = &pak {
        if plan.pak_head.hash != p.pak.hash {
            return Err(dir::DirError::Fork.into());
        }
    }
    let (c, (ak_seq, ak_hash), (pak_seq, pak_hash)) = keys::reanchor_commit(&plan, &st.device_id, api.now_ms as u64)?;
    commit(api, &c)?;
    if let Some(a) = st.ak.as_mut() {
        a.seq = ak_seq;
        a.hash = ak_hash;
    }
    if let Some(p) = pak {
        p.pak.seq = pak_seq;
        p.pak.hash = pak_hash;
    }
    st.secure_needed = false;
    Ok(())
}

// ---- Following the directories ------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refresh {
    Unchanged,
    Changed,
    /// This device was removed: wipe the sync keys and state (keep local data).
    SelfRevoked,
}

/// Brings the pinned heads up to the server's (PROTOCOL §4.4). A fork or a bad tag is an error
/// with `is_alarm()`.
pub fn refresh(api: &Api, st: &mut SyncState, pak: Option<&mut PakState>) -> SResult<Refresh> {
    let acc = st.account_bytes().map_err(|_| SyncFail::BadAnswer("account id"))?;
    let Some((ak_head, pak_head)) = head(api)? else { return Ok(Refresh::SelfRevoked) };
    let pin = st.ak.clone().ok_or(SyncFail::BadAnswer("no keys"))?;
    let ak_same = dir::hash(&ak_head.bytes) == pin.hash;
    let pak_same = pak.as_ref().is_none_or(|p| dir::hash(&pak_head.bytes) == p.pak.hash);
    if ak_same && pak_same {
        if ak_head.escrow_mac.is_some() {
            // Some device wrote a new anchor at this head ("Finish securing").
            st.secure_needed = false;
        }
        return Ok(Refresh::Unchanged);
    }
    let pak_seq = pak.as_ref().map_or(u64::MAX >> 1, |p| p.pak.seq);
    let chain: ChainWire = api.get(&format!("keys/chain?from={},{}", pin.seq, pak_seq))?;
    let me = Opener::Device { id: &st.device_id, sk: &st.device_sk };
    let blobs: Vec<Blob> = chain.ak.iter().map(BlobWire::to_blob).collect::<SResult<_>>()?;
    let mut old = st.ak_old.clone();
    match keys::advance_ak(pin.head(acc), &mut old, &blobs, &me)? {
        None => return Ok(Refresh::SelfRevoked),
        Some(h) => {
            // The walk must end at the head the server showed (different bytes at a pinned seq,
            // or a chain that doesn't reach the head, is a fork).
            if h.hash != dir::hash(&ak_head.bytes) {
                return Err(dir::DirError::Fork.into());
            }
            if h.epoch > pin.epoch {
                st.secure_needed = true;
            }
            if ak_head.escrow_mac.is_some() {
                st.secure_needed = false;
            }
            st.ak = Some(AkPin::from_head(&h));
            st.ak_old = old;
        }
    }
    if let Some(p) = pak {
        let blobs: Vec<Blob> = chain.pak.iter().map(BlobWire::to_blob).collect::<SResult<_>>()?;
        let vault_me = Opener::Device { id: &st.device_id, sk: &p.vault_sk };
        let mut old = p.pak_old.clone();
        match keys::advance_pak(p.pak.head(acc), &mut old, &blobs, &vault_me)? {
            // Removed from the vault keys only: passwords stop syncing on this device.
            None => return Ok(Refresh::SelfRevoked),
            Some(h) => {
                if h.hash != dir::hash(&pak_head.bytes) {
                    return Err(dir::DirError::Fork.into());
                }
                p.pak = keys::PakPin::from_head(&h);
                p.pak_old = old;
            }
        }
    }
    Ok(Refresh::Changed)
}

// ---- Approval: the new device -------------------------------------------------------------------

/// Where N's approval stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NStep {
    /// Waiting for a device with keys to answer.
    Waiting(String),
    /// The user types the 6 digits shown on the other device.
    EnterCode,
    /// The keys arrived: call [`NApproval::finish`].
    Ready,
    /// The approval was cancelled or expired.
    Gone,
}

pub struct NApproval {
    pub id: String,
    side: keys::NewSide,
    agreed: Option<keys::Agreed>,
    box_a: Option<Vec<u8>>,
}

impl NApproval {
    pub fn start(api: &Api, with_vault: bool) -> SResult<NApproval> {
        let side = keys::new_side(with_vault)?;
        #[derive(Deserialize)]
        struct Created {
            approval_id: String,
        }
        let c: Created = api.post("approvals", &json!({"commit_n": b64(&side.commit)}))?;
        Ok(NApproval { id: c.approval_id, side, agreed: None, box_a: None })
    }

    /// "Ask my PC" (Android 0.6.2): a device that syncs asks another for the passwords key, with
    /// its listed general key and a new vault key. Finish with [`NApproval::finish_pak`].
    pub fn start_pak(api: &Api, st: &SyncState) -> SResult<NApproval> {
        let side = keys::new_side_pak(&st.device_sk)?;
        #[derive(Deserialize)]
        struct Created {
            approval_id: String,
        }
        let c: Created =
            api.post("approvals", &json!({"commit_n": b64(&side.commit), "kind": keys::WANT_PAK}))?;
        Ok(NApproval { id: c.approval_id, side, agreed: None, box_a: None })
    }

    /// The passwords key arrived: checks it against `pak_dir` and adds this device's vault key.
    /// The caller keeps the returned state in the unlocked vault.
    pub fn finish_pak(&self, api: &Api, st: &SyncState) -> SResult<keys::PakState> {
        let ag = self.agreed.as_ref().ok_or(SyncFail::BadAnswer("not ready"))?;
        let box_a = self.box_a.as_ref().ok_or(SyncFail::BadAnswer("no keys yet"))?;
        let acc = st.account_bytes().map_err(|_| SyncFail::BadAnswer("account id"))?;
        let (_, pak) = head(api)?.ok_or(SyncFail::BadAnswer("no keyset"))?;
        let mut p = keys::n_accept_pak(ag, &self.side, box_a, acc, &pak, st.esc_pk)?;
        let v = self.side.vault.as_ref().ok_or(SyncFail::BadAnswer("no vault key"))?;
        let pd = dir::parse_pak(&pak.bytes)?;
        let (c, pin) = keys::add_vault_key(&p, &pd, acc, &st.device_id, &v.pk, api.now_ms as u64)?;
        commit(api, &c)?;
        p.pak = pin;
        Ok(p)
    }

    pub fn poll(&mut self, api: &Api, account_id: &AccountId, my_id: &str, name: &str) -> SResult<NStep> {
        let v: ApprovalView = match api.get(&format!("approvals/{}", self.id)) {
            Ok(v) => v,
            Err(SyncFail::Api(ApiError::NotFound(_))) => return Ok(NStep::Gone),
            Err(e) => return Err(e),
        };
        if v.state == "done" {
            self.box_a = Some(unb64(v.box_a.as_deref().ok_or(SyncFail::BadAnswer("box_A"))?)?);
            return Ok(NStep::Ready);
        }
        if v.state == "responded" && self.agreed.is_none() {
            let approver = v.approver.clone().ok_or(SyncFail::BadAnswer("approver"))?;
            let ctx = ApprovalCtx {
                account_id,
                approval_id: &self.id,
                dev_id_a: &approver,
                dev_id_n: my_id,
                n_label: &v.device.label,
                n_platform: &v.device.platform,
            };
            let (ag, bx) = keys::n_agree(&self.side, &ctx, &d32(&v.epk_a)?, &d32(&v.nonce_a)?, name)?;
            api.post_empty(
                &format!("approvals/{}/reveal", self.id),
                &json!({"epk_N": b64(&self.side.epk), "nonce_N": b64(&self.side.nonce), "box_N": b64(&bx)}),
            )?;
            self.agreed = Some(ag);
            return Ok(NStep::EnterCode);
        }
        if self.agreed.is_some() && v.state == "revealed" {
            return Ok(NStep::EnterCode);
        }
        Ok(NStep::Waiting(v.state))
    }

    /// The code the user typed. A mismatch cancels the approval (one attempt only).
    pub fn confirm(&mut self, api: &Api, typed: &str) -> SResult<()> {
        let ag = self.agreed.as_ref().ok_or(SyncFail::BadAnswer("not ready"))?;
        match keys::n_confirm(ag, typed) {
            Ok(tag) => api.post_empty(&format!("approvals/{}/confirm", self.id), &json!({"tag_N": b64(&tag)})),
            Err(e) => {
                let _ = api.post_empty(&format!("approvals/{}/cancel", self.id), &json!({}));
                Err(e.into())
            }
        }
    }

    pub fn cancel(&self, api: &Api) {
        let _ = api.post_empty(&format!("approvals/{}/cancel", self.id), &json!({}));
    }

    /// Checks the keys against the directories and, with PAK, adds this device's vault key.
    pub fn finish(&self, api: &Api, account_id: AccountId, my_id: &str) -> SResult<(Joined, keys::Payload)> {
        let ag = self.agreed.as_ref().ok_or(SyncFail::BadAnswer("not ready"))?;
        let box_a = self.box_a.as_ref().ok_or(SyncFail::BadAnswer("no keys yet"))?;
        let (ak, pak) = head(api)?.ok_or(SyncFail::BadAnswer("no keyset"))?;
        let (mut joined, payload) = keys::n_accept(ag, &self.side, box_a, account_id, my_id, &ak, &pak)?;
        if let (Some(p), Some(v)) = (joined.pak.as_mut(), self.side.vault.as_ref()) {
            let pd = dir::parse_pak(&pak.bytes)?;
            let (c, pin) = keys::add_vault_key(p, &pd, account_id, my_id, &v.pk, api.now_ms as u64)?;
            commit(api, &c)?;
            p.pak = pin;
        }
        Ok((joined, payload))
    }
}

// ---- Approval: the device with keys -------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AStep {
    Waiting(String),
    /// Show this code (and the device's name); the user types it on the new device.
    ShowCode(String),
    /// The new device confirmed: call [`AApproval::admit`].
    Confirmed,
    Gone,
}

pub struct AApproval {
    pub id: String,
    pub device: ApprovalDevice,
    side: keys::ApproverSide,
    agreed: Option<keys::Agreed>,
    n_box: Option<keys::BoxN>,
    confirmed: bool,
    /// The server listed the request as one for the passwords key only; `box_N` must agree.
    pub asked_pak: bool,
}

pub fn approvals(api: &Api) -> SResult<Vec<ApprovalItem>> {
    api.get("approvals")
}

impl AApproval {
    pub fn respond(api: &Api, item: &ApprovalItem) -> SResult<AApproval> {
        let side = keys::approver_side()?;
        api.post_empty(
            &format!("approvals/{}/respond", item.approval_id),
            &json!({"epk_A": b64(&side.epk), "nonce_A": b64(&side.nonce)}),
        )?;
        Ok(AApproval {
            id: item.approval_id.clone(),
            device: item.device.clone(),
            side,
            agreed: None,
            n_box: None,
            confirmed: false,
            asked_pak: item.wants_pak(),
        })
    }

    pub fn poll(&mut self, api: &Api, account_id: &AccountId, my_id: &str) -> SResult<AStep> {
        let v: ApprovalView = match api.get(&format!("approvals/{}", self.id)) {
            Ok(v) => v,
            Err(SyncFail::Api(ApiError::NotFound(_))) => return Ok(AStep::Gone),
            Err(e) => return Err(e),
        };
        if self.agreed.is_none() && (v.state == "revealed" || v.state == "confirmed") {
            let ctx = ApprovalCtx {
                account_id,
                approval_id: &self.id,
                dev_id_a: my_id,
                dev_id_n: &self.device.id,
                n_label: &v.device.label,
                n_platform: &v.device.platform,
            };
            let box_n = unb64(v.box_n.as_deref().ok_or(SyncFail::BadAnswer("box_N"))?)?;
            match keys::a_agree(&self.side, &ctx, &d32(&v.commit_n)?, &d32(&v.epk_n)?, &d32(&v.nonce_n)?, &box_n) {
                Ok((ag, b)) => {
                    self.agreed = Some(ag);
                    self.n_box = Some(b);
                }
                Err(e) => {
                    let _ = api.post_empty(&format!("approvals/{}/cancel", self.id), &json!({}));
                    return Err(e.into());
                }
            }
        }
        if v.state == "confirmed" {
            let ag = self.agreed.as_ref().ok_or(SyncFail::BadAnswer("not agreed"))?;
            let tag = d32(&v.tag_n)?;
            if let Err(e) = keys::a_check_tag(ag, &tag) {
                let _ = api.post_empty(&format!("approvals/{}/cancel", self.id), &json!({}));
                return Err(e.into());
            }
            self.confirmed = true;
            return Ok(AStep::Confirmed);
        }
        match &self.agreed {
            Some(ag) => Ok(AStep::ShowCode(ag.sas_text())),
            None => Ok(AStep::Waiting(v.state)),
        }
    }

    pub fn cancel(&self, api: &Api) {
        let _ = api.post_empty(&format!("approvals/{}/cancel", self.id), &json!({}));
    }

    /// Whether the confirmed device asked for the passwords key only (then [`Self::admit_pak`]).
    pub fn wants_pak(&self) -> bool {
        self.n_box.as_ref().is_some_and(|b| b.want.is_some())
    }

    /// Hands the passwords key to a device that already syncs ("Ask my PC"). Commits nothing:
    /// the device adds its own vault key. `pak`: this device's PAK (the vault is unlocked).
    pub fn admit_pak(&self, api: &Api, st: &SyncState, pak: &keys::PakPin) -> SResult<()> {
        if !self.confirmed {
            return Err(SyncFail::BadAnswer("not confirmed"));
        }
        if !self.asked_pak {
            return Err(SyncFail::BadAnswer("the device asked to join"));
        }
        let ag = self.agreed.as_ref().ok_or(SyncFail::BadAnswer("not agreed"))?;
        let nb = self.n_box.as_ref().ok_or(SyncFail::BadAnswer("box_N"))?;
        let pin = st.ak.clone().ok_or(SyncFail::BadAnswer("no keys"))?;
        let (ak_blob, pak_blob) = head(api)?.ok_or(SyncFail::BadAnswer("no keyset"))?;
        if dir::hash(&ak_blob.bytes) != pin.hash || dir::hash(&pak_blob.bytes) != pak.hash {
            return Err(SyncFail::BadAnswer("the keys changed: refresh first"));
        }
        let head_dir = dir::parse_ak(&ak_blob.bytes)?;
        let pd = dir::parse_pak(&pak_blob.bytes)?;
        let esc_pk: [u8; 32] = unb64(&pd.esc_pk)?.try_into().map_err(|_| SyncFail::BadAnswer("esc_pk"))?;
        let box_a = keys::a_share_pak(ag, &head_dir, &self.device.id, nb, pak, pak.hash, esc_pk)?;
        api.post_empty(&format!("approvals/{}/keys", self.id), &json!({"box_A": b64(&box_a)}))?;
        Ok(())
    }

    /// Adds the new device to `ak_dir` and hands it the keys. `pak`: share passwords too (the
    /// caller asked for the master password for this). Returns the new pin.
    pub fn admit(&self, api: &Api, st: &SyncState, pak: Option<&keys::PakPin>) -> SResult<AkPin> {
        if !self.confirmed {
            return Err(SyncFail::BadAnswer("not confirmed"));
        }
        if self.asked_pak || self.wants_pak() {
            return Err(SyncFail::BadAnswer("the device asked for the passwords key only"));
        }
        let ag = self.agreed.as_ref().ok_or(SyncFail::BadAnswer("not agreed"))?;
        let nb = self.n_box.as_ref().ok_or(SyncFail::BadAnswer("box_N"))?;
        let acc = st.account_bytes().map_err(|_| SyncFail::BadAnswer("account id"))?;
        let pin = st.ak.clone().ok_or(SyncFail::BadAnswer("no keys"))?;
        let (ak_blob, pak_blob) = head(api)?.ok_or(SyncFail::BadAnswer("no keyset"))?;
        if dir::hash(&ak_blob.bytes) != pin.hash {
            return Err(SyncFail::BadAnswer("the keys changed: refresh first"));
        }
        let head_dir = dir::parse_ak(&ak_blob.bytes)?;
        let pd = dir::parse_pak(&pak_blob.bytes)?;
        let esc_pk: [u8; 32] = unb64(&pd.esc_pk)?.try_into().map_err(|_| SyncFail::BadAnswer("esc_pk"))?;
        let n_short = devices(api)?
            .into_iter()
            .find(|d| d.id == self.device.id)
            .map(|d| d.short)
            .ok_or(SyncFail::BadAnswer("device"))?;
        let (c, box_a, _) = keys::a_admit(
            ag,
            &pin.head(acc),
            &head_dir,
            dir::hash(&pak_blob.bytes),
            esc_pk,
            &st.device_id,
            &self.device.id,
            n_short,
            nb,
            pak,
            api.now_ms as u64,
        )?;
        commit(api, &c)?;
        api.post_empty(&format!("approvals/{}/keys", self.id), &json!({"box_A": b64(&box_a)}))?;
        #[allow(clippy::expect_used)] // a_admit always sets ak_dir
        let blob = c.ak_dir.expect("ak_dir");
        Ok(AkPin { seq: pin.seq + 1, hash: dir::hash(&blob.bytes), ..pin })
    }
}
