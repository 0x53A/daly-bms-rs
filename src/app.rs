use eframe::egui;
use std::cell::RefCell;
use std::rc::Rc;

#[cfg(target_arch = "wasm32")]
use crate::bluetooth as bt;

#[derive(Clone, Debug)]
pub struct Reading {
    pub device: String,
    pub service: String,
    pub characteristic: String,
    pub value_hex: String,
    pub value_text: Option<String>,
    pub ts: f64,
}

#[derive(Debug, Clone)]
pub struct ParsedStatus {
    pub total_voltage: Option<f32>,
    pub current: Option<f32>,
    pub soc: Option<f32>,
    pub cell_voltages: Vec<f32>,
    pub temperatures: Vec<f32>,
    pub cell_count: Option<u16>,
    pub charging_cycles: Option<u16>,
    pub balancing: Option<bool>,
    pub charging_mos: Option<bool>,
    pub discharging_mos: Option<bool>,
    pub battery_status: Option<String>,
}

impl ParsedStatus {
    fn empty() -> Self {
        Self {
            total_voltage: None,
            current: None,
            soc: None,
            cell_voltages: Vec::new(),
            temperatures: Vec::new(),
            cell_count: None,
            charging_cycles: None,
            balancing: None,
            charging_mos: None,
            discharging_mos: None,
            battery_status: None,
        }
    }
}

fn hex_to_bytes(s: &str) -> Option<Vec<u8>> {
    let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.len() % 2 != 0 {
        return None;
    }
    (0..cleaned.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&cleaned[i..i + 2], 16).ok())
        .collect()
}

fn parse_daly_status_from_bytes(data: &[u8]) -> Option<ParsedStatus> {
    if data.len() < 10 || data[0] != 0xD2 || data[1] != 0x03 {
        return None;
    }

    let frame_len = data.len();
    let get_u16 = |i: usize| -> Option<u16> {
        if i + 1 >= frame_len {
            None
        } else {
            Some(((data[i] as u16) << 8) | (data[i + 1] as u16))
        }
    };

    let mut res = ParsedStatus::empty();

    // Cell voltages: infer count from byte 102 if available, otherwise from length
    let cell_count = if frame_len > 102 {
        std::cmp::min(data[102] as usize, 32)
    } else {
        (frame_len.saturating_sub(3)) / 2
    };

    for i in 0..cell_count {
        let idx = 3 + i * 2;
        if let Some(v) = get_u16(idx) {
            res.cell_voltages.push((v as f32) * 0.001);
        } else {
            break;
        }
    }

    // Temperatures: start at 67, count at 104
    if frame_len > 68 {
        let temp_count = if frame_len > 104 {
            std::cmp::min(data[104] as usize, 8)
        } else {
            0
        };
        for i in 0..temp_count {
            let idx = 67 + i * 2;
            if let Some(v) = get_u16(idx) {
                res.temperatures.push((v as f32) - 40.0);
            } else {
                break;
            }
        }
    }

    // Total voltage at 83
    if let Some(v) = get_u16(83) {
        res.total_voltage = Some((v as f32) * 0.1);
    }

    // Current at 85
    if let Some(v) = get_u16(85) {
        res.current = Some(((v as i32 - 30000) as f32) * 0.1);
    }

    // SOC at 87
    if let Some(v) = get_u16(87) {
        res.soc = Some((v as f32) * 0.1);
    }

    // Battery status at 98
    if frame_len > 98 {
        res.battery_status = Some(
            match data[98] {
                0 => "Idle",
                1 => "Charging",
                2 => "Discharging",
                _ => "Unknown",
            }
            .to_string(),
        );
    }

    // Cell count at 101
    if let Some(v) = get_u16(101) {
        res.cell_count = Some(v);
    }

    // Charging cycles at 105
    if let Some(v) = get_u16(105) {
        res.charging_cycles = Some(v);
    }

    // Balancing, charging MOS, discharging MOS
    if let Some(v) = get_u16(107) {
        res.balancing = Some(v == 0x0001);
    }
    if let Some(v) = get_u16(109) {
        res.charging_mos = Some(v == 0x0001);
    }
    if let Some(v) = get_u16(111) {
        res.discharging_mos = Some(v == 0x0001);
    }

    Some(res)
}

pub struct BMSApp {
    readings: Rc<RefCell<Vec<Reading>>>,
    #[cfg(target_arch = "wasm32")]
    _listeners: Rc<RefCell<Vec<wasm_bindgen::prelude::Closure<dyn FnMut(wasm_bindgen::JsValue)>>>>,
    #[cfg(target_arch = "wasm32")]
    control_char: Rc<RefCell<Option<web_sys::BluetoothRemoteGattCharacteristic>>>,
}

impl Default for BMSApp {
    fn default() -> Self {
        Self {
            readings: Rc::new(RefCell::new(Vec::new())),
            #[cfg(target_arch = "wasm32")]
            _listeners: Rc::new(RefCell::new(Vec::new())),
            #[cfg(target_arch = "wasm32")]
            control_char: Rc::new(RefCell::new(None)),
        }
    }
}

impl BMSApp {
    pub fn ui(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Daly BMS (WebBluetooth)");

            ui.horizontal(|ui| {
                #[cfg(target_arch = "wasm32")]
                {
                    if ui.button("Start scan / connect").clicked() {
                        bt::start_scan(
                            self.readings.clone(),
                            self._listeners.clone(),
                            self.control_char.clone()
                        );
                    }

                    ui.separator();
                    ui.label("Control:");
                    if ui.button("Request status").clicked() {
                        if self.control_char.borrow().is_some() {
                            bt::write_control_command(self.control_char.clone());
                        } else {
                            ui.label("No control characteristic discovered yet.");
                        }
                    }
                }

                #[cfg(not(target_arch = "wasm32"))]
                {
                    ui.label("WebBluetooth is only available in the browser (wasm build).");
                }

                if ui.button("Clear readings").clicked() {
                    self.readings.borrow_mut().clear();
                }
            });

            ui.separator();

            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    let rows = self.readings.borrow();
                    if rows.is_empty() {
                        ui.label("No readings yet. Click 'Start scan / connect' and allow Bluetooth access.");
                    } else {
                        for r in rows.iter().rev() {
                            self.show_reading(ui, r);
                        }
                    }
                });
        });
    }

    fn show_reading(&self, ui: &mut egui::Ui, r: &Reading) {
        ui.group(|ui| {
            ui.label(format!("Device: {}", r.device));
            ui.label(format!("Service: {}", r.service));
            ui.label(format!("Characteristic: {}", r.characteristic));
            ui.code(format!("Value: {}", r.value_hex));

            if let Some(ref t) = r.value_text {
                ui.label(format!("Interpreted: {}", t));
            }
            ui.small(format!("{:.0}", r.ts));

            // Try parsing as Daly status
            if let Some(bytes) = hex_to_bytes(&r.value_hex) {
                if let Some(parsed) = parse_daly_status_from_bytes(&bytes) {
                    self.show_parsed_status(ui, &parsed);
                }
            }
        });
    }

    fn show_parsed_status(&self, ui: &mut egui::Ui, parsed: &ParsedStatus) {
        ui.separator();
        ui.label("Parsed Daly status:");

        if let Some(v) = parsed.total_voltage {
            ui.label(format!("Total: {:.2} V", v));
        }
        if let Some(c) = parsed.current {
            ui.label(format!("Current: {:.2} A", c));
        }
        if let Some(s) = parsed.soc {
            ui.label(format!("SOC: {:.1} %", s));
        }

        if !parsed.cell_voltages.is_empty() {
            ui.label(format!("Cells: {}", parsed.cell_voltages.len()));
            for (i, cv) in parsed.cell_voltages.iter().enumerate().take(16) {
                ui.small(format!("C{}: {:.3} V", i + 1, cv));
            }
        }

        if !parsed.temperatures.is_empty() {
            ui.label(format!("Temps: {}", parsed.temperatures.len()));
            for (i, t) in parsed.temperatures.iter().enumerate() {
                ui.small(format!("T{}: {:.1} °C", i + 1, t));
            }
        }

        if let Some(ref status) = parsed.battery_status {
            ui.label(format!("Status: {}", status));
        }
    }
}
