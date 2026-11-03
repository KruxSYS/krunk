//! Safe wrappers over the WebView2 COM interfaces used by the browser.

use super::loader::Loader;
use super::window::Window;
use std::path::Path;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2Controller, ICoreWebView2Environment, ICoreWebView2EnvironmentOptions,
    ICoreWebView2Settings4,
};
use webview2_com::{
    CoreWebView2EnvironmentOptions, CreateCoreWebView2ControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler,
};
use windows::core::{Interface, HSTRING};
use windows::Win32::Foundation::RECT;

/// Options for the single shared environment.
pub struct EnvironmentConfig {
    /// `AdditionalBrowserArguments` (from `Settings::browser_arguments`).
    pub browser_arguments: String,
    pub tracking_prevention: bool,
}

/// The shared `CoreWebView2Environment` (one per process).
#[derive(Clone)]
pub struct Environment(ICoreWebView2Environment);

impl Environment {
    /// Starts creating the environment; `done` runs on the UI thread.
    pub fn create(
        loader: &Loader,
        user_data_dir: &Path,
        config: &EnvironmentConfig,
        done: impl FnOnce(Result<Environment, String>) + 'static,
    ) -> Result<(), String> {
        let options = CoreWebView2EnvironmentOptions::default();
        // SAFETY: the setters write to the options object before it is shared
        // with WebView2; nothing else holds a reference yet.
        unsafe {
            options.set_additional_browser_arguments(config.browser_arguments.clone());
            options.set_enable_tracking_prevention(config.tracking_prevention);
            options.set_are_browser_extensions_enabled(false);
            options.set_allow_single_sign_on_using_os_primary_account(false);
        }
        let options: ICoreWebView2EnvironmentOptions = options.into();
        let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(move |result, env| {
            done(match (result, env) {
                (Ok(()), Some(env)) => Ok(Environment(env)),
                (Err(e), _) => Err(format!("WebView2 environment creation failed: {e}")),
                (Ok(()), None) => Err("WebView2 returned no environment".into()),
            });
            Ok(())
        }));
        loader.create_environment(user_data_dir, &options, &handler)
    }

    /// Starts creating a controller hosted in `window`.
    pub fn create_controller(
        &self,
        window: Window,
        done: impl FnOnce(Result<Controller, String>) + 'static,
    ) -> Result<(), String> {
        let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |result, controller| {
            done(match (result, controller) {
                (Ok(()), Some(c)) => Controller::wrap(c),
                (Err(e), _) => Err(format!("WebView2 controller creation failed: {e}")),
                (Ok(()), None) => Err("WebView2 returned no controller".into()),
            });
            Ok(())
        }));
        // SAFETY: valid environment and window handle on the UI thread.
        unsafe { self.0.CreateCoreWebView2Controller(window.hwnd(), &handler) }
            .map_err(|e| format!("CreateCoreWebView2Controller failed: {e}"))
    }
}

/// One WebView (controller + its `CoreWebView2`).
#[derive(Clone)]
pub struct Controller {
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
}

impl Controller {
    fn wrap(controller: ICoreWebView2Controller) -> Result<Controller, String> {
        // SAFETY: a freshly created controller always has a CoreWebView2.
        let webview = unsafe { controller.CoreWebView2() }.map_err(|e| e.to_string())?;
        let c = Controller { controller, webview };
        c.apply_base_settings();
        Ok(c)
    }

    /// Settings every WebView gets: no built-in autofill or password saving
    /// (we have our own vault), no status bar.
    fn apply_base_settings(&self) {
        // SAFETY: COM calls on a live WebView; failures only mean an older
        // runtime without the setting, which we ignore.
        unsafe {
            if let Ok(s) = self.webview.Settings() {
                let _ = s.SetIsStatusBarEnabled(false);
                let _ = s.SetAreDevToolsEnabled(true);
                if let Ok(s4) = s.cast::<ICoreWebView2Settings4>() {
                    let _ = s4.SetIsGeneralAutofillEnabled(false);
                    let _ = s4.SetIsPasswordAutosaveEnabled(false);
                }
            }
        }
    }

    pub fn set_bounds(&self, left: i32, top: i32, width: i32, height: i32) {
        let r = RECT { left, top, right: left + width.max(0), bottom: top + height.max(0) };
        // SAFETY: live controller.
        let _ = unsafe { self.controller.SetBounds(r) };
    }

    pub fn set_visible(&self, visible: bool) {
        // SAFETY: live controller.
        let _ = unsafe { self.controller.SetIsVisible(visible) };
    }

    pub fn notify_parent_moved(&self) {
        // SAFETY: live controller.
        let _ = unsafe { self.controller.NotifyParentWindowPositionChanged() };
    }

    pub fn navigate(&self, url: &str) -> Result<(), String> {
        // SAFETY: live WebView; HSTRING outlives the call.
        unsafe { self.webview.Navigate(&HSTRING::from(url)) }.map_err(|e| e.to_string())
    }

    pub fn close(&self) {
        // SAFETY: live controller; closing twice is harmless.
        let _ = unsafe { self.controller.Close() };
    }
}
