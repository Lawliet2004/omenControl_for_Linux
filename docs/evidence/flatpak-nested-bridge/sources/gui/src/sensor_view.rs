//! Native sensor browser. Only provider-reported device identities form device
//! groups; labels do not establish device ownership. Rows retain their identity.
use super::{sensor_age_text, sensor_matches, sensor_max_age, Ui};
use crate::data::{Snapshot, SourceTracker};
use gtk::prelude::*;
use omen_core::sensors::{SensorKind, SensorSample};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

const KINDS: [SensorKind; 9] = [
    SensorKind::Temperature,
    SensorKind::FanRpm,
    SensorKind::Frequency,
    SensorKind::Power,
    SensorKind::Voltage,
    SensorKind::Current,
    SensorKind::Percent,
    SensorKind::Count,
    SensorKind::Text,
];
const KIND_NAMES: [&str; 9] = [
    "Temperatures",
    "Fan speeds",
    "Frequencies",
    "Power",
    "Voltages",
    "Currents",
    "Percentages",
    "Counts",
    "Information",
];

fn kind_index(kind: SensorKind) -> usize {
    KINDS.iter().position(|k| *k == kind).unwrap_or(8)
}
fn reported_device(sensor: &SensorSample) -> Option<&str> {
    sensor
        .device_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    source: String,
    id: String,
    kind: usize,
    device: Option<String>,
}
impl Key {
    fn of(sensor: &SensorSample) -> Self {
        Self {
            source: sensor.source.to_string(),
            id: sensor.id.clone(),
            kind: kind_index(sensor.kind),
            device: reported_device(sensor).map(str::to_owned),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct GroupKey {
    device: String,
    kind: usize,
}
impl GroupKey {
    fn of(sensor: &SensorSample) -> Self {
        Self {
            device: reported_device(sensor)
                .map(|id| format!("Device {id}"))
                .unwrap_or_else(|| format!("{} · device not reported", sensor.source)),
            kind: kind_index(sensor.kind),
        }
    }
    fn label(&self) -> String {
        format!("{} · {}", self.device, KIND_NAMES[self.kind])
    }
}
fn available(sensor: &SensorSample, now: u64) -> bool {
    sensor.measured_unix_ms != 0
        && sensor.measured_unix_ms <= now
        && now - sensor.measured_unix_ms <= sensor_max_age()
        && (sensor.value.is_some_and(f64::is_finite)
            || sensor.text.as_ref().is_some_and(|text| !text.is_empty()))
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum DeviceFilter {
    #[default]
    All,
    Unknown,
    Reported(String),
}
#[derive(Default)]
struct Filter {
    query: String,
    kind: Option<SensorKind>,
    device: DeviceFilter,
    availability: u32,
    descending: bool,
}
impl Filter {
    fn matches(&self, sensor: &SensorSample, now: u64) -> bool {
        sensor_matches(sensor, &self.query.to_lowercase())
            && self.kind.is_none_or(|kind| kind == sensor.kind)
            && match &self.device {
                DeviceFilter::All => true,
                DeviceFilter::Unknown => reported_device(sensor).is_none(),
                DeviceFilter::Reported(id) => reported_device(sensor) == Some(id),
            }
            && match self.availability {
                1 => available(sensor, now),
                2 => !available(sensor, now),
                _ => true,
            }
    }
}
fn compare(a: &SensorSample, b: &SensorSample, descending: bool, now: u64) -> std::cmp::Ordering {
    GroupKey::of(a).cmp(&GroupKey::of(b)).then_with(|| {
        let number = |sensor: &SensorSample| {
            available(sensor, now)
                .then_some(sensor.value)
                .flatten()
                .filter(|v| v.is_finite())
        };
        let order = if descending {
            match (number(a), number(b)) {
                (Some(a), Some(b)) => b.total_cmp(&a),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            }
        } else {
            std::cmp::Ordering::Equal
        };
        order
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
            .then_with(|| Key::of(a).cmp(&Key::of(b)))
    })
}
fn unique(sensors: &[SensorSample]) -> (Vec<&SensorSample>, usize) {
    let mut counts = BTreeMap::<Key, usize>::new();
    for sensor in sensors {
        *counts.entry(Key::of(sensor)).or_default() += 1;
    }
    let kept: Vec<_> = sensors
        .iter()
        .filter(|sensor| counts[&Key::of(sensor)] == 1)
        .collect();
    let omitted = sensors.len() - kept.len();
    (kept, omitted)
}
fn text(label: &gtk::Label, value: &str) {
    if label.text() != value {
        label.set_text(value);
    }
}
struct Row {
    root: gtk::Box,
    value: gtk::Label,
    detail: gtk::Label,
    held: gtk::Label,
}
impl Row {
    fn new() -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(3)
            .css_classes(["sensor-row"])
            .build();
        let label = || {
            gtk::Label::builder()
                .xalign(0.0)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .selectable(true)
                .build()
        };
        let value = label();
        let detail = label();
        detail.add_css_class("dim");
        detail.add_css_class("small");
        let held = label();
        held.set_selectable(false);
        held.add_css_class("dim");
        held.set_visible(false);
        crate::widgets::announce_changes(&held);
        for child in [&value, &detail, &held] {
            root.append(child);
        }
        Self {
            root,
            value,
            detail,
            held,
        }
    }
    fn update(&self, sensor: &SensorSample, now: u64) {
        let fresh = sensor.measured_unix_ms != 0
            && sensor.measured_unix_ms <= now
            && now - sensor.measured_unix_ms <= sensor_max_age();
        let value = if !fresh {
            "Stale".into()
        } else {
            sensor
                .value
                .filter(|v| v.is_finite())
                .map(|v| format!("{v:.1} {}", sensor.kind.unit()).trim().to_owned())
                .or_else(|| sensor.text.clone().filter(|text| !text.is_empty()))
                .unwrap_or_else(|| "Unavailable".into())
        };
        let name = format!("{}: {value}", sensor.label);
        let detail = format!(
            "{} · {} · {} · {}",
            sensor.source,
            sensor.id,
            reported_device(sensor)
                .map(|id| format!("Device {id}"))
                .unwrap_or_else(|| "Device not reported".into()),
            sensor_age_text(sensor.measured_unix_ms, now, sensor_max_age())
        );
        let selected = [&self.value, &self.detail]
            .iter()
            .any(|label| label.has_focus() && label.selection_bounds().is_some());
        let holding = available(sensor, now) && selected;
        if !holding {
            text(&self.value, &name);
            text(&self.detail, &detail);
        }
        let changed = self.value.text() != name || self.detail.text() != detail;
        self.held.set_visible(holding && changed);
        text(
            &self.held,
            if holding && changed {
                "Sensor reading held for selection. Clear the selection or leave this reading to resume live updates."
            } else {
                ""
            },
        );
    }
}
struct Group {
    root: gtk::Box,
    list: gtk::Box,
}
impl Group {
    fn new(key: &GroupKey) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.set_margin_top(8);
        root.set_accessible_role(gtk::AccessibleRole::Group);
        root.update_property(&[gtk::accessible::Property::Label(&key.label())]);
        let heading = gtk::Label::builder()
            .label(key.label())
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .css_classes(["heading"])
            .build();
        let list = gtk::Box::new(gtk::Orientation::Vertical, 4);
        root.append(&heading);
        root.append(&list);
        Self { root, list }
    }
}
#[derive(Default)]
struct Rows {
    rows: BTreeMap<Key, Row>,
    groups: BTreeMap<GroupKey, Group>,
}
impl Rows {
    fn update(
        &mut self,
        list: &gtk::Box,
        sensors: &[SensorSample],
        filter: &Filter,
        now: u64,
    ) -> (usize, usize) {
        let (mut ordered, omitted) = unique(sensors);
        let wanted: BTreeSet<_> = ordered.iter().map(|sensor| Key::of(sensor)).collect();
        self.rows.retain(|key, row| {
            if wanted.contains(key) {
                true
            } else {
                if let Some(parent) = row.root.parent().and_downcast::<gtk::Box>() {
                    parent.remove(&row.root);
                }
                false
            }
        });
        let groups: BTreeSet<_> = ordered.iter().map(|sensor| GroupKey::of(sensor)).collect();
        self.groups.retain(|key, group| {
            if groups.contains(key) {
                true
            } else {
                list.remove(&group.root);
                false
            }
        });
        ordered.sort_by(|a, b| compare(a, b, filter.descending, now));
        let mut previous = BTreeMap::<GroupKey, gtk::Box>::new();
        let mut shown = BTreeSet::new();
        let mut count = 0;
        for sensor in ordered {
            let key = GroupKey::of(sensor);
            let group = self.groups.entry(key.clone()).or_insert_with(|| {
                let group = Group::new(&key);
                list.append(&group.root);
                group
            });
            let row = self.rows.entry(Key::of(sensor)).or_insert_with(|| {
                let row = Row::new();
                group.list.append(&row.root);
                row
            });
            row.update(sensor, now);
            let visible = filter.matches(sensor, now);
            row.root.set_visible(visible);
            if visible {
                count += 1;
                shown.insert(key.clone());
            }
            group
                .list
                .reorder_child_after(&row.root, previous.get(&key));
            previous.insert(key, row.root.clone());
        }
        let mut previous = None;
        for (key, group) in &self.groups {
            group.root.set_visible(shown.contains(key));
            list.reorder_child_after(&group.root, previous);
            previous = Some(&group.root);
        }
        (count, omitted)
    }
}
fn dropdown(name: &str, choices: &[&str]) -> gtk::DropDown {
    let control = gtk::DropDown::from_strings(choices);
    control.set_expression(Some(&gtk::PropertyExpression::new(
        gtk::StringObject::static_type(),
        None::<gtk::Expression>,
        "string",
    )));
    control.set_hexpand(true);
    control.update_property(&[gtk::accessible::Property::Label(name)]);
    let factory = |popup: bool| {
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(move |_, item| {
            if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
                item.set_child(Some(
                    &gtk::Label::builder()
                        .wrap(popup)
                        .wrap_mode(gtk::pango::WrapMode::WordChar)
                        .ellipsize(if popup {
                            gtk::pango::EllipsizeMode::None
                        } else {
                            gtk::pango::EllipsizeMode::End
                        })
                        .max_width_chars(36)
                        .xalign(0.0)
                        .build(),
                ));
            }
        });
        factory.connect_bind(|_, item| {
            if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
                if let (Some(label), Some(value)) = (
                    item.child().and_downcast::<gtk::Label>(),
                    item.item().and_downcast::<gtk::StringObject>(),
                ) {
                    label.set_text(&value.string());
                    label.set_tooltip_text(Some(&value.string()));
                }
            }
        });
        factory
    };
    control.set_factory(Some(&factory(false)));
    control.set_list_factory(Some(&factory(true)));
    control
}
fn field(name: &str, control: &gtk::DropDown) -> gtk::Box {
    let field = gtk::Box::new(gtk::Orientation::Vertical, 4);
    field.set_hexpand(true);
    let label = gtk::Label::builder()
        .label(name)
        .xalign(0.0)
        .wrap(true)
        .build();
    control.update_relation(&[gtk::accessible::Relation::LabelledBy(&[label.upcast_ref()])]);
    // The focusable toggle belongs to GtkDropDown. Give that native focus
    // target the same contextual label as its enclosing combo box.
    let mut child = control.first_child();
    while let Some(widget) = child {
        if widget.is::<gtk::ToggleButton>() {
            widget.update_relation(&[gtk::accessible::Relation::LabelledBy(&[label.upcast_ref()])]);
        }
        child = widget.next_sibling();
    }
    field.append(&label);
    field.append(control);
    field
}

pub fn bind(ui: &Rc<Ui>, page: &gtk::Box) -> gtk::SearchEntry {
    let section = super::section("All sensors");
    let search = gtk::SearchEntry::builder()
        .placeholder_text("Search sensor names, identifiers, devices or sources")
        .hexpand(true)
        .build();
    search.update_property(&[gtk::accessible::Property::Label("Search sensors")]);
    section.append(&search);
    let kind = dropdown(
        "Sensor type",
        &[
            "All types",
            "Temperatures",
            "Fan speeds",
            "Frequencies",
            "Power",
            "Voltages",
            "Currents",
            "Percentages",
            "Counts",
            "Information",
        ],
    );
    let device = dropdown("Sensor device", &["All devices", "Device not reported"]);
    device.set_enable_search(true);
    let state = dropdown(
        "Sensor availability",
        &["All readings", "Available now", "Unavailable or stale"],
    );
    let order = dropdown("Sensor order", &["Name", "Highest value within type"]);
    let reset = gtk::Button::with_label("Clear sensor filters");
    for fields in [
        [field("Type", &kind), field("Device", &device)],
        [field("Availability", &state), field("Order", &order)],
    ] {
        let row = crate::widgets::action_row();
        for field in fields {
            crate::widgets::append_action(&row, &field);
        }
        section.append(&row.widget);
    }
    reset.set_halign(gtk::Align::Start);
    section.append(&reset);
    section.append(&gtk::Label::builder().label("Grouped by reported device and measurement type. A provider group means no device identity was reported; labels do not establish device ownership.")
        .xalign(0.0).wrap(true).css_classes(["dim"]).build());
    let status = gtk::Label::builder()
        .label("Collecting sensors…")
        .xalign(0.0)
        .wrap(true)
        .build();
    section.append(&status);
    let list = gtk::Box::new(gtk::Orientation::Vertical, 12);
    section.append(&list);
    page.append(&section);
    let rows = RefCell::new(Rows::default());
    let snapshot = Rc::new(RefCell::new(None::<Snapshot>));
    let options = Rc::new(RefCell::new(vec![DeviceFilter::All, DeviceFilter::Unknown]));
    let syncing = Rc::new(Cell::new(false));
    let refresh: Rc<dyn Fn()> = {
        let (snapshot, options, syncing) = (snapshot.clone(), options.clone(), syncing.clone());
        let (kind, device, state, order, search) = (
            kind.clone(),
            device.clone(),
            state.clone(),
            order.clone(),
            search.clone(),
        );
        Rc::new(move || {
            if syncing.get() {
                return;
            }
            let snapshot = snapshot.borrow();
            let Some(snapshot) = snapshot.as_ref() else {
                return;
            };
            let filter = Filter {
                query: search.text().into(),
                kind: kind
                    .selected()
                    .checked_sub(1)
                    .and_then(|index| KINDS.get(index as usize).copied()),
                device: options
                    .borrow()
                    .get(device.selected() as usize)
                    .cloned()
                    .unwrap_or_default(),
                availability: state.selected(),
                descending: order.selected() == 1,
            };
            let (shown, duplicates) = rows.borrow_mut().update(
                &list,
                &snapshot.sensors,
                &filter,
                crate::overlay::now_ms(),
            );
            let message = if snapshot.sensors.is_empty() {
                "No sensor readings available.".into()
            } else if shown == 0 {
                if filter.query.is_empty() {
                    "No sensors match these filters. Clear filters to show all readings.".into()
                } else {
                    "No sensors match this search.".into()
                }
            } else {
                format!(
                    "Showing {shown} of {} sensor readings.",
                    snapshot.sensors.len()
                )
            };
            text(
                &status,
                &format!(
                    "{message}{}",
                    if duplicates > 0 {
                        format!(" {duplicates} readings with duplicate identifiers omitted.")
                    } else {
                        String::new()
                    }
                ),
            );
        })
    };
    for control in [&kind, &device, &state, &order] {
        let refresh = Rc::downgrade(&refresh);
        control.connect_selected_notify(move |_| {
            if let Some(refresh) = refresh.upgrade() {
                refresh();
            }
        });
    }
    {
        let refresh = Rc::downgrade(&refresh);
        search.connect_search_changed(move |_| {
            if let Some(refresh) = refresh.upgrade() {
                refresh();
            }
        });
    }
    {
        let (search, kind, device, state, order, syncing, refresh) = (
            search.clone(),
            kind.clone(),
            device.clone(),
            state.clone(),
            order.clone(),
            syncing.clone(),
            Rc::downgrade(&refresh),
        );
        reset.connect_clicked(move |_| {
            syncing.set(true);
            search.set_text("");
            kind.set_selected(0);
            device.set_selected(0);
            state.set_selected(0);
            order.set_selected(0);
            syncing.set(false);
            if let Some(refresh) = refresh.upgrade() {
                refresh();
            }
        });
    }
    let option_names = RefCell::new(vec![
        "All devices".to_string(),
        "Device not reported".to_string(),
    ]);
    let origin = RefCell::new(SourceTracker::default());
    // Keep the rows within the refresh closure; invalidate them through one
    // empty frame before accepting a different source's identifiers.
    ui.register(page, move |next| {
        if origin.borrow_mut().changed(&next.origin) {
            let mut empty = next.clone();
            empty.sensors.clear();
            *snapshot.borrow_mut() = Some(empty);
            refresh();
        }
        let selected = options
            .borrow()
            .get(device.selected() as usize)
            .cloned()
            .unwrap_or_default();
        let mut devices: Vec<_> = next
            .sensors
            .iter()
            .filter_map(|sensor| reported_device(sensor).map(str::to_owned))
            .collect();
        devices.sort();
        devices.dedup();
        let mut choices = vec![DeviceFilter::All, DeviceFilter::Unknown];
        choices.extend(devices.iter().cloned().map(DeviceFilter::Reported));
        if let DeviceFilter::Reported(id) = &selected {
            if !devices.contains(id) {
                choices.push(selected.clone());
            }
        }
        let names: Vec<_> = choices
            .iter()
            .map(|choice| match choice {
                DeviceFilter::All => "All devices".into(),
                DeviceFilter::Unknown => "Device not reported".into(),
                DeviceFilter::Reported(id) => {
                    if devices.contains(id) {
                        id.clone()
                    } else {
                        format!("{id} (not reporting)")
                    }
                }
            })
            .collect();
        if *options.borrow() != choices || *option_names.borrow() != names {
            syncing.set(true);
            let refs: Vec<_> = names.iter().map(String::as_str).collect();
            device.set_model(Some(&gtk::StringList::new(&refs)));
            device.set_selected(
                choices
                    .iter()
                    .position(|choice| *choice == selected)
                    .unwrap_or(0) as u32,
            );
            *options.borrow_mut() = choices;
            *option_names.borrow_mut() = names;
            syncing.set(false);
        }
        *snapshot.borrow_mut() = Some(next.clone());
        refresh();
    });
    search
}

#[cfg(test)]
mod tests {
    use super::*;
    use omen_core::sensors::SensorSource;
    fn sensor(id: &str, kind: SensorKind, value: f64) -> SensorSample {
        let mut sample = SensorSample::numeric(id, id, kind, SensorSource::Hwmon, value);
        sample.measured_unix_ms = 10_000;
        sample
    }
    #[test]
    fn grouping_uses_reported_identity_and_type_never_a_display_label() {
        let mut sample = sensor("cpu/temp", SensorKind::Temperature, 50.0);
        sample.label = "NVIDIA GPU".into();
        assert_eq!(
            GroupKey::of(&sample).label(),
            "hwmon · device not reported · Temperatures"
        );
        sample.device_id = Some("0000:03:00.0".into());
        assert_eq!(
            GroupKey::of(&sample).label(),
            "Device 0000:03:00.0 · Temperatures"
        );
        let before = Key::of(&sample);
        sample.label = "Renamed by provider".into();
        assert_eq!(before, Key::of(&sample));
        sample.device_id = Some("0000:04:00.0".into());
        assert_ne!(before, Key::of(&sample));
        sample.kind = SensorKind::Power;
        assert!(GroupKey::of(&sample).label().ends_with("Power"));
    }
    #[test]
    fn availability_rejects_stale_future_missing_and_nonfinite_values() {
        let mut sample = sensor("a", SensorKind::Temperature, 0.0);
        assert!(available(&sample, 10_000));
        sample.value = Some(f64::NAN);
        assert!(!available(&sample, 10_000));
        sample.value = None;
        assert!(!available(&sample, 10_000));
        sample.text = Some("Online".into());
        assert!(available(&sample, 10_000));
        sample.measured_unix_ms = 10_001;
        assert!(!available(&sample, 10_000));
        sample.measured_unix_ms = 0;
        assert!(!available(&sample, 10_000));
        sample.measured_unix_ms = 1;
        assert!(!available(&sample, 40_000));
    }
    #[test]
    fn filters_intersect_search_device_type_and_availability() {
        let mut sample = sensor("gpu/temp", SensorKind::Temperature, 55.0);
        sample.device_id = Some("0000:03:00.0".into());
        let mut filter = Filter {
            query: "gpu 0000:03".into(),
            kind: Some(SensorKind::Temperature),
            device: DeviceFilter::Reported("0000:03:00.0".into()),
            availability: 1,
            ..Default::default()
        };
        assert!(filter.matches(&sample, 10_000));
        filter.kind = Some(SensorKind::Power);
        assert!(!filter.matches(&sample, 10_000));
        filter.kind = None;
        filter.device = DeviceFilter::Unknown;
        assert!(!filter.matches(&sample, 10_000));
        filter.device = DeviceFilter::All;
        filter.availability = 2;
        assert!(!filter.matches(&sample, 10_000));
        assert!(filter.matches(&sample, 40_000));
    }
    #[test]
    fn highest_sort_never_compares_values_across_measurement_types_and_puts_missing_last() {
        let low = sensor("a", SensorKind::Temperature, 50.0);
        let high = sensor("b", SensorKind::Temperature, 60.0);
        let mut absent = sensor("c", SensorKind::Temperature, 100.0);
        absent.value = None;
        let power = sensor("d", SensorKind::Power, 500.0);
        let mut samples = [&absent, &power, &low, &high];
        samples.sort_by(|a, b| compare(a, b, true, 10_000));
        assert_eq!(
            samples.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["b", "a", "c", "d"]
        );
        samples.sort_by(|a, b| compare(a, b, false, 10_000));
        assert_eq!(
            samples.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b", "c", "d"]
        );
    }
    #[test]
    fn ambiguous_duplicate_identifiers_are_omitted_without_collapsing_other_sources_or_devices() {
        let a = sensor("same", SensorKind::Temperature, 50.0);
        let duplicate = a.clone();
        let mut source = a.clone();
        source.source = SensorSource::NvidiaSmi;
        let mut device = a.clone();
        device.device_id = Some("0000:03:00.0".into());
        let samples = vec![a, duplicate, source, device];
        let (kept, omitted) = unique(&samples);
        assert_eq!(omitted, 2);
        assert_eq!(kept.len(), 2);
        assert_ne!(Key::of(kept[0]), Key::of(kept[1]));
    }
}
