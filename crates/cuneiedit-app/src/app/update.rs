//! Looking for, downloading and installing new versions (see `cuneiedit-update`): the first-start
//! question, the daily check, the updates dialog and the restart into the new version.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use cuneiedit_update::{Error, NotInstallable, Release, Version};
use eframe::egui::{self, RichText, ScrollArea};

use super::CuneiEditApp;
use super::dialogs::{Dialog, dialog_buttons, modal_sized};
use crate::i18n::{Texts, fill};
use crate::session;
use crate::settings::{UpdateMode, config_file};

const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

/// What should be started after CuneiEdit has quit, set by "Restart Now".
static RELAUNCH: Mutex<Option<PathBuf>> = Mutex::new(None);

/// The new version to start after quitting, if the user asked for a restart.
pub fn take_relaunch() -> Option<PathBuf> {
    RELAUNCH
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
}

/// Called when quitting was cancelled, so a later quit does not restart.
pub fn cancel_relaunch() {
    take_relaunch();
}

#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Installing(Release),
    Installed {
        release: Release,
        relaunch: PathBuf,
    },
    Failed(Error),
}

#[derive(Default)]
pub struct Updater {
    phase: Arc<Mutex<Phase>>,
    /// Versions already shown without being asked for, so they are not shown again.
    announced: Option<Version>,
    /// The first-start question was shown in this run.
    asked: bool,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

fn updates_dir() -> Option<PathBuf> {
    let dir = config_file("updates")?;
    session::create_private_dir(&dir).ok()?;
    Some(dir)
}

impl Updater {
    fn set(phase: &Mutex<Phase>, value: Phase) {
        *phase.lock().unwrap_or_else(PoisonError::into_inner) = value;
    }

    fn busy(&self) -> bool {
        matches!(
            *self.phase.lock().unwrap_or_else(PoisonError::into_inner),
            Phase::Checking | Phase::Installing(_)
        )
    }

    /// Looks for a new version in the background; with `automatic`, installs it right away
    /// if that needs no password.
    fn check(&self, ctx: &egui::Context, automatic: bool) {
        if self.busy() {
            return;
        }
        let phase = self.phase.clone();
        let ctx = ctx.clone();
        Self::set(&phase, Phase::Checking);
        thread::spawn(move || {
            let result = updates_dir()
                .ok_or_else(|| Error::Io(std::io::ErrorKind::NotFound.into()))
                .and_then(|dir| cuneiedit_update::check(&dir));
            let next = match result {
                Ok(None) => Phase::UpToDate,
                Ok(Some(release))
                    if automatic
                        && release.installable()
                        && cuneiedit_update::can_install_silently() =>
                {
                    Self::set(&phase, Phase::Installing(release.clone()));
                    install(release)
                }
                Ok(Some(release)) => Phase::Available(release),
                Err(err) => Phase::Failed(err),
            };
            Self::set(&phase, next);
            ctx.request_repaint();
        });
    }

    fn install(&self, ctx: &egui::Context, release: Release) {
        let phase = self.phase.clone();
        let ctx = ctx.clone();
        Self::set(&phase, Phase::Installing(release.clone()));
        thread::spawn(move || {
            Self::set(&phase, install(release));
            ctx.request_repaint();
        });
    }
}

fn install(release: Release) -> Phase {
    let result = updates_dir()
        .ok_or_else(|| Error::Io(std::io::ErrorKind::NotFound.into()))
        .and_then(|dir| {
            let file = cuneiedit_update::download(&release, &dir)?;
            let installed = cuneiedit_update::install(&file, &release);
            let _ = std::fs::remove_file(&file);
            installed
        });
    match result {
        Ok(relaunch) => Phase::Installed { release, relaunch },
        Err(err) => Phase::Failed(err),
    }
}

fn error_text(err: &Error, texts: &Texts) -> String {
    let reason = match err {
        Error::Download(_, message) => fill(texts.update_err_download, &[("error", message)]),
        Error::Signature => texts.update_err_signature.to_owned(),
        Error::Manifest | Error::Checksum => texts.update_err_damaged.to_owned(),
        Error::NotInstallable(NotInstallable::Location) => texts.update_err_location.to_owned(),
        Error::NotInstallable(NotInstallable::Platform) => texts.update_err_platform.to_owned(),
        Error::Denied => texts.update_err_denied.to_owned(),
        Error::Io(err) => err.to_string(),
    };
    fill(texts.update_failed, &[("error", &reason)])
}

fn notes(ui: &mut egui::Ui, release: &Release, texts: &Texts) {
    if release.notes().is_empty() {
        return;
    }
    ui.add_space(6.0);
    ui.label(RichText::new(texts.update_notes).strong());
    ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
        ui.label(release.notes());
    });
}

impl CuneiEditApp {
    /// Asks on the first start, and looks for updates once a day if the user wants that.
    pub(super) fn update_tick(&mut self, ctx: &egui::Context) {
        let Some(mode) = self.settings.update_mode else {
            if !self.updater.asked && self.dialog.is_none() {
                self.updater.asked = true;
                self.dialog = Some(Dialog::UpdateConsent);
            }
            return;
        };
        let now = now_secs();
        if mode != UpdateMode::Off
            && now.saturating_sub(self.settings.update_checked) >= CHECK_INTERVAL_SECS
            && !self.updater.busy()
        {
            self.settings.update_checked = now;
            self.persist();
            self.updater.check(ctx, mode == UpdateMode::Automatic);
        }
        // A new version found or installed in the background is shown once.
        let shown = match &*self
            .updater
            .phase
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
        {
            Phase::Available(release) | Phase::Installed { release, .. } => Some(release.version()),
            _ => None,
        };
        if let Some(version) = shown
            && self.updater.announced != Some(version)
            && self.dialog.is_none()
        {
            self.updater.announced = Some(version);
            self.dialog = Some(Dialog::Update);
        }
    }

    /// "Check for Updates…": always looks now and shows the result.
    pub(super) fn check_for_updates(&mut self, ctx: &egui::Context) {
        let installed = matches!(
            *self
                .updater
                .phase
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
            Phase::Installed { .. }
        );
        if !installed {
            self.updater.check(ctx, false);
            self.settings.update_checked = now_secs();
            self.persist();
        }
        self.dialog = Some(Dialog::Update);
    }

    pub(super) fn update_consent_dialog(&mut self, ctx: &egui::Context) -> Option<Dialog> {
        let texts = self.texts();
        let (clicked, escaped) = modal_sized(ctx, "cuneiedit-update-consent", 440.0, |ui| {
            ui.heading(texts.updates_title);
            ui.label(texts.update_question);
            dialog_buttons(
                ui,
                &[
                    texts.update_automatic,
                    texts.update_notify,
                    texts.update_off,
                ],
            )
        });
        let mode = match clicked {
            Some(0) => UpdateMode::Automatic,
            Some(1) => UpdateMode::Notify,
            Some(_) => UpdateMode::Off,
            // Escape is no answer; the question comes again on the next start.
            None => return (!escaped).then_some(Dialog::UpdateConsent),
        };
        self.settings.update_mode = Some(mode);
        self.persist();
        None
    }

    pub(super) fn update_dialog(&mut self, ctx: &egui::Context) -> Option<Dialog> {
        let texts = self.texts();
        let current = Version::current().to_string();
        enum Action {
            Close,
            Install(Release),
            Restart(PathBuf),
        }
        let (action, escaped) = {
            let phase = self
                .updater
                .phase
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            modal_sized(ctx, "cuneiedit-update", 460.0, |ui| {
                ui.heading(texts.updates_title);
                match &*phase {
                    Phase::Idle | Phase::Checking => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(texts.update_checking);
                        });
                        dialog_buttons(ui, &[texts.cancel]).map(|_| Action::Close)
                    }
                    Phase::UpToDate => {
                        ui.label(fill(texts.update_current, &[("version", &current)]));
                        dialog_buttons(ui, &[texts.ok]).map(|_| Action::Close)
                    }
                    Phase::Available(release) => {
                        let version = release.version().to_string();
                        ui.label(fill(
                            texts.update_available,
                            &[("version", &version), ("current", &current)],
                        ));
                        notes(ui, release, texts);
                        if release.installable() {
                            match dialog_buttons(ui, &[texts.update_install, texts.update_later]) {
                                Some(0) => Some(Action::Install(release.clone())),
                                Some(_) => Some(Action::Close),
                                None => None,
                            }
                        } else {
                            ui.add_space(6.0);
                            let url = release.page_url();
                            ui.label(fill(texts.update_manual, &[("url", &url)]));
                            dialog_buttons(ui, &[texts.ok]).map(|_| Action::Close)
                        }
                    }
                    Phase::Installing(release) => {
                        let version = release.version().to_string();
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(fill(texts.update_installing, &[("version", &version)]));
                        });
                        None
                    }
                    Phase::Installed { release, relaunch } => {
                        let version = release.version().to_string();
                        ui.label(fill(texts.update_installed, &[("version", &version)]));
                        match dialog_buttons(ui, &[texts.update_restart, texts.update_later]) {
                            Some(0) => Some(Action::Restart(relaunch.clone())),
                            Some(_) => Some(Action::Close),
                            None => None,
                        }
                    }
                    Phase::Failed(err) => {
                        ui.label(error_text(err, texts));
                        dialog_buttons(ui, &[texts.ok]).map(|_| Action::Close)
                    }
                }
            })
        };
        let installing = self.updater.busy()
            && matches!(
                *self
                    .updater
                    .phase
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner),
                Phase::Installing(_)
            );
        match action {
            Some(Action::Install(release)) => {
                self.updater.install(ctx, release);
                Some(Dialog::Update)
            }
            Some(Action::Restart(path)) => {
                *RELAUNCH.lock().unwrap_or_else(PoisonError::into_inner) = Some(path);
                self.quit(ctx);
                None
            }
            Some(Action::Close) => None,
            // Installing cannot be interrupted halfway.
            None if escaped && !installing => None,
            None => Some(Dialog::Update),
        }
    }
}
