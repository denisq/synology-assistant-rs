// Release builds on Windows are GUI apps (no console window); CLI flags re-attach to the parent console.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use synology_assistant_rs::{discovery, help, protocol, wol};
use discovery::{Device, Discovery};
use eframe::egui;
use std::time::{Duration, Instant};
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
            println!("Synology Assistant (native)\n\n  (no args)      open the app\n  --scan         list Synology devices on the LAN\n  --wake <MAC>   send a Wake-on-LAN packet");
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
            println!("{:<18} {:<16} {:<32} {:<18} {:<10} {:<14} {}", "NAME", "IP", "STATUS", "MAC", "MODEL", "DSM", "SERIAL");
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
            eprintln!("Cannot listen on UDP port {}: {e}\n(Is the old Synology Assistant still running?)", protocol::PORT);
            2
        }
    }
}

fn cli_wake(mac: Option<&String>) -> i32 {
    let Some(mac) = mac else {
        eprintln!("usage: --wake <MAC>");
        return 2;
    };
    let dev = WolDevice { name: String::new(), mac: mac.clone(), ip: String::new(), target: String::new(), port: 9 };
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
    let Some(bytes) = candidates.iter().find_map(|p| std::fs::read(p).ok()) else { return };
    let mut fonts = egui::FontDefinitions::default();
    let tweak = egui::FontTweak { hinting: Some(false), ..Default::default() };
    fonts
        .font_data
        .insert("system".into(), std::sync::Arc::new(egui::FontData::from_owned(bytes).tweak(tweak)));
    fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "system".into());
    ctx.set_fonts(fonts);
    ctx.global_style_mut(|style| {
        use egui::{FontFamily::Proportional, FontId, TextStyle};
        style.text_styles.insert(TextStyle::Body, FontId::new(13.5, Proportional));
        style.text_styles.insert(TextStyle::Button, FontId::new(13.5, Proportional));
        style.text_styles.insert(TextStyle::Small, FontId::new(11.0, Proportional));
        style.text_styles.insert(TextStyle::Heading, FontId::new(18.0, Proportional));
    });
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);
        let ctx = cc.egui_ctx.clone();
        let disco = Discovery::start(move || ctx.request_repaint()).map_err(|e| {
            format!(
                "Cannot listen on UDP port {} ({e}). Quit the old Synology Assistant if it is still running, then relaunch.",
                protocol::PORT
            )
        });
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
                    format!("{}: one or more errors have been found by the memory diagnostic test.", n.name)
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
        let modal = egui::Modal::new(egui::Id::unique("memtest")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.heading("Memory Diagnostic Tool");
            ui.add_space(6.0);
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
                        ui.label("Account");
                        ui.text_edit_singleline(&mut dlg.account);
                        ui.end_row();
                        ui.label("Password");
                        let r = ui.add(egui::TextEdit::singleline(&mut dlg.password).password(true));
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
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let ok = !dlg.account.trim().is_empty() && (!dlg.unencrypted || dlg.accept_unencrypted);
                        if ui.add_enabled(ok, egui::Button::new("Start memory test")).clicked() {
                            start = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
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
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if r.is_err() && ui.button("Try again").clicked() {
                            retry = true;
                        }
                        if ui.button("Close").clicked() {
                            close = true;
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

    fn search(&mut self) {
        if let Ok(d) = &self.disco {
            d.search();
            self.last_search = Some(Instant::now());
        }
    }

    fn devices(&self) -> Vec<Device> {
        let mut v = self.disco.as_ref().map(|d| d.snapshot()).unwrap_or_default();
        v.sort_by(|a, b| a.info.name.to_lowercase().cmp(&b.info.name.to_lowercase()));
        v
    }

    fn selected_device(&self, devs: &[Device]) -> Option<Device> {
        let mac = self.selected.as_ref()?;
        devs.iter().find(|d| &d.info.mac == mac).cloned()
    }

    fn is_online(&self, mac: &str) -> bool {
        let Some(m) = wol::parse_mac(mac).map(|m| wol::format_mac(&m)) else { return false };
        self.disco
            .as_ref()
            .ok()
            .and_then(|d| d.devices.lock().unwrap().get(&m).map(|x| x.last_seen.elapsed() < ONLINE_WINDOW))
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
        if let Some(url) = d.info.web_url(https) {
            if let Err(e) = open::that(&url) {
                self.status = format!("Could not open {url}: {e}");
            }
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

        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Devices, "Management");
                ui.selectable_value(&mut self.tab, Tab::Wol, "Wake on LAN");
                ui.separator();
                match self.tab {
                    Tab::Devices => {
                        if ui.button("🔍 Search").clicked() {
                            if let Ok(d) = &self.disco {
                                d.clear();
                            }
                            self.selected = None;
                            self.search();
                        }
                        ui.add_enabled_ui(sel.is_some(), |ui| {
                            if ui.button("Connect").on_hover_text("Open DSM in your browser (HTTP)").clicked() {
                                self.open(sel.as_ref().unwrap(), false);
                            }
                            if ui.button("Connect (HTTPS)").clicked() {
                                self.open(sel.as_ref().unwrap(), true);
                            }
                            let running = sel.as_ref().is_some_and(|d| d.info.is_memtest_running());
                            let b = ui
                                .add_enabled(!running, egui::Button::new("Memory Test"))
                                .on_hover_text("Reboot the NAS into Synology's memory diagnostic test")
                                .on_disabled_hover_text("A memory test is already running");
                            if b.clicked() {
                                let d = sel.as_ref().unwrap();
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
                            if ui.button("Set up WOL").on_hover_text("Remember this device for Wake on LAN").clicked() {
                                self.add_to_wol(sel.as_ref().unwrap());
                            }
                            if ui.button("Copy IP").clicked() {
                                if let Some(ip) = sel.as_ref().unwrap().info.ip {
                                    ui.ctx().copy_text(ip.to_string());
                                }
                            }
                        });
                    }
                    Tab::Wol => {
                        if ui.button("➕ Add device…").clicked() {
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
                            if ui.button("⏻ Wake Up").clicked() {
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
                    let label = if self.show_help { "✕ Help" } else { "? Help" };
                    if ui.selectable_label(self.show_help, label).on_hover_text("Show help (F1)").clicked() {
                        self.show_help = !self.show_help;
                    }
                });
            });
            ui.add_space(4.0);
        });
        if ctx.input(|i| i.key_pressed(egui::Key::F1)) {
            self.show_help = !self.show_help;
        }

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("{} device(s)", devs.len()));
                ui.separator();
                let msg = if self.status == "Searching…" && !devs.is_empty() { "" } else { self.status.as_str() };
                ui.label(msg);
            });
        });

        if self.show_help {
            let code = sel.as_ref().and_then(|d| d.info.status_code);
            let tab = self.tab;
            egui::Panel::right("help").resizable(true).default_size(340.0).size_range(260.0..=560.0).show(ui, |ui| {
                help_panel(ui, tab, code)
            });
        }

        match self.tab {
            Tab::Devices => {
                if let Some(d) = &sel {
                    egui::Panel::bottom("details").resizable(false).show(ui, |ui| details(ui, &d.info));
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
                ui.label(if self.disco.is_ok() { "Searching for Synology devices on your network…" } else { self.status.as_str() });
            });
            return;
        }
        let mut open_req = None;
        TableBuilder::new(ui)
            .striped(true)
            .sense(egui::Sense::click())
            .column(Column::initial(150.0).resizable(true))
            .column(Column::initial(115.0).resizable(true))
            .column(Column::initial(110.0).resizable(true))
            .column(Column::initial(135.0).resizable(true))
            .column(Column::initial(80.0).resizable(true))
            .column(Column::initial(110.0).resizable(true))
            .column(Column::remainder())
            .header(22.0, |mut h| {
                for t in ["Server name", "IP address", "Status", "MAC address", "Model", "Version", "Serial no"] {
                    h.col(|ui| {
                        ui.strong(t);
                    });
                }
            })
            .body(|mut body| {
                for d in devs {
                    let n = &d.info;
                    body.row(22.0, |mut row| {
                        row.set_selected(self.selected.as_deref() == Some(n.mac.as_str()));
                        let stale = d.last_seen.elapsed() > ONLINE_WINDOW;
                        let cells = [
                            n.name.clone(),
                            n.ip.map(|i| i.to_string()).unwrap_or_default(),
                            if stale { "Offline".into() } else { n.status() },
                            n.mac.clone(),
                            n.model.clone(),
                            n.dsm_version(),
                            n.serial.clone(),
                        ];
                        for c in cells {
                            row.col(|ui| {
                                ui.label(c);
                            });
                        }
                        let r = row.response();
                        if r.clicked() {
                            self.selected = Some(n.mac.clone());
                        }
                        if r.double_clicked() {
                            open_req = Some(d.clone());
                        }
                    });
                }
            });
        if let Some(d) = open_req {
            self.open(&d, false);
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
        let online: Vec<bool> = self.wol.iter().map(|w| self.is_online(&w.mac)).collect();
        TableBuilder::new(ui)
            .striped(true)
            .sense(egui::Sense::click())
            .column(Column::initial(170.0).resizable(true))
            .column(Column::initial(140.0).resizable(true))
            .column(Column::initial(120.0).resizable(true))
            .column(Column::initial(140.0).resizable(true))
            .column(Column::initial(50.0))
            .column(Column::remainder())
            .header(22.0, |mut h| {
                for t in ["Name", "MAC address", "Last IP", "Target", "Port", "Status"] {
                    h.col(|ui| {
                        ui.strong(t);
                    });
                }
            })
            .body(|mut body| {
                for (i, w) in self.wol.iter().enumerate() {
                    body.row(22.0, |mut row| {
                        row.set_selected(self.wol_selected == Some(i));
                        let st = if online[i] { "On" } else { "Off / unknown" };
                        let target = if w.target.is_empty() { "Local network".to_string() } else { w.target.clone() };
                        for c in [w.name.clone(), w.mac.clone(), w.ip.clone(), target, w.port.to_string(), st.into()] {
                            row.col(|ui| {
                                ui.label(c);
                            });
                        }
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
        let modal = egui::Modal::new(egui::Id::unique("addwol")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading("Add a device for Wake on LAN");
            ui.add_space(8.0);
            egui::Grid::new("addwol_form").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut w.name).hint_text("My NAS").desired_width(220.0));
                ui.end_row();
                ui.label("MAC address");
                ui.add(egui::TextEdit::singleline(&mut w.mac).hint_text("00:11:32:xx:xx:xx").desired_width(220.0));
                ui.end_row();
                ui.label("Target");
                ui.add(egui::TextEdit::singleline(&mut w.target).hint_text("Optional").desired_width(220.0))
                    .on_hover_text("Leave empty for the local network. For remote wake-up: a directed broadcast (192.168.1.255) or a host name with UDP forwarding.");
                ui.end_row();
                ui.label("Port");
                ui.add(egui::DragValue::new(&mut w.port).range(1..=65535));
                ui.end_row();
            });
            if !w.mac.trim().is_empty() && valid.is_none() {
                ui.colored_label(ui.visuals().error_fg_color, "Enter a MAC address like 00:11:32:12:34:56.");
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(valid.is_some(), egui::Button::new("Add")).clicked() {
                    add = true;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
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
            egui::CollapsingHeader::new(egui::RichText::new(s.title).strong()).default_open(open).show(ui, |ui| {
                for p in s.body {
                    ui.label(*p);
                    ui.add_space(4.0);
                }
            });
        };
        section(ui, &help::GETTING_STARTED, tab == Tab::Devices);
        egui::CollapsingHeader::new(egui::RichText::new("Device status").strong()).default_open(false).show(ui, |ui| {
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
    ui.add_space(4.0);
    egui::Grid::new("details").num_columns(4).spacing([24.0, 4.0]).show(ui, |ui| {
        ui.strong("Server name");
        ui.label(&n.name);
        ui.strong("Model");
        ui.label(format!("{} ({})", n.model, n.unique));
        ui.end_row();
        ui.strong("IP address");
        ui.label(format!(
            "{}{}",
            ip(n.ip),
            match n.dhcp {
                Some(true) => "  (DHCP)",
                Some(false) => "  (static)",
                None => "",
            }
        ));
        ui.strong("Operating system");
        ui.label(format!("{} {}", n.os_name, n.dsm_version()));
        ui.end_row();
        ui.strong("Subnet mask");
        ui.label(ip(n.netmask));
        ui.strong("Status");
        ui.label(n.status()).on_hover_text(help::status_explanation(n.status_code));
        ui.end_row();
        ui.strong("Gateway");
        ui.label(ip(n.gateway));
        ui.strong("Serial no");
        ui.label(&n.serial);
        ui.end_row();
        ui.strong("DNS");
        ui.label(ip(n.dns));
        ui.strong("Web");
        if let Some(u) = n.web_url(false) {
            ui.hyperlink(u);
        }
        ui.end_row();
    });
    ui.add_space(4.0);
}
