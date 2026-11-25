#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x08000000;

static WPING_EXE_BYTES: &[u8] = include_bytes!("../target/release/wping.exe");
static README_BYTES: &[u8] = include_bytes!("../README.md");
static ADMIN_BAT_BYTES: &[u8] = include_bytes!("../Apply_Tweaks_Admin.bat");

#[derive(PartialEq, Clone, Copy, Debug)]
enum Language {
    Ukrainian,
    English,
    Russian,
}

impl Language {
    fn label(&self) -> &'static str {
        match self {
            Language::Ukrainian => "🇺🇦 UA",
            Language::English => "🇬🇧 EN",
            Language::Russian => "🇷🇺 RU",
        }
    }
}

fn detect_system_language() -> Language {
    extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
        fn GetUserDefaultLocaleName(lpLocaleName: *mut u16, cchLocaleName: i32) -> i32;
    }

    unsafe {
        let mut buf = [0u16; 32];
        let len = GetUserDefaultLocaleName(buf.as_mut_ptr(), 32);
        if len > 0 {
            let locale = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
            if locale.starts_with("uk") {
                return Language::Ukrainian;
            } else if locale.starts_with("ru") {
                return Language::Russian;
            } else if locale.starts_with("en") {
                return Language::English;
            }
        }

        let lang_id = GetUserDefaultUILanguage();
        let primary = lang_id & 0x03FF;
        if primary == 0x22 {
            return Language::Ukrainian;
        } else if primary == 0x19 {
            return Language::Russian;
        }
    }

    Language::English
}

struct SetupApp {
    language: Language,
    install_dir: String,
    create_desktop_shortcut: bool,
    create_start_menu_shortcut: bool,
    apply_network_tweaks: bool,
    adapt_to_pc: bool,
    launch_after_install: bool,
    status_message: String,
    detected_adapter: String,
    is_installed: bool,
    is_installing: bool,
}

impl Default for SetupApp {
    fn default() -> Self {
        let default_dir = dirs_local_appdata().unwrap_or_else(|| PathBuf::from(r"C:\Program Files")).join("WPing");

        let lang = detect_system_language();
        let (initial_status, initial_adapter) = match lang {
            Language::Ukrainian => ("Натисніть 'Встановити' для початку.".into(), "Мережевий адаптер ПК".into()),
            Language::English => ("Click 'Install Now' to begin.".into(), "PC Network Adapter".into()),
            Language::Russian => ("Нажмите 'Установить' для начала.".into(), "Сетевой адаптер ПК".into()),
        };

        Self {
            language: lang,
            install_dir: default_dir.to_string_lossy().to_string(),
            create_desktop_shortcut: true,
            create_start_menu_shortcut: true,
            apply_network_tweaks: true,
            adapt_to_pc: true,
            launch_after_install: true,
            status_message: initial_status,
            detected_adapter: initial_adapter,
            is_installed: false,
            is_installing: false,
        }
    }
}

fn dirs_local_appdata() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|v| PathBuf::from(v).join("Programs"))
}

impl eframe::App for SetupApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut style = (*ctx.style()).clone();
        style.visuals.dark_mode = true;
        style.visuals.override_text_color = Some(egui::Color32::from_rgb(220, 230, 242));
        style.visuals.window_fill = egui::Color32::from_rgb(15, 18, 24);
        style.visuals.panel_fill = egui::Color32::from_rgb(20, 24, 32);
        ctx.set_style(style);

        let lang = self.language;

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(10.0);

            // Title
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("⚡ WPING v2.0 SETUP")
                        .font(egui::FontId::proportional(22.0))
                        .color(egui::Color32::from_rgb(0, 230, 153))
                        .strong(),
                );

                let sub_title = match lang {
                    Language::Ukrainian => "Оптимізатор пінгу, FPS, 0.5ms & Інпут лагу",
                    Language::English => "Ping, FPS, 0.5ms Timer & Input Delay Setup",
                    Language::Russian => "Оптимизатор пинга, FPS, 0.5ms и Инпут лага",
                };
                ui.label(
                    egui::RichText::new(sub_title)
                        .font(egui::FontId::proportional(12.0))
                        .color(egui::Color32::from_rgb(140, 160, 185)),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::ComboBox::from_id_salt("setup_lang_combo")
                        .selected_text(self.language.label())
                        .width(75.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.language, Language::Ukrainian, Language::Ukrainian.label());
                            ui.selectable_value(&mut self.language, Language::English, Language::English.label());
                            ui.selectable_value(&mut self.language, Language::Russian, Language::Russian.label());
                        });
                });
            });

            ui.label(
                egui::RichText::new("Ultimate Latency, Ping & FPS Optimizer • Created by Vsqy")
                    .font(egui::FontId::proportional(11.0))
                    .color(egui::Color32::from_rgb(0, 200, 255)),
            );

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(12.0);

            if !self.is_installed {
                let dir_lbl = match lang {
                    Language::Ukrainian => "Папка для встановлення:",
                    Language::English => "Installation Folder:",
                    Language::Russian => "Папка для установки:",
                };
                ui.label(egui::RichText::new(dir_lbl).strong());
                ui.add_space(4.0);
                ui.text_edit_singleline(&mut self.install_dir);

                ui.add_space(12.0);
                let opt_heading = match lang {
                    Language::Ukrainian => "Параметри адаптації та встановлення:",
                    Language::English => "Optimization & Installation Options:",
                    Language::Russian => "Параметры адаптации и установки:",
                };
                ui.label(egui::RichText::new(opt_heading).strong());
                ui.add_space(6.0);

                let (chk_adapt, chk_tweaks, chk_desk, chk_start, chk_launch) = match lang {
                    Language::Ukrainian => (
                        "Адаптувати мережеву карту під цей ПК (InterruptMod=Off, Metric=5)",
                        "Застосувати твіки Windows (Nagle, QoS, GameDVR FPS & Win11 0.5ms)",
                        "Створити ярлик на Робочому столі",
                        "Додати ярлик у меню 'Пуск'",
                        "Запустити WPing v2.0 після завершення",
                    ),
                    Language::English => (
                        "Tune PC network adapter hardware (InterruptMod=Off, Metric=5)",
                        "Apply Windows tweaks (Nagle, QoS, GameDVR FPS & Win11 0.5ms)",
                        "Create Desktop shortcut",
                        "Add Start Menu shortcut",
                        "Launch WPing v2.0 after installation",
                    ),
                    Language::Russian => (
                        "Адаптировать сетевую карту под этот ПК (InterruptMod=Off, Metric=5)",
                        "Применить твики Windows (Nagle, QoS, GameDVR FPS и Win11 0.5ms)",
                        "Создать ярлык на Рабочем столе",
                        "Добавить ярлык в меню 'Пуск'",
                        "Запустить WPing v2.0 после завершения",
                    ),
                };

                ui.checkbox(&mut self.adapt_to_pc, chk_adapt);
                ui.checkbox(&mut self.apply_network_tweaks, chk_tweaks);
                ui.checkbox(&mut self.create_desktop_shortcut, chk_desk);
                ui.checkbox(&mut self.create_start_menu_shortcut, chk_start);
                ui.checkbox(&mut self.launch_after_install, chk_launch);

                ui.add_space(18.0);

                let btn_text = if self.is_installing {
                    match lang {
                        Language::Ukrainian => "⏳ Адаптація та встановлення...",
                        Language::English => "⏳ Optimizing & Installing...",
                        Language::Russian => "⏳ Адаптация и установка...",
                    }
                } else {
                    match lang {
                        Language::Ukrainian => "🚀 ВСТАНОВИТИ ЗАРАЗ",
                        Language::English => "🚀 INSTALL NOW",
                        Language::Russian => "🚀 УСТАНОВИТЬ СЕЙЧАС",
                    }
                };

                let install_btn = egui::Button::new(
                    egui::RichText::new(btn_text)
                        .font(egui::FontId::proportional(16.0))
                        .color(egui::Color32::BLACK)
                        .strong(),
                )
                .fill(egui::Color32::from_rgb(0, 230, 153))
                .min_size(egui::vec2(ui.available_width(), 46.0));

                if ui.add_enabled(!self.is_installing, install_btn).clicked() {
                    self.perform_installation();
                }

                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(&self.status_message)
                        .font(egui::FontId::proportional(12.0))
                        .color(egui::Color32::from_rgb(160, 180, 200)),
                );
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    let done_title = match lang {
                        Language::Ukrainian => "🎉 ВСТАНОВЛЕННЯ ЗАВЕРШЕНО!",
                        Language::English => "🎉 INSTALLATION COMPLETE!",
                        Language::Russian => "🎉 УСТАНОВКА ЗАВЕРШЕНА!",
                    };
                    ui.label(
                        egui::RichText::new(done_title)
                            .font(egui::FontId::proportional(22.0))
                            .color(egui::Color32::from_rgb(0, 255, 128))
                            .strong(),
                    );
                    ui.add_space(8.0);

                    let installed_msg = match lang {
                        Language::Ukrainian => format!("WPing успішно адаптовано та встановлено в:\n{}", self.install_dir),
                        Language::English => format!("WPing successfully optimized and installed to:\n{}", self.install_dir),
                        Language::Russian => format!("WPing успешно адаптирован и установлен в:\n{}", self.install_dir),
                    };
                    ui.label(
                        egui::RichText::new(installed_msg)
                            .font(egui::FontId::proportional(13.0))
                            .color(egui::Color32::from_rgb(200, 215, 235)),
                    );

                    ui.add_space(10.0);
                    let (det_lbl, relay_lbl) = match lang {
                        Language::Ukrainian => (
                            format!("✔ Виявлено адаптер: {}", self.detected_adapter),
                            "✔ Налаштовано фоновий замір релеїв кожні 10 секунд".to_string(),
                        ),
                        Language::English => (
                            format!("✔ Detected adapter: {}", self.detected_adapter),
                            "✔ 10-second background relay pinging active".to_string(),
                        ),
                        Language::Russian => (
                            format!("✔ Обнаружен адаптер: {}", self.detected_adapter),
                            "✔ Настроен фоновый замер релеев каждые 10 секунд".to_string(),
                        ),
                    };

                    ui.label(
                        egui::RichText::new(det_lbl)
                            .font(egui::FontId::proportional(12.0))
                            .color(egui::Color32::from_rgb(0, 230, 153)),
                    );
                    ui.label(
                        egui::RichText::new(relay_lbl)
                            .font(egui::FontId::proportional(12.0))
                            .color(egui::Color32::from_rgb(0, 210, 255)),
                    );

                    ui.add_space(24.0);
                    let finish_btn_text = match lang {
                        Language::Ukrainian => "ГОТОВО (ЗАКРИТИ)",
                        Language::English => "FINISH (CLOSE)",
                        Language::Russian => "ГОТОВО (ЗАКРЫТЬ)",
                    };
                    let close_btn = egui::Button::new(
                        egui::RichText::new(finish_btn_text)
                            .font(egui::FontId::proportional(15.0))
                            .color(egui::Color32::WHITE)
                            .strong(),
                    )
                    .fill(egui::Color32::from_rgb(30, 140, 90))
                    .min_size(egui::vec2(220.0, 42.0));

                    if ui.add(close_btn).clicked() {
                        std::process::exit(0);
                    }
                });
            }
        });
    }
}

impl SetupApp {
    fn perform_installation(&mut self) {
        self.is_installing = true;
        let lang = self.language;
        self.status_message = match lang {
            Language::Ukrainian => "Визначення апаратних налаштувань ПК...".into(),
            Language::English => "Detecting PC hardware settings...".into(),
            Language::Russian => "Определение аппаратных настроек ПК...".into(),
        };

        // 1. Detect active network adapter on this specific PC (prefer physical)
        let adapter_out = Command::new("powershell")
            .args(&[
                "-NoProfile",
                "-Command",
                "$a = Get-NetAdapter | Where-Object { $_.Status -eq 'Up' -and $_.Virtual -eq $false -and $_.InterfaceDescription -notmatch 'VPN|Virtual|Radmin|Hamachi|TAP' } | Select-Object -First 1; \
                 if (-not $a) { $a = Get-NetAdapter | Where-Object Status -eq 'Up' | Select-Object -First 1 }; \
                 if ($a) { $a.InterfaceDescription }",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        if let Ok(out) = adapter_out {
            let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !name.is_empty() {
                self.detected_adapter = name;
            }
        }

        self.status_message = match lang {
            Language::Ukrainian => "Копіювання файлів...".into(),
            Language::English => "Copying files...".into(),
            Language::Russian => "Копирование файлов...".into(),
        };
        let target_dir = PathBuf::from(&self.install_dir);

        // 2. Create target directory
        if let Err(e) = fs::create_dir_all(&target_dir) {
            self.status_message = format!("Error creating folder: {}", e);
            self.is_installing = false;
            return;
        }

        // 3. Write WPing.exe
        let exe_path = target_dir.join("WPing.exe");
        if let Err(e) = fs::write(&exe_path, WPING_EXE_BYTES) {
            self.status_message = format!("Error writing WPing.exe: {}", e);
            self.is_installing = false;
            return;
        }

        // 4. Write README.md and Apply_Tweaks_Admin.bat
        let _ = fs::write(target_dir.join("README.md"), README_BYTES);
        let admin_bat = target_dir.join("Apply_Tweaks_Admin.bat");
        let _ = fs::write(&admin_bat, ADMIN_BAT_BYTES);

        // 5. Write Uninstall.bat
        let uninstall_bat = target_dir.join("Uninstall.bat");
        let uninstall_content = format!(
            "@echo off\r\n\
             echo Uninstalling WPing...\r\n\
             taskkill /F /IM WPing.exe >nul 2>&1\r\n\
             powershell -Command \"Remove-Item -Path '$([Environment]::GetFolderPath('Desktop'))\\WPing.lnk' -Force -ErrorAction SilentlyContinue\"\r\n\
             powershell -Command \"Remove-Item -Path '$([Environment]::GetFolderPath('StartMenu'))\\Programs\\WPing' -Recurse -Force -ErrorAction SilentlyContinue\"\r\n\
             cd ..\r\n\
             rmdir /S /Q \"{}\"\r\n\
             echo WPing has been removed successfully.\r\n\
             pause\r\n",
            target_dir.to_string_lossy()
        );
        let _ = fs::write(uninstall_bat, uninstall_content);

        // 6. Create Desktop shortcut
        if self.create_desktop_shortcut {
            let script = format!(
                "$WshShell = New-Object -ComObject WScript.Shell; \
                 $Shortcut = $WshShell.CreateShortcut(\"$([Environment]::GetFolderPath('Desktop'))\\WPing.lnk\"); \
                 $Shortcut.TargetPath = \"{}\"; \
                 $Shortcut.WorkingDirectory = \"{}\"; \
                 $Shortcut.Description = \"WPing - Game Latency & Route Optimizer (Created by Vsqy)\"; \
                 $Shortcut.Save()",
                exe_path.to_string_lossy().replace('\\', "\\\\"),
                target_dir.to_string_lossy().replace('\\', "\\\\"),
            );
            let _ = Command::new("powershell")
                .args(&["-NoProfile", "-Command", &script])
                .creation_flags(CREATE_NO_WINDOW)
                .output();
        }

        // 7. Create Start Menu shortcut
        if self.create_start_menu_shortcut {
            let script = format!(
                "$dir = \"$([Environment]::GetFolderPath('StartMenu'))\\Programs\\WPing\"; \
                 New-Item -ItemType Directory -Path $dir -Force | Out-Null; \
                 $WshShell = New-Object -ComObject WScript.Shell; \
                 $Shortcut = $WshShell.CreateShortcut(\"$dir\\WPing.lnk\"); \
                 $Shortcut.TargetPath = \"{}\"; \
                 $Shortcut.WorkingDirectory = \"{}\"; \
                 $Shortcut.Save()",
                exe_path.to_string_lossy().replace('\\', "\\\\"),
                target_dir.to_string_lossy().replace('\\', "\\\\"),
            );
            let _ = Command::new("powershell")
                .args(&["-NoProfile", "-Command", &script])
                .creation_flags(CREATE_NO_WINDOW)
                .output();
        }

        // 8. PC Adaptation & Network Adapter tuning (prioritize physical adapter over VPN)
        if self.adapt_to_pc {
            let adapt_script = "
                Get-NetAdapter | Where-Object { $_.Status -eq 'Up' -and $_.Virtual -eq $false } | ForEach-Object {
                    Enable-NetAdapterRss -Name $_.Name -ErrorAction SilentlyContinue;
                    Disable-NetAdapterPowerManagement -Name $_.Name -ErrorAction SilentlyContinue;
                    Set-NetIPInterface -InterfaceIndex $_.InterfaceIndex -InterfaceMetric 5 -ErrorAction SilentlyContinue;
                }
                Get-NetAdapter | Where-Object { $_.Virtual -eq $true -or $_.InterfaceDescription -match 'VPN|Virtual|Radmin|Hamachi|TAP|ZeroTier' -or $_.Name -match 'VPN|Radmin|Hamachi' } | ForEach-Object {
                    Set-NetIPInterface -InterfaceIndex $_.InterfaceIndex -InterfaceMetric 50 -ErrorAction SilentlyContinue;
                }
            ";
            let _ = Command::new("powershell")
                .args(&["-NoProfile", "-Command", adapt_script])
                .creation_flags(CREATE_NO_WINDOW)
                .output();
        }

        // 9. Apply network tweaks if checked
        if self.apply_network_tweaks {
            let _ = Command::new("powershell")
                .args(&[
                    "-NoProfile",
                    "-Command",
                    &format!("Start-Process -FilePath '{}' -Verb RunAs", admin_bat.to_string_lossy()),
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn();
        }

        // 10. Launch WPing if checked
        if self.launch_after_install {
            let _ = Command::new(&exe_path)
                .current_dir(&target_dir)
                .spawn();
        }

        self.is_installed = true;
        self.is_installing = false;
        self.status_message = match lang {
            Language::Ukrainian => "Встановлення та адаптацію завершено!".into(),
            Language::English => "Installation and optimization completed!".into(),
            Language::Russian => "Установка и адаптация завершены!".into(),
        };
    }
}

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([560.0, 450.0])
            .with_resizable(false)
            .with_title("WPing Setup"),
        ..Default::default()
    };

    eframe::run_native(
        "WPing Setup",
        native_options,
        Box::new(|_cc| Ok(Box::new(SetupApp::default()))),
    )
}
