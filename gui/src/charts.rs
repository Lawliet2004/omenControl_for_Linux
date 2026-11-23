//! Time-based sensor histories. Missing and stale readings are gaps, not lines.

use crate::i18n::tr;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
struct Point {
    time: u64,
    value: Option<f64>,
    source: String,
}

#[derive(Clone)]
pub struct History(Rc<RefCell<HistoryData>>);

struct HistoryData {
    limit: usize,
    max_age: u64,
    latest_frame: u64,
    samples: BTreeMap<String, VecDeque<Point>>,
}

impl History {
    pub fn clear(&self) {
        let mut history = self.0.borrow_mut();
        history.samples.clear();
        history.latest_frame = 0;
    }
    /// Adopt a newly saved capacity in place. Shrinking drops the oldest
    /// entries from every series; growing keeps existing entries so future
    /// samples can fill the larger buffer. Stored points keep their original
    /// acquisition time, value, and source label, including gaps.
    pub fn resize(&self, limit: usize, poll_ms: u64) {
        let mut history = self.0.borrow_mut();
        history.limit = limit.clamp(10, 10_000);
        history.max_age = poll_ms.saturating_mul(3).clamp(7_000, 30_000);
        let limit = history.limit;
        for queue in history.samples.values_mut() {
            while queue.len() > limit {
                queue.pop_front();
            }
        }
    }
    pub fn new(limit: usize, poll_ms: u64) -> Self {
        Self(Rc::new(RefCell::new(HistoryData {
            limit: limit.clamp(10, 10_000),
            max_age: poll_ms.saturating_mul(3).clamp(7_000, 30_000),
            latest_frame: 0,
            samples: Default::default(),
        })))
    }
}

/// Record a frame even when a source is missing. Values without a trustworthy
/// acquisition time become gaps. Fan readings use the snapshot start as their
/// best available time, and identify that approximation in the source label.
pub fn push(history: &History, key: &str, frame: u64, reading: Option<(u64, f64, String)>) {
    let mut h = history.0.borrow_mut();
    let limit = h.limit;
    let max_age = h.max_age;
    h.latest_frame = h.latest_frame.max(frame);
    let q = h.samples.entry(key.to_string()).or_default();
    let reading = reading.filter(|(time, value, _)| {
        *time != 0
            && value.is_finite()
            && frame.checked_sub(*time).is_some_and(|age| age <= max_age)
    });
    let point = match reading {
        Some((time, value, source)) => Point {
            time,
            value: Some(value),
            source,
        },
        None => Point {
            time: frame,
            value: None,
            source: String::new(),
        },
    };
    // Cached samples keep their original acquisition timestamp. Never invent
    // additional acquisitions, or connect a late sample backwards in time.
    if let Some(last) = q.back() {
        if point.time <= last.time {
            return;
        }
    }
    q.push_back(point);
    while q.len() > limit {
        q.pop_front();
    }
}

#[derive(Clone)]
pub struct Series {
    pub key: &'static str,
    pub label: &'static str,
    pub rgb: (f64, f64, f64),
}

pub struct Chart {
    pub widget: gtk::Box,
    area: gtk::DrawingArea,
    summary: gtk::Label,
    history: History,
    series: Vec<Series>,
}

impl Chart {
    /// Refresh independently of incoming samples, so a stalled poller ages.
    pub fn refresh(&self) {
        let line_height = self
            .area
            .create_pango_layout(Some("0123456789"))
            .pixel_size()
            .1;
        let height = (line_height * 7).max(140);
        if self.area.height_request() != height {
            self.area.set_height_request(height);
        }
        self.summary.set_text(&summary(
            &self.history.0.borrow(),
            &self.series,
            crate::overlay::now_ms(),
        ));
        if self.area.is_mapped() {
            self.area.queue_draw();
        }
    }
}

pub fn chart(
    title: &str,
    history: History,
    series: Vec<Series>,
    min: Option<f64>,
    max: Option<f64>,
) -> Chart {
    let area = gtk::DrawingArea::builder()
        .height_request(140)
        .hexpand(true)
        .valign(gtk::Align::Fill)
        .css_classes(["chart"])
        .build();
    let range = Rc::new(Cell::new(None));
    let draw_range = range.clone();
    let draw_history = history.clone();
    let draw_series = series.clone();
    area.set_draw_func(move |a, cr, w, h| {
        let foreground = a.color();
        let layout = a.create_pango_layout(None);
        draw(
            cr,
            (w, h),
            &draw_history,
            &draw_series,
            (min.unwrap_or(0.0), max),
            draw_range.get(),
            PlotStyle {
                foreground: (
                    foreground.red() as f64,
                    foreground.green() as f64,
                    foreground.blue() as f64,
                ),
                layout: &layout,
            },
        );
    });
    // A real GTK label exposes the same information to assistive technology,
    // supports text scaling, and does not rely on identifying line colors.
    let label = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .selectable(true)
        .css_classes(["dim", "small"])
        .build();
    let selector = gtk::DropDown::from_strings(&[
        "All retained history",
        "Last minute",
        "Last 5 minutes",
        "Last 15 minutes",
    ]);
    selector.set_tooltip_text(Some(
        "Choose the displayed history window. Retention is limited by the history setting.",
    ));
    selector.update_property(&[gtk::accessible::Property::Label(&format!(
        "{title} time range"
    ))]);
    let controls = crate::widgets::action_row();
    let range_label = gtk::Label::new(Some("Time range"));
    crate::widgets::append_action(&controls, &range_label);
    crate::widgets::append_action(&controls, &selector);
    let export = gtk::Button::with_label(&tr("Export CSV…"));
    export.set_tooltip_text(Some(
        "Export the selected time window with acquisition timestamps, sources, and gaps to a new file.",
    ));
    export.update_property(&[gtk::accessible::Property::Description(title)]);
    crate::widgets::append_action(&controls, &export);
    let export_status = gtk::Label::builder().xalign(0.0).wrap(true).build();
    crate::widgets::announce_changes(&export_status);
    let detail = gtk::TextView::builder()
        .editable(false)
        .accepts_tab(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .build();
    detail.update_property(&[gtk::accessible::Property::Label(&tr("History readings"))]);
    detail.update_property(&[gtk::accessible::Property::Description(title)]);
    let detail_scroll = gtk::ScrolledWindow::builder()
        .min_content_height(120)
        .max_content_height(220)
        .propagate_natural_height(true)
        .child(&detail)
        .build();
    let detail_content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    detail_content.append(
        &gtk::Label::builder()
            .label(tr(
                "Snapshot taken when opened. Refresh to update; the graph continues monitoring.",
            ))
            .xalign(0.0)
            .wrap(true)
            .build(),
    );
    let refresh_detail = gtk::Button::with_label(&tr("Refresh history readings"));
    refresh_detail.set_halign(gtk::Align::Start);
    detail_content.append(&refresh_detail);
    detail_content.append(&detail_scroll);
    let detail_expander = gtk::Expander::builder()
        .label(tr("Read history data"))
        .child(&detail_content)
        .build();
    crate::widgets::disclosure(&detail_expander);
    let widget = gtk::Box::new(gtk::Orientation::Vertical, 6);
    widget.set_accessible_role(gtk::AccessibleRole::Group);
    widget.update_property(&[gtk::accessible::Property::Label(title)]);
    widget.append(&controls.widget);
    widget.append(&area);
    widget.append(&label);
    widget.append(&detail_expander);
    widget.append(&export_status);
    {
        let range = range.clone();
        let detail = detail.clone();
        let history = history.clone();
        let series = series.clone();
        refresh_detail.connect_clicked(move |_| {
            detail
                .buffer()
                .set_text(&history_text(&history.0.borrow(), &series, range.get()));
        });
    }
    {
        let range = range.clone();
        let area = area.clone();
        let detail = detail.clone();
        let expander = detail_expander.clone();
        let history = history.clone();
        let series = series.clone();
        selector.connect_selected_notify(move |selector| {
            range.set(match selector.selected() {
                1 => Some(60_000),
                2 => Some(300_000),
                3 => Some(900_000),
                _ => None,
            });
            if expander.is_expanded() {
                detail
                    .buffer()
                    .set_text(&history_text(&history.0.borrow(), &series, range.get()));
            }
            area.queue_draw();
        });
    }
    {
        let range = range.clone();
        let detail = detail.clone();
        let history = history.clone();
        let series = series.clone();
        detail_expander.connect_expanded_notify(move |expander| {
            if expander.is_expanded() {
                detail
                    .buffer()
                    .set_text(&history_text(&history.0.borrow(), &series, range.get()));
            }
        });
    }
    {
        let range = range.clone();
        let history = history.clone();
        let series = series.clone();
        let status = export_status.clone();
        export.connect_clicked(move |button| {
            let csv = history_csv(&history.0.borrow(), &series, range.get());
            let status = status.clone();
            let parent = button.root().and_downcast::<gtk::Window>();
            gtk::FileDialog::builder()
                .title(tr("Export chart history to a new file"))
                .initial_name("omen-history.csv")
                .modal(true)
                .build()
                .save(
                    parent.as_ref(),
                    None::<&gtk::gio::Cancellable>,
                    move |result| match result {
                        Ok(file) => match file.path() {
                            Some(path) => match write_history_csv(
                                &path,
                                &csv,
                                &[
                                    omen_core::config::OmenConfig::path(),
                                    omen_core::config::OmenConfig::system_path(),
                                ],
                            ) {
                                Ok(()) => {
                                    status.set_text(&format!("History saved to {}", path.display()))
                                }
                                Err(error) => {
                                    status.set_text(&format!("Could not save history: {error}"))
                                }
                            },
                            None => status.set_text(&tr("Choose a local file to export history.")),
                        },
                        Err(error)
                            if error.matches(gtk::DialogError::Dismissed)
                                || error.matches(gtk::DialogError::Cancelled) => {}
                        Err(error) => status.set_text(&format!("File selection failed: {error}")),
                    },
                );
        });
    }
    let result = Chart {
        widget,
        area,
        summary: label,
        history,
        series,
    };
    result.refresh();
    result
}

/// Export to a new local file only. The chooser can race with filesystem
/// changes, so a no-replace hard link commits the fully written private file.
/// This also avoids following or replacing a destination symlink.
fn write_history_csv(
    path: &std::path::Path,
    csv: &str,
    active: &[std::path::PathBuf],
) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind, Write};
    use std::os::unix::fs::OpenOptionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    let name = path
        .file_name()
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "Choose a file name"))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    // Resolve the parent once and use that same directory for validation and
    // writing, including when a directory in the chosen path is a symlink.
    let parent = parent.canonicalize()?;
    let destination = parent.join(name);
    for config in active {
        let resolved = config
            .parent()
            .and_then(|p| p.canonicalize().ok())
            .and_then(|p| config.file_name().map(|n| p.join(n)));
        if path == config || resolved.as_deref() == Some(destination.as_path()) {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "Choose a new file instead of an active configuration",
            ));
        }
    }
    if std::fs::symlink_metadata(&destination).is_ok() {
        return Err(Error::new(
            ErrorKind::AlreadyExists,
            "Choose a new file; history export does not overwrite existing files",
        ));
    }

    struct TempFile(std::path::PathBuf);
    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let mut created = None;
    for _ in 0..100 {
        let counter = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let temp_path = parent.join(format!(
            ".omen-history-{}-{counter}.tmp",
            std::process::id()
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp_path)
        {
            Ok(file) => {
                created = Some((TempFile(temp_path), file));
                break;
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    let (temp, mut file) = created.ok_or_else(|| {
        Error::new(
            ErrorKind::AlreadyExists,
            "Could not create a temporary export file",
        )
    })?;
    file.write_all(csv.as_bytes())?;
    file.sync_all()?;
    // A hard link has atomic no-replace semantics. If another process created
    // the selected file after the dialog, the export fails without altering it.
    std::fs::hard_link(&temp.0, &destination).map_err(|error| {
        if error.kind() == ErrorKind::AlreadyExists {
            Error::new(
                ErrorKind::AlreadyExists,
                "Choose a new file; history export does not overwrite existing files",
            )
        } else {
            error
        }
    })?;
    drop(temp);
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn summary(h: &HistoryData, series: &[Series], now: u64) -> String {
    let mut lines = Vec::new();
    for (index, s) in series.iter().enumerate() {
        let text = match h.samples.get(s.key).and_then(|q| q.back()) {
            Some(p) if p.value.is_some() => match now.checked_sub(p.time) {
                Some(age) if age <= h.max_age => format!(
                    "{}: {:.1} · {} · {}s ago",
                    s.label,
                    p.value.unwrap(),
                    p.source,
                    age / 1000
                ),
                _ => format!("{}: stale", s.label),
            },
            _ => format!("{}: unavailable", s.label),
        };
        lines.push(format!(
            "{} ({})",
            text,
            if index % 2 == 0 { "solid" } else { "dashed" }
        ));
    }
    format!(
        "{}  |  Gaps indicate unavailable or stale readings.",
        lines.join("  |  ")
    )
}

// All series use the same acquisition-time window. The end remains the last
// frame, rather than pretending a stalled poller has produced newer data.
fn time_window(h: &HistoryData, series: &[Series], range: Option<u64>) -> (u64, u64) {
    let earliest = series
        .iter()
        .filter_map(|s| h.samples.get(s.key).and_then(|q| q.front()).map(|p| p.time))
        .min()
        .unwrap_or(h.latest_frame);
    let end = h.latest_frame.max(earliest.saturating_add(1));
    let start = range
        .map(|duration| end.saturating_sub(duration).max(earliest))
        .unwrap_or(earliest);
    (start, end)
}

fn history_text(h: &HistoryData, series: &[Series], range: Option<u64>) -> String {
    let (start, end) = time_window(h, series, range);
    let mut text =
        String::from("Acquisition times are Unix milliseconds. Missing readings appear as gaps.\n");
    for s in series {
        text.push_str(&format!("\n{}\n", s.label));
        if let Some(points) = h.samples.get(s.key) {
            for p in points.iter().filter(|p| p.time >= start && p.time <= end) {
                match p.value {
                    Some(value) => {
                        text.push_str(&format!("{}: {} · {}\n", p.time, value, p.source))
                    }
                    None => text.push_str(&format!("{}: unavailable (gap)\n", p.time)),
                }
            }
        }
    }
    text
}

// Quote all strings, and neutralize spreadsheet formula prefixes in data from
// external sensor sources. The numeric timestamp and value remain numeric.
fn csv_string(value: &str) -> String {
    let safe = if value
        .trim_start_matches(char::is_whitespace)
        .starts_with(['=', '+', '-', '@'])
    {
        format!("'{value}")
    } else {
        value.to_string()
    };
    format!("\"{}\"", safe.replace('\"', "\"\""))
}

fn history_csv(h: &HistoryData, series: &[Series], range: Option<u64>) -> String {
    let (start, end) = time_window(h, series, range);
    let mut text = String::from("series,timestamp_unix_ms,value,source,state\r\n");
    for s in series {
        if let Some(points) = h.samples.get(s.key) {
            for p in points.iter().filter(|p| p.time >= start && p.time <= end) {
                let value = p.value.map(|v| v.to_string()).unwrap_or_default();
                let state = if p.value.is_some() { "reading" } else { "gap" };
                text.push_str(&format!(
                    "{},{},{},{},{}\r\n",
                    csv_string(s.label),
                    p.time,
                    value,
                    csv_string(&p.source),
                    state
                ));
            }
        }
    }
    text
}

fn position(time: u64, start: u64, end: u64) -> f64 {
    (time.saturating_sub(start) as f64 / end.saturating_sub(start).max(1) as f64).clamp(0.0, 1.0)
}

fn connects(previous: &Point, current: &Point, max_age: u64) -> bool {
    previous.value.is_some()
        && current.value.is_some()
        && current
            .time
            .checked_sub(previous.time)
            .is_some_and(|elapsed| elapsed <= max_age)
        && previous.source == current.source
}

struct PlotStyle<'a> {
    foreground: (f64, f64, f64),
    layout: &'a gtk::pango::Layout,
}

fn draw(
    cr: &gtk::cairo::Context,
    size: (i32, i32),
    history: &History,
    series: &[Series],
    bounds: (f64, Option<f64>),
    range: Option<u64>,
    style: PlotStyle<'_>,
) {
    let PlotStyle { foreground, layout } = style;
    let (w, h) = (size.0 as f64, size.1 as f64);
    let (min, max) = bounds;
    let history = history.0.borrow();
    let hist = &history.samples;
    let (start, end) = time_window(&history, series, range);
    let mut hi = max.unwrap_or(f64::NEG_INFINITY);
    if max.is_none() {
        for s in series {
            if let Some(q) = hist.get(s.key) {
                for p in q {
                    if let Some(v) = p.value.filter(|_| p.time >= start && p.time <= end) {
                        hi = hi.max(v);
                    }
                }
            }
        }
        hi = if hi.is_finite() {
            (hi * 1.15).max(min + 1.0)
        } else {
            min + 10.0
        };
    }
    let labels: Vec<_> = (0..=4)
        .map(|i| format!("{:.0}", hi - (hi - min) * (i as f64 / 4.0)))
        .collect();
    let label_width = labels
        .iter()
        .map(|text| {
            layout.set_text(text);
            layout.pixel_size().0
        })
        .max()
        .unwrap_or(30) as f64;
    let line_height = layout.pixel_size().1 as f64;
    let (pad_l, pad_r, pad_t, pad_b) = (
        label_width + 12.0,
        12.0,
        line_height / 2.0 + 6.0,
        line_height * 1.6 + 6.0,
    );
    let plot_w = (w - pad_l - pad_r).max(1.0);
    let plot_h = (h - pad_t - pad_b).max(1.0);
    cr.set_source_rgba(foreground.0, foreground.1, foreground.2, 0.25);
    cr.set_line_width(1.0);
    for (i, text) in labels.iter().enumerate() {
        let y = pad_t + plot_h * (i as f64 / 4.0);
        cr.move_to(pad_l, y);
        cr.line_to(pad_l + plot_w, y);
        let _ = cr.stroke();
        cr.set_source_rgba(foreground.0, foreground.1, foreground.2, 1.0);
        cr.move_to(4.0, y - line_height / 2.0);
        layout.set_text(text);
        pangocairo::functions::show_layout(cr, layout);
        cr.set_source_rgba(foreground.0, foreground.1, foreground.2, 0.25);
    }
    for (index, s) in series.iter().enumerate() {
        let Some(q) = hist.get(s.key) else { continue };
        // The series colors were selected for dark surfaces; darken them on
        // light themes to retain line contrast against the theme background.
        let light_theme =
            0.2126 * foreground.0 + 0.7152 * foreground.1 + 0.0722 * foreground.2 < 0.5;
        let factor = if light_theme { 0.62 } else { 1.0 };
        cr.set_source_rgb(s.rgb.0 * factor, s.rgb.1 * factor, s.rgb.2 * factor);
        cr.set_line_width(1.8);
        if index % 2 == 1 {
            cr.set_dash(&[5.0, 3.0], 0.0);
        } else {
            cr.set_dash(&[], 0.0);
        }
        let mut previous: Option<&Point> = None;
        for p in q {
            if p.time < start || p.time > end {
                continue;
            }
            let Some(v) = p.value else {
                previous = None;
                continue;
            };
            let x = pad_l + plot_w * position(p.time, start, end);
            let y = pad_t + plot_h * (1.0 - ((v - min) / (hi - min).max(0.0001)).clamp(0.0, 1.0));
            if let Some(prev) = previous.filter(|prev| connects(prev, p, history.max_age)) {
                let px = pad_l + plot_w * position(prev.time, start, end);
                let py = pad_t
                    + plot_h
                        * (1.0
                            - ((prev.value.unwrap() - min) / (hi - min).max(0.0001))
                                .clamp(0.0, 1.0));
                cr.move_to(px, py);
                cr.line_to(x, y);
                let _ = cr.stroke();
            }
            // A dot keeps isolated samples visible on either side of a gap.
            cr.arc(x, y, 1.5, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
            previous = Some(p);
        }
    }
    cr.set_dash(&[], 0.0);
    cr.set_source_rgba(foreground.0, foreground.1, foreground.2, 1.0);
    cr.move_to(pad_l, h - line_height - 4.0);
    layout.set_text(&format!("{}s ago", (end - start) / 1000));
    pangocairo::functions::show_layout(cr, layout);
    let end_label = "latest frame";
    let start_label_width = layout.pixel_size().0 as f64;
    layout.set_text(end_label);
    let end_label_width = layout.pixel_size().0 as f64;
    cr.move_to(
        (w - pad_r - end_label_width).max(pad_l),
        h - line_height - 4.0,
    );
    if start_label_width + end_label_width + 12.0 < plot_w {
        pangocairo::functions::show_layout(cr, layout);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reading(time: u64, value: f64, source: &str) -> Option<(u64, f64, String)> {
        Some((time, value, source.into()))
    }
    #[test]
    fn retention_keeps_newest_frames_including_gaps() {
        let h = History::new(10, 1000);
        for i in 1..=20 {
            push(&h, "cpu", i * 1000, reading(i * 1000, i as f64, "hwmon"));
        }
        let data = h.0.borrow();
        assert_eq!(data.samples["cpu"].len(), 10);
        assert_eq!(data.samples["cpu"].front().unwrap().time, 11_000);
    }
    #[test]
    fn gaps_stale_and_cached_values_are_not_fabricated() {
        let h = History::new(20, 1000);
        push(&h, "cpu", 1000, reading(1000, 60.0, "hwmon"));
        push(&h, "cpu", 2000, reading(1000, 60.0, "hwmon"));
        push(&h, "cpu", 3000, None);
        push(&h, "cpu", 4000, reading(1000, 60.0, "hwmon"));
        push(&h, "cpu", 5000, reading(5000, 61.0, "hwmon"));
        push(&h, "gpu", 5000, reading(4500, 50.0, "nvidia"));
        let data = h.0.borrow();
        let cpu = &data.samples["cpu"];
        assert_eq!(
            cpu.iter().map(|p| (p.time, p.value)).collect::<Vec<_>>(),
            vec![(1000, Some(60.0)), (3000, None), (5000, Some(61.0))]
        );
        assert_eq!(data.samples["gpu"][0].time, 4500);
        assert_eq!(data.latest_frame, 5000);
    }
    #[test]
    fn shared_time_axis_and_breaks_follow_timestamps_and_sources() {
        assert_eq!(position(2_000, 1_000, 5_000), 0.25);
        assert_eq!(position(4_000, 1_000, 5_000), 0.75);
        let point = |time, value, source: &str| Point {
            time,
            value,
            source: source.into(),
        };
        let first = point(1_000, Some(50.0), "hwmon");
        assert!(connects(&first, &point(2_000, Some(51.0), "hwmon"), 7_000));
        assert!(!connects(&first, &point(2_000, None, ""), 7_000));
        assert!(!connects(&first, &point(9_000, Some(51.0), "hwmon"), 7_000));
        assert!(!connects(&first, &point(2_000, Some(51.0), "other"), 7_000));
    }
    #[test]
    fn selected_window_applies_to_all_series_and_exports_gaps() {
        let h = History::new(20, 1000);
        push(&h, "cpu", 1_000, reading(1_000, 40.0, "hwmon"));
        push(&h, "cpu", 70_000, None);
        push(
            &h,
            "cpu",
            80_000,
            reading(80_000, 45.0, "=danger,\"quoted\""),
        );
        push(&h, "gpu", 80_000, reading(79_000, 50.0, "gpu"));
        let series = [
            Series {
                key: "cpu",
                label: "CPU",
                rgb: (0.0, 0.0, 0.0),
            },
            Series {
                key: "gpu",
                label: "GPU",
                rgb: (0.0, 0.0, 0.0),
            },
        ];
        let data = h.0.borrow();
        assert_eq!(time_window(&data, &series, Some(60_000)), (20_000, 80_000));
        let csv = history_csv(&data, &series, Some(60_000));
        assert!(!csv.contains("1000,40"));
        assert!(csv.contains("\"CPU\",70000,,\"\",gap"));
        assert!(csv.contains("\"CPU\",80000,45,\"'=danger,\"\"quoted\"\"\",reading"));
        assert!(csv.contains("\"GPU\",79000,50,\"gpu\",reading"));
        let accessible = history_text(&data, &series, Some(60_000));
        assert!(accessible.contains("70000: unavailable (gap)"));
        assert!(accessible.contains("79000: 50 · gpu"));
        assert!(!accessible.contains("1000: 40"));
    }
    #[test]
    fn csv_strings_neutralize_whitespace_prefixed_formulas() {
        assert_eq!(csv_string("  =1+1"), "\"'  =1+1\"");
        assert_eq!(csv_string("hw,\"mon\""), "\"hw,\"\"mon\"\"\"");
    }
    #[test]
    fn export_is_private_atomic_and_never_replaces_existing_files() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "omen-chart-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let config_dir = root.join("config");
        std::fs::create_dir(&config_dir).unwrap();
        let config = config_dir.join("config.toml");
        let active = [config.clone()];
        assert!(write_history_csv(&config, "csv", &active).is_err());
        let dir_alias = root.join("config-alias");
        symlink(&config_dir, &dir_alias).unwrap();
        assert!(write_history_csv(&dir_alias.join("config.toml"), "csv", &active).is_err());
        std::fs::write(&config, "configuration").unwrap();
        let alias = root.join("alias.csv");
        symlink(&config, &alias).unwrap();
        assert!(write_history_csv(&alias, "csv", &active).is_err());
        assert_eq!(std::fs::read_to_string(&config).unwrap(), "configuration");
        let path = root.join("history.csv");
        write_history_csv(&path, "first", &active).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(write_history_csv(&path, "second", &active).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first");
        assert!(write_history_csv(&root.join("missing/history.csv"), "csv", &active).is_err());
        assert!(std::fs::read_dir(&root).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".omen-history-")));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn invalid_and_old_readings_are_unavailable_and_summary_ages() {
        let h = History::new(20, 1000);
        push(&h, "cpu", 1000, reading(1000, f64::NAN, "hwmon"));
        push(&h, "cpu", 9000, reading(1000, 10.0, "hwmon"));
        let s = [Series {
            key: "cpu",
            label: "CPU",
            rgb: (0.0, 0.0, 0.0),
        }];
        assert!(summary(&h.0.borrow(), &s, 9000).contains("unavailable"));
        push(&h, "cpu", 10_000, reading(10_000, 20.0, "hwmon"));
        assert!(summary(&h.0.borrow(), &s, 11_000).contains("20.0 · hwmon · 1s ago"));
        assert!(summary(&h.0.borrow(), &s, 18_000).contains("stale"));
    }
    #[test]
    fn resize_shrinks_grows_and_preserves_gaps_and_sample_identity() {
        let h = History::new(20, 500);
        // Two series, with gaps and original acquisition times.
        for i in 1..=12 {
            let frame = i * 1000;
            let cpu = if i == 5 {
                None
            } else {
                reading(frame, i as f64, "hwmon")
            };
            push(&h, "cpu", frame, cpu);
            let gpu = if i == 3 {
                // Original acquisition time precedes the frame; it must be kept.
                reading(frame - 500, 50.0 + i as f64, "nvidia")
            } else if i == 8 {
                None
            } else {
                reading(frame, 40.0 + i as f64, "nvidia")
            };
            push(&h, "gpu", frame, gpu);
        }
        {
            let data = h.0.borrow();
            assert_eq!(data.samples["cpu"].len(), 12);
            assert_eq!(data.samples["gpu"].len(), 12);
            // Gaps keep the frame time with no value.
            let cpu_gap = data.samples["cpu"].get(4).unwrap();
            assert_eq!(cpu_gap.time, 5000);
            assert_eq!(cpu_gap.value, None);
        }
        let before_cpu: Vec<_> = h.0.borrow().samples["cpu"]
            .iter()
            .map(|p| (p.time, p.value, p.source.clone()))
            .collect();
        let before_gpu: Vec<_> = h.0.borrow().samples["gpu"]
            .iter()
            .map(|p| (p.time, p.value, p.source.clone()))
            .collect();
        // Shrink: immediately drop oldest in every series, keep the rest intact.
        h.resize(10, 500);
        {
            let data = h.0.borrow();
            assert_eq!(data.limit, 10);
            assert_eq!(data.max_age, 7_000);
            let cpu: Vec<_> = data.samples["cpu"]
                .iter()
                .map(|p| (p.time, p.value, p.source.clone()))
                .collect();
            let gpu: Vec<_> = data.samples["gpu"]
                .iter()
                .map(|p| (p.time, p.value, p.source.clone()))
                .collect();
            assert_eq!(cpu, before_cpu[2..].to_vec());
            assert_eq!(gpu, before_gpu[2..].to_vec());
        }
        // Grow: existing entries are kept; new samples fill the larger buffer.
        h.resize(20, 500);
        {
            let data = h.0.borrow();
            assert_eq!(data.limit, 20);
            assert_eq!(data.samples["cpu"].len(), 10);
            assert_eq!(data.samples["gpu"].len(), 10);
        }
        for i in 13..=22 {
            let frame = i * 1000;
            push(&h, "cpu", frame, reading(frame, i as f64, "hwmon"));
            push(&h, "gpu", frame, reading(frame, 40.0 + i as f64, "nvidia"));
        }
        {
            let data = h.0.borrow();
            assert_eq!(data.samples["cpu"].len(), 20);
            assert_eq!(data.samples["gpu"].len(), 20);
            assert_eq!(data.samples["cpu"].front().unwrap().time, 3000);
            assert_eq!(data.samples["gpu"].front().unwrap().time, 2500);
        }
    }
    #[test]
    fn resize_updates_acquisition_freshness_cutoff() {
        let h = History::new(20, 500);
        assert_eq!(h.0.borrow().max_age, 7_000);
        // An 8,000 ms-old acquisition is stale at 7,000 ms and becomes fresh at 9,000 ms.
        push(&h, "cpu", 9_000, reading(1_000, 41.0, "hwmon"));
        assert_eq!(h.0.borrow().samples["cpu"][0].value, None);
        h.resize(20, 3_000);
        assert_eq!(h.0.borrow().max_age, 9_000);
        let fresh = History::new(20, 3_000);
        push(&fresh, "cpu", 9_000, reading(1_000, 41.0, "hwmon"));
        assert_eq!(fresh.0.borrow().samples["cpu"][0].value, Some(41.0));
    }
}
