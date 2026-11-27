//! Casting (0.30.0): what the engine's Media Router bridge tells the host, and what the device
//! picker shows and starts.
//!
//! The engine side is `KrunkCastController` (scripts/cef-build/patches/krunk-cast-bridge.patch):
//! it keeps Chromium's own cast model (sinks, cast modes, routes) for a tab and sends it to the
//! host as browser-process messages ([`MSG_OPEN`], [`MSG_MODEL`], [`MSG_STATE`], [`MSG_CLOSED`],
//! one JSON argument each). The host answers with the `krunk.cast` engine preference
//! ([`Command`]). Everything here is pure: parsing, the picker's view, the start decision.

use serde::{Deserialize, Serialize};

/// The picker should open (a page asked, or the host's `open`), with the first model.
pub const MSG_OPEN: &str = "krunk.cast.open";
/// The devices changed.
pub const MSG_MODEL: &str = "krunk.cast.model";
/// This tab's session changed (casting or not, the device, the media status).
pub const MSG_STATE: &str = "krunk.cast.state";
/// The engine ended the picker session (the tab navigated, closed or crashed).
pub const MSG_CLOSED: &str = "krunk.cast.closed";
/// The engine preference the host sends commands with.
pub const PREF: &str = "krunk.cast";

/// Whether a process message is one of the cast bridge's. They are only believed from the
/// browser process: a renderer could send a message with the same name.
pub fn accept_message(name: &str, from_browser_process: bool) -> bool {
    from_browser_process && matches!(name, MSG_OPEN | MSG_MODEL | MSG_STATE | MSG_CLOSED)
}

/// How a device can be used for this tab (Chromium's `MediaCastMode`; desktop mirroring is
/// not offered by Krunk and dropped by the engine).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CastMode {
    /// The site's own Cast integration (Presentation API, e.g. YouTube's receiver app).
    Presentation,
    /// Mirror the tab (picture and sound).
    Tab,
    /// The page's playing video through media remoting.
    Video,
    /// The whole screen (never started by Krunk).
    Desktop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SinkState {
    #[default]
    Available,
    Connecting,
    Connected,
    Disconnecting,
    Unavailable,
}

/// A device as the engine reports it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Sink {
    pub id: String,
    pub name: String,
    /// cast | speaker | speakerGroup | display | generic
    pub icon: String,
    /// cast | dial | wired | other
    pub provider: String,
    pub state: SinkState,
    /// The engine's own status line ("Casting YouTube", "Source not supported", …).
    pub status: String,
    pub modes: Vec<CastMode>,
    pub route: Option<String>,
    pub route_mode: Option<CastMode>,
    pub route_local: bool,
    pub issue: Option<String>,
}

/// Why the page's video can't be cast on its own ("" when it can).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct VideoEligibility {
    pub castable: bool,
    /// "" (castable) | none (no playing video) | disabled (`disableRemotePlayback`, or several
    /// players) | drm (encrypted media) | codec (not remotable) | site (the site's own Cast
    /// integration casts it)
    pub reason: String,
}

/// The page's media while it is cast.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Media {
    pub title: String,
    pub playing: bool,
    pub can_play_pause: bool,
    pub can_seek: bool,
    pub can_volume: bool,
    pub can_mute: bool,
    pub volume: f64,
    pub muted: bool,
    pub time: f64,
    pub duration: f64,
}

/// A tab's session ([`MSG_STATE`], also inside every model).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct TabState {
    pub casting: bool,
    pub route: String,
    /// The device's name.
    pub sink: String,
    pub mode: Option<CastMode>,
    pub connecting: bool,
    pub media: Media,
}

/// The picker's model ([`MSG_OPEN`] / [`MSG_MODEL`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Model {
    /// CEF's browser identifier of the tab.
    pub browser: i32,
    /// page (the site asked: its cast button) | menu
    pub trigger: String,
    pub private: bool,
    pub video: VideoEligibility,
    /// The site the PRESENTATION mode is for (Chromium's dialog header, e.g. "youtube.com").
    pub header: String,
    pub sinks: Vec<Sink>,
    pub state: TabState,
}

/// Parses a model or a state message's JSON (lenient: unknown fields are ignored).
pub fn parse_model(json: &str) -> Option<Model> {
    serde_json::from_str(json).ok()
}

pub fn parse_state(json: &str) -> Option<TabState> {
    serde_json::from_str(json).ok()
}

/// What the user picks in the picker's source switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    /// "This tab": mirroring.
    Tab,
    /// "Video on this page".
    Video,
    /// The site's own Cast app (offered when the site asked, or has a Cast integration).
    Site,
}

/// The status a device shows in the picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Available,
    Connecting,
    /// Casting this tab (or its video).
    Casting,
    /// Busy with something else (another tab, another app, another sender).
    Busy,
    Unavailable,
}

pub fn sink_status(sink: &Sink, tab: &TabState) -> Status {
    match sink.state {
        SinkState::Available => Status::Available,
        SinkState::Connecting | SinkState::Disconnecting => Status::Connecting,
        SinkState::Unavailable => Status::Unavailable,
        SinkState::Connected => {
            if tab.casting && sink.route.as_deref() == Some(tab.route.as_str()) {
                Status::Casting
            } else {
                Status::Busy
            }
        }
    }
}

/// What a click on a device does for a source: the mode to start and, when the choice can't
/// be honoured as asked, the notice to show (the tab is mirrored instead).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub mode: CastMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
}

fn fallback_notice(reason: &str, device: &str) -> String {
    let why = match reason {
        "drm" => "This video is copy-protected (DRM) and can't be cast on its own".to_string(),
        "disabled" => "This page doesn't let its video be cast on its own".to_string(),
        "codec" => "This video's format can't be cast on its own".to_string(),
        "none" => "No playing video was found on this page".to_string(),
        _ => format!("{device} can't play this video on its own"),
    };
    format!("{why} — casting the whole tab instead.")
}

/// The start decision for `source` on `sink`; `None` when the device can do neither.
///
/// - Tab: mirroring.
/// - Video: media remoting when the engine found a remotable video and the device takes it;
///   the site's own receiver when the site has a Cast integration (`reason == "site"`);
///   otherwise mirroring with a notice (DRM, `disableRemotePlayback`, an unsupported format,
///   a device that can't play it).
pub fn plan(source: Source, sink: &Sink, video: &VideoEligibility) -> Option<Plan> {
    let has = |m: CastMode| sink.modes.contains(&m);
    let mirror = |notice: Option<String>| has(CastMode::Tab).then_some(Plan { mode: CastMode::Tab, notice });
    match source {
        Source::Tab => mirror(None),
        Source::Site => site_plan(sink),
        Source::Video => {
            if video.castable && has(CastMode::Video) {
                return Some(Plan { mode: CastMode::Video, notice: None });
            }
            if video.reason == "site" && has(CastMode::Presentation) {
                return Some(Plan { mode: CastMode::Presentation, notice: None });
            }
            let reason = if video.castable { "device" } else { video.reason.as_str() };
            mirror(Some(fallback_notice(reason, &sink.name)))
        }
    }
}

/// The site's own Cast app (a page-initiated request with no mirroring offered, e.g. a DIAL
/// device for YouTube): started as PRESENTATION.
pub fn site_plan(sink: &Sink) -> Option<Plan> {
    sink.modes.contains(&CastMode::Presentation).then_some(Plan { mode: CastMode::Presentation, notice: None })
}

/// One device in the picker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SinkView {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub provider: String,
    pub status: Status,
    /// The line under the name.
    pub status_text: String,
    /// What a click starts per source (missing: not possible on this device).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab: Option<Plan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video: Option<Plan>,
    /// The site's own app (page-initiated requests).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site: Option<Plan>,
    /// The route the Stop button ends (a local session on this device).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<String>,
}

/// The picker's whole view (the popup content and the `cast` event).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct View {
    /// The tab the picker is for (the host's id).
    pub tab_id: u32,
    pub private: bool,
    /// The site asked (its cast button): its own app is offered first.
    pub from_page: bool,
    /// The site's name for its own Cast app.
    pub site: String,
    /// Offer "Video on this page" (the page has a video; see `plan` for what it does).
    pub offer_video: bool,
    /// Preselected source.
    pub source: Option<Source>,
    pub devices: Vec<SinkView>,
    pub state: TabState,
    /// The engine has not answered yet (shown as "Looking for devices…").
    pub searching: bool,
    /// The engine refused (casting turned off and not restarted yet, or no cast bridge).
    pub unavailable: bool,
}

fn status_text(status: Status, sink: &Sink, tab: &TabState) -> String {
    if let Some(issue) = sink.issue.as_deref().filter(|i| !i.is_empty()) {
        return issue.to_string();
    }
    match status {
        Status::Available => {
            if sink.status.is_empty() {
                "Available".into()
            } else {
                sink.status.clone()
            }
        }
        Status::Connecting => {
            if sink.state == SinkState::Disconnecting {
                "Stopping…".into()
            } else {
                "Connecting…".into()
            }
        }
        Status::Casting => match tab.mode {
            Some(CastMode::Video) => "Casting this video".into(),
            Some(CastMode::Presentation) => {
                if sink.status.is_empty() {
                    "Casting".into()
                } else {
                    sink.status.clone()
                }
            }
            _ => "Casting this tab".into(),
        },
        Status::Busy => {
            if sink.status.is_empty() {
                "Busy".into()
            } else {
                sink.status.clone()
            }
        }
        Status::Unavailable => {
            if sink.status.is_empty() {
                "Not available".into()
            } else {
                sink.status.clone()
            }
        }
    }
}

/// Builds the picker view from the engine's model. `page_has_video` comes from the host's
/// own video tracking (the Picture-in-Picture list): the engine's media session only knows
/// about playing media.
pub fn view(tab_id: u32, model: &Model, page_has_video: bool) -> View {
    let tab = &model.state;
    let devices = model
        .sinks
        .iter()
        .filter(|s| s.modes.iter().any(|m| *m != CastMode::Desktop) || s.route.is_some())
        .map(|s| {
            let status = sink_status(s, tab);
            SinkView {
                id: s.id.clone(),
                name: s.name.clone(),
                icon: s.icon.clone(),
                provider: s.provider.clone(),
                status,
                status_text: status_text(status, s, tab),
                tab: plan(Source::Tab, s, &model.video),
                video: plan(Source::Video, s, &model.video),
                site: site_plan(s),
                stop: s.route.clone().filter(|_| s.route_local && s.state != SinkState::Disconnecting),
            }
        })
        .collect();
    let offer_video = model.video.castable || model.video.reason != "none" || page_has_video;
    let source = Some(if model.trigger == "page" || !model.video.castable { Source::Tab } else { Source::Video });
    View {
        tab_id,
        private: model.private,
        from_page: model.trigger == "page",
        site: model.header.clone(),
        offer_video,
        source,
        devices,
        state: tab.clone(),
        searching: false,
        unavailable: false,
    }
}

/// The toolbar's cast button: shown while some tab of the window casts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ButtonView {
    pub show: bool,
    /// The device ("Casting to Living Room TV").
    pub sink: String,
    /// The casting tab is the window's active tab.
    pub active_tab: bool,
}

/// A media control from the picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaAction {
    Play,
    Pause,
    Seek,
    Volume,
    Mute,
}

impl MediaAction {
    pub fn name(self) -> &'static str {
        match self {
            MediaAction::Play => "play",
            MediaAction::Pause => "pause",
            MediaAction::Seek => "seek",
            MediaAction::Volume => "volume",
            MediaAction::Mute => "mute",
        }
    }

    /// Clamps the value the page sent: seconds ≥ 0 for seek, 0..1 for volume, 0/1 for mute.
    pub fn clamp(self, value: f64) -> f64 {
        let v = if value.is_finite() { value } else { 0.0 };
        match self {
            MediaAction::Seek => v.max(0.0),
            MediaAction::Volume => v.clamp(0.0, 1.0),
            MediaAction::Mute => f64::from(u8::from(v != 0.0)),
            MediaAction::Play | MediaAction::Pause => 0.0,
        }
    }
}

/// A command to the engine (the `krunk.cast` preference's dictionary).
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Open,
    Start { sink: String, mode: CastMode },
    Stop { route: String },
    Close,
    Media { action: MediaAction, value: f64 },
}

impl Command {
    /// The dictionary's string and number entries (besides `browser`).
    pub fn fields(&self) -> (Vec<(&'static str, String)>, Option<f64>) {
        match self {
            Command::Open => (vec![("cmd", "open".into())], None),
            Command::Close => (vec![("cmd", "close".into())], None),
            Command::Start { sink, mode } => {
                let mode = match mode {
                    CastMode::Presentation => "presentation",
                    CastMode::Tab => "tab",
                    CastMode::Video => "video",
                    CastMode::Desktop => "desktop",
                };
                (vec![("cmd", "start".into()), ("sink", sink.clone()), ("mode", mode.into())], None)
            }
            Command::Stop { route } => (vec![("cmd", "stop".into()), ("route", route.clone())], None),
            Command::Media { action, value } => {
                (vec![("cmd", "media".into()), ("action", action.name().into())], Some(action.clamp(*value)))
            }
        }
    }
}

/// Error text when the engine refused a command.
pub const NOT_AVAILABLE: &str = "Casting is not available (turned off in Settings, or the engine can't cast)";

#[cfg(test)]
mod tests {
    use super::*;

    const MODEL: &str = r#"{"browser":7,"trigger":"menu","private":false,
        "video":{"castable":true,"reason":""},"header":"example.com",
        "sinks":[
          {"id":"cast:1","name":"Living Room TV","icon":"cast","provider":"cast","state":"available",
           "status":"","modes":["presentation","tab","video"]},
          {"id":"cast:2","name":"Kitchen speaker","icon":"speaker","provider":"cast","state":"available",
           "status":"","modes":["tab"]},
          {"id":"dial:3","name":"Old TV","icon":"generic","provider":"dial","state":"connected",
           "status":"YouTube","modes":["presentation"],"route":"r9","routeMode":"presentation","routeLocal":false},
          {"id":"cast:4","name":"Desk","icon":"cast","provider":"cast","state":"connected",
           "status":"Casting tab","modes":["tab"],"route":"r1","routeMode":"tab","routeLocal":true},
          {"id":"cast:5","name":"Screen only","icon":"cast","provider":"cast","state":"available","modes":["desktop"]}
        ],
        "state":{"casting":true,"route":"r1","sink":"Desk","mode":"tab","media":{"playing":true}},
        "extra":"ignored"}"#;

    #[test]
    fn only_browser_process_messages_are_believed() {
        for name in [MSG_OPEN, MSG_MODEL, MSG_STATE, MSG_CLOSED] {
            assert!(accept_message(name, true), "{name}");
            assert!(!accept_message(name, false), "a renderer must not fake {name}");
        }
        assert!(!accept_message("krunk.cast.other", true));
        assert!(!accept_message("lb-hello", true));
    }

    #[test]
    fn model_parses_leniently() {
        let m = parse_model(MODEL).unwrap();
        assert_eq!(m.browser, 7);
        assert_eq!(m.sinks.len(), 5);
        assert_eq!(m.sinks[0].modes, vec![CastMode::Presentation, CastMode::Tab, CastMode::Video]);
        assert_eq!(m.sinks[3].route.as_deref(), Some("r1"));
        assert!(m.state.casting && m.state.media.playing);
        assert_eq!(m.state.mode, Some(CastMode::Tab));
        // Empty and partial messages give defaults, garbage gives None.
        assert_eq!(parse_model("{}").unwrap(), Model::default());
        assert!(parse_model("not json").is_none());
        let s = parse_state(r#"{"casting":false,"browser":7}"#).unwrap();
        assert!(!s.casting);
    }

    #[test]
    fn statuses() {
        let m = parse_model(MODEL).unwrap();
        let st: Vec<Status> = m.sinks.iter().map(|s| sink_status(s, &m.state)).collect();
        assert_eq!(st[0], Status::Available);
        assert_eq!(st[2], Status::Busy, "another sender's YouTube session");
        assert_eq!(st[3], Status::Casting, "this tab's route");
        // The same route seen from another tab is busy.
        let other = TabState::default();
        assert_eq!(sink_status(&m.sinks[3], &other), Status::Busy);
        let connecting = Sink { state: SinkState::Connecting, ..Sink::default() };
        assert_eq!(sink_status(&connecting, &other), Status::Connecting);
        let gone = Sink { state: SinkState::Unavailable, ..Sink::default() };
        assert_eq!(sink_status(&gone, &other), Status::Unavailable);
    }

    #[test]
    fn video_plans_and_fallbacks() {
        let m = parse_model(MODEL).unwrap();
        let tv = &m.sinks[0];
        let speaker = &m.sinks[1];
        // A remotable video on a device that takes it: remoting, no notice.
        assert_eq!(plan(Source::Video, tv, &m.video), Some(Plan { mode: CastMode::Video, notice: None }));
        // The speaker can't play the video: the tab with a notice naming it.
        let p = plan(Source::Video, speaker, &m.video).unwrap();
        assert_eq!(p.mode, CastMode::Tab);
        assert!(p.notice.as_deref().unwrap().starts_with("Kitchen speaker can't play this video"), "{p:?}");
        // DRM / disableRemotePlayback / format: the tab, with the reason.
        for (reason, text) in [("drm", "DRM"), ("disabled", "doesn't let"), ("codec", "format"), ("none", "No playing")]
        {
            let v = VideoEligibility { castable: false, reason: reason.into() };
            let p = plan(Source::Video, tv, &v).unwrap();
            assert_eq!(p.mode, CastMode::Tab, "{reason}");
            assert!(p.notice.as_deref().unwrap().contains(text), "{reason}: {p:?}");
            assert!(p.notice.as_deref().unwrap().ends_with("casting the whole tab instead."));
        }
        // The site's own Cast integration casts its video.
        let site = VideoEligibility { castable: false, reason: "site".into() };
        assert_eq!(plan(Source::Video, tv, &site), Some(Plan { mode: CastMode::Presentation, notice: None }));
        // "This tab" is mirroring; a presentation-only DIAL device can't mirror.
        assert_eq!(plan(Source::Tab, tv, &m.video), Some(Plan { mode: CastMode::Tab, notice: None }));
        assert_eq!(plan(Source::Tab, &m.sinks[2], &m.video), None);
        assert_eq!(plan(Source::Video, &m.sinks[2], &VideoEligibility::default()), None);
        assert_eq!(site_plan(&m.sinks[2]), Some(Plan { mode: CastMode::Presentation, notice: None }));
        assert_eq!(site_plan(speaker), None);
    }

    #[test]
    fn picker_view() {
        let m = parse_model(MODEL).unwrap();
        let v = view(3, &m, false);
        assert_eq!(v.tab_id, 3);
        assert_eq!(v.devices.len(), 4, "a desktop-only device is not listed");
        assert!(v.offer_video);
        assert_eq!(v.source, Some(Source::Video), "menu + remotable video: video preselected");
        assert_eq!(v.devices[3].status, Status::Casting);
        assert_eq!(v.devices[3].status_text, "Casting this tab");
        assert_eq!(v.devices[3].stop.as_deref(), Some("r1"));
        assert_eq!(v.devices[2].stop, None, "another sender's session is not ours to stop");
        assert_eq!(v.devices[2].status_text, "YouTube");
        assert_eq!(v.devices[0].status_text, "Available");
        assert_eq!(v.site, "example.com");
        // Page request: the tab first, the page's own app offered.
        let page = Model { trigger: "page".into(), ..m.clone() };
        let v = view(3, &page, false);
        assert!(v.from_page);
        assert_eq!(v.source, Some(Source::Tab));
        // No video at all: the video source is not offered unless the host saw one.
        let none = Model { video: VideoEligibility { castable: false, reason: "none".into() }, ..m.clone() };
        assert!(!view(3, &none, false).offer_video);
        assert!(view(3, &none, true).offer_video);
        // A DRM video is offered (it falls back to the tab, with a notice).
        let drm = Model { video: VideoEligibility { castable: false, reason: "drm".into() }, ..m.clone() };
        let v = view(3, &drm, false);
        assert!(v.offer_video);
        assert_eq!(v.source, Some(Source::Tab));
        assert!(v.devices[0].video.as_ref().unwrap().notice.is_some());
        // Private tabs are marked.
        let private = Model { private: true, ..m };
        assert!(view(3, &private, false).private);
        // Issues win over the status.
        let s = Sink { issue: Some("Could not connect".into()), ..Sink::default() };
        assert_eq!(status_text(Status::Available, &s, &TabState::default()), "Could not connect");
        // The view serializes in camelCase with the plans.
        let j = serde_json::to_value(view(3, &parse_model(MODEL).unwrap(), false)).unwrap();
        assert_eq!(j["devices"][0]["video"]["mode"], "video");
        assert_eq!(j["devices"][1]["statusText"], "Available");
        assert_eq!(j["offerVideo"], true);
    }

    #[test]
    fn commands() {
        let (f, n) = Command::Start { sink: "cast:1".into(), mode: CastMode::Video }.fields();
        assert_eq!(f, vec![("cmd", "start".to_string()), ("sink", "cast:1".into()), ("mode", "video".into())]);
        assert_eq!(n, None);
        let (f, n) = Command::Media { action: MediaAction::Volume, value: 3.0 }.fields();
        assert_eq!(f[1], ("action", "volume".to_string()));
        assert_eq!(n, Some(1.0), "volume clamped");
        assert_eq!(MediaAction::Seek.clamp(-5.0), 0.0);
        assert_eq!(MediaAction::Seek.clamp(f64::NAN), 0.0);
        assert_eq!(MediaAction::Mute.clamp(0.4), 1.0);
        assert_eq!(Command::Stop { route: "r1".into() }.fields().0[1], ("route", "r1".to_string()));
        assert_eq!(Command::Open.fields().0, vec![("cmd", "open".to_string())]);
        assert_eq!(Command::Close.fields().0, vec![("cmd", "close".to_string())]);
    }
}
