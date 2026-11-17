// Release builds on Windows are GUI apps (no console window); CLI flags re-attach to the parent console.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod theme;

use discovery::{Device, Discovery};
use eframe::egui;
use protocol::NasInfo;
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};
use synology_assistant_rs::{discovery, help, known, memtest, net, protocol, store, wol};
use wol::WolDevice;

const AUTO_SEARCH: Duration = Duration::from_secs(10);
const ONLINE_WINDOW: Duration = Duration::from_secs(35);
/// How long "Wake Up" keeps waiting for the device to answer.
const WAKE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// One line of the device list: a device answering now, or one remembered from before.
#[derive(Clone)]
struct Row {
    info: NasInfo,
    online: bool,
    /// Unix seconds.
    last_seen: u64,
}

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
    /// Memory-test runs per device (persisted).
    history: memtest::History,
    /// Devices seen before (persisted), shown greyed out while offline.
    known: known::KnownDevices,
    /// Last time the persisted files were written.
    saved_at: Instant,
    dirty: bool,
    /// This computer's networks, to warn about a NAS on another subnet.
    local_nets: Vec<(Ipv4Addr, Ipv4Addr)>,
    /// Devices we sent a wake-up to: MAC -> (when, name).
    waking: HashMap<String, (Instant, String)>,
    /// False in UI tests: no files written, no notifications.
    live: bool,
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
        let mut app = Self::build(cc, disco);
        app.live = true;
        app.history = memtest::History::load();
        app.known = known::KnownDevices::load();
        app.local_nets = net::local_networks();
        #[cfg(target_os = "macos")]
        let _ = notify_rust::set_application("io.github.denisq.synology-assistant-rs");
        app
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
            history: Default::default(),
            known: Default::default(),
            saved_at: Instant::now(),
            dirty: false,
            local_nets: Vec::new(),
            waking: HashMap::new(),
            live: false,
        }
    }

    /// Desktop notification plus a Dock/taskbar bounce. Never blocks the UI.
    fn notify(&self, ctx: &egui::Context, title: &str, body: &str) {
        if !self.live {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
            egui::UserAttentionType::Informational,
        ));
        let (title, body) = (title.to_string(), body.to_string());
        std::thread::spawn(move || {
            let _ = notify_rust::Notification::new()
                .appname("Synology Assistant")
                .summary(&title)
                .body(&body)
                .show();
        });
    }

    /// Update remembered devices, memory-test history and pending wake-ups from
    /// what is on the network right now.
    fn track(&mut self, ctx: &egui::Context, live: &[Device]) {
        let now = store::now();
        for d in live.iter().filter(|d| d.last_seen.elapsed() < ONLINE_WINDOW) {
            let n = &d.info;
            let prev = self
                .known
                .devices
                .get(&n.mac)
                .map(|k| (k.info.status_code, k.info.progress));
            self.known.seen(n, now);
            if prev != Some((n.status_code, n.progress)) {
                self.dirty = true;
            }
            if let memtest::Event::Finished(outcome) = self.history.observe(n, now) {
                self.dirty = true;
                let (title, body) = match outcome {
                    memtest::Outcome::NoErrors => (
                        format!("{}: memory test passed", n.name),
                        "No errors were found. Details are in the DSM system log.".to_string(),
                    ),
                    memtest::Outcome::Errors => (
                        format!("{}: memory errors found", n.name),
                        "One or more errors were found. Check the DSM system log and contact Synology support."
                            .to_string(),
                    ),
                };
                self.status = format!("{title}. {body}");
                self.notify(ctx, &title, &body);
            }
            if let Some((_, name)) = self.waking.remove(&n.mac) {
                let msg = format!("{name} is awake");
                self.status = format!("{msg}. Double-click it to open DSM.");
                self.notify(ctx, &msg, "It answered on the network.");
            }
        }
        let expired: Vec<String> = self
            .waking
            .iter()
            .filter(|(_, (t, _))| t.elapsed() > WAKE_TIMEOUT)
            .map(|(m, _)| m.clone())
            .collect();
        for m in expired {
            if let Some((_, name)) = self.waking.remove(&m) {
                self.status = format!(
                    "{name} did not answer within 5 minutes. Check that Wake on LAN is enabled in DSM and that it is plugged in."
                );
            }
        }
        if self.live && self.dirty && self.saved_at.elapsed() > Duration::from_secs(30) {
            let _ = self.known.save();
            let _ = self.history.save();
            self.dirty = false;
            self.saved_at = Instant::now();
        }
    }

    fn save_now(&mut self) {
        if self.live {
            let _ = self.known.save();
            let _ = self.history.save();
            self.dirty = false;
            self.saved_at = Instant::now();
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
                        Ok(()) => {
                            self.history.started(&dlg.mac, &dlg.name, store::now());
                            self.dirty = true;
                            self.saved_at = Instant::now() - Duration::from_secs(60);
                            Ok(format!(
                                "{} is rebooting into the memory test. Progress and an estimate of the time left \
                                 appear in the Status column; you'll get a notification when it finishes.",
                                dlg.name
                            ))
                        }
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

    fn open_memtest(&mut self, n: &NasInfo) {
        // Use the live record: it carries the session key the remembered copy lacks.
        let live = self
            .disco
            .as_ref()
            .ok()
            .and_then(|d| d.devices.lock().unwrap().get(&n.mac).map(|x| x.info.clone()));
        let n = live.as_ref().unwrap_or(n);
        self.memtest = Some(MemTestDialog {
            mac: n.mac.clone(),
            name: n.name.clone(),
            account: String::new(),
            password: String::new(),
            unencrypted: n.public_key.is_empty() || n.session_id.is_none(),
            accept_unencrypted: false,
            phase: MemTestPhase::Form,
        });
    }

    fn search(&mut self) {
        if let Ok(d) = &self.disco {
            d.search();
            self.last_search = Some(Instant::now());
            if self.live {
                self.local_nets = net::local_networks();
            }
        }
    }

    fn live_devices(&self) -> Vec<Device> {
        match &self.fake_devices {
            Some(f) => f.clone(),
            None => self.disco.as_ref().map(|d| d.snapshot()).unwrap_or_default(),
        }
    }

    /// Devices answering now, plus remembered ones that are offline.
    fn rows(&self, live: &[Device]) -> Vec<Row> {
        let now = store::now();
        let mut rows: Vec<Row> = live
            .iter()
            .map(|d| Row {
                info: d.info.clone(),
                online: d.last_seen.elapsed() < ONLINE_WINDOW,
                last_seen: now.saturating_sub(d.last_seen.elapsed().as_secs()),
            })
            .collect();
        for k in self.known.devices.values() {
            if !rows.iter().any(|r| r.info.mac == k.info.mac) {
                rows.push(Row {
                    info: k.info.clone(),
                    online: false,
                    last_seen: k.last_seen,
                });
            }
        }
        rows.sort_by_key(|r| (!r.online, r.info.name.to_lowercase()));
        rows
    }

    fn selected_row(&self, rows: &[Row]) -> Option<Row> {
        let mac = self.selected.as_ref()?;
        rows.iter().find(|r| &r.info.mac == mac).cloned()
    }

    /// Status text with the memory-test estimate appended.
    fn status_text(&self, r: &Row) -> String {
        if !r.online {
            return format!("Offline · seen {}", store::ago(r.last_seen, store::now()));
        }
        let n = &r.info;
        if !n.is_memtest_running() {
            return n.status();
        }
        let p = n.progress.unwrap_or(0);
        let mut s = format!("Memory test {}.{:02}%", p / 100, p % 100);
        if let Some(left) = self.history.running(&n.mac).and_then(|h| h.remaining()) {
            s.push_str(&format!(" · ~{} left", store::duration(left)));
        }
        s
    }

    fn wol_for(n: &NasInfo) -> WolDevice {
        WolDevice {
            name: n.name.clone(),
            mac: n.mac.clone(),
            ip: n.ip.map(|i| i.to_string()).unwrap_or_default(),
            target: String::new(),
            port: 9,
        }
    }

    fn open_shares(&mut self, n: &NasInfo) {
        let Some(ip) = n.ip else { return };
        #[cfg(windows)]
        let url = format!("\\\\{ip}\\");
        #[cfg(not(windows))]
        let url = format!("smb://{ip}");
        if let Err(e) = open::that(&url) {
            self.status = format!("Could not open {url}: {e}");
        }
    }

    fn is_online(&self, mac: &str) -> bool {
        let Some(m) = wol::parse_mac(mac).map(|m| wol::format_mac(&m)) else {
            return false;
        };
        self.live_devices()
            .iter()
            .any(|d| d.info.mac == m && d.last_seen.elapsed() < ONLINE_WINDOW)
    }

    fn save_wol(&mut self) {
        if let Err(e) = wol::save(&self.wol) {
            self.status = format!("Could not save Wake-on-LAN list: {e}");
        }
    }

    fn add_to_wol(&mut self, n: &NasInfo) {
        if self.wol.iter().any(|w| w.mac.eq_ignore_ascii_case(&n.mac)) {
            self.status = format!("{} is already in the Wake-on-LAN list.", n.name);
            return;
        }
        self.wol.push(Self::wol_for(n));
        self.save_wol();
        self.status = format!("Added {} to the Wake-on-LAN list.", n.name);
    }

    fn wake(&mut self, w: &WolDevice) {
        if self.is_online(&w.mac) {
            self.status = format!("{} is already on.", w.name);
            return;
        }
        match wol::wake(w) {
            Ok(_) => {
                let mac = wol::parse_mac(&w.mac).map(|m| wol::format_mac(&m)).unwrap_or_default();
                self.waking.insert(mac, (Instant::now(), w.name.clone()));
                self.status = format!("Waking {}… this usually takes 1–3 minutes.", w.name);
                // Search more often while waiting.
                self.last_search = None;
            }
            Err(e) => self.status = format!("Wake-up failed for {}: {e}", w.name),
        }
    }

    fn waking_label(&self, mac: &str) -> Option<String> {
        let m = wol::parse_mac(mac).map(|m| wol::format_mac(&m))?;
        let (t, _) = self.waking.get(&m)?;
        let s = t.elapsed().as_secs();
        Some(format!("Waking… {}:{:02}", s / 60, s % 60))
    }

    fn open(&mut self, n: &NasInfo, https: bool) {
        if let Some(url) = n.web_url(https)
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

        let live = self.live_devices();
        self.track(&ctx, &live);
        let devs = self.rows(&live);
        let sel = self.selected_row(&devs);
        // Search every 3 s while waiting for a device to wake up.
        if !self.waking.is_empty() && self.last_search.is_some_and(|t| t.elapsed() >= Duration::from_secs(3)) {
            self.search();
        }

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
                                let n = sel.as_ref().map(|r| r.info.clone()).unwrap_or_default();
                                ui.menu_button("Connect", |ui| {
                                    if ui.button("Open DSM (HTTP)").clicked() {
                                        self.open(&n, false);
                                    }
                                    if ui.button("Open DSM (HTTPS)").clicked() {
                                        self.open(&n, true);
                                    }
                                    ui.separator();
                                    if ui.button("Open shared folders").clicked() {
                                        self.open_shares(&n);
                                    }
                                })
                                .response
                                .on_hover_text("Open DSM or the shared folders (double-click a device opens DSM)");
                                let online = sel.as_ref().is_some_and(|r| r.online);
                                let running = n.is_memtest_running();
                                let b = ui
                                    .add_enabled(online && !running, egui::Button::new("Memory Test"))
                                    .on_hover_text("Reboot the NAS into Synology's memory diagnostic test")
                                    .on_disabled_hover_text(if running {
                                        "A memory test is already running"
                                    } else {
                                        "The device is offline"
                                    });
                                if b.clicked() {
                                    self.open_memtest(&n);
                                }
                                if online {
                                    if ui
                                        .button("Set up WOL")
                                        .on_hover_text("Remember this device for Wake on LAN")
                                        .clicked()
                                    {
                                        self.add_to_wol(&n);
                                    }
                                } else if ui
                                    .button("Wake Up")
                                    .on_hover_text("Send a Wake-on-LAN packet")
                                    .clicked()
                                {
                                    let w = Self::wol_for(&n);
                                    self.wake(&w);
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
                    let offline = devs.iter().filter(|r| !r.online).count();
                    if self.tab == Tab::Devices && offline > 0 {
                        ui.label(format!("{} online · {offline} offline", n - offline));
                    } else {
                        ui.label(format!("{n} {what}{}", if n == 1 { "" } else { "s" }));
                    }
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
                if let Some(r) = &sel {
                    let warning = r
                        .info
                        .ip
                        .filter(|_| r.online)
                        .and_then(|ip| net::reachability_warning(ip, &self.local_nets));
                    let history = &self.history;
                    egui::Panel::bottom("details")
                        .frame(theme::chrome_frame(&ctx, egui::Margin::symmetric(12, 8)))
                        .resizable(false)
                        .show(ui, |ui| details(ui, r, history, warning));
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
    fn device_table(&mut self, ui: &mut egui::Ui, devs: &[Row]) {
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
        let mut action: Option<(NasInfo, RowAction)> = None;
        let pal = theme::palette(ui);
        quiet_column_lines(ui);
        TableBuilder::new(ui)
            .striped(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(150.0).at_least(80.0).resizable(true))
            .column(Column::initial(115.0).at_least(80.0).resizable(true))
            .column(Column::initial(250.0).at_least(110.0).resizable(true))
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
                        let status = self.status_text(d);
                        let color = if d.online {
                            status_color(pal, n.status_code)
                        } else {
                            pal.gray
                        };
                        // Offline rows in secondary text color (selected rows stay white).
                        let dim = !d.online && self.selected.as_deref() != Some(n.mac.as_str());
                        let dimmed = |ui: &mut egui::Ui| {
                            if dim {
                                ui.visuals_mut().override_text_color = Some(pal.text_secondary);
                            }
                        };
                        let ip = n.ip.map(|i| i.to_string()).unwrap_or_default();
                        row.col(|ui| {
                            dimmed(ui);
                            cell(ui, &n.name);
                        });
                        row.col(|ui| {
                            dimmed(ui);
                            cell(ui, &ip);
                        });
                        row.col(|ui| {
                            dimmed(ui);
                            status_dot(ui, color);
                            cell(ui, &status);
                        });
                        for c in [&n.mac, &n.model, &n.dsm_version(), &n.serial] {
                            row.col(|ui| {
                                dimmed(ui);
                                cell(ui, c);
                            });
                        }
                        let r = row.response();
                        if r.clicked() || r.secondary_clicked() {
                            self.selected = Some(n.mac.clone());
                        }
                        if r.double_clicked() && d.online {
                            open_req = Some(n.clone());
                        }
                        r.context_menu(|ui| {
                            let mut pick = |a| action = Some((n.clone(), a));
                            if ui.button("Open DSM (HTTP)").clicked() {
                                pick(RowAction::Http);
                            }
                            if ui.button("Open DSM (HTTPS)").clicked() {
                                pick(RowAction::Https);
                            }
                            if ui.button("Open shared folders").clicked() {
                                pick(RowAction::Shares);
                            }
                            ui.separator();
                            if ui.button("Copy IP address").clicked() {
                                pick(RowAction::CopyIp);
                            }
                            if ui.button("Copy MAC address").clicked() {
                                pick(RowAction::CopyMac);
                            }
                            ui.separator();
                            if !d.online && ui.button("Wake Up").clicked() {
                                pick(RowAction::Wake);
                            }
                            if ui.button("Set up WOL").clicked() {
                                pick(RowAction::Wol);
                            }
                            if ui
                                .add_enabled(d.online && !n.is_memtest_running(), egui::Button::new("Memory Test…"))
                                .clicked()
                            {
                                pick(RowAction::MemTest);
                            }
                            if !d.online {
                                ui.separator();
                                if ui.button("Forget this device").clicked() {
                                    pick(RowAction::Forget);
                                }
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
                RowAction::Shares => self.open_shares(&d),
                RowAction::CopyIp => ui.ctx().copy_text(d.ip.map(|i| i.to_string()).unwrap_or_default()),
                RowAction::CopyMac => ui.ctx().copy_text(d.mac.clone()),
                RowAction::Wol => self.add_to_wol(&d),
                RowAction::MemTest => self.open_memtest(&d),
                RowAction::Wake => {
                    let w = Self::wol_for(&d);
                    self.wake(&w);
                }
                RowAction::Forget => {
                    self.known.forget(&d.mac);
                    if self.selected.as_deref() == Some(d.mac.as_str()) {
                        self.selected = None;
                    }
                    self.save_now();
                }
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
        let waking: Vec<Option<String>> = self.wol.iter().map(|w| self.waking_label(&w.mac)).collect();
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
                        let waking = waking[i].clone();
                        let (st, color) = if online[i] {
                            ("On".to_string(), pal.green)
                        } else if let Some(l) = waking {
                            (l, pal.orange)
                        } else {
                            ("Off / unknown".to_string(), pal.gray)
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
                            cell(ui, &st);
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
    Shares,
    Wake,
    Forget,
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

fn details(ui: &mut egui::Ui, row: &Row, history: &memtest::History, warning: Option<String>) {
    let n = &row.info;
    let pal = theme::palette(ui);
    let ip = |a: Option<Ipv4Addr>| a.map(|x| x.to_string()).unwrap_or_else(|| "–".into());
    let key = |ui: &mut egui::Ui, t: &str| {
        ui.label(egui::RichText::new(t).color(pal.text_secondary));
    };
    if let Some(w) = warning {
        ui.horizontal_wrapped(|ui| {
            status_dot(ui, pal.orange);
            ui.label(egui::RichText::new(w).color(pal.orange));
        });
        ui.add_space(4.0);
    }
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
            let status = if row.online {
                n.status()
            } else {
                format!("Offline (last seen {})", store::ago(row.last_seen, store::now()))
            };
            ui.label(status).on_hover_text(help::status_explanation(n.status_code));
            ui.end_row();
            key(ui, "Gateway");
            ui.label(ip(n.gateway));
            key(ui, "Serial no");
            ui.label(&n.serial);
            ui.end_row();
            key(ui, "DNS");
            ui.label(ip(n.dns));
            key(ui, "Memory tests");
            memtest_summary(ui, &n.mac, history);
            ui.end_row();
        });
    ui.add_space(4.0);
}

/// One line about the device's memory tests, with the full history on hover.
fn memtest_summary(ui: &mut egui::Ui, mac: &str, history: &memtest::History) {
    let now = store::now();
    let runs: Vec<&memtest::Record> = history.for_device(mac).collect();
    let text = if let Some(r) = runs.iter().rev().find(|r| r.is_running()) {
        let (el, exact) = r.elapsed(now);
        let mut t = format!("Running {}{}", if exact { "" } else { "≥ " }, store::duration(el));
        if let Some(left) = r.remaining() {
            t.push_str(&format!(" · ~{} left", store::duration(left)));
        }
        t
    } else if runs.is_empty() {
        "None recorded on this computer".into()
    } else {
        let ok = runs
            .iter()
            .filter(|r| r.outcome == Some(memtest::Outcome::NoErrors))
            .count();
        let bad = runs
            .iter()
            .filter(|r| r.outcome == Some(memtest::Outcome::Errors))
            .count();
        let last = runs.iter().filter_map(|r| r.finished).max().unwrap_or(now);
        format!(
            "{} run{}: {ok} passed, {bad} with errors · last {}",
            runs.len(),
            if runs.len() == 1 { "" } else { "s" },
            store::ago(last, now)
        )
    };
    let resp = ui.add(egui::Label::new(text).truncate());
    if !runs.is_empty() {
        resp.on_hover_ui(|ui| {
            for r in runs.iter().rev().take(10) {
                let (el, exact) = r.elapsed(now);
                let result = match (r.is_running(), r.outcome) {
                    (true, _) => "running",
                    (_, Some(memtest::Outcome::NoErrors)) => "no errors",
                    (_, Some(memtest::Outcome::Errors)) => "errors found",
                    _ => "interrupted",
                };
                let when = r.started.unwrap_or(r.first_seen.0);
                ui.label(format!(
                    "{} · {}{} · {result}",
                    store::ago(when, now),
                    if exact { "" } else { "≥ " },
                    store::duration(el)
                ));
            }
        });
    }
}

/// Renders the UI to PNGs in target/ui-shots for visual review:
/// `cargo test --release --bin synology-assistant ui_snapshots -- --ignored`
#[cfg(test)]
mod ui_snapshots {
    use super::*;
    use egui_kittest::Harness;

    fn dev(name: &str, ip: u8, code: u32, progress: Option<u32>, model: &str) -> Device {
        Device {
            info: NasInfo {
                packet_type: protocol::PKT_INFO,
                name: name.into(),
                mac: format!("00:11:32:00:00:{ip:02x}"),
                ip: Some(Ipv4Addr::new(192, 168, 1, ip)),
                netmask: Some(Ipv4Addr::new(255, 255, 255, 0)),
                gateway: Some(Ipv4Addr::new(192, 168, 1, 1)),
                dns: Some(Ipv4Addr::new(192, 168, 1, 1)),
                dhcp: Some(true),
                model: model.into(),
                unique: "synology_geminilake_220+".into(),
                os_name: "DSM".into(),
                version: "7.4.1".into(),
                build: Some(90080),
                serial: format!("2080ABCDEF{ip:03}"),
                status_code: Some(code),
                progress,
                public_key: "00".repeat(32),
                session_id: Some(1),
                ..Default::default()
            },
            last_seen: Instant::now(),
        }
    }

    fn sample_app(cc: &eframe::CreationContext<'_>) -> App {
        let mut app = App::build(cc, Err("test".into()));
        app.fake_devices = Some(vec![
            dev("DS220plus", 15, 9, Some(7536), "DS220+"),
            dev("backup-nas", 20, 1, None, "DS923+"),
            dev("new-box", 30, 2, None, "DS224+"),
        ]);
        app.status.clear();
        app.local_nets = vec![(Ipv4Addr::new(192, 168, 1, 236), Ipv4Addr::new(255, 255, 255, 0))];
        let now = store::now();
        // A memory test observed for 3.5 hours, so an estimate is available.
        app.history.records.push(memtest::Record {
            mac: "00:11:32:00:00:0f".into(),
            name: "DS220plus".into(),
            started: None,
            first_seen: (now - 12_600, 3855),
            last_seen: (now, 7536),
            finished: None,
            outcome: None,
        });
        // Two earlier passes on another device.
        for ago in [86_400 * 3, 86_400 * 2] {
            app.history.records.push(memtest::Record {
                mac: "00:11:32:00:00:14".into(),
                name: "backup-nas".into(),
                started: Some(now - ago),
                first_seen: (now - ago, 0),
                last_seen: (now - ago + 30_000, 9_990),
                finished: Some(now - ago + 30_600),
                outcome: Some(memtest::Outcome::NoErrors),
            });
        }
        // An offline device remembered from yesterday.
        let off = dev("office-nas", 40, 1, None, "DS1522+").info;
        app.known.seen(&off, now - 26 * 3600);
        app
    }

    fn shot(theme: egui::Theme, name: &str, setup: impl FnOnce(&mut App)) {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1000.0, 560.0))
            .with_pixels_per_point(2.0)
            .with_theme(theme)
            .build_eframe(|cc| sample_app(cc));
        setup(h.state_mut());
        h.run();
        let img = h.render().expect("render");
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/ui-shots");
        std::fs::create_dir_all(&dir).unwrap();
        img.save(dir.join(format!("{name}.png"))).unwrap();
    }

    /// End-to-end: real discovery on the LAN, rendered to target/ui-shots/live.png.
    #[test]
    #[ignore = "needs a Synology device on the network"]
    fn live_snapshot() {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1000.0, 560.0))
            .with_pixels_per_point(2.0)
            .build_eframe(|cc| {
                let disco = Discovery::start(|| {}).map_err(|e| e.to_string());
                let mut app = App::build(cc, disco);
                app.local_nets = net::local_networks();
                app
            });
        for _ in 0..6 {
            h.run();
            std::thread::sleep(Duration::from_secs(1));
        }
        let first = h.state().live_devices().first().map(|d| d.info.mac.clone());
        assert!(first.is_some(), "no device answered");
        h.state_mut().selected = first;
        h.run();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/ui-shots");
        std::fs::create_dir_all(&dir).unwrap();
        h.render().unwrap().save(dir.join("live.png")).unwrap();
    }

    #[test]
    #[ignore = "renders PNGs for manual review"]
    fn ui_snapshots() {
        for (t, tn) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
            shot(t, &format!("{tn}-devices"), |a| {
                a.selected = Some("00:11:32:00:00:0f".into())
            });
            shot(t, &format!("{tn}-details"), |a| {
                a.selected = Some("00:11:32:00:00:14".into())
            });
            shot(t, &format!("{tn}-offline"), |a| {
                a.selected = Some("00:11:32:00:00:28".into())
            });
            shot(t, &format!("{tn}-subnet"), |a| {
                a.fake_devices.as_mut().unwrap()[2].info.ip = Some(Ipv4Addr::new(192, 168, 2, 30));
                a.selected = Some("00:11:32:00:00:1e".into());
            });
            shot(t, &format!("{tn}-help"), |a| {
                a.selected = Some("00:11:32:00:00:0f".into());
                a.show_help = true;
            });
            shot(t, &format!("{tn}-wol"), |a| {
                a.tab = Tab::Wol;
                let ds = a.fake_devices.as_ref().unwrap()[0].info.clone();
                let off = a.known.devices["00:11:32:00:00:28"].info.clone();
                a.wol = vec![App::wol_for(&ds), App::wol_for(&off)];
                a.waking.insert(
                    "00:11:32:00:00:28".into(),
                    (Instant::now() - Duration::from_secs(42), "office-nas".into()),
                );
            });
            shot(t, &format!("{tn}-addwol"), |a| {
                a.tab = Tab::Wol;
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
                let d = a.fake_devices.as_ref().unwrap()[1].info.clone();
                a.open_memtest(&d);
            });
        }
    }
}
