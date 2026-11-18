//! Built-in help, shown in a side panel. Written for this project (not copied from
//! Synology's documentation).

/// One-line explanation of a status code, with what to do about it.
pub fn status_explanation(code: Option<u32>) -> &'static str {
    match code {
        Some(0) => "DSM is installed but its first-time setup (admin account) is unfinished. Connect to finish it.",
        Some(1) => "DSM is installed and running. Double-click the device or use Connect to open it.",
        Some(2) => {
            "No usable DSM was found: drives missing or not detected, DSM never installed, or the system was erased. If drives were in use before, stop and check them before installing anything."
        }
        Some(3) => "DSM is being updated. Wait for it to finish; don't power it off.",
        Some(4) => {
            "The settings were reset (typically a 4-second reset-button press, \"mode 2\"). Reinstall DSM from the web page; data on the volumes is kept."
        }
        Some(5) => "The system is booting.",
        Some(6) => {
            "Drive quotas or the file system are being checked, often after an improper shutdown. Wait for it to finish."
        }
        Some(7) => "DSM services are starting; this can take up to about 10 minutes.",
        Some(9) => {
            "A memory test is running; the NAS is not usable until it finishes. Results are written to the DSM system log."
        }
        Some(11) => {
            "The DSM version on the drives is newer than the one on the device (common after a repair or replacement). Connect to recover."
        }
        Some(13) => "DSM is being installed or configured.",
        Some(14) => {
            "The drives come from another, compatible device and can be migrated. Connect to start the migration."
        }
        _ => "This status is not documented.",
    }
}

pub struct Section {
    pub title: &'static str,
    pub body: &'static [&'static str],
}

pub const STATUSES: &[(&str, u32)] = &[
    ("Ready", 1),
    ("Booting", 5),
    ("Starting services", 7),
    ("Upgrading", 3),
    ("Performing memory test (X%)", 9),
    ("Checking quota", 6),
    ("Not configured", 0),
    ("Not installed", 2),
    ("Configuration lost", 4),
    ("Migratable", 14),
    ("Recoverable", 11),
    ("Check progress", 13),
];

pub const GETTING_STARTED: Section = Section {
    title: "Getting started",
    body: &[
        "The app searches your local network for Synology devices every 10 seconds; click Search to start over.",
        "Select a device to see its network details below the list. Double-click it, or click Connect, to open DSM in your browser. Connect › Open shared folders opens its SMB shares in Finder or Explorer.",
        "Devices you have seen before stay listed in grey while offline, with when they were last seen. Right-click one to wake it up or to forget it.",
        "If a device is on a different subnet than your computer, an orange note under the list explains why DSM may not open.",
        "Discovery only works on the same network segment as the NAS: it uses UDP broadcasts on port 9999, which routers and most VPNs don't forward.",
    ],
};

pub const WOL: Section = Section {
    title: "Wake on LAN",
    body: &[
        "1. In DSM, enable Wake on LAN under Control Panel › Hardware & Power › General (on models that support it).",
        "2. While the NAS is on, select it under Management and click Set up WOL. It is remembered on this computer.",
        "3. To wake it later, open the Wake on LAN tab, select it, and click Wake Up (or double-click it). You can also right-click an offline device in the Management list and choose Wake Up.",
        "After a wake-up the status shows \"Waking…\" with a timer; you get a notification as soon as the NAS answers (usually 1–3 minutes).",
        "Use Add device… for a NAS that is currently off and whose MAC address you know. Fill in Target only to wake through a router (a directed broadcast such as 192.168.1.255, or a host name with port forwarding).",
        "Status shows On while the NAS answers searches, and Off / unknown otherwise.",
    ],
};

pub const MEMTEST: Section = Section {
    title: "Memory test",
    body: &[
        "Select a device under Management and click Memory Test. You need a DSM administrator account.",
        "The NAS reboots into a diagnostic mode and is unavailable until the test ends. Depending on the amount of RAM this takes from minutes to many hours; progress shows in the Status column.",
        "Once some progress has been measured, the Status column shows an estimate of the time left.",
        "When the test ends while this app is open, you get a desktop notification saying whether errors were found. Details are written to the DSM system log.",
        "Each run is recorded on this computer: the details panel shows how many runs passed or found errors (hover it for the list).",
        "If Synology support asks you to test memory, run it several times (they usually ask for at least 3) and attach the debug log to your ticket.",
        "On dual-controller models, test one controller at a time so the system keeps serving.",
        "Your password is encrypted with the NAS's public key before it is sent. Older systems that don't offer a key would receive it unencrypted; the app warns you first.",
    ],
};

pub const NETWORK: Section = Section {
    title: "Network settings",
    body: &[
        "Select a device under Management and click Network Settings to switch it between an automatic (DHCP) and a fixed address, or to change its gateway and DNS. You need a DSM administrator account.",
        "The NAS applies the change right away and may reappear at a new address. Pick an address in your computer's subnet, or you won't reach it from here afterwards.",
        "Not listed? Use Find by IP… to ask a NAS directly: the app tells you whether it answers, and whether its firewall blocks discovery.",
    ],
};

pub const TROUBLESHOOTING: Section = Section {
    title: "Troubleshooting",
    body: &[
        "Nothing found? On macOS, allow \"Local Network\" access for Synology Assistant in System Settings › Privacy & Security. On Windows, allow the app through the firewall for private networks.",
        "Only one program can listen for answers on UDP port 9999. Quit the original Synology Assistant (or other finders) and relaunch this app.",
        "Make sure your computer and the NAS are on the same subnet and that client isolation (guest Wi-Fi) is off.",
        "Still nothing? Try https://finds.synology.com from a browser on the same network.",
    ],
};

pub const ABOUT: Section = Section {
    title: "About",
    body: &[
        "An independent, open-source replacement for Synology Assistant. It is not affiliated with or endorsed by Synology Inc.; \"Synology\" and \"DSM\" are their trademarks.",
        "Built with AI assistance (\"vibe coded\") from a reverse-engineered protocol. Use at your own risk, and report issues or ideas on GitHub.",
    ],
};

pub const SECTIONS: &[&Section] = &[&GETTING_STARTED, &WOL, &MEMTEST, &NETWORK, &TROUBLESHOOTING, &ABOUT];

/// Every text of the help content, for translation coverage checks.
pub fn all_texts() -> Vec<&'static str> {
    let mut v = Vec::new();
    for s in SECTIONS {
        v.push(s.title);
        v.extend_from_slice(s.body);
    }
    for (name, code) in STATUSES {
        v.push(name);
        v.push(status_explanation(Some(*code)));
    }
    v.push(status_explanation(None));
    v.extend([
        "Help",
        "Selected device",
        "Device status",
        "Project page & feedback",
        "Language",
    ]);
    v
}
