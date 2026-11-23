//! Read-only geometry/style evidence for the existing own-window capture tool.
use gtk::{glib::translate::IntoGlib, prelude::*};
use serde_json::{json, Value};

fn color(color: gtk::gdk::RGBA) -> [f32; 4] {
    [color.red(), color.green(), color.blue(), color.alpha()]
}

fn collect(
    widget: &gtk::Widget,
    window: &gtk::Window,
    parents: &[String],
    opacity: f64,
    out: &mut Vec<Value>,
    icons: &mut Vec<Value>,
) {
    if !widget.is_mapped() {
        return;
    }
    let opacity = opacity * widget.opacity();
    let name = widget.css_name().to_string();
    let mut path = parents.to_vec();
    path.push(format!("{name}.{}", widget.css_classes().join(".")));
    let bounds = widget
        .compute_bounds(window)
        .map(|r| [r.x(), r.y(), r.width(), r.height()]);
    let mut clips = Vec::new();
    let mut ancestor = widget.parent();
    while let Some(parent) = ancestor {
        if parent.overflow() == gtk::Overflow::Hidden || parent.is::<gtk::ScrolledWindow>() {
            if let Some(rect) = parent.compute_bounds(window) {
                clips.push([rect.x(), rect.y(), rect.width(), rect.height()]);
            }
        }
        ancestor = parent.parent();
    }
    if let Some(label) = widget.downcast_ref::<gtk::Label>() {
        let layout = label.layout();
        let (ink, logical) = layout.pixel_extents();
        let offsets = label.layout_offsets();
        out.push(json!({"kind":"label", "text":label.text().to_string(), "path":path, "bounds":bounds,
            "clips":clips, "opacity":opacity,"foreground":color(label.color()),
            "sensitive":label.is_sensitive(), "font":label.pango_context().font_description().map(|f|f.to_string().to_string()),
            "font_weight":label.pango_context().font_description().map(|f|f.weight().into_glib()),
            "layout_offset":[offsets.0,offsets.1],"logical":[logical.x(),logical.y(),logical.width(),logical.height()],
            "ink":[ink.x(),ink.y(),ink.width(),ink.height()], "ellipsized":layout.is_ellipsized(), "lines":layout.line_count()}));
    } else if widget.is::<gtk::DrawingArea>() && widget.has_css_class("chart") {
        let layout = widget.create_pango_layout(Some("0123456789"));
        out.push(
            json!({"kind":"chart","path":path,"bounds":bounds,"clips":clips,
            "font":widget.pango_context().font_description().map(|f|f.to_string().to_string()),
            "line_height":layout.pixel_size().1,"foreground":color(widget.color())}),
        );
    }
    if let Some(image) = widget.downcast_ref::<gtk::Image>() {
        if let Some(icon_name) = image.icon_name() {
            let theme = gtk::IconTheme::for_display(&widget.display());
            let icon = theme.lookup_icon(
                &icon_name,
                &[],
                image.pixel_size().max(16),
                widget.scale_factor(),
                widget.direction(),
                gtk::IconLookupFlags::empty(),
            );
            icons.push(json!({"requested_name":icon_name.to_string(),
                "resolved_name":icon.icon_name(),"file":icon.file().map(|f|f.uri().to_string()),
                "symbolic":icon.is_symbolic(),"bounds":bounds}));
        }
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        collect(&widget, window, &path, opacity, out, icons);
        child = widget.next_sibling();
    }
}

pub fn save(window: &gtk::Window, path: &std::path::Path) -> std::io::Result<()> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let mut widgets = Vec::new();
    let mut icons = Vec::new();
    collect(
        window.upcast_ref(),
        window,
        &[],
        1.0,
        &mut widgets,
        &mut icons,
    );
    let style = libadwaita::StyleManager::default();
    let snapshot = gtk::Snapshot::new();
    gtk::WidgetPaintable::new(Some(window)).snapshot(
        &snapshot,
        window.width() as f64,
        window.height() as f64,
    );
    let render_bounds = snapshot.to_node().map(|node| {
        let rect = node.bounds();
        [rect.x(), rect.y(), rect.width(), rect.height()]
    });
    let report = json!({"window_size":[window.width(),window.height()],
        "render_bounds":render_bounds,
        "icon_theme":gtk::IconTheme::for_display(&gtk::prelude::WidgetExt::display(window)).theme_name().to_string(),
        "icons":icons,
            "gtk_font_name":gtk::Settings::for_display(&gtk::prelude::WidgetExt::display(window)).gtk_font_name().map(|s|s.to_string()),
        "dark":style.is_dark(),"high_contrast":style.is_high_contrast(),"widgets":widgets});
    // Explicit developer capture only. Never replace another file or config.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(serde_json::to_string_pretty(&report)?.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}
