#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use eframe::egui::{self, Color32, Frame, Rounding, Stroke, Vec2};
use tokio::sync::Mutex;

mod discovery;
mod file_transfer;
mod mirror;
mod p2p_client;

#[derive(Clone, PartialEq)]
enum Tab {
    Dashboard,
    Files,
    Mirror,
    Settings,
}

struct DosUiApp {
    // App state
    tab: Tab,
    dark_mode: bool,

    // Discovery
    discovered_nodes: Arc<Mutex<Vec<discovery::P2pNode>>>,

    // Connected node info
    connected_node: Option<discovery::P2pNode>,
    node_battery: u8,
    node_status: String,

    // File transfer
    transfer_speed: f64, // MB/s
    transfer_progress: f64, // 0-100
    transfer_active: bool,
    transfer_history: Vec<String>,

    // Mirror
    mirror_connected: bool,
    mirror_fps: f64,

    // Settings
    listen_port: u16,
    mirror_port: u16,
    auto_discover: bool,

    // Network info
    local_ip: String,
}

impl Default for DosUiApp {
    fn default() -> Self {
        Self {
            tab: Tab::Dashboard,
            dark_mode: true,
            discovered_nodes: Arc::new(Mutex::new(Vec::new())),
            connected_node: None,
            node_battery: 41,
            node_status: "Connected".into(),
            transfer_speed: 0.0,
            transfer_progress: 0.0,
            transfer_active: false,
            transfer_history: Vec::new(),
            mirror_connected: false,
            mirror_fps: 0.0,
            listen_port: 7891,
            mirror_port: 7892,
            auto_discover: true,
            local_ip: "".into(),
        }
    }
}

impl eframe::App for DosUiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Set dark/light mode
        ctx.set_visuals(if self.dark_mode {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });

        // Top bar
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("⚡ PDOS Hub");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("☰").clicked() {
                        self.dark_mode = !self.dark_mode;
                    }
                });
            });
        });

        // Tab bar
        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let tabs = [
                    (Tab::Dashboard, "📊 Dashboard"),
                    (Tab::Files, "📁 Files"),
                    (Tab::Mirror, "🖥 Mirror"),
                    (Tab::Settings, "⚙ Settings"),
                ];
                for (tab, label) in &tabs {
                    let selected = self.tab == *tab;
                    if ui.selectable_label(selected, *label).clicked() {
                        self.tab = (*tab).clone();
                    }
                }
            });
        });

        // Bottom status bar
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(node) = &self.connected_node {
                    ui.label(format!("📱 {} | {}%", node.node_name, self.node_battery));
                    ui.separator();
                    ui.label(self.node_status.clone());
                    ui.separator();
                }
                if self.transfer_active {
                    ui.label(format!("⬆ {:.1} MB/s", self.transfer_speed));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(&self.local_ip);
                });
            });
        });

        // Main content
        egui::CentralPanel::default().show(ctx, |ui| {
            match self.tab {
                Tab::Dashboard => self.ui_dashboard(ui),
                Tab::Files => self.ui_files(ui),
                Tab::Mirror => self.ui_mirror(ui),
                Tab::Settings => self.ui_settings(ui),
            }
        });

        // Auto-refresh
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

impl DosUiApp {
    fn ui_dashboard(&mut self, ui: &mut egui::Ui) {
        ui.heading("Connected Devices");
        ui.separator();

        let node_battery = self.node_battery;
        let node_status = self.node_status.clone();
        let has_node = self.connected_node.is_some();
        let node_name = self.connected_node.as_ref().map(|n| n.node_name.clone()).unwrap_or_default();
        let node_ip = self.connected_node.as_ref().map(|n| n.ip.clone()).unwrap_or_default();

        if has_node {

            Frame::NONE
                .fill(ui.style().visuals.extreme_bg_color)
                .corner_radius(12)
                .stroke(Stroke::new(1.0, Color32::from_rgb(60, 60, 60)))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(8.0);
                            ui.label("📱");
                            ui.label("Samsung");
                        });
                        ui.separator();
                        ui.vertical(|ui| {
                            ui.heading(node_name);
                            ui.label(format!("{} • {}", node_ip, node_status));
                            ui.horizontal(|ui| {
                                ui.label(format!("🔋 {}%", node_battery));
                                ui.separator();
                                ui.label("📶 WiFi");
                            });
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.vertical(|ui| {
                                                if ui.button("Mirror").clicked() {
                                    self.tab = Tab::Mirror;
                                }
                                if ui.button("Files").clicked() {
                                    self.tab = Tab::Files;
                                }
                                if ui.button("Disconnect").clicked() {
                                    self.connected_node = None;
                                }
                            });
                        });
                    });
                });
            ui.add_space(8.0);
        } else {
            ui.horizontal(|ui| {
                ui.label("No device connected.");
                if ui.button("🔍 Scan").clicked() {}
            });
        }

        // Quick actions
        ui.add_space(16.0);
        ui.heading("Quick Actions");
        ui.separator();
        ui.horizontal(|ui| {
            let actions = [
                ("📋 Send Clipboard", "Send"),
                ("📤 Upload to phone", "Upload"),
                ("🔄 Sync", "Sync"),
            ];
            for (label, _) in &actions {
                if ui.button(*label).clicked() {
                    self.transfer_history
                        .push(format!("[{}] {}", Utc::now().format("%H:%M:%S"), label));
                }
            }
        });

        // Transfer history
        ui.add_space(16.0);
        ui.heading("Activity");
        ui.separator();
        egui::ScrollArea::vertical()
            .max_height(200.0)
            .show(ui, |ui| {
                for entry in self.transfer_history.iter().rev().take(20) {
                    ui.label(entry);
                }
            });
    }

    fn ui_files(&mut self, ui: &mut egui::Ui) {
        ui.heading("File Transfer");
        ui.separator();

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label("Local file:");
                let local_path = "/Users/mohak/Documents";
                ui.label(local_path);
                if ui.button("📂 Browse Local").clicked() {
                    // File dialog
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.label("Remote path:");
                ui.label("/storage/emulated/0/Download");
                if ui.button("📂 Browse Remote").clicked() {
                    // Remote browsing
                }
            });
        });

        ui.add_space(8.0);
        if self.transfer_active {
            ui.horizontal(|ui| {
                ui.label(format!("Speed: {:.1} MB/s", self.transfer_speed));
                ui.separator();
                ui.label(format!("Progress: {:.0}%", self.transfer_progress));
            });
            let progress = self.transfer_progress as f32 / 100.0;
            ui.add(
                egui::ProgressBar::new(progress)
                    .text(format!("{:.0}%", self.transfer_progress)),
            );
        } else {
            ui.label("No active transfers");
        }

        ui.add_space(8.0);
        if ui.button("🚀 Transfer Now").clicked() {
            self.transfer_active = true;
            self.transfer_progress = 0.0;
            self.transfer_speed = 0.0;
        }

        ui.add_space(16.0);
        ui.heading("Transfer History");
        ui.separator();
        egui::ScrollArea::vertical()
            .max_height(300.0)
            .show(ui, |ui| {
                for entry in self.transfer_history.iter().rev().take(50) {
                    ui.label(entry);
                }
            });
    }

    fn ui_mirror(&mut self, ui: &mut egui::Ui) {
        ui.heading("Screen Mirror");
        ui.separator();

        let node_name = self.connected_node.as_ref().map(|n| n.node_name.clone()).unwrap_or_default();
        let mirror_connected = self.mirror_connected;
        let mirror_fps = self.mirror_fps;
        let connected = self.connected_node.is_some();

        if connected {
            ui.horizontal(|ui| {
                ui.label(format!("Mirroring: {}", node_name));
                if mirror_connected {
                    ui.label("🟢 Live");
                    ui.label(format!("{:.0} fps", mirror_fps));
                }
            });

            // Mirror view placeholder
            Frame::NONE
                .fill(Color32::from_rgb(20, 20, 20))
                .corner_radius(8)
                .stroke(Stroke::new(1.0, Color32::from_rgb(40, 40, 40)))
                .show(ui, |ui| {
                    let size = Vec2::new(ui.available_width(), ui.available_height() - 80.0);
                    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    ui.painter().rect_filled(rect, egui::CornerRadius::same(4), Color32::from_rgb(10, 10, 10));
                    let text = if mirror_connected {
                        "🔴 Live screen — H.264"
                    } else {
                        "Connect to start mirroring"
                    };
                    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(rect.height() / 3.0);
                            ui.label(text);
                        });
                    });
                });

            ui.horizontal(|ui| {
                if mirror_connected {
                    if ui.button("⏹ Stop Mirror").clicked() {
                        self.mirror_connected = false;
                    }
                } else {
                    if ui.button("▶ Start Mirror").clicked() {
                        self.mirror_connected = true;
                    }
                }
                ui.separator();
                ui.label("Port: 7892");
            });
        } else {
            ui.label("No device connected. Connect from Dashboard first.");
        }
    }

    fn ui_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        ui.separator();

        ui.group(|ui| {
            ui.heading("Connection");
            ui.horizontal(|ui| {
                ui.label("Listen Port:");
                ui.add(egui::Slider::new(&mut self.listen_port, 1024..=65535));
            });
            ui.horizontal(|ui| {
                ui.label("Mirror Port:");
                ui.add(egui::Slider::new(&mut self.mirror_port, 1024..=65535));
            });
            ui.checkbox(&mut self.auto_discover, "Auto-discover devices on startup");
        });

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.heading("Display");
            ui.horizontal(|ui| {
                ui.label("Theme:");
                if ui
                    .selectable_label(self.dark_mode, "🌙 Dark")
                    .clicked()
                {
                    self.dark_mode = true;
                }
                if ui
                    .selectable_label(!self.dark_mode, "☀ Light")
                    .clicked()
                {
                    self.dark_mode = false;
                }
            });
        });

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.heading("Storage");
            ui.label("Download path: ~/Downloads/PDOS");
            if ui.button("Change...").clicked() {
                // File dialog
            }
        });

        ui.add_space(16.0);
        if ui.button("Save & Apply").clicked() {
            // Save config
        }
    }
}

fn main() -> Result<(), eframe::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new("info"))
        .init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(egui::vec2(900.0, 650.0))
            .with_min_inner_size(egui::vec2(600.0, 400.0)),
        ..Default::default()
    };

    eframe::run_native(
        "PDOS Hub",
        options,
        Box::new(|_cc| Ok(Box::new(DosUiApp::default()))),
    )
}
