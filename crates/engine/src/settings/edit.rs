// Editing settings in a Dear ImGui interface, a widget per value type: checkboxes, steppers for integers, number fields,
// hex fields for addresses, multi-number fields for pairs and triples, choice lists, path fields with a file panel.
// Optional values show their default as a choice of its own (an empty path, "Default" in a list, the game's own
// aspect), or a checkbox in front where a number has no natural "unset"; lists get add and remove buttons. Every group
// lists its fields with their help text as tooltips, user-facing fields apart from developer ones.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use dear_imgui_rs as imgui;
use imgui::{InputScalarFlags, Ui};

use super::values::*;

/// What a file panel lets the user choose.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathKind {
    File,
    Directory,
    Either,
}

thread_local! {
    /// A file panel requested during a frame (key, what it chooses), and the paths chosen by key. The window running
    /// the interface (host::ui::window on macOS, the builder's own on Windows) opens the panel between frames.
    static PICK: RefCell<(Option<(String, PathKind)>, HashMap<String, PathBuf>)> = RefCell::new((None, HashMap::new()));
}

/// Opens a file panel after this frame; its choice is collected with `take_path(key)`.
pub fn request_path(key: &str, kind: PathKind) {
    PICK.with(|p| p.borrow_mut().0 = Some((key.to_string(), kind)));
}

/// The path chosen for `key` since the last call, if any.
pub fn take_path(key: &str) -> Option<PathBuf> {
    PICK.with(|p| p.borrow_mut().1.remove(key))
}

/// For the window: the file panel asked for since the last call, if any.
pub fn take_request() -> Option<(String, PathKind)> {
    PICK.with(|p| p.borrow_mut().0.take())
}

/// For the window: what the user chose in the panel asked for under `key`.
pub fn put_path(key: String, path: PathBuf) {
    PICK.with(|p| p.borrow_mut().1.insert(key, path));
}

/// A value that can be edited in place; true when it changed.
pub trait Edit {
    fn edit(&mut self, ui: &Ui, id: &str, label: &str) -> bool;
}

/// The value an optional field starts with when it is turned on, and a list element when it is added.
pub trait Initial {
    fn initial() -> Self;
}

/// Who a field is for: user-facing settings, or the developer ones (diagnostics, A/B switches, internals).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    User,
    Developer,
}

impl Audience {
    pub fn shows(self, user_field: bool) -> bool {
        user_field == (self == Audience::User)
    }
}

/// A settings group: its heading, which fields are user-facing, and an editor for each field.
pub trait EditGroup {
    const HEADING: &'static str;
    const USER: &'static [&'static str];
    const FIELDS: usize;
    fn edit_fields(&mut self, ui: &Ui, audience: Audience) -> bool;
}

/// A settings group as a section of an editor (object-safe, for lists of groups).
pub trait Section {
    fn heading(&self) -> &'static str;
    /// Whether it has fields for `audience`.
    fn has(&self, audience: Audience) -> bool;
    fn edit(&mut self, ui: &Ui, audience: Audience) -> bool;
}

impl<G: EditGroup> Section for G {
    fn heading(&self) -> &'static str {
        G::HEADING
    }
    fn has(&self, audience: Audience) -> bool {
        match audience {
            Audience::User => !G::USER.is_empty(),
            Audience::Developer => G::FIELDS > G::USER.len(),
        }
    }
    fn edit(&mut self, ui: &Ui, audience: Audience) -> bool {
        let _id = ui.push_id(G::HEADING);
        self.edit_fields(ui, audience)
    }
}

/// A printf format for a number field (constant and valid; Dear ImGui checks it against the value type).
fn format<T: imgui::internal::DataTypeKind>(f: &'static str) -> imgui::NumericFormat<'static, T> {
    imgui::NumericFormat::new(f).expect("number format")
}

fn full(id: &str, label: &str) -> String {
    format!("{label}##{id}")
}

impl Edit for bool {
    fn edit(&mut self, ui: &Ui, id: &str, label: &str) -> bool {
        ui.checkbox(full(id, label), self)
    }
}

impl Initial for bool {
    fn initial() -> Self {
        true
    }
}

macro_rules! integer {
    ($($t:ty),*) => {$(
        impl Edit for $t {
            fn edit(&mut self, ui: &Ui, id: &str, label: &str) -> bool {
                ui.drag_config(full(id, label)).speed(1.0).build(ui, self)
            }
        }
        impl Initial for $t {
            fn initial() -> Self {
                0
            }
        }
    )*};
}

integer!(u8, u16, u32, u64, usize, i32, i64);

macro_rules! float {
    ($($t:ty),*) => {$(
        impl Edit for $t {
            fn edit(&mut self, ui: &Ui, id: &str, label: &str) -> bool {
                ui.drag_config(full(id, label)).speed(0.05).build(ui, self)
            }
        }
        impl Initial for $t {
            fn initial() -> Self {
                1.0
            }
        }
    )*};
}

float!(f32, f64);

impl Edit for String {
    fn edit(&mut self, ui: &Ui, id: &str, label: &str) -> bool {
        ui.input_text(full(id, label), self).build()
    }
}

impl Initial for String {
    fn initial() -> Self {
        String::new()
    }
}

/// A path field: its text (empty: `empty` as a hint), a Choose… button that opens a file panel, then the label.
pub fn path_field(
    ui: &Ui,
    id: &str,
    label: &str,
    text: &mut String,
    empty: &str,
    kind: PathKind,
) -> bool {
    let mut changed = false;
    if let Some(p) = take_path(id) {
        *text = p.display().to_string();
        changed = true;
    }
    let button = format!("Choose…##{id}");
    let style = ui.clone_style();
    let bw =
        ui.calc_text_size("Choose…")[0] + style.frame_padding()[0] * 2.0 + style.item_spacing()[0];
    ui.set_next_item_width((ui.calc_item_width() - bw).max(60.0));
    changed |= ui.input_text(format!("##{id}"), text).hint(empty).build();
    ui.same_line();
    if ui.button(button) {
        request_path(id, kind);
    }
    ui.same_line();
    ui.text(label);
    changed
}

impl Edit for PathBuf {
    fn edit(&mut self, ui: &Ui, id: &str, label: &str) -> bool {
        let mut text = self.display().to_string();
        let changed = path_field(ui, id, label, &mut text, "", PathKind::Either);
        if changed {
            *self = PathBuf::from(text);
        }
        changed
    }
}

impl Initial for PathBuf {
    fn initial() -> Self {
        PathBuf::new()
    }
}

/// An address in hex.
impl Edit for Addr {
    fn edit(&mut self, ui: &Ui, id: &str, label: &str) -> bool {
        hex_field(ui, &full(id, label), &mut self.0)
    }
}

fn hex_field(ui: &Ui, label: &str, v: &mut usize) -> bool {
    let mut x = *v as u64;
    let changed = ui
        .input_scalar(label, &mut x)
        .display_format(format("%llX"))
        .flags(InputScalarFlags::CHARS_HEXADECIMAL)
        .build();
    if changed {
        *v = x as usize;
    }
    changed
}

impl Initial for Addr {
    fn initial() -> Self {
        Addr(0)
    }
}

/// Several numbers of one type in one field (component names in the tooltip).
fn numbers<T: imgui::internal::DataTypeKind>(
    ui: &Ui,
    id: &str,
    label: &str,
    parts: &str,
    values: &mut [T],
) -> bool {
    let changed = ui.input_scalar_n(full(id, label), values).build();
    if ui.is_item_hovered() {
        ui.tooltip_text(parts);
    }
    changed
}

impl Edit for Size {
    fn edit(&mut self, ui: &Ui, id: &str, label: &str) -> bool {
        let mut v = [self.w, self.h];
        let changed = numbers(ui, id, label, "width, height", &mut v);
        [self.w, self.h] = v;
        changed
    }
}

impl Initial for Size {
    fn initial() -> Self {
        Size { w: 1280, h: 720 }
    }
}

impl Edit for Span {
    fn edit(&mut self, ui: &Ui, id: &str, label: &str) -> bool {
        let mut v = [self.first, self.last];
        let changed = numbers(ui, id, label, "first, last", &mut v);
        [self.first, self.last] = v;
        changed
    }
}

impl Initial for Span {
    fn initial() -> Self {
        Span { first: 0, last: 0 }
    }
}

impl Edit for Aspect {
    fn edit(&mut self, ui: &Ui, id: &str, label: &str) -> bool {
        let mut v = [self.w, self.h];
        let changed = ui
            .input_scalar_n(full(id, label), &mut v)
            .display_format(format("%g"))
            .build();
        if ui.is_item_hovered() {
            ui.tooltip_text("width : height");
        }
        if changed && v[0] > 0.0 && v[1] > 0.0 {
            [self.w, self.h] = v;
            return true;
        }
        false
    }
}

impl Initial for Aspect {
    fn initial() -> Self {
        Aspect { w: 16.0, h: 9.0 }
    }
}

impl Edit for ReplaySpec {
    fn edit(&mut self, ui: &Ui, id: &str, label: &str) -> bool {
        let mut v = [self.frame, self.reps as u64, self.every];
        let changed = numbers(
            ui,
            id,
            label,
            "frame, repetitions, every N frames (0: once)",
            &mut v,
        );
        (self.frame, self.reps, self.every) = (v[0], v[1].min(u32::MAX as u64) as u32, v[2]);
        changed
    }
}

impl Initial for ReplaySpec {
    fn initial() -> Self {
        ReplaySpec {
            frame: 0,
            reps: 10,
            every: 0,
        }
    }
}

impl Edit for GlitchScan {
    fn edit(&mut self, ui: &Ui, id: &str, label: &str) -> bool {
        let mut v = [self.from, self.count, self.min_px as u64];
        let changed = numbers(ui, id, label, "first frame, frames, minimum pixels", &mut v);
        (self.from, self.count, self.min_px) = (v[0], v[1], v[2] as usize);
        changed
    }
}

impl Initial for GlitchScan {
    fn initial() -> Self {
        GlitchScan {
            from: 0,
            count: 1000,
            min_px: 0,
        }
    }
}

impl Edit for DrawMask {
    fn edit(&mut self, ui: &Ui, id: &str, label: &str) -> bool {
        let mut v = [self.first, self.every];
        let changed = numbers(
            ui,
            id,
            label,
            "N, M (shader pairs whose hash % M == N)",
            &mut v,
        );
        (self.first, self.every) = (v[0], v[1].max(1));
        changed
    }
}

impl Initial for DrawMask {
    fn initial() -> Self {
        DrawMask { first: 0, every: 2 }
    }
}

/// An address range: start in hex, then its length.
impl Edit for MemRange {
    fn edit(&mut self, ui: &Ui, id: &str, label: &str) -> bool {
        let w = (ui.calc_item_width() / 2.0 - 4.0).max(60.0);
        ui.set_next_item_width(w);
        let mut changed = hex_field(ui, &format!("##{id}.addr"), &mut self.addr);
        if ui.is_item_hovered() {
            ui.tooltip_text("start (hex)");
        }
        ui.same_line();
        ui.set_next_item_width(w);
        changed |= ui
            .input_scalar(format!("{label}##{id}.len"), &mut self.len)
            .build();
        changed
    }
}

impl Initial for MemRange {
    fn initial() -> Self {
        MemRange {
            addr: 0,
            len: 0x100,
        }
    }
}

impl Edit for MemDump {
    fn edit(&mut self, ui: &Ui, id: &str, label: &str) -> bool {
        let mut changed = self.range.edit(ui, &format!("{id}.range"), label);
        changed |= self.path.edit(ui, &format!("{id}.path"), "file");
        changed
    }
}

impl Initial for MemDump {
    fn initial() -> Self {
        MemDump {
            range: MemRange::initial(),
            path: PathBuf::new(),
        }
    }
}

impl Edit for FrameFile {
    fn edit(&mut self, ui: &Ui, id: &str, label: &str) -> bool {
        let mut changed = self.path.edit(ui, &format!("{id}.path"), label);
        changed |= self.frame.edit(ui, &format!("{id}.frame"), "at frame");
        changed
    }
}

impl Initial for FrameFile {
    fn initial() -> Self {
        FrameFile {
            path: PathBuf::new(),
            frame: 0,
        }
    }
}

/// Values written in a notation of their own (trace points, held-key scripts): a text field checked as it is typed.
macro_rules! notation {
    ($($t:ty => $init:expr),*) => {$(
        impl Edit for $t {
            fn edit(&mut self, ui: &Ui, id: &str, label: &str) -> bool {
                notation_field(ui, id, label, self)
            }
        }
        impl Initial for $t {
            fn initial() -> Self {
                $init.parse().unwrap()
            }
        }
    )*};
}

notation!(TracePoint => "0x0", HeldKeys => "0-0:0");

thread_local! {
    /// Text being typed per field: (what was typed, the value's text when it was last in sync).
    static TYPED: RefCell<HashMap<String, (String, String)>> = RefCell::new(HashMap::new());
}

/// What is typed stays until it parses; a value changed elsewhere (a preset loaded) replaces the text.
fn notation_field<T: std::fmt::Display + std::str::FromStr>(
    ui: &Ui,
    id: &str,
    label: &str,
    value: &mut T,
) -> bool {
    let current = value.to_string();
    TYPED.with(|t| {
        let mut t = t.borrow_mut();
        let entry = t
            .entry(id.to_string())
            .or_insert_with(|| (current.clone(), current.clone()));
        if entry.1 != current {
            *entry = (current.clone(), current.clone());
        }
        let edited = ui.input_text(full(id, label), &mut entry.0).build();
        let parsed: Option<T> = entry.0.trim().parse().ok();
        if parsed.is_none() {
            ui.same_line();
            ui.text_colored([1.0, 0.45, 0.4, 1.0], "invalid");
        }
        match parsed {
            Some(v) if edited => {
                *value = v;
                entry.1 = value.to_string();
                true
            }
            _ => false,
        }
    })
}

/// Choice options: a list of their command-line names.
macro_rules! choice {
    ($($t:ty),*) => {$(
        impl Edit for $t {
            fn edit(&mut self, ui: &Ui, id: &str, label: &str) -> bool {
                let all = <$t as clap::ValueEnum>::value_variants();
                let names: Vec<String> = all.iter().map(|v| clap::ValueEnum::to_possible_value(v).map_or_else(String::new, |p| p.get_name().to_string())).collect();
                let mut i = all.iter().position(|v| v == self).unwrap_or(0);
                let changed = ui.combo_simple_string(full(id, label), &mut i, &names);
                if changed {
                    *self = all[i];
                }
                changed
            }
        }
        impl Initial for $t {
            fn initial() -> Self {
                <$t as clap::ValueEnum>::value_variants()[0]
            }
        }
        /// Optional: "Default" first, then the options.
        impl EditOptional for $t {
            fn edit_optional(v: &mut Option<Self>, ui: &Ui, id: &str, label: &str) -> bool {
                let all = <$t as clap::ValueEnum>::value_variants();
                let names: Vec<String> = std::iter::once("Default".to_string())
                    .chain(all.iter().map(|x| clap::ValueEnum::to_possible_value(x).map_or_else(String::new, |p| p.get_name().to_string())))
                    .collect();
                let mut i = v.map_or(0, |x| all.iter().position(|a| *a == x).map_or(0, |k| k + 1));
                let changed = ui.combo_simple_string(full(id, label), &mut i, &names);
                if changed {
                    *v = (i > 0).then(|| all[i - 1]);
                }
                changed
            }
        }
    )*};
}

choice!(
    GfxMode,
    GpuBackend,
    ReportGuard,
    TexBacking,
    FenceTrap,
    FpsMode
);

/// Optional values: each type says how it shows "not set".
impl<T: EditOptional> Edit for Option<T> {
    fn edit(&mut self, ui: &Ui, id: &str, label: &str) -> bool {
        T::edit_optional(self, ui, id, label)
    }
}

pub trait EditOptional: Sized {
    fn edit_optional(v: &mut Option<Self>, ui: &Ui, id: &str, label: &str) -> bool;
}

/// A checkbox in front turns the value on (from its initial value) or back to the default; unset, the editor shows
/// that initial value, inactive.
fn checkbox_optional<T: Edit + Initial>(v: &mut Option<T>, ui: &Ui, id: &str, label: &str) -> bool {
    let mut on = v.is_some();
    let mut changed = ui.checkbox(format!("##{id}.on"), &mut on);
    if ui.is_item_hovered() {
        ui.tooltip_text(if on {
            "Set (uncheck for the default)"
        } else {
            "Default (check to set)"
        });
    }
    if changed {
        *v = on.then(T::initial);
    }
    ui.same_line();
    match v {
        Some(x) => changed |= x.edit(ui, id, label),
        None => {
            let mut placeholder = T::initial();
            {
                let _d = ui.begin_disabled();
                placeholder.edit(ui, id, &format!("##{id}.default"));
            }
            ui.same_line();
            ui.text(label);
        }
    }
    changed
}

macro_rules! checkbox_optional {
    ($($t:ty),*) => {$(
        impl EditOptional for $t {
            fn edit_optional(v: &mut Option<Self>, ui: &Ui, id: &str, label: &str) -> bool {
                checkbox_optional(v, ui, id, label)
            }
        }
    )*};
}

checkbox_optional!(
    bool, u8, u16, u32, u64, usize, i32, i64, f32, f64, Addr, Size, Span, ReplaySpec, GlitchScan,
    DrawMask, MemRange, MemDump, FrameFile, TracePoint, HeldKeys
);

/// An optional path: empty is the default.
impl EditOptional for PathBuf {
    fn edit_optional(v: &mut Option<Self>, ui: &Ui, id: &str, label: &str) -> bool {
        let mut text = v
            .as_ref()
            .map_or_else(String::new, |p| p.display().to_string());
        let changed = path_field(ui, id, label, &mut text, "default", PathKind::Either);
        if changed {
            *v = (!text.trim().is_empty()).then(|| PathBuf::from(text.trim()));
        }
        changed
    }
}

/// Optional text: empty is the default.
impl EditOptional for String {
    fn edit_optional(v: &mut Option<Self>, ui: &Ui, id: &str, label: &str) -> bool {
        let mut text = v.clone().unwrap_or_default();
        let changed = ui
            .input_text(full(id, label), &mut text)
            .hint("default")
            .build();
        if changed {
            *v = (!text.is_empty()).then_some(text);
        }
        changed
    }
}

thread_local! {
    static CUSTOM_ASPECT: RefCell<HashMap<String, bool>> = RefCell::new(HashMap::new());
}

/// The display aspect: the game's own, the common phone ratios, or a custom W:H.
impl EditOptional for Aspect {
    fn edit_optional(v: &mut Option<Self>, ui: &Ui, id: &str, label: &str) -> bool {
        const COMMON: [(f64, f64); 4] = [(16.0, 9.0), (19.5, 9.0), (20.0, 9.0), (21.0, 9.0)];
        let mut names: Vec<String> = vec!["Game's own".into()];
        names.extend(COMMON.iter().map(|(w, h)| format!("{w}:{h}")));
        names.push("Custom".into());
        let is_custom = CUSTOM_ASPECT.with(|m| m.borrow().get(id).copied().unwrap_or(false));
        let mut i = match v {
            None => 0,
            Some(a) if !is_custom => COMMON
                .iter()
                .position(|&(w, h)| (w - a.w).abs() < 1e-6 && (h - a.h).abs() < 1e-6)
                .map_or(COMMON.len() + 1, |k| k + 1),
            Some(_) => COMMON.len() + 1,
        };
        let mut changed = ui.combo_simple_string(full(id, label), &mut i, &names);
        if changed {
            let custom_selected = i == COMMON.len() + 1;
            CUSTOM_ASPECT.with(|m| m.borrow_mut().insert(id.to_string(), custom_selected));
            *v = match i {
                0 => None,
                k if k <= COMMON.len() => Some(Aspect {
                    w: COMMON[k - 1].0,
                    h: COMMON[k - 1].1,
                }),
                _ => Some(v.unwrap_or(Aspect::initial())),
            };
        }
        if i == COMMON.len() + 1 {
            if let Some(a) = v {
                changed |= a.edit(ui, &format!("{id}.custom"), "W:H");
            }
        }
        changed
    }
}

/// Lists: one editor per element with a remove button, and an add button.
impl<T: Edit + Initial> Edit for Vec<T> {
    fn edit(&mut self, ui: &Ui, id: &str, label: &str) -> bool {
        let mut changed = false;
        if let Some(_node) = ui.tree_node(format!("{label} ({})##{id}", self.len())) {
            let mut remove = None;
            for (i, v) in self.iter_mut().enumerate() {
                if ui.small_button(format!("−##{id}.{i}")) {
                    remove = Some(i);
                }
                ui.same_line();
                changed |= v.edit(ui, &format!("{id}.{i}"), &format!("{}", i + 1));
            }
            if let Some(i) = remove {
                self.remove(i);
                changed = true;
            }
            if ui.small_button(format!("+ Add##{id}.add")) {
                self.push(T::initial());
                changed = true;
            }
        }
        changed
    }
}

/// One field: its editor, labelled from the field name, with its help text as the tooltip.
pub fn field<T: Edit>(ui: &Ui, value: &mut T, name: &str, docs: &[&str]) -> bool {
    let label = match name {
        "launch_aspect" => "Launch aspect ratio".to_string(),
        _ => {
            let mut label = name.replace('_', " ");
            if let Some(c) = label.get_mut(0..1) {
                c.make_ascii_uppercase();
            }
            label
        }
    };
    let changed = value.edit(ui, name, &label);
    if !docs.is_empty() && ui.is_item_hovered() {
        ui.tooltip_text(docs.iter().map(|d| d.trim()).collect::<Vec<_>>().join("\n"));
    }
    changed
}
