//! A small, non-blocking introduction for new desktop users.
use crate::i18n::tr;
use gtk::prelude::*;

/// The caller owns placement and may show this widget again from a help action.
/// `open_diagnostics` navigates to the application's settings page.
pub fn welcome(open_diagnostics: impl Fn() + 'static) -> gtk::Box {
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 8);
    panel.add_css_class("card");
    panel.set_margin_bottom(14);
    panel.set_visible(
        !omen_core::config::OmenConfig::load()
            .map(|config| config.general.onboarding_dismissed)
            .unwrap_or(false),
    );

    let title = gtk::Label::builder()
        .label(tr("Welcome to OMEN Control"))
        .xalign(0.0)
        .wrap(true)
        .css_classes(["section-title"])
        .build();
    panel.append(&title);
    let details = gtk::Label::builder()
        .label(tr("Monitor readings from the connected service or available system sources. Sources and readings vary by device. Controls may be read-only, unavailable, or experimental; check each page for its current status. Settings & Diagnostics shows connection details and diagnostic export."))
        .xalign(0.0)
        .wrap(true)
        .build();
    panel.append(&details);

    let error = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .visible(false)
        .css_classes(["error"])
        .build();
    panel.append(&error);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let diagnostics = gtk::Button::with_label(&tr("Open Settings & Diagnostics"));
    let dismiss = gtk::Button::with_label(&tr("Dismiss"));
    actions.append(&diagnostics);
    actions.append(&dismiss);
    panel.append(&actions);

    // Keep the panel visible on a failed save, so the error and retry action
    // remain available. The diagnostics action still navigates on failure.
    let save_and_hide = {
        let panel = panel.downgrade();
        let error = error.clone();
        move || {
            let result = omen_ipc::settings::SettingsController::new(
                None,
                omen_core::config::OmenConfig::path(),
            )
            .set_onboarding_dismissed(true);
            match result {
                Ok(()) => {
                    error.set_visible(false);
                    if let Some(panel) = panel.upgrade() {
                        panel.set_visible(false);
                    }
                }
                Err(cause) => {
                    error.set_text(
                        &tr("Could not save welcome preference: {error}. Try dismissing again.")
                            .replace("{error}", &cause.to_string()),
                    );
                    error.set_visible(true);
                }
            }
        }
    };
    let dismiss_save = save_and_hide.clone();
    dismiss.connect_clicked(move |_| dismiss_save());
    diagnostics.connect_clicked(move |_| {
        save_and_hide();
        open_diagnostics();
    });
    panel
}
