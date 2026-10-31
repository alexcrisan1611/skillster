//! The window's contents: skills missing from the source, then what needs fixing
//! in each folder, then the folders that are fine as one line each. Searching
//! lists the skills in the source instead, to look at or delete, and the ones
//! on skills.sh, to look at or install.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::collapsing_header::CollapsingState;
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, Id, Layout, Pos2, Rect, RichText, Sense, Shape, Stroke, TextureHandle,
    UiBuilder, Vec2, emath::easing,
};
use skillster::config::{self, Config, contract};
use skillster::platform::{self, DAEMON_EXE, normalize};
use skillster::status::{self, Kind, Problem, Report};

use crate::skills_sh::{self, Hit, Reply, Request};
use crate::theme::{self, colours};
use crate::watch::{Scan, Scanner};
use crate::window;

/// Padding inside the card.
const PAD: Vec2 = Vec2::new(24.0, 18.0);
const FOOTER: f32 = 30.0;
/// How long a notice stays, and how long it takes to come and go.
const NOTICE: Duration = Duration::from_millis(4500);
const NOTICE_ERROR: Duration = Duration::from_secs(9);
const NOTICE_IN: f32 = 0.18;
const NOTICE_OUT: f32 = 0.13;
/// Most diff lines shown per file.
const DIFF_LINES: usize = 300;
/// How long a Delete waits for its second click.
const CONFIRM: Duration = Duration::from_secs(4);
/// Long enough for a deleted skill's row to close.
const LEAVING: Duration = Duration::from_millis(600);
/// How long the dialog takes to open and to close.
const DIALOG_IN: f32 = 0.18;
const DIALOG_OUT: f32 = 0.12;
/// How tall the fades over the dialog's text are.
const FADE: f32 = 36.0;

pub struct App {
    config_path: PathBuf,
    scanner: Scanner,
    scan: Option<Scan>,
    logo: Option<TextureHandle>,
    light: bool,
    reduced_motion: bool,
    /// Edited copies being reviewed, by `key`.
    open: HashSet<String>,
    /// Diffs shown so far, by key and file; cleared whenever anything changes.
    diffs: HashMap<(String, String), Option<String>>,
    notice: Option<Notice>,
    /// The search box; while it isn't empty the list shows matching skills.
    query: String,
    /// Each source skill's `SKILL.md`, read when first needed; cleared whenever
    /// anything changes.
    texts: HashMap<String, Option<String>>,
    /// The skill whose Delete was clicked once, and when.
    confirm: Option<(String, Instant)>,
    /// Skills just deleted, shown closing until the scan no longer has them.
    leaving: Vec<(String, Instant)>,
    skills_sh: skills_sh::Client,
    remote: Remote,
    /// The skill shown in the dialog; kept while the dialog fades out.
    dialog: Option<Dialog>,
    /// Whether the dialog is open, rather than closing.
    dialog_open: bool,
}

/// A skill to read in the dialog.
#[derive(Clone)]
enum Dialog {
    /// One in the source, by name.
    Local(String),
    /// One on skills.sh.
    Remote(Hit),
}

impl Dialog {
    /// The row it was opened from, for that row's highlight.
    fn row_key(&self) -> String {
        match self {
            Dialog::Local(name) => view_key(name),
            Dialog::Remote(hit) => remote_key(&hit.id),
        }
    }
}

/// The search on skills.sh.
struct Remote {
    /// The last search asked for; only Search (or Enter) asks.
    asked: String,
    /// The last answer, and the search it answers.
    found: Option<(String, Result<Vec<Hit>, String>)>,
    /// Skills' files, fetched to view, by id.
    files: HashMap<String, Result<Vec<skills_sh::File>, String>>,
    /// Ids of the skills being installed.
    installing: HashSet<String>,
}

struct Notice {
    text: String,
    error: bool,
    at: Instant,
}

enum Action {
    Fix(Vec<Problem>),
    Toggle(String),
    /// Opens a skill in the source, by name, in the dialog.
    View(String),
    StartDaemon,
    Open(PathBuf),
    /// Asks before deleting: the first click arms it, the second deletes.
    Delete(String),
    /// Opens a skill on skills.sh in the dialog, fetching it the first time.
    ViewRemote(Hit),
    Install(Hit),
    /// Searches skills.sh.
    Search(String),
}

/// Identifies a problem across scans.
fn key(p: &Problem) -> String {
    format!("{}|{}", normalize(&p.target), p.skill)
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, config_path: PathBuf) -> Self {
        let light = cc.egui_ctx.system_theme() == Some(egui::Theme::Light);
        let reduced_motion = platform::reduced_motion();
        theme::install(&cc.egui_ctx, light, reduced_motion);
        window::round_corners(cc);
        let logo = eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icon.png")).ok().map(|icon| {
            let size = [icon.width as usize, icon.height as usize];
            let img = egui::ColorImage::from_rgba_unmultiplied(size, &icon.rgba);
            cc.egui_ctx.load_texture("logo", img, egui::TextureOptions::LINEAR)
        });
        App {
            scanner: Scanner::spawn(cc.egui_ctx.clone(), config_path.clone()),
            config_path,
            scan: None,
            logo,
            light,
            reduced_motion,
            open: HashSet::new(),
            diffs: HashMap::new(),
            notice: None,
            query: String::new(),
            texts: HashMap::new(),
            confirm: None,
            leaving: Vec::new(),
            skills_sh: skills_sh::Client::spawn(cc.egui_ctx.clone()),
            remote: Remote {
                asked: String::new(),
                found: None,
                files: HashMap::new(),
                installing: HashSet::new(),
            },
            dialog: None,
            dialog_open: false,
        }
    }

    fn take_scans(&mut self) {
        while let Ok(scan) = self.scanner.scans.try_recv() {
            if self.scan.as_ref().is_none_or(|s| s.report != scan.report) {
                self.diffs.clear();
                let leaving = &self.leaving;
                self.texts.retain(|k, _| leaving.iter().any(|(n, _)| n == k));
            }
            self.scan = Some(scan);
        }
    }

    fn take_replies(&mut self) {
        while let Ok(reply) = self.skills_sh.replies.try_recv() {
            match reply {
                Reply::Found { query, hits } => self.remote.found = Some((query, hits)),
                Reply::Fetched { id, files } => {
                    self.remote.files.insert(id, files);
                }
                Reply::Installed { id, result } => {
                    self.remote.installing.remove(&id);
                    match result {
                        Ok(msg) => self.notify(msg, false),
                        Err(e) => self.notify(e, true),
                    }
                    self.scanner.refresh();
                }
            }
        }
    }

    fn open_dialog(&mut self, dialog: Dialog) {
        self.dialog = Some(dialog);
        self.dialog_open = true;
    }

    /// The key of the row whose skill the dialog shows, while it is open.
    fn viewing(&self) -> Option<String> {
        self.dialog.as_ref().filter(|_| self.dialog_open).map(Dialog::row_key)
    }

    fn notify(&mut self, text: String, error: bool) {
        self.notice = Some(Notice { text, error, at: Instant::now() });
    }

    fn run(&mut self, action: Action) {
        match action {
            Action::Toggle(k) => {
                if !self.open.remove(&k) {
                    self.open.insert(k);
                }
            }
            Action::Open(p) => platform::open(&p),
            Action::View(name) => self.open_dialog(Dialog::Local(name)),
            Action::Search(query) => {
                self.remote.found = None;
                self.skills_sh.send(Request::Search(query.clone()));
                self.remote.asked = query;
            }
            Action::ViewRemote(hit) => {
                // Fetched once; a failed fetch is tried again.
                if !matches!(self.remote.files.get(&hit.id), Some(Ok(_))) {
                    self.remote.files.remove(&hit.id);
                    self.skills_sh.send(Request::Fetch(hit.clone()));
                }
                self.open_dialog(Dialog::Remote(hit));
            }
            Action::Install(hit) => {
                let Some(Ok(config)) = self.scan.as_ref().map(|s| &s.config) else { return };
                self.remote.installing.insert(hit.id.clone());
                self.skills_sh.send(Request::Install(hit, config.source.clone()));
            }
            Action::Delete(skill) => {
                if self.confirm.as_ref().is_none_or(|(s, at)| *s != skill || at.elapsed() > CONFIRM) {
                    self.confirm = Some((skill, Instant::now()));
                    return;
                }
                self.confirm = None;
                let Some(Ok(config)) = self.scan.as_ref().map(|s| s.config.clone()) else { return };
                match status::remove(&config, &skill, &platform::backups_dir()) {
                    Ok(msg) => {
                        if matches!(&self.dialog, Some(Dialog::Local(n)) if *n == skill) {
                            self.dialog_open = false;
                        }
                        self.leaving.push((skill, Instant::now()));
                        self.notify(msg, false);
                    }
                    Err(e) => self.notify(e, true),
                }
                self.scanner.refresh();
            }
            Action::StartDaemon => match self.start_daemon() {
                Ok(()) => self.notify("Started background sync".into(), false),
                Err(e) => self.notify(e, true),
            },
            Action::Fix(problems) => {
                let Some(Ok(config)) = self.scan.as_ref().map(|s| &s.config) else { return };
                let source = config.source.clone();
                let backups = platform::backups_dir();
                let (mut done, mut failed) = (Vec::new(), Vec::new());
                for p in &problems {
                    match status::fix(&source, p, &backups) {
                        Ok(msg) => {
                            self.open.remove(&key(p));
                            done.push(msg);
                        }
                        Err(e) => failed.push(e),
                    }
                }
                let backed_up = problems.iter().any(|p| match &p.kind {
                    Kind::Same | Kind::Differs(_) => true,
                    Kind::Unshared { same, .. } => !same.is_empty(),
                    _ => false,
                });
                match (done.len(), failed.first()) {
                    (_, Some(e)) => self.notify(e.clone(), true),
                    (1, None) if !backed_up => self.notify(done.remove(0), false),
                    (n, None) => self.notify(
                        if backed_up {
                            format!("Made {n} fixes; the copies are in {}", contract(&backups))
                        } else {
                            format!("Made {n} fixes")
                        },
                        false,
                    ),
                }
                self.scanner.refresh();
            }
        }
    }

    /// Starts the `skillsterd` next to this program, or the installed one.
    fn start_daemon(&self) -> Result<(), String> {
        let beside = std::env::current_exe().ok().map(|p| p.with_file_name(DAEMON_EXE)).filter(|p| p.is_file());
        let daemon = beside.unwrap_or_else(|| platform::install_dir().join(DAEMON_EXE));
        let custom = normalize(&self.config_path) != normalize(&config::default_path());
        platform::start_daemon(&daemon, custom.then_some(self.config_path.as_path()))?;
        self.scanner.refresh();
        Ok(())
    }

    fn content(&mut self, ui: &mut egui::Ui) -> Option<Action> {
        self.header(ui);
        ui.add_space(14.0);
        let Some(scan) = self.scan.take() else {
            ui.label(RichText::new("Looking at your folders…").color(colours().text_weak));
            return None;
        };
        let action = self.body(ui, &scan);
        self.scan = Some(scan);
        action
    }

    fn body(&mut self, ui: &mut egui::Ui, scan: &Scan) -> Option<Action> {
        let config = match &scan.config {
            Ok(c) => c,
            Err(e) => {
                return failure(ui, "The config can't be read", e, ("Open config", self.config_path.clone()));
            }
        };
        if let Some(e) = &scan.report.error {
            return failure(ui, "The source folder can't be read", e, ("Open config", self.config_path.clone()));
        }
        let mut action = summary(ui, config, &scan.report, scan.daemon_running);
        ui.add_space(14.0);
        if let Some(a) = self.search_box(ui, scan.report.skills.len()) {
            action = Some(a);
        }
        ui.add_space(10.0);
        let list_height = (ui.available_height() - FOOTER).max(60.0);
        ui.allocate_ui_with_layout(Vec2::new(ui.available_width(), list_height), Layout::top_down(Align::Min), |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                // Room for the scroll bar.
                ui.set_width(ui.available_width() - 10.0);
                let found = if self.query.trim().is_empty() {
                    self.list(ui, config, &scan.report)
                } else {
                    self.results(ui, config, &scan.report)
                };
                if let Some(a) = found {
                    action = Some(a);
                }
            });
        });
        footer(ui, config, &self.config_path).or(action)
    }

    /// The wordmark, which with the empty space beside it moves the window.
    fn header(&self, ui: &mut egui::Ui) {
        let rect = Rect::from_min_size(ui.cursor().min, Vec2::new(ui.available_width(), 26.0));
        let handle = ui.interact(rect, Id::unique("header-drag"), Sense::CLICK | Sense::DRAG);
        window::drag_window(ui.ctx(), &handle);
        ui.allocate_ui_with_layout(rect.size(), Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if let Some(logo) = &self.logo {
                ui.add(egui::Image::new((logo.id(), Vec2::splat(20.0))));
            }
            ui.label(RichText::new("Skillster").font(theme::strong(15.0)).color(colours().text));
        });
    }

    /// Ctrl+F (Cmd+F on macOS) focuses it, Escape clears it, Enter searches
    /// skills.sh too.
    fn search_box(&mut self, ui: &mut egui::Ui, count: usize) -> Option<Action> {
        let id = Id::unique("search");
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F)) {
            ui.memory_mut(|m| m.request_focus(id));
        }
        let clearable = !self.query.is_empty();
        let right = if clearable { 38 } else { 10 };
        let response = ui.add(
            egui::TextEdit::singleline(&mut self.query)
                .id(id)
                .hint_text(format!("Search {count} skills"))
                .desired_width(f32::INFINITY)
                .margin(egui::Margin { left: 10, right, top: 7, bottom: 7 }),
        );
        if (response.has_focus() || response.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.query.clear();
        }
        let query = self.query.trim();
        let entered = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let search = (entered && query.chars().count() >= 2).then(|| Action::Search(query.to_string()));
        if clearable {
            let centre = Pos2::new(response.rect.right() - 19.0, response.rect.center().y);
            let rect = Rect::from_center_size(centre, Vec2::splat(24.0));
            if close_button(ui, rect, "search-clear", "Clear the search (Esc)").clicked() {
                self.query.clear();
                ui.memory_mut(|m| m.request_focus(id));
            }
        }
        search
    }

    /// The source's skills whose name or description has every word searched for.
    /// Every skill keeps a row: one that stops matching, or is deleted, closes,
    /// and one that matches again opens, so the list settles instead of jumping.
    fn results(&mut self, ui: &mut egui::Ui, config: &Config, report: &Report) -> Option<Action> {
        let c = colours();
        self.leaving.retain(|(n, at)| report.skills.contains(n) || at.elapsed() < LEAVING);
        if !self.leaving.is_empty() {
            ui.ctx().request_repaint_after(LEAVING);
        }
        let mut names: Vec<String> = report.skills.clone();
        names.extend(self.leaving.iter().map(|(n, _)| n.clone()));
        names.sort();
        names.dedup();
        let terms: Vec<String> = self.query.to_lowercase().split_whitespace().map(String::from).collect();
        let mut rows = Vec::new();
        for name in names {
            let text = self.texts.entry(name.clone()).or_insert_with(|| status::skill_text(&config.source.join(&name)));
            let desc = text.as_deref().and_then(status::description);
            let hay = format!("{name} {}", desc.as_deref().unwrap_or_default()).to_lowercase();
            let leaving = self.leaving.iter().any(|(n, _)| *n == name);
            let matches = !leaving && terms.iter().all(|t| hay.contains(t.as_str()));
            rows.push((name, desc, matches));
        }
        let shown = rows.iter().filter(|r| r.2).count();
        group_heading(
            ui,
            &format!("Skills in {}", contract(&config.source)),
            &format!("{shown} of {}", report.skills.len()),
        );
        if shown == 0 {
            ui.add_space(6.0);
            let text = format!("No skill matches \u{201c}{}\u{201d}.", self.query.trim());
            ui.label(RichText::new(text).color(c.text_weak));
        }
        let mut action = None;
        for (name, desc, matches) in rows {
            if let Some(a) = self.result(ui, &name, desc.as_deref(), matches) {
                action = Some(a);
            }
        }
        ui.add_space(14.0);
        if let Some(a) = self.remote_results(ui, config, report) {
            action = Some(a);
        }
        action
    }

    /// The same search on skills.sh, asked only when Search (or Enter) is
    /// pressed. Each answer replaces the last, its rows opening in.
    fn remote_results(&mut self, ui: &mut egui::Ui, config: &Config, report: &Report) -> Option<Action> {
        let c = colours();
        let query = self.query.trim().to_string();
        let found = self.remote.found.as_ref().filter(|(q, _)| *q == query).map(|(_, hits)| hits.clone());
        let count = match &found {
            Some(Ok(hits)) => format!("{} found", hits.len()),
            _ => String::new(),
        };
        group_heading(ui, "On skills.sh", &count);
        ui.add_space(4.0);
        let weak = |ui: &mut egui::Ui, text: &str| {
            ui.label(RichText::new(text).color(c.text_weak));
        };
        if query.chars().count() < 2 {
            weak(ui, "Type two letters or more to search skills.sh.");
            return None;
        }
        let mut action = None;
        let mut search_button = |ui: &mut egui::Ui, label: &str| {
            ui.add_space(4.0);
            if ui.button(label).on_hover_text("Or press Enter in the search box").clicked() {
                action = Some(Action::Search(query.clone()));
            }
        };
        match found {
            None if self.remote.asked == query => weak(ui, "Searching skills.sh\u{2026}"),
            None => search_button(ui, &format!("Search skills.sh for \u{201c}{query}\u{201d}")),
            Some(Err(e)) => {
                ui.label(RichText::new(format!("Couldn't reach skills.sh: {e}")).color(c.error));
                search_button(ui, "Try again");
            }
            Some(Ok(hits)) if hits.is_empty() => weak(ui, "Nothing on skills.sh matches."),
            Some(Ok(hits)) => {
                for hit in &hits {
                    let installed = report.skills.contains(&hit.skill_id);
                    // New to this answer: opens in. Kept from the last: stays put.
                    let id = Id::unique(("remote-row", &hit.id));
                    let mut row = CollapsingState::load_with_default_open(ui.ctx(), id, false);
                    row.set_open(true);
                    let openness = row.openness(ui.ctx());
                    row.show_body_unindented(ui, |ui| {
                        ui.set_opacity(openness);
                        if let Some(a) = self.remote_result(ui, config, hit, installed) {
                            action = Some(a);
                        }
                    });
                }
            }
        }
        action
    }

    /// A skill on skills.sh: where it comes from and how often it is installed,
    /// View and Install, and when viewed its `SKILL.md`.
    fn remote_result(&mut self, ui: &mut egui::Ui, config: &Config, hit: &Hit, installed: bool) -> Option<Action> {
        let c = colours();
        let k = remote_key(&hit.id);
        let viewing = self.viewing().as_ref() == Some(&k);
        let installing = self.remote.installing.contains(&hit.id);
        let mut action = None;
        let backdrop = ui.painter().add(Shape::Noop);
        let response = ui
            .horizontal(|ui| {
                ui.set_min_height(42.0);
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(6.0);
                    if installed {
                        ui.add_enabled(false, egui::Button::new("Installed"));
                    } else if installing {
                        ui.add_enabled(false, egui::Button::new("Installing\u{2026}"));
                    } else if ui
                        .button("Install")
                        .on_hover_text(format!("Download it into {} and link it into every tool", contract(&config.source)))
                        .clicked()
                    {
                        action = Some(Action::Install(hit.clone()));
                    }
                    if ui.button("View").clicked() {
                        action = Some(Action::ViewRemote(hit.clone()));
                    }
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        ui.add_space(4.0);
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.label(RichText::new(&hit.name).color(c.text));
                        let from = format!("{} \u{b7} {} installs", hit.source, skills_sh::count(hit.installs));
                        ui.add(egui::Label::new(RichText::new(from).small().color(c.text_weak)).truncate());
                    });
                });
            })
            .response;
        self.backdrop(ui, backdrop, response.rect, &k, viewing);
        action
    }

    /// A row's wash, while hovered or open.
    fn backdrop(&self, ui: &egui::Ui, at: egui::layers::ShapeIdx, rect: Rect, key: &str, open: bool) {
        let hovered = ui.rect_contains_pointer(rect);
        let t = ui.ctx().animate_bool_with_time(Id::unique(("row-hover", key)), hovered || open, 0.1);
        if t > 0.0 {
            let fill = colours().wash.gamma_multiply(t * if self.light { 1.0 } else { 3.0 });
            ui.painter().set(at, Shape::rect_filled(rect, CornerRadius::same(8), fill));
        }
    }

    /// One skill: its name and description, View and Delete, and when viewed its
    /// `SKILL.md`. The row opens and closes as `shown` changes, and the view as it
    /// is toggled, over the theme's `animation_time` with a cubic ease-out
    /// (instantly with reduced motion).
    fn result(
        &mut self,
        ui: &mut egui::Ui,
        name: &str,
        desc: Option<&str>,
        shown: bool,
    ) -> Option<Action> {
        let mut action = None;
        let mut row = CollapsingState::load_with_default_open(ui.ctx(), Id::unique(("result", name)), true);
        row.set_open(shown);
        let openness = row.openness(ui.ctx());
        row.show_body_unindented(ui, |ui| {
            ui.set_opacity(openness);
            let c = colours();
            let k = view_key(name);
            let viewing = self.viewing().as_ref() == Some(&k);
            let confirming = self.confirm.as_ref().is_some_and(|(s, at)| s == name && at.elapsed() < CONFIRM);
            if confirming {
                ui.ctx().request_repaint_after(CONFIRM);
            }
            let backdrop = ui.painter().add(Shape::Noop);
            let response = ui
                .horizontal(|ui| {
                    ui.set_min_height(42.0);
                    ui.add_space(8.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(6.0);
                        let delete = if confirming {
                            ui.button(RichText::new("Confirm delete").color(c.error))
                        } else {
                            ui.button("Delete")
                        };
                        if delete.on_hover_text("Moves it to backups and removes its link from every tool").clicked() {
                            action = Some(Action::Delete(name.to_string()));
                        }
                        if ui.button("View").clicked() {
                            action = Some(Action::View(name.to_string()));
                        }
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            ui.add_space(4.0);
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.label(RichText::new(name).color(c.text));
                            if let Some(d) = desc {
                                ui.add(egui::Label::new(RichText::new(d).small().color(c.text_weak)).truncate());
                            }
                        });
                    });
                })
                .response;
            self.backdrop(ui, backdrop, response.rect, &k, viewing);
        });
        action
    }

    /// The skill being read, in a dialog over the window: its name and where it
    /// comes from, a close button, its `SKILL.md` fading out at whichever edge
    /// has more to scroll to, and a way to its folder or page. Escape, the close
    /// button or a click outside closes it. It fades and lifts in and out, or
    /// just appears with reduced motion.
    fn dialog(&mut self, ctx: &egui::Context) -> Option<Action> {
        let dialog = self.dialog.clone()?;
        let open = self.dialog_open;
        let time = match (self.reduced_motion, open) {
            (true, _) => 0.0,
            (false, true) => DIALOG_IN,
            (false, false) => DIALOG_OUT,
        };
        let t = ctx.animate_bool_with_time_and_easing(Id::unique("dialog"), open, time, easing::cubic_out);
        if !open && t <= 0.0 {
            self.dialog = None;
            return None;
        }
        let c = colours();
        let config = self.scan.as_ref().and_then(|s| s.config.as_ref().ok()).cloned();
        let (title, from, link) = match &dialog {
            Dialog::Local(name) => {
                let folder = config.as_ref().map(|c| c.source.join(name));
                let from = folder.as_deref().map(contract).unwrap_or_default();
                (name.clone(), from, folder.map(|f| ("Open folder", f)))
            }
            Dialog::Remote(hit) => {
                let from = format!("{} \u{b7} {} installs", hit.source, skills_sh::count(hit.installs));
                (hit.name.clone(), from, Some(("Open on skills.sh", PathBuf::from(hit.page()))))
            }
        };
        let screen = ctx.content_rect();
        let width = (screen.width() - 64.0).min(760.0);
        let height = (screen.height() - 220.0).clamp(140.0, 640.0);
        let lift = if self.reduced_motion { 0.0 } else { 10.0 * (1.0 - t) };
        let id = Id::unique("dialog-area");
        let area = egui::Modal::default_area(id).fade_in(false).anchor(Align2::CENTER_CENTER, Vec2::new(0.0, lift));
        let mut action = None;
        let mut close = false;
        let modal = egui::Modal::new(id).area(area).backdrop_color(Color32::TRANSPARENT).frame(egui::Frame::NONE).show(
            ctx,
            |ui| {
                // Our own backdrop, so it follows the window's rounded corners.
                let shade = Color32::from_black_alpha((if self.light { 70.0 } else { 120.0 } * t) as u8);
                ui.painter().rect_filled(screen, window::window_radius(ctx), shade);
                ui.set_opacity(t);
                theme::card().inner_margin(egui::Margin::same(16)).show(ui, |ui| {
                    ui.set_width(width);
                    ui.horizontal(|ui| {
                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                            let (rect, _) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
                            if close_button(ui, rect, "dialog-close", "Close (Esc)").clicked() {
                                close = true;
                            }
                            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                                ui.add_space(2.0);
                                ui.spacing_mut().item_spacing.y = 2.0;
                                let name = RichText::new(&title).font(theme::strong(15.0)).color(c.text_strong);
                                ui.add(egui::Label::new(name).truncate());
                                ui.add(egui::Label::new(RichText::new(&from).small().color(c.text_weak)).truncate());
                            });
                        });
                    });
                    ui.add_space(12.0);
                    let (text, error, more) = self.dialog_text(&dialog, config.as_ref());
                    reader(ui, &text, error, height, &title);
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if let Some((label, path)) = link
                            && ui.button(label).clicked()
                        {
                            action = Some(Action::Open(path));
                        }
                        if let Some(more) = more {
                            ui.label(RichText::new(more).small().color(c.text_weak));
                        }
                    });
                });
            },
        );
        if open && (close || modal.should_close()) {
            self.dialog_open = false;
        }
        if t < 1.0 || !open {
            ctx.request_repaint();
        }
        action
    }

    /// What the dialog shows for `dialog`: the text, whether it is a message
    /// rather than the skill, and a note on any other files.
    fn dialog_text(&mut self, dialog: &Dialog, config: Option<&Config>) -> (String, bool, Option<String>) {
        match dialog {
            Dialog::Local(name) => {
                let text = config.and_then(|config| {
                    let source = &config.source;
                    self.texts.entry(name.clone()).or_insert_with(|| status::skill_text(&source.join(name))).clone()
                });
                match text {
                    Some(text) => (text, false, None),
                    None => ("This skill has no readable SKILL.md.".into(), true, None),
                }
            }
            Dialog::Remote(hit) => match self.remote.files.get(&hit.id) {
                None => ("Fetching it from skills.sh\u{2026}".into(), true, None),
                Some(Err(e)) => (format!("Couldn't fetch it: {e}"), true, None),
                Some(Ok(files)) => {
                    let more = match files.len() {
                        0 | 1 => None,
                        2 => Some("And 1 more file.".to_string()),
                        n => Some(format!("And {} more files.", n - 1)),
                    };
                    match skills_sh::skill_md(files) {
                        Some(text) => (text.to_string(), false, more),
                        None => ("It has no SKILL.md.".into(), true, more),
                    }
                }
            },
        }
    }

    fn list(&mut self, ui: &mut egui::Ui, config: &Config, report: &Report) -> Option<Action> {
        let c = colours();
        let mut action = None;
        let total = report.skills.len();
        let unshared: Vec<&Problem> = report.problems.iter().filter(|p| is_unshared(p)).collect();
        if !unshared.is_empty() {
            let n = unshared.len();
            group_heading(
                ui,
                &format!("Not in {}", contract(&config.source)),
                &format!("{n} {}", if n == 1 { "skill" } else { "skills" }),
            );
            for p in unshared {
                if let Some(a) = self.row(ui, config, p) {
                    action = Some(a);
                }
            }
            ui.add_space(14.0);
        }
        for target in &report.targets {
            let problems: Vec<&Problem> =
                report.problems.iter().filter(|p| p.target == target.path && !is_unshared(p)).collect();
            if problems.is_empty() {
                continue;
            }
            group_heading(ui, &contract(&target.path), &format!("{} of {total}", target.linked));
            for p in problems {
                if let Some(a) = self.row(ui, config, p) {
                    action = Some(a);
                }
            }
            ui.add_space(14.0);
        }
        let fine: Vec<_> = report
            .targets
            .iter()
            .filter(|t| t.linked == total && !report.problems.iter().any(|p| involves(p, &t.path)))
            .collect();
        if !fine.is_empty() {
            if !report.problems.is_empty() {
                ui.label(RichText::new("In sync").font(theme::strong(12.5)).color(c.text_weak));
                ui.add_space(2.0);
            }
            for t in fine {
                ui.horizontal(|ui| {
                    ui.set_min_height(28.0);
                    check(ui, c.accent);
                    ui.label(RichText::new(contract(&t.path)).color(c.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(format!("{total} of {total}")).small().color(c.text_weak));
                    });
                });
            }
        }
        action
    }

    fn row(&mut self, ui: &mut egui::Ui, config: &Config, p: &Problem) -> Option<Action> {
        let c = colours();
        let k = key(p);
        let open = self.open.contains(&k);
        let mut action = None;
        let backdrop = ui.painter().add(Shape::Noop);
        let response = ui
            .horizontal(|ui| {
                ui.set_min_height(42.0);
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.add_space(4.0);
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(RichText::new(&p.skill).color(c.text));
                    let (text, colour) = describe(p);
                    ui.label(RichText::new(text).small().color(colour));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(6.0);
                    let clicked = match &p.kind {
                        Kind::Missing => ui.button("Link").on_hover_text("Link it from the source now").clicked(),
                        Kind::Same => {
                            ui.button("Replace").on_hover_text("Move the copy to backups and link the source").clicked()
                        }
                        Kind::Elsewhere(_) => ui.button("Relink").on_hover_text("Point the link at the source").clicked(),
                        Kind::Unshared { .. } => ui
                            .button("Move to source")
                            .on_hover_text(format!(
                                "Move the copy in {} into the source and link it there; identical copies go to backups",
                                contract(&p.target)
                            ))
                            .clicked(),
                        Kind::Differs(_) => {
                            if ui.button(if open { "Hide" } else { "Review" }).clicked() {
                                action = Some(Action::Toggle(k.clone()));
                            }
                            false
                        }
                    };
                    if clicked {
                        action = Some(Action::Fix(vec![p.clone()]));
                    }
                });
            })
            .response;
        let hovered = ui.rect_contains_pointer(response.rect);
        let t = ui.ctx().animate_bool_with_time(Id::unique(("row-hover", &k)), hovered || open, 0.1);
        if t > 0.0 {
            let fill = c.wash.gamma_multiply(t * if self.light { 1.0 } else { 3.0 });
            ui.painter().set(backdrop, Shape::rect_filled(response.rect, CornerRadius::same(8), fill));
        }
        if open && let Kind::Differs(d) = &p.kind {
            ui.add_space(4.0);
            if let Some(a) = self.review(ui, config, p, d) {
                action = Some(a);
            }
            ui.add_space(8.0);
        }
        action
    }

    /// The files an edited copy changes, the diff of each changed text file, and
    /// what can be done about it.
    fn review(&mut self, ui: &mut egui::Ui, config: &Config, p: &Problem, d: &status::Diff) -> Option<Action> {
        let c = colours();
        let src = config.source.join(&p.skill);
        let copy = p.target.join(&p.skill);
        let mut action = None;
        egui::Frame::new()
            .fill(c.code)
            .corner_radius(CornerRadius::same(8))
            .inner_margin(egui::Margin::same(12))
            .outer_margin(egui::Margin { left: 8, right: 0, top: 0, bottom: 0 })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 3.0;
                if *d == status::Diff::default() {
                    ui.label(RichText::new("This copy couldn't be compared with the source.").color(c.text_weak));
                }
                let mono = |text: String, colour: Color32| RichText::new(text).monospace().color(colour);
                for f in &d.changed {
                    ui.label(mono(format!("~ {f}"), c.warn));
                }
                for f in &d.only_here {
                    ui.label(mono(format!("+ {f}   only in this copy"), c.added));
                }
                for f in &d.only_in_source {
                    ui.label(mono(format!("- {f}   only in the source"), c.removed));
                }
                for f in &d.changed {
                    let text = self
                        .diffs
                        .entry((key(p), f.clone()))
                        .or_insert_with(|| status::file_diff(&src, &copy, f))
                        .clone();
                    let Some(text) = text else { continue };
                    ui.add_space(8.0);
                    ui.label(RichText::new(f).font(theme::strong(12.5)).color(c.text));
                    egui::ScrollArea::both().id_salt(("diff", key(p), f)).max_height(220.0).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        // The two header lines repeat what is already said.
                        for line in text.lines().skip(2).take(DIFF_LINES) {
                            let colour = match line.as_bytes().first() {
                                Some(b'+') => c.added,
                                Some(b'-') => c.removed,
                                Some(b'@') => c.text_weak,
                                _ => c.text,
                            };
                            ui.add(egui::Label::new(mono(line.to_string(), colour)).extend());
                        }
                    });
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Replace with link").clicked() {
                        action = Some(Action::Fix(vec![p.clone()]));
                    }
                    if ui.button("Show copy").clicked() {
                        action = Some(Action::Open(copy.clone()));
                    }
                    ui.label(RichText::new("The copy is moved to backups first.").small().color(c.text_weak));
                });
            });
        action
    }

    fn notice(&mut self, ctx: &egui::Context, card: Rect) {
        let Some(n) = &self.notice else { return };
        let lasts = if n.error { NOTICE_ERROR } else { NOTICE };
        let showing = n.at.elapsed() < lasts;
        let id = Id::unique("notice").with(n.at);
        let (enter, exit) = if self.reduced_motion { (0.0, 0.0) } else { (NOTICE_IN, NOTICE_OUT) };
        let t = ctx.animate_bool_with_time_and_easing(id, showing, if showing { enter } else { exit }, easing::cubic_out);
        if !showing && t <= 0.0 {
            self.notice = None;
            return;
        }
        if showing {
            ctx.request_repaint_after(lasts.saturating_sub(n.at.elapsed()));
        }
        let c = colours();
        let lift = if self.reduced_motion { 0.0 } else { 8.0 * (1.0 - t) };
        let pos = Pos2::new(card.center().x, card.bottom() - FOOTER - PAD.y - 6.0 + lift);
        let response = egui::Area::new(Id::unique("notice-area"))
            .order(egui::Order::Foreground)
            .pivot(Align2::CENTER_BOTTOM)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                ui.set_opacity(t);
                ui.set_max_width(card.width() - 2.0 * PAD.x);
                theme::pill().inner_margin(egui::Margin::symmetric(14, 8)).show(ui, |ui| {
                    ui.label(RichText::new(&n.text).color(if n.error { c.error } else { c.text }));
                })
            })
            .response;
        if response.interact(Sense::click()).clicked() {
            self.notice = None;
        }
    }
}

/// The headline, how things stand, and the one button that fixes everything safe.
fn summary(ui: &mut egui::Ui, config: &Config, report: &Report, daemon: bool) -> Option<Action> {
    let c = colours();
    let mut action = None;
    let n = report.problems.len();
    let plural = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let unshared = report.problems.iter().filter(|p| is_unshared(p)).count();
    let headline = match (n - unshared, unshared) {
        (0, 0) => "Every tool is up to date".to_string(),
        (l, 0) => format!("{} to fix", plural(l, "link", "links")),
        (0, u) => format!("{} not in the source", plural(u, "skill", "skills")),
        (l, u) => format!("{} to fix, {} not in the source", plural(l, "link", "links"), plural(u, "skill", "skills")),
    };
    ui.label(RichText::new(headline).heading().color(c.text_strong));
    let links = report.links();
    let total = report.skills.len() * report.targets.len();
    ui.label(
        RichText::new(format!(
            "{} from {} into {}. {links} of {total} in place.",
            plural(report.skills.len(), "skill", "skills"),
            contract(&config.source),
            plural(report.targets.len(), "folder", "folders"),
        ))
        .color(c.text_weak),
    );
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let (dot, text) = if daemon {
            (c.accent, "Syncing in the background")
        } else {
            (c.warn, "Background sync is off, so new skills aren't linked")
        };
        let (rect, _) = ui.allocate_exact_size(Vec2::new(8.0, 16.0), Sense::hover());
        ui.painter().circle_filled(rect.center(), 3.5, dot);
        ui.label(RichText::new(text).small().color(c.text_weak));
        if !daemon && ui.small_button("Start").clicked() {
            action = Some(Action::StartDaemon);
        }
    });

    let (safe, edited): (Vec<Problem>, Vec<Problem>) = report.problems.iter().cloned().partition(|p| p.kind.safe());
    if !safe.is_empty() {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let label = if safe.len() == n { format!("Fix all {}", safe.len()) } else { format!("Fix {} safely", safe.len()) };
            if primary_button(ui, &label).clicked() {
                action = Some(Action::Fix(safe.clone()));
            }
            let note = if edited.is_empty() {
                "Copies are moved to backups first.".to_string()
            } else {
                format!("Leaves the {} for you to review.", plural(edited.len(), "edited copy", "edited copies"))
            };
            ui.label(RichText::new(note).small().color(c.text_weak));
        });
    }
    if !edited.is_empty() {
        ui.add_space(if safe.is_empty() { 10.0 } else { 6.0 });
        ui.horizontal(|ui| {
            let label = format!("Replace {} with links", plural(edited.len(), "edited copy", "edited copies"));
            // The main action once nothing safe is left.
            let response = if safe.is_empty() {
                primary_button(ui, &label)
            } else {
                ui.add(egui::Button::new(RichText::new(label).font(theme::strong(13.0))).min_size(Vec2::new(0.0, 30.0)))
            };
            if response.on_hover_text("Each copy is moved to backups first, so its edits are kept there").clicked() {
                action = Some(Action::Fix(edited.clone()));
            }
            ui.label(RichText::new("Their edits are kept in backups.").small().color(c.text_weak));
        });
    }
    action
}

fn group_heading(ui: &mut egui::Ui, title: &str, count: &str) {
    let c = colours();
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).font(theme::strong(12.5)).color(c.text_weak));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(count).small().color(c.text_weak));
        });
    });
    ui.add_space(2.0);
}

fn view_key(skill: &str) -> String {
    format!("view|{skill}")
}

fn remote_key(id: &str) -> String {
    format!("remote|{id}")
}

/// The dialog's text, scrolling in an inset panel `height` tall, with a dark
/// fade at the top while there is more above and at the bottom while there is
/// more below.
fn reader(ui: &mut egui::Ui, text: &str, error: bool, height: f32, salt: &str) {
    let c = colours();
    let pad = 14.0;
    egui::Frame::new()
        .fill(c.code)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin { left: pad as i8, right: 4, top: 0, bottom: 0 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let scroll =
                egui::ScrollArea::vertical().id_salt(("dialog-text", salt)).auto_shrink(false).max_height(height).show(
                    ui,
                    |ui| {
                        // Room for the scroll bar.
                        ui.set_width(ui.available_width() - 12.0);
                        ui.add_space(12.0);
                        let style = RichText::new(text).color(if error { c.text_weak } else { c.text });
                        ui.add(egui::Label::new(if error { style } else { style.monospace() }).wrap());
                        ui.add_space(12.0);
                    },
                );
            let view = scroll.inner_rect;
            let above = scroll.state.clamped_offset().y;
            let below = scroll.content_size.y - view.height() - above;
            // Across the panel's left padding too, but inside its rounded corners.
            let band = view.with_min_x(view.left() - pad);
            let painter = ui.painter_at(band);
            let top = Rect::from_min_size(band.left_top(), Vec2::new(band.width(), FADE));
            let bottom = Rect::from_min_size(band.left_bottom() - Vec2::new(0.0, FADE), top.size());
            fade(&painter, top, c.code.gamma_multiply((above / FADE).clamp(0.0, 1.0)), true);
            fade(&painter, bottom, c.code.gamma_multiply((below / FADE).clamp(0.0, 1.0)), false);
        });
}

/// Paints `colour` over `rect`, easing out to nothing towards its bottom when
/// `down`, or towards its top.
fn fade(painter: &egui::Painter, rect: Rect, colour: Color32, down: bool) {
    if colour.a() == 0 {
        return;
    }
    const STEPS: u32 = 12;
    let mut mesh = egui::Mesh::default();
    for i in 0..=STEPS {
        let f = i as f32 / STEPS as f32;
        let y = rect.top() + f * rect.height();
        let away = if down { f } else { 1.0 - f };
        // Smooth at both ends, so the fade has no visible edge.
        let col = colour.gamma_multiply(1.0 - away * away * (3.0 - 2.0 * away));
        mesh.colored_vertex(Pos2::new(rect.left(), y), col);
        mesh.colored_vertex(Pos2::new(rect.right(), y), col);
        if i > 0 {
            let n = 2 * i;
            mesh.add_triangle(n - 2, n - 1, n);
            mesh.add_triangle(n - 1, n, n + 1);
        }
    }
    painter.add(mesh);
}

fn is_unshared(p: &Problem) -> bool {
    matches!(p.kind, Kind::Unshared { .. })
}

/// Whether fixing `p` touches the target folder `target`.
fn involves(p: &Problem, target: &Path) -> bool {
    p.target == target
        || matches!(&p.kind, Kind::Unshared { same, differ } if same.iter().chain(differ).any(|t| t == target))
}

fn describe(p: &Problem) -> (String, Color32) {
    let c = colours();
    match &p.kind {
        Kind::Missing => ("Not linked yet".into(), c.text_weak),
        Kind::Same => ("A copy, identical to the source".into(), c.text_weak),
        Kind::Elsewhere(t) if !t.exists() => (format!("Links to {}, which is gone", contract(t)), c.warn),
        Kind::Elsewhere(t) => (format!("Links to {}", contract(t)), c.warn),
        Kind::Unshared { same, differ } => {
            let mut text = match same.len() {
                0 => format!("Only in {}", contract(&p.target)),
                n => format!("In {} identical copies", n + 1),
            };
            if !differ.is_empty() {
                let n = differ.len();
                text += &format!("; {n} different {} to review after", if n == 1 { "copy" } else { "copies" });
            }
            (text, if differ.is_empty() { c.text_weak } else { c.warn })
        }
        Kind::Differs(d) if *d == status::Diff::default() => ("A copy that couldn't be read".into(), c.warn),
        Kind::Differs(d) => {
            let mut parts = Vec::new();
            let count = |n: usize, what: &str| format!("{n} {what}");
            if !d.changed.is_empty() {
                parts.push(count(d.changed.len(), "changed"));
            }
            if !d.only_here.is_empty() {
                parts.push(count(d.only_here.len(), "added"));
            }
            if !d.only_in_source.is_empty() {
                parts.push(count(d.only_in_source.len(), "missing"));
            }
            (format!("An edited copy: {}", parts.join(", ")), c.warn)
        }
    }
}

/// A headline and a reason, when there is nothing else to show.
fn failure(ui: &mut egui::Ui, title: &str, detail: &str, (label, path): (&str, PathBuf)) -> Option<Action> {
    let c = colours();
    ui.label(RichText::new(title).heading().color(c.text_strong));
    ui.label(RichText::new(detail).color(c.error));
    ui.add_space(10.0);
    ui.button(label).clicked().then_some(Action::Open(path))
}

fn footer(ui: &mut egui::Ui, config: &Config, config_path: &Path) -> Option<Action> {
    let c = colours();
    let mut action = None;
    ui.add_space((ui.available_height() - FOOTER).max(0.0));
    ui.horizontal(|ui| {
        ui.set_min_height(FOOTER);
        // Unframed until hovered: line the text up with the content above.
        ui.spacing_mut().button_padding.x = 0.0;
        ui.spacing_mut().item_spacing.x = 18.0;
        let links = [
            ("Source", config.source.clone()),
            ("Config", config_path.to_path_buf()),
            ("Log", config.log_path.clone()),
            ("Backups", platform::backups_dir()),
        ];
        for (label, path) in links {
            let exists = path.exists();
            let button = egui::Button::new(RichText::new(label).small().color(c.text_weak)).frame_when_inactive(false);
            let hint = if exists { contract(&path) } else { format!("{} (not there yet)", contract(&path)) };
            if ui.add_enabled(exists, button).on_hover_text(hint).on_disabled_hover_text("Not there yet").clicked() {
                action = Some(Action::Open(path));
            }
        }
    });
    action
}

/// Filled with the accent: the one main action on screen.
fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let c = colours();
    ui.scope(|ui| {
        let w = &mut ui.visuals_mut().widgets;
        w.inactive.weak_bg_fill = c.accent;
        w.hovered.weak_bg_fill = c.accent.gamma_multiply(0.88);
        w.active.weak_bg_fill = c.accent.gamma_multiply(0.76);
        for s in [&mut w.inactive, &mut w.hovered, &mut w.active] {
            s.fg_stroke = Stroke::new(1.0, c.on_accent);
        }
        ui.add(egui::Button::new(RichText::new(text).font(theme::strong(13.0))).min_size(Vec2::new(0.0, 30.0)))
    })
    .inner
}

/// A small tick.
fn check(ui: &mut egui::Ui, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
    let p = |x: f32, y: f32| rect.left_top() + Vec2::new(x, y);
    let stroke = Stroke::new(1.6, colour);
    ui.painter().line_segment([p(3.0, 7.5), p(6.0, 10.5)], stroke);
    ui.painter().line_segment([p(6.0, 10.5), p(11.5, 4.0)], stroke);
}

/// The round button that clears the search or closes the dialog, over `rect`:
/// a dark disc with a faint rim and a thin cross, brighter when hovered.
fn close_button(ui: &mut egui::Ui, rect: Rect, id: &str, hint: &str) -> egui::Response {
    let c = colours();
    let centre = rect.center();
    let response = ui
        .interact(rect, Id::unique(id), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(hint);
    let hovered = response.hovered() || response.has_focus();
    let (fill, _) = if hovered { c.button_hovered } else { c.button };
    let rim = if hovered { c.text_weak.gamma_multiply(0.5) } else { c.card_stroke };
    let painter = ui.painter();
    painter.circle(centre, 11.0, fill, Stroke::new(1.0, rim));
    let s = 3.75;
    let stroke = Stroke::new(1.4, if hovered { c.text_strong } else { c.text });
    painter.line_segment([centre + Vec2::new(-s, -s), centre + Vec2::new(s, s)], stroke);
    painter.line_segment([centre + Vec2::new(-s, s), centre + Vec2::new(s, -s)], stroke);
    response
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // Transparent where the window's rounded corners are painted.
        if window::PAINT_CORNERS { [0.0; 4] } else { colours().card.to_normalized_gamma_f32() }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.take_scans();
        self.take_replies();
        // The system's theme can change while we run.
        let light = ctx.system_theme() == Some(egui::Theme::Light);
        if light != self.light {
            self.light = light;
            theme::apply(&ctx, light);
        }
        let c = colours();
        let window = ui.max_rect();
        ui.painter().rect_filled(window, window::window_radius(&ctx), c.card);
        let mut card = window;
        card.min.y += window::top_gap();
        let inner = Rect::from_min_max(card.min + PAD, card.max - PAD);
        // First, so it takes Escape before the search box sees it.
        let from_dialog = self.dialog(&ctx);
        let action = ui.scope_builder(UiBuilder::new().max_rect(inner), |ui| self.content(ui)).inner.or(from_dialog);
        window::frame_controls(ui, window, card);
        window::buttons(&ctx, card, 10.0);
        self.notice(&ctx, card);
        if let Some(action) = action {
            self.run(action);
            // What it changed, such as an opened dialog, shows next frame.
            ctx.request_repaint();
        }
    }
}
