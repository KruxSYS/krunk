//! Browser state and lifecycle. Milestone 2: one window with one WebView.

use crate::ffi::dialogs;
use crate::ffi::loader::Loader;
use crate::ffi::webview::{Controller, Environment, EnvironmentConfig};
use crate::ffi::window::{Window, WindowEvents};
use crate::log::{info, warning};
use crate::paths::DataDirs;
use lb_core::settings::{Settings, TrackingPrevention};
use std::cell::{OnceCell, RefCell};
use std::rc::Rc;

pub struct Browser {
    window: OnceCell<Window>,
    settings: Settings,
    /// For the About page.
    runtime_version: String,
    environment: RefCell<Option<Environment>>,
    controller: RefCell<Option<Controller>>,
}

impl Browser {
    /// Opens the window and starts creating the WebView2 environment.
    pub fn start(
        loader: &Loader,
        dirs: &DataDirs,
        settings: Settings,
        runtime_version: String,
    ) -> Result<Rc<Browser>, String> {
        let browser = Rc::new(Browser {
            window: OnceCell::new(),
            settings,
            runtime_version,
            environment: RefCell::new(None),
            controller: RefCell::new(None),
        });
        let window = Window::create("LightBrowser", browser.clone())?;
        let _ = browser.window.set(window);

        let config = EnvironmentConfig {
            browser_arguments: browser.settings.browser_arguments(),
            tracking_prevention: browser.settings.tracking_prevention != TrackingPrevention::None,
        };
        info!("browser arguments: {}", config.browser_arguments);
        let b = browser.clone();
        Environment::create(loader, &dirs.profile(), &config, move |result| match result {
            Ok(env) => b.environment_ready(env),
            Err(e) => b.fatal(&e),
        })?;
        Ok(browser)
    }

    fn window(&self) -> Option<Window> {
        self.window.get().copied()
    }

    fn environment_ready(self: &Rc<Self>, env: Environment) {
        info!("WebView2 environment created");
        let Some(window) = self.window() else { return };
        let b = self.clone();
        let started = env.create_controller(window, move |result| match result {
            Ok(c) => b.controller_ready(c),
            Err(e) => b.fatal(&e),
        });
        *self.environment.borrow_mut() = Some(env);
        if let Err(e) = started {
            self.fatal(&e);
        }
    }

    fn controller_ready(&self, controller: Controller) {
        info!("WebView2 controller created (runtime {})", self.runtime_version);
        if let Some(w) = self.window() {
            let (width, height) = w.client_size();
            controller.set_bounds(0, 0, width, height);
        }
        controller.set_visible(true);
        let start = if self.settings.homepage.is_empty() { "https://duckduckgo.com/" } else { &self.settings.homepage };
        if let Err(e) = controller.navigate(start) {
            warning!("navigate failed: {e}");
        }
        *self.controller.borrow_mut() = Some(controller);
    }

    /// Environment/controller creation failed: explain and quit.
    fn fatal(&self, error: &str) {
        warning!("fatal: {error}");
        dialogs::runtime_missing(&format!("Details: {error}\n"));
        if let Some(w) = self.window() {
            w.destroy();
        }
    }

    fn with_controller(&self, f: impl FnOnce(&Controller)) {
        // A clone keeps the borrow short (callbacks may re-enter).
        let c = self.controller.try_borrow().ok().and_then(|c| c.clone());
        if let Some(c) = c {
            f(&c);
        }
    }
}

impl WindowEvents for Browser {
    fn resized(&self, width: i32, height: i32) {
        self.with_controller(|c| c.set_bounds(0, 0, width, height));
    }

    fn minimized(&self, minimized: bool) {
        // Hidden WebViews use less CPU and memory.
        self.with_controller(|c| c.set_visible(!minimized));
    }

    fn moved(&self) {
        self.with_controller(|c| c.notify_parent_moved());
    }

    fn close_requested(&self) -> bool {
        true
    }

    fn destroyed(&self) {
        if let Ok(mut c) = self.controller.try_borrow_mut() {
            if let Some(c) = c.take() {
                c.close();
            }
        }
        if let Ok(mut e) = self.environment.try_borrow_mut() {
            e.take();
        }
    }
}
