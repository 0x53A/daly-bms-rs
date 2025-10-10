use eframe::egui;
use egui::{Color32, FontFamily, FontId};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

#[cfg(target_arch = "wasm32")]
use crate::bluetooth as bt;

use crate::data_structure::*;


pub struct BMSApp {
    readings: Rc<RefCell<Vec<Reading>>>,
    #[cfg(target_arch = "wasm32")]
        _listeners: Rc<RefCell<Vec<wasm_bindgen::prelude::Closure<dyn FnMut(wasm_bindgen::JsValue)>>>>,
        #[cfg(target_arch = "wasm32")]
        control_char: Rc<RefCell<Option<web_sys::BluetoothRemoteGattCharacteristic>>>,
    #[cfg(target_arch = "wasm32")]
    device: Rc<RefCell<Option<bt::DalyBtleDevice>>>,
}

impl Default for BMSApp {
    fn default() -> Self {
        Self {
            readings: Rc::new(RefCell::new(Vec::new())),
            #[cfg(target_arch = "wasm32")]
            _listeners: Rc::new(RefCell::new(Vec::new())),
                #[cfg(target_arch = "wasm32")]
                control_char: Rc::new(RefCell::new(None)),
                #[cfg(target_arch = "wasm32")]
                device: Rc::new(RefCell::new(None)),
        }
    }
}

#[cfg(target_arch = "wasm32")]
use futures::StreamExt;

impl BMSApp {
    pub fn ui(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {

            // Decorative header: emulate two-tone misprint by painting text twice with offsets
            let painter = ui.painter();
            let rect = ui.max_rect();
            let x = rect.left() + 24.0; // left-align with a small left margin
            let y = rect.top() + 18.0;
            let text = "Daly BMS";
            // pink shadow behind
            painter.text(
                egui::pos2(x + 4.0, y + 2.0),
                egui::Align2::LEFT_TOP,
                text,
                FontId::new(36.0, FontFamily::Name(Arc::from("Cynatar"))),
                Color32::from_rgb(255, 45, 149),
            );
            // foreground yellow
            painter.text(
                egui::pos2(x, y),
                egui::Align2::LEFT_TOP,
                text,
                FontId::new(36.0, FontFamily::Name(Arc::from("Cynatar"))),
                Color32::from_rgb(255, 212, 0),
            );

            // add vertical gap so the button appears below the header
            ui.add_space(64.0);

            ui.horizontal(|ui| {
                #[cfg(target_arch = "wasm32")]
                {
                    if ui.button("Start scan / connect").clicked() {
                        let readings = self.readings.clone();
                        let listeners = self._listeners.clone();
                        let control = self.control_char.clone();
                        // clone an Rc pointing at the device slot so the spawned task can persist the device
                        let device_slot = self.device.clone();

                        // spawn an async task to open the device and wire the receiver
                        wasm_bindgen_futures::spawn_local(async move {
                            match bt::open_device().await {
                                Ok((mut device, mut rx)) => {
                                    // move listeners into the app so closures are kept alive
                                    device.move_listeners_into(listeners.clone());

                                    // store control char if available
                                    if let Some(cc) = device.control_char_clone() {
                                        control.borrow_mut().replace(cc);
                                    }

                                    // persist device into the shared slot so UI code can call methods on it
                                    device_slot.borrow_mut().replace(device);

                                    // forward incoming events into the readings vector
                                    let r = readings.clone();
                                    wasm_bindgen_futures::spawn_local(async move {
                                        while let Some(evt) = rx.next().await {
                                            let reading = match evt {
                                                bt::DalyBtleEvent::Received(r) => r,
                                            };
                                            r.borrow_mut().push(reading);
                                        }
                                    });
                                }
                                Err(e) => web_sys::console::error_1(&e),
                            }
                        });
                    }

                    ui.separator();
                    ui.label("Control:");
                    if ui.button("Request status").clicked() {
                        // Prefer calling request_status on the persisted device (if available).
                        let dev_opt = self.device.borrow();
                        if let Some(dev) = dev_opt.as_ref() {
                            dev.request_status();
                        } else {
                            ui.label("No connected device. Start scan to connect first.");
                        }
                    }

                    // Show connected device name and disconnect control
                    if let Some(dev) = self.device.borrow().as_ref() {
                        if let Some(name) = dev.name() {
                            ui.label(format!("Connected: {}", name));
                        } else {
                            ui.label("Connected: (unknown)");
                        }
                        if ui.button("Disconnect").clicked() {
                            // call disconnect and clear stored device & control char
                            dev.disconnect();
                            self.device.borrow_mut().take();
                            self.control_char.borrow_mut().take();
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
