// Release builds on Windows are GUI apps (no console window); CLI flags re-attach to the parent console.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod theme;

use discovery::{Device, Discovery};
use eframe::egui;
use std::time::{Duration, Instant};
use synology_assistant_rs::{discovery, help, protocol, wol};
use wol::WolDevice;

const AUTO_SEARCH: Duration = Duration::from_secs(10);
const ONLINE_WINDOW: Duration = Duration::from_secs(35);

#[cfg(windows)]
fn attach_console() {
    unsafe extern "system" {
        fn AttachConsole(pid: u32) -> i32;
    }
    // ATTACH_PARENT_PROCESS
    unsafe { AttachConsole(u32::MAX) };
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(windows)]
    if !args.is_empty() {
        attach_console();
    }
    match args.first().map(String::as_str) {
        Some("--scan") => std::process::exit(cli_scan()),
        Some("--wake") => std::process::exit(cli_wake(args.get(1))),
        Some("--help" | "-h") => {
            println!(
                "Synology Assistant (native)\n\n  (no args)      open the app\n  --scan         list Synology devices on the LAN\n  --wake <MAC>   send a Wake-on-LAN packet"
            );
        }
        _ => run_gui(),
    }
}

fn cli_scan() -> i32 {
    match discovery::scan_blocking(Duration::from_secs(4)) {
        Ok(list) if list.is_empty() => {
            eprintln!("No Synology devices found.");
            1
        }
        Ok(list) => {
            println!(
                "{:<18} {:<16} {:<32} {:<18} {:<10} {:<14} SERIAL",
                "NAME", "IP", "STATUS", "MAC", "MODEL", "DSM"
            );
            for n in list {
                println!(
                    "{:<18} {:<16} {:<32} {:<18} {:<10} {:<14} {}",
                    n.name,
                    n.ip.map(|i| i.to_string()).unwrap_or_default(),
                    n.status(),
                    n.mac,
                    n.model,
                    n.dsm_version(),
                    n.serial
                );
            }
            0
        }
        Err(e) => {
            eprintln!(
                "Cannot listen on UDP port {}: {e}\n(Is the old Synology Assistant still running?)",
                protocol::PORT
            );
            2
        }
    }
}

fn cli_wake(mac: Option<&String>) -> i32 {
    let Some(mac) = mac else {
        eprintln!("usage: --wake <MAC>");
        return 2;
    };
    let dev = WolDevice {
        name: String::new(),
        mac: mac.clone(),
        ip: String::new(),
        target: String::new(),
        port: 9,
    };
    match wol::wake(&dev) {
        Ok(n) => {
            println!("Sent {n} magic packets to {mac}");
            0
        }
        Err(e) => {
            eprintln!("Wake failed: {e}");
            1
        }
    }
}

fn run_gui() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Synology Assistant")
            .with_inner_size([980.0, 560.0])
            .with_min_inner_size([640.0, 360.0])
            .with_icon(std::sync::Arc::new(
                eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon_256.png")).unwrap_or_default(),
            )),
        ..Default::default()
    };
    if let Err(e) = eframe::run_native("Synology Assistant", options, Box::new(|cc| Ok(Box::new(App::new(cc))))) {
        eprintln!("{e}");
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Devices,
    Wol,
}

struct App {
    disco: Result<Discovery, String>,
    last_search: Option<Instant>,
    selected: Option<String>,
    tab: Tab,
    wol: Vec<WolDevice>,
    wol_selected: Option<usize>,
    /// "Add device" dialog for Wake on LAN.
    new_wol: Option<WolDevice>,
    show_help: bool,
    /// Devices injected by UI tests instead of live discovery.
    fake_devices: Option<Vec<Device>>,
    status: String,
    memtest: Option<MemTestDialog>,
    /// Devices seen running a memory test in this session (to report the result).
    testing: std::collections::HashSet<String>,
}

enum MemTestPhase {
    Form,
    Waiting(Instant),
    Done(Result<String, String>),
}

struct MemTestDialog {
    mac: String,
    name: String,
    account: String,
    password: String,
    /// The NAS offered no public key: the password would travel unencrypted.
    unencrypted: bool,
    accept_unencrypted: bool,
    phase: MemTestPhase,
}

const MEMTEST_ACK_TIMEOUT: Duration = Duration::from_secs(15);

/// System UI font per platform; egui's bundled fonts stay as fallback for symbols.
const SYSTEM_FONTS: &[&str] = &[
    "/System/Library/Fonts/SFNS.ttf",
    "/System/Library/Fonts/HelveticaNeue.ttc",
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/google-noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/truetype/ubuntu/Ubuntu-R.ttf",
    "/usr/share/fonts/cantarell/Cantarell-VF.otf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
];

/// Use the platform's UI font instead of egui's bundled one.
fn setup_fonts(ctx: &egui::Context) {
    let candidates = SYSTEM_FONTS;
    let Some(bytes) = candidates.iter().find_map(|p| std::fs::read(p).ok()) else {
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    let tweak = egui::FontTweak {
        hinting: Some(false),
        ..Default::default()
    };
    fonts.font_data.insert(
        "system".into(),
        std::sync::Arc::new(egui::FontData::from_owned(bytes).tweak(tweak)),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "system".into());
    ctx.set_fonts(fonts);
    ctx.global_style_mut(|style| {
        use egui::{FontFamily::Proportional, FontId, TextStyle};
        style
            .text_styles
            .insert(TextStyle::Body, FontId::new(13.5, Proportional));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::new(13.5, Proportional));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::new(11.0, Proportional));
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::new(18.0, Proportional));
    });
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = cc.egui_ctx.clone();
        let disco = Discovery::start(move || ctx.request_repaint()).map_err(|e| {
            format!(
                "Cannot listen on UDP port {} ({e}). Quit the old Synology Assistant if it is still running, then relaunch.",
                protocol::PORT
            )
        });
        Self::build(cc, disco)
    }

    fn build(cc: &eframe::CreationContext<'_>, disco: Result<Discovery, String>) -> Self {
        setup_fonts(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx);
        let status = match &disco {
            Ok(_) => "Searching…".to_string(),
            Err(e) => e.clone(),
        };
        Self {
            disco,
            last_search: None,
            selected: None,
            tab: Tab::Devices,
            wol: wol::load(),
            wol_selected: None,
            new_wol: None,
            show_help: false,
            fake_devices: None,
            status,
            memtest: None,
            testing: Default::default(),
        }
    }

    /// Report the outcome when a device we saw testing leaves the test mode,
    /// the way the original app does (field 0x79 non-zero = errors found).
    fn track_memtests(&mut self, devs: &[Device]) {
        for d in devs {
            let n = &d.info;
            if n.is_memtest_running() {
                self.testing.insert(n.mac.clone());
            } else if self.testing.remove(&n.mac) {
                self.status = if n.progress.unwrap_or(0) != 0 {
                    format!(
                        "{}: one or more errors have been found by the memory diagnostic test.",
                        n.name
                    )
                } else {
                    format!("{}: memory test finished, no errors were found.", n.name)
                };
            }
        }
    }

    fn memtest_window(&mut self, ctx: &egui::Context) {
        let Some(dlg) = self.memtest.as_mut() else { return };
        let disco = self.disco.as_ref().ok();
        // Poll for the acknowledgement.
        if let MemTestPhase::Waiting(since) = dlg.phase {
            match disco.and_then(|d| d.ack(&dlg.mac)) {
                Some((code, _)) => {
                    dlg.phase = MemTestPhase::Done(match protocol::memtest_result_message(code) {
                        Ok(()) => Ok(format!(
                            "{} is rebooting into the memory test. Progress appears in the Status column; this can take hours.",
                            dlg.name
                        )),
                        Err(m) => Err(m.to_string()),
                    })
                }
                None if since.elapsed() > MEMTEST_ACK_TIMEOUT => {
                    dlg.phase = MemTestPhase::Done(Err(format!(
                        "No answer from {}. Check that it is on this network, then try again.",
                        dlg.name
                    )))
                }
                None => {}
            }
        }
        let mut close = false;
        let mut start = false;
        let mut retry = false;
        let modal = egui::Modal::new(egui::Id::unique("memtest")).frame(dialog_frame(ctx)).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.heading("Memory Diagnostic Tool");
            ui.add_space(8.0);
            match &dlg.phase {
                MemTestPhase::Form => {
                    ui.label(format!(
                        "Access to {} will be disabled until the memory test is finished. \
                         The NAS reboots into a test mode; the time required depends on its \
                         memory size and can range from minutes to hours.",
                        dlg.name
                    ));
                    ui.add_space(6.0);
                    ui.label("Only an administrator can run the memory test. Enter a DSM administrator account:");
                    ui.add_space(4.0);
                    egui::Grid::new("memtest_form").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                        let l = ui.label("Account");
                        ui.text_edit_singleline(&mut dlg.account).labelled_by(l.id);
                        ui.end_row();
                        let l = ui.label("Password");
                        let r = ui.add(egui::TextEdit::singleline(&mut dlg.password).password(true)).labelled_by(l.id);
                        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            start = true;
                        }
                        ui.end_row();
                    });
                    if dlg.unencrypted {
                        ui.add_space(6.0);
                        ui.colored_label(
                            ui.visuals().warn_fg_color,
                            "This device did not offer an encryption key, so the password would be sent unencrypted over the network.",
                        );
                        ui.checkbox(&mut dlg.accept_unencrypted, "Send it anyway");
                    }
                    ui.add_space(12.0);
                    let ok = !dlg.account.trim().is_empty() && (!dlg.unencrypted || dlg.accept_unencrypted);
                    dialog_buttons(ui, "Start memory test", ok, &mut start, &mut close);
                    if dlg.account.trim().is_empty() || (dlg.unencrypted && !dlg.accept_unencrypted) {
                        start = false;
                    }
                }
                MemTestPhase::Waiting(_) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(format!("Sending the request to {}…", dlg.name));
                    });
                }
                MemTestPhase::Done(r) => {
                    match r {
                        Ok(m) => ui.label(m),
                        Err(m) => ui.colored_label(ui.visuals().error_fg_color, m),
                    };
                    ui.add_space(12.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if primary_button(ui, "Close", true).clicked() {
                            close = true;
                        }
                        if r.is_err() && ui.button("Try again").clicked() {
                            retry = true;
                        }
                    });
                }
            }
        });
        if modal.should_close() && !matches!(dlg.phase, MemTestPhase::Waiting(_)) {
            close = true;
        }
        if retry {
            dlg.phase = MemTestPhase::Form;
        }
        if start {
            let dev = disco.and_then(|d| d.devices.lock().unwrap().get(&dlg.mac).map(|x| x.info.clone()));
            dlg.phase = match (disco, dev) {
                (Some(d), Some(info)) => {
                    let r = d.start_memtest(&info, dlg.account.trim(), &dlg.password);
                    match r {
                        Ok(()) => MemTestPhase::Waiting(Instant::now()),
                        Err(e) => MemTestPhase::Done(Err(format!("Could not send the request: {e}"))),
                    }
                }
                _ => MemTestPhase::Done(Err("The device is no longer listed. Search again.".into())),
            };
            // Don't keep the password around longer than needed.
            dlg.password.clear();
        }
        if close {
            self.memtest = None;
        }
    }

    fn open_memtest(&mut self, d: &Device) {
        self.memtest = Some(MemTestDialog {
            mac: d.info.mac.clone(),
            name: d.info.name.clone(),
            account: String::new(),
            password: String::new(),
            unencrypted: d.info.public_key.is_empty() || d.info.session_id.is_none(),
            accept_unencrypted: false,
            phase: MemTestPhase::Form,
        });
    }

    fn search(&mut self) {
        if let Ok(d) = &self.disco {
            d.search();
            self.last_search = Some(Instant::now());
        }
    }

    fn devices(&self) -> Vec<Device> {
        let mut v = match &self.fake_devices {
            Some(f) => f.clone(),
            None => self.disco.as_ref().map(|d| d.snapshot()).unwrap_or_default(),
        };
        v.sort_by_key(|a| a.info.name.to_lowercase());
        v
    }

    fn selected_device(&self, devs: &[Device]) -> Option<Device> {
        let mac = self.selected.as_ref()?;
        devs.iter().find(|d| &d.info.mac == mac).cloned()
    }

    fn is_online(&self, mac: &str) -> bool {
        let Some(m) = wol::parse_mac(mac).map(|m| wol::format_mac(&m)) else {
            return false;
        };
        self.disco
            .as_ref()
            .ok()
            .and_then(|d| {
                d.devices
                    .lock()
                    .unwrap()
                    .get(&m)
                    .map(|x| x.last_seen.elapsed() < ONLINE_WINDOW)
            })
            .unwrap_or(false)
    }

    fn save_wol(&mut self) {
        if let Err(e) = wol::save(&self.wol) {
            self.status = format!("Could not save Wake-on-LAN list: {e}");
        }
    }

    fn add_to_wol(&mut self, d: &Device) {
        if self.wol.iter().any(|w| w.mac.eq_ignore_ascii_case(&d.info.mac)) {
            self.status = format!("{} is already in the Wake-on-LAN list.", d.info.name);
            return;
        }
        self.wol.push(WolDevice {
            name: d.info.name.clone(),
            mac: d.info.mac.clone(),
            ip: d.info.ip.map(|i| i.to_string()).unwrap_or_default(),
            target: String::new(),
            port: 9,
        });
        self.save_wol();
        self.status = format!("Added {} to the Wake-on-LAN list.", d.info.name);
    }

    fn wake(&mut self, w: &WolDevice) {
        self.status = match wol::wake(w) {
            Ok(_) => format!("Wake-up packet sent to {} ({}).", w.name, w.mac),
            Err(e) => format!("Wake-up failed for {}: {e}", w.name),
        };
    }

    fn open(&mut self, d: &Device, https: bool) {
        if let Some(url) = d.info.web_url(https)
            && let Err(e) = open::that(&url)
        {
            self.status = format!("Could not open {url}: {e}");
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.last_search.is_none_or(|t| t.elapsed() >= AUTO_SEARCH) {
            self.search();
        }
        ctx.request_repaint_after(Duration::from_secs(1));

        let devs = self.devices();
        self.track_memtests(&devs);
        let sel = self.selected_device(&devs);

        egui::Panel::top("toolbar")
            .frame(theme::chrome_frame(&ctx, egui::Margin::symmetric(10, 8)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    segmented(ui, &mut self.tab);
                    ui.add_space(8.0);
                    match self.tab {
                        Tab::Devices => {
                            if ui.button("Search").clicked() {
                                if let Ok(d) = &self.disco {
                                    d.clear();
                                }
                                self.selected = None;
                                self.search();
                            }
                            ui.add_enabled_ui(sel.is_some(), |ui| {
                                ui.menu_button("Connect", |ui| {
                                    if ui.button("Open DSM (HTTP)").clicked() {
                                        self.open(sel.as_ref().unwrap(), false);
                                    }
                                    if ui.button("Open DSM (HTTPS)").clicked() {
                                        self.open(sel.as_ref().unwrap(), true);
                                    }
                                })
                                .response
                                .on_hover_text("Open DSM in your browser (double-click a device does the same)");
                                let running = sel.as_ref().is_some_and(|d| d.info.is_memtest_running());
                                let b = ui
                                    .add_enabled(!running, egui::Button::new("Memory Test"))
                                    .on_hover_text("Reboot the NAS into Synology's memory diagnostic test")
                                    .on_disabled_hover_text("A memory test is already running");
                                if b.clicked() {
                                    self.open_memtest(sel.as_ref().unwrap());
                                }
                                if ui
                                    .button("Set up WOL")
                                    .on_hover_text("Remember this device for Wake on LAN")
                                    .clicked()
                                {
                                    self.add_to_wol(sel.as_ref().unwrap());
                                }
                            });
                        }
                        Tab::Wol => {
                            if ui.button("Add device…").clicked() {
                                self.new_wol = Some(WolDevice {
                                    name: String::new(),
                                    mac: String::new(),
                                    ip: String::new(),
                                    target: String::new(),
                                    port: 9,
                                });
                            }
                            let w = self.wol_selected.and_then(|i| self.wol.get(i).cloned());
                            ui.add_enabled_ui(w.is_some(), |ui| {
                                if ui.button("Wake Up").clicked() {
                                    self.wake(w.as_ref().unwrap());
                                }
                                if ui.button("Remove").clicked() {
                                    self.wol.remove(self.wol_selected.unwrap());
                                    self.wol_selected = None;
                                    self.save_wol();
                                }
                            });
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(egui::Button::new("Help").selected(self.show_help))
                            .on_hover_text("Show or hide help (F1)")
                            .clicked()
                        {
                            self.show_help = !self.show_help;
                        }
                    });
                });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::F1)) {
            self.show_help = !self.show_help;
        }

        egui::Panel::bottom("status")
            .frame(theme::chrome_frame(&ctx, egui::Margin::symmetric(10, 4)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let n = if self.tab == Tab::Wol {
                        self.wol.len()
                    } else {
                        devs.len()
                    };
                    let what = if self.tab == Tab::Wol { "saved device" } else { "device" };
                    ui.label(format!("{n} {what}{}", if n == 1 { "" } else { "s" }));
                    let msg = if self.status == "Searching…" && !devs.is_empty() {
                        ""
                    } else {
                        self.status.as_str()
                    };
                    if !msg.is_empty() {
                        ui.separator();
                        ui.label(msg);
                    }
                });
            });

        if self.show_help {
            let code = sel.as_ref().and_then(|d| d.info.status_code);
            let tab = self.tab;
            egui::Panel::right("help")
                .frame(theme::chrome_frame(&ctx, egui::Margin::symmetric(12, 6)))
                .resizable(true)
                .default_size(340.0)
                .size_range(260.0..=560.0)
                .show(ui, |ui| help_panel(ui, tab, code));
        }

        match self.tab {
            Tab::Devices => {
                if let Some(d) = &sel {
                    egui::Panel::bottom("details")
                        .frame(theme::chrome_frame(&ctx, egui::Margin::symmetric(12, 8)))
                        .resizable(false)
                        .show(ui, |ui| details(ui, &d.info));
                }
                egui::CentralPanel::default().show(ui, |ui| self.device_table(ui, &devs));
            }
            Tab::Wol => {
                egui::CentralPanel::default().show(ui, |ui| self.wol_view(ui));
            }
        }
        self.memtest_window(&ctx);
        self.add_wol_window(&ctx);
    }
}

impl App {
    fn device_table(&mut self, ui: &mut egui::Ui, devs: &[Device]) {
        use egui_extras::{Column, TableBuilder};
        if devs.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(if self.disco.is_ok() {
                    "Searching for Synology devices on your network…"
                } else {
                    self.status.as_str()
                });
            });
            return;
        }
        let mut open_req = None;
        let mut action: Option<(Device, RowAction)> = None;
        let pal = theme::palette(ui);
        quiet_column_lines(ui);
        TableBuilder::new(ui)
            .striped(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(150.0).at_least(80.0).resizable(true))
            .column(Column::initial(115.0).at_least(80.0).resizable(true))
            .column(Column::initial(230.0).at_least(110.0).resizable(true))
            .column(Column::initial(135.0).resizable(true))
            .column(Column::initial(80.0).resizable(true))
            .column(Column::initial(110.0).resizable(true))
            .column(Column::remainder())
            .header(ROW_H, |mut h| {
                for t in [
                    "Server name",
                    "IP address",
                    "Status",
                    "MAC address",
                    "Model",
                    "Version",
                    "Serial no",
                ] {
                    h.col(|ui| header_cell(ui, t));
                }
            })
            .body(|mut body| {
                for d in devs {
                    let n = &d.info;
                    body.row(ROW_H, |mut row| {
                        row.set_selected(self.selected.as_deref() == Some(n.mac.as_str()));
                        let stale = d.last_seen.elapsed() > ONLINE_WINDOW;
                        let (status, color) = if stale {
                            ("Offline".to_string(), pal.gray)
                        } else {
                            (n.status(), status_color(pal, n.status_code))
                        };
                        row.col(|ui| cell(ui, &n.name));
                        row.col(|ui| cell(ui, &n.ip.map(|i| i.to_string()).unwrap_or_default()));
                        row.col(|ui| {
                            status_dot(ui, color);
                            cell(ui, &status);
                        });
                        row.col(|ui| cell(ui, &n.mac));
                        row.col(|ui| cell(ui, &n.model));
                        row.col(|ui| cell(ui, &n.dsm_version()));
                        row.col(|ui| cell(ui, &n.serial));
                        let r = row.response();
                        if r.clicked() || r.secondary_clicked() {
                            self.selected = Some(n.mac.clone());
                        }
                        if r.double_clicked() {
                            open_req = Some(d.clone());
                        }
                        r.context_menu(|ui| {
                            let mut pick = |a| action = Some((d.clone(), a));
                            if ui.button("Open DSM (HTTP)").clicked() {
                                pick(RowAction::Http);
                            }
                            if ui.button("Open DSM (HTTPS)").clicked() {
                                pick(RowAction::Https);
                            }
                            ui.separator();
                            if ui.button("Copy IP address").clicked() {
                                pick(RowAction::CopyIp);
                            }
                            if ui.button("Copy MAC address").clicked() {
                                pick(RowAction::CopyMac);
                            }
                            ui.separator();
                            if ui.button("Set up WOL").clicked() {
                                pick(RowAction::Wol);
                            }
                            if ui
                                .add_enabled(!n.is_memtest_running(), egui::Button::new("Memory Test…"))
                                .clicked()
                            {
                                pick(RowAction::MemTest);
                            }
                        });
                    });
                }
            });
        if let Some(d) = open_req {
            self.open(&d, false);
        }
        if let Some((d, a)) = action {
            match a {
                RowAction::Http => self.open(&d, false),
                RowAction::Https => self.open(&d, true),
                RowAction::CopyIp => ui.ctx().copy_text(d.info.ip.map(|i| i.to_string()).unwrap_or_default()),
                RowAction::CopyMac => ui.ctx().copy_text(d.info.mac.clone()),
                RowAction::Wol => self.add_to_wol(&d),
                RowAction::MemTest => self.open_memtest(&d),
            }
        }
    }

    fn wol_view(&mut self, ui: &mut egui::Ui) {
        use egui_extras::{Column, TableBuilder};
        if self.wol.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    "No devices registered for Wake on LAN yet.\n\n\
                     Select a running NAS under Management and click Set up WOL,\n\
                     or click Add device… to enter a MAC address.",
                );
            });
            return;
        }
        let mut wake_req = None;
        let pal = theme::palette(ui);
        quiet_column_lines(ui);
        let online: Vec<bool> = self.wol.iter().map(|w| self.is_online(&w.mac)).collect();
        TableBuilder::new(ui)
            .striped(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(170.0).at_least(80.0).resizable(true))
            .column(Column::initial(140.0).at_least(80.0).resizable(true))
            .column(Column::initial(120.0).at_least(60.0).resizable(true))
            .column(Column::initial(140.0).at_least(60.0).resizable(true))
            .column(Column::initial(50.0))
            .column(Column::remainder())
            .header(ROW_H, |mut h| {
                for t in ["Name", "MAC address", "Last IP", "Target", "Port", "Status"] {
                    h.col(|ui| header_cell(ui, t));
                }
            })
            .body(|mut body| {
                for (i, w) in self.wol.iter().enumerate() {
                    body.row(ROW_H, |mut row| {
                        row.set_selected(self.wol_selected == Some(i));
                        let (st, color) = if online[i] {
                            ("On", pal.green)
                        } else {
                            ("Off / unknown", pal.gray)
                        };
                        let target = if w.target.is_empty() {
                            "Local network".to_string()
                        } else {
                            w.target.clone()
                        };
                        for c in [w.name.clone(), w.mac.clone(), w.ip.clone(), target, w.port.to_string()] {
                            row.col(|ui| cell(ui, &c));
                        }
                        row.col(|ui| {
                            status_dot(ui, color);
                            cell(ui, st);
                        });
                        let r = row.response();
                        if r.clicked() {
                            self.wol_selected = Some(i);
                        }
                        if r.double_clicked() {
                            wake_req = Some(w.clone());
                        }
                    });
                }
            });
        if let Some(w) = wake_req {
            self.wake(&w);
        }
    }

    fn add_wol_window(&mut self, ctx: &egui::Context) {
        let Some(w) = self.new_wol.as_mut() else { return };
        let mut close = false;
        let mut add = false;
        let valid = wol::parse_mac(&w.mac);
        let modal = egui::Modal::new(egui::Id::unique("addwol")).frame(dialog_frame(ctx)).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.heading("Add a device for Wake on LAN");
            ui.add_space(10.0);
            egui::Grid::new("addwol_form").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                let l = ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut w.name).hint_text("My NAS").desired_width(220.0)).labelled_by(l.id);
                ui.end_row();
                let l = ui.label("MAC address");
                ui.add(egui::TextEdit::singleline(&mut w.mac).hint_text("00:11:32:xx:xx:xx").desired_width(220.0)).labelled_by(l.id);
                ui.end_row();
                let l = ui.label("Target");
                ui.add(egui::TextEdit::singleline(&mut w.target).hint_text("Optional").desired_width(220.0))
                    .labelled_by(l.id)
                    .on_hover_text("Leave empty for the local network. For remote wake-up: a directed broadcast (192.168.1.255) or a host name with UDP forwarding.");
                ui.end_row();
                let l = ui.label("Port");
                ui.add(egui::DragValue::new(&mut w.port).range(1..=65535)).labelled_by(l.id);
                ui.end_row();
            });
            if !w.mac.trim().is_empty() && valid.is_none() {
                ui.colored_label(ui.visuals().error_fg_color, "Enter a MAC address like 00:11:32:12:34:56.");
            }
            ui.add_space(12.0);
            dialog_buttons(ui, "Add", valid.is_some(), &mut add, &mut close);
        });
        if modal.should_close() {
            close = true;
        }
        if add {
            let mut w = self.new_wol.take().unwrap();
            w.mac = wol::format_mac(&valid.unwrap());
            if w.name.trim().is_empty() {
                w.name = w.mac.clone();
            }
            self.status = format!("Added {} to the Wake-on-LAN list.", w.name);
            self.wol.push(w);
            self.save_wol();
        } else if close {
            self.new_wol = None;
        }
    }
}

const ROW_H: f32 = 28.0;

/// Two-segment tab switcher (Management / Wake on LAN).
fn segmented(ui: &mut egui::Ui, tab: &mut Tab) {
    let pal = theme::palette(ui);
    egui::Frame::new()
        .fill(pal.control_active)
        .corner_radius(8)
        .inner_margin(egui::Margin::same(2))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (t, label) in [(Tab::Devices, "Management"), (Tab::Wol, "Wake on LAN")] {
                let on = *tab == t;
                let text = egui::RichText::new(label).color(if on { pal.text } else { pal.text_secondary });
                let b = egui::Button::new(text)
                    .fill(if on { pal.control } else { egui::Color32::TRANSPARENT })
                    .stroke(egui::Stroke::NONE)
                    .corner_radius(6);
                if ui.add(b).clicked() {
                    *tab = t;
                }
            }
        });
}

/// Column dividers only appear while hovering/resizing, like macOS tables.
fn quiet_column_lines(ui: &mut egui::Ui) {
    ui.visuals_mut().widgets.noninteractive.bg_stroke.color = egui::Color32::TRANSPARENT;
}

fn header_cell(ui: &mut egui::Ui, text: &str) {
    let pal = theme::palette(ui);
    let rect = ui.max_rect();
    let x = rect.x_range().expand(ui.spacing().item_spacing.x);
    ui.painter()
        .hline(x, rect.bottom(), egui::Stroke::new(1.0, pal.separator));
    ui.add(
        egui::Label::new(egui::RichText::new(text).color(pal.text_secondary).strong())
            .truncate()
            .selectable(false),
    );
}

fn primary_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
    let pal = theme::palette(ui);
    let b = egui::Button::new(egui::RichText::new(text).color(egui::Color32::WHITE))
        .fill(pal.accent)
        .stroke(egui::Stroke::NONE);
    ui.add_enabled(enabled, b)
}

/// macOS-style dialog buttons: primary action on the right, Cancel to its left.
fn dialog_buttons(ui: &mut egui::Ui, primary: &str, enabled: bool, ok: &mut bool, cancel: &mut bool) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if primary_button(ui, primary, enabled).clicked() {
            *ok = true;
        }
        if ui.button("Cancel").clicked() {
            *cancel = true;
        }
    });
}

fn dialog_frame(ctx: &egui::Context) -> egui::Frame {
    egui::Frame::popup(&ctx.global_style())
        .inner_margin(egui::Margin::same(20))
        .corner_radius(12)
}

#[derive(Clone, Copy)]
enum RowAction {
    Http,
    Https,
    CopyIp,
    CopyMac,
    Wol,
    MemTest,
}

/// Single-line table cell: never wraps, shows "…" and the full text on hover when too long.
fn cell(ui: &mut egui::Ui, text: &str) {
    let r = ui.add(egui::Label::new(text).truncate().selectable(false));
    if r.hovered()
        && ui.fonts_mut(|f| {
            f.layout_no_wrap(
                text.to_string(),
                egui::TextStyle::Body.resolve(ui.style()),
                egui::Color32::WHITE,
            )
            .size()
            .x
        }) > r.rect.width() + 0.5
    {
        r.on_hover_text(text);
    }
}

fn status_dot(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
}

fn status_color(p: &theme::Palette, code: Option<u32>) -> egui::Color32 {
    match code {
        Some(1) => p.green,
        Some(3 | 5 | 6 | 7 | 9 | 13) => p.orange,
        Some(0 | 2 | 4 | 11 | 14) => p.red,
        _ => p.gray,
    }
}

fn help_panel(ui: &mut egui::Ui, tab: Tab, selected_status: Option<u32>) {
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        ui.add_space(6.0);
        ui.heading("Help");
        ui.add_space(4.0);
        if let Some(code) = selected_status {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.strong("Selected device");
                ui.label(help::status_explanation(Some(code)));
            });
            ui.add_space(6.0);
        }
        let section = |ui: &mut egui::Ui, s: &help::Section, open: bool| {
            egui::CollapsingHeader::new(egui::RichText::new(s.title).strong())
                .default_open(open)
                .show(ui, |ui| {
                    for p in s.body {
                        ui.label(*p);
                        ui.add_space(4.0);
                    }
                });
        };
        section(ui, &help::GETTING_STARTED, tab == Tab::Devices);
        egui::CollapsingHeader::new(egui::RichText::new("Device status").strong())
            .default_open(false)
            .show(ui, |ui| {
                for (name, code) in help::STATUSES {
                    ui.label(egui::RichText::new(*name).strong());
                    ui.label(help::status_explanation(Some(*code)));
                    ui.add_space(4.0);
                }
            });
        section(ui, &help::WOL, tab == Tab::Wol);
        section(ui, &help::MEMTEST, false);
        section(ui, &help::TROUBLESHOOTING, false);
        section(ui, &help::ABOUT, false);
        ui.add_space(8.0);
        ui.hyperlink_to("Project page & feedback", env!("CARGO_PKG_REPOSITORY"));
    });
}

fn details(ui: &mut egui::Ui, n: &protocol::NasInfo) {
    let ip = |a: Option<std::net::Ipv4Addr>| a.map(|x| x.to_string()).unwrap_or_else(|| "–".into());
    let key_color = theme::palette(ui).text_secondary;
    let key = |ui: &mut egui::Ui, t: &str| {
        ui.label(egui::RichText::new(t).color(key_color));
    };
    ui.add_space(2.0);
    egui::Grid::new("details")
        .num_columns(4)
        .spacing([24.0, 4.0])
        .show(ui, |ui| {
            key(ui, "Server name");
            ui.label(&n.name);
            key(ui, "Model");
            ui.label(format!("{} ({})", n.model, n.unique));
            ui.end_row();
            key(ui, "IP address");
            ui.label(format!(
                "{}{}",
                ip(n.ip),
                match n.dhcp {
                    Some(true) => "  (DHCP)",
                    Some(false) => "  (static)",
                    None => "",
                }
            ));
            key(ui, "Operating system");
            ui.label(format!("{} {}", n.os_name, n.dsm_version()));
            ui.end_row();
            key(ui, "Subnet mask");
            ui.label(ip(n.netmask));
            key(ui, "Status");
            ui.label(n.status())
                .on_hover_text(help::status_explanation(n.status_code));
            ui.end_row();
            key(ui, "Gateway");
            ui.label(ip(n.gateway));
            key(ui, "Serial no");
            ui.label(&n.serial);
            ui.end_row();
            key(ui, "DNS");
            ui.label(ip(n.dns));
            key(ui, "Web");
            if let Some(u) = n.web_url(false) {
                ui.hyperlink(u);
            }
            ui.end_row();
        });
    ui.add_space(4.0);
}

/// Renders the UI to PNGs in target/ui-shots for visual review:
/// `cargo test --release ui_snapshots -- --ignored`
#[cfg(test)]
mod ui_snapshots {
    use super::*;
    use egui_kittest::Harness;
    use protocol::NasInfo;

    fn dev(name: &str, ip: u8, code: u32, progress: Option<u32>, model: &str) -> Device {
        Device {
            info: NasInfo {
                packet_type: protocol::PKT_INFO,
                name: name.into(),
                mac: format!("00:11:32:00:00:{ip:02x}"),
                ip: Some(std::net::Ipv4Addr::new(192, 168, 1, ip)),
                netmask: Some(std::net::Ipv4Addr::new(255, 255, 255, 0)),
                gateway: Some(std::net::Ipv4Addr::new(192, 168, 1, 1)),
                dns: Some(std::net::Ipv4Addr::new(192, 168, 1, 1)),
                dhcp: Some(true),
                model: model.into(),
                unique: "synology_geminilake_220+".into(),
                os_name: "DSM".into(),
                version: "7.4.1".into(),
                build: Some(90080),
                serial: "0000ABCDEFGHI".into(),
                status_code: Some(code),
                progress,
                public_key: "00".repeat(32),
                session_id: Some(1),
                ..Default::default()
            },
            last_seen: Instant::now(),
        }
    }

    fn shot(theme: egui::Theme, name: &str, setup: impl FnOnce(&mut App)) {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1000.0, 560.0))
            .with_pixels_per_point(2.0)
            .with_theme(theme)
            .build_eframe(|cc| {
                let mut app = App::build(cc, Err("test".into()));
                app.fake_devices = Some(vec![
                    dev("DS220plus", 15, 9, Some(7536), "DS220+"),
                    dev("backup-nas", 20, 1, None, "DS923+"),
                    dev("new-box", 30, 2, None, "DS224+"),
                ]);
                app.status.clear();
                app
            });
        setup(h.state_mut());
        h.run();
        let img = h.render().expect("render");
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/ui-shots");
        std::fs::create_dir_all(&dir).unwrap();
        img.save(dir.join(format!("{name}.png"))).unwrap();
    }

    #[test]
    #[ignore = "renders PNGs for manual review"]
    fn ui_snapshots() {
        for (t, tn) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
            shot(t, &format!("{tn}-devices"), |a| {
                a.selected = Some("00:11:32:00:00:0f".into())
            });
            shot(t, &format!("{tn}-help"), |a| {
                a.selected = Some("00:11:32:00:00:0f".into());
                a.show_help = true;
            });
            shot(t, &format!("{tn}-wol"), |a| {
                a.tab = Tab::Wol;
                a.wol = vec![WolDevice {
                    name: "DS220plus".into(),
                    mac: "00:11:32:00:00:0f".into(),
                    ip: "192.168.1.15".into(),
                    target: String::new(),
                    port: 9,
                }];
            });
            shot(t, &format!("{tn}-addwol"), |a| {
                a.tab = Tab::Wol;
                a.wol.clear();
                a.new_wol = Some(WolDevice {
                    name: String::new(),
                    mac: "00:11".into(),
                    ip: String::new(),
                    target: String::new(),
                    port: 9,
                });
            });
            shot(t, &format!("{tn}-memtest"), |a| {
                a.selected = Some("00:11:32:00:00:14".into());
                let d = a.fake_devices.as_ref().unwrap()[1].clone();
                a.open_memtest(&d);
            });
        }
    }
}
