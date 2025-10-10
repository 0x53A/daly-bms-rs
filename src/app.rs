use anyhow::Result;
use eframe::egui;
use egui::{Color32, FontFamily, FontId, vec2};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[cfg(target_arch = "wasm32")]
use crate::bluetooth as bt;
#[cfg(target_arch = "wasm32")]
use futures::StreamExt;

use crate::data_structure::*;

// -----------------
// Shared State Types
// -----------------

#[derive(Default, Clone)]
struct AppState {
    readings: Vec<Reading>,
    is_connected: bool,
    device_name: Option<String>,
    last_update: Option<Instant>,
    status_message: String,
}

// -----------------
// Handler Messages
// -----------------

#[derive(Debug)]
enum HandlerMessage {
    #[cfg(target_arch = "wasm32")]
    Connect,
    #[cfg(target_arch = "wasm32")]
    Disconnect,
    #[cfg(target_arch = "wasm32")]
    RequestStatus,
    ClearReadings,
}

// -----------------
// Handler Implementation
// -----------------

#[cfg(target_arch = "wasm32")]
fn create_handler(state: Arc<Mutex<AppState>>) -> Result<futures::channel::mpsc::UnboundedSender<HandlerMessage>> {
    let (tx, mut rx) = futures::channel::mpsc::unbounded::<HandlerMessage>();
    
    // Store device and its resources
    let device_slot: Rc<RefCell<Option<bt::DalyBtleDevice>>> = Rc::new(RefCell::new(None));
    let listeners: Rc<RefCell<Vec<wasm_bindgen::prelude::Closure<dyn FnMut(wasm_bindgen::JsValue)>>>> = 
        Rc::new(RefCell::new(Vec::new()));
    let control_char: Rc<RefCell<Option<web_sys::BluetoothRemoteGattCharacteristic>>> = 
        Rc::new(RefCell::new(None));
    
    // Spawn handler task
    wasm_bindgen_futures::spawn_local(async move {
        while let Some(msg) = rx.next().await {
            match msg {
                HandlerMessage::Connect => {
                    let state_clone = state.clone();
                    let device_slot_clone = device_slot.clone();
                    let listeners_clone = listeners.clone();
                    let control_clone = control_char.clone();
                    
                    wasm_bindgen_futures::spawn_local(async move {
                        match bt::open_device().await {
                            Ok((mut device, mut rx)) => {
                                // Move listeners
                                device.move_listeners_into(listeners_clone.clone());
                                
                                // Store control char
                                if let Some(cc) = device.control_char_clone() {
                                    control_clone.borrow_mut().replace(cc);
                                }
                                
                                let device_name = device.name();
                                
                                // Update state
                                {
                                    let mut state = state_clone.lock().unwrap();
                                    state.is_connected = true;
                                    state.device_name = device_name.clone();
                                    state.status_message = format!("Connected to {}", device_name.unwrap_or_else(|| "device".to_string()));
                                    state.last_update = Some(Instant::now());
                                }
                                
                                // Store device
                                device_slot_clone.borrow_mut().replace(device);
                                
                                // Forward incoming events into readings
                                let state_for_rx = state_clone.clone();
                                wasm_bindgen_futures::spawn_local(async move {
                                    while let Some(evt) = rx.next().await {
                                        let reading = match evt {
                                            bt::DalyBtleEvent::Received(r) => r,
                                        };
                                        let mut state = state_for_rx.lock().unwrap();
                                        state.readings.push(reading);
                                        state.last_update = Some(Instant::now());
                                    }
                                });
                            }
                            Err(e) => {
                                web_sys::console::error_1(&e);
                                let mut state = state_clone.lock().unwrap();
                                state.status_message = format!("Connection failed: {:?}", e);
                                state.last_update = Some(Instant::now());
                            }
                        }
                    });
                }
                
                HandlerMessage::Disconnect => {
                    if let Some(dev) = device_slot.borrow().as_ref() {
                        dev.disconnect();
                    }
                    device_slot.borrow_mut().take();
                    control_char.borrow_mut().take();
                    
                    let mut state = state.lock().unwrap();
                    state.is_connected = false;
                    state.device_name = None;
                    state.status_message = "Disconnected".to_string();
                    state.last_update = Some(Instant::now());
                }
                
                HandlerMessage::RequestStatus => {
                    if let Some(dev) = device_slot.borrow().as_ref() {
                        dev.request_status();
                        let mut state = state.lock().unwrap();
                        state.status_message = "Requested status".to_string();
                        state.last_update = Some(Instant::now());
                    }
                }
                
                HandlerMessage::ClearReadings => {
                    let mut state = state.lock().unwrap();
                    state.readings.clear();
                    state.status_message = "Readings cleared".to_string();
                    state.last_update = Some(Instant::now());
                }
            }
        }
    });
    
    Ok(tx)
}

#[cfg(not(target_arch = "wasm32"))]
fn create_handler(_state: Arc<Mutex<AppState>>) -> Result<futures::channel::mpsc::UnboundedSender<HandlerMessage>> {
    let (tx, _rx) = futures::channel::mpsc::unbounded::<HandlerMessage>();
    Ok(tx)
}

// -----------------
// Main App Structure
// -----------------

pub struct BMSApp {
    state: Arc<Mutex<AppState>>,
    handler: futures::channel::mpsc::UnboundedSender<HandlerMessage>,
}

impl Default for BMSApp {
    fn default() -> Self {
        Self::new(&egui::Context::default()).unwrap()
    }
}

impl BMSApp {
    pub fn new(_cc: &egui::Context) -> Result<Self> {
        let state = Arc::new(Mutex::new(AppState::default()));
        let handler = create_handler(state.clone())?;
        
        Ok(Self {
            state,
            handler,
        })
    }
    
    pub fn ui(&mut self, ctx: &egui::Context) {
        // Clone state Arc for UI access
        let state = self.state.clone();
        let state = state.lock().unwrap();
        
        egui::CentralPanel::default().show(ctx, |ui| {
            // Set dark theme
            ui.ctx().set_visuals(egui::Visuals::dark());
            
            // Header section
            ui.group(|ui| {
                self.draw_header(ui, &state);
            });
            
            ui.add_space(10.0);
            
            // Main content area
            ui.horizontal(|ui| {
                // Left panel
                ui.allocate_ui_with_layout(
                    vec2(200.0, ui.available_height()),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        ui.group(|ui| {
                            self.draw_controls(ui, &state);
                        });
                    },
                );
                
                ui.separator();
                
                // Right panel - readings list
                ui.group(|ui| {
                    self.draw_readings(ui, &state);
                });
            });
        });
        
        // Request repaint for animations or updates
        ctx.request_repaint_after(Duration::from_secs(1));
    }
    
    fn draw_header(&mut self, ui: &mut egui::Ui, state: &AppState) {
        // Decorative header: emulate two-tone misprint by painting text twice with offsets
        let painter = ui.painter();
        let rect = ui.max_rect();
        let x = rect.left() + 24.0;
        let y = rect.top() + 18.0;
        let text = "Daly BMS";
        
        // Pink shadow behind
        painter.text(
            egui::pos2(x + 4.0, y + 2.0),
            egui::Align2::LEFT_TOP,
            text,
            FontId::new(36.0, FontFamily::Name(Arc::from("Cynatar"))),
            Color32::from_rgb(255, 45, 149),
        );
        
        // Foreground yellow
        painter.text(
            egui::pos2(x, y),
            egui::Align2::LEFT_TOP,
            text,
            FontId::new(36.0, FontFamily::Name(Arc::from("Cynatar"))),
            Color32::from_rgb(255, 212, 0),
        );
        
        ui.add_space(64.0);
        
        ui.horizontal(|ui| {
            // Connection status indicator
            let (status_text, status_color) = if state.is_connected {
                ("● Connected", Color32::GREEN)
            } else {
                ("○ Disconnected", Color32::RED)
            };
            ui.colored_label(status_color, status_text);
            
            if let Some(name) = &state.device_name {
                ui.label(format!("({})", name));
            }
            
            ui.separator();
            
            // Last update indicator
            if let Some(last_update) = state.last_update {
                let elapsed = last_update.elapsed().as_secs_f32();
                let color = if elapsed < 1.0 {
                    Color32::GREEN
                } else if elapsed < 5.0 {
                    Color32::YELLOW
                } else {
                    Color32::RED
                };
                ui.colored_label(color, format!("Last update: {:.1}s ago", elapsed));
            }
        });
        
        ui.label(&state.status_message);
    }
    
    fn draw_controls(&mut self, ui: &mut egui::Ui, state: &AppState) {
        ui.label(egui::RichText::new("Controls").heading());
        ui.separator();
        
        #[cfg(target_arch = "wasm32")]
        {
            // Connection controls
            if state.is_connected {
                if ui.button("Disconnect").clicked() {
                    let _ = self.handler.unbounded_send(HandlerMessage::Disconnect);
                }
                
                ui.add_space(5.0);
                
                if ui.button("Request Status").clicked() {
                    let _ = self.handler.unbounded_send(HandlerMessage::RequestStatus);
                }
            } else {
                if ui.button("Connect").clicked() {
                    let _ = self.handler.unbounded_send(HandlerMessage::Connect);
                }
            }
            
            ui.add_space(10.0);
        }
        
        #[cfg(not(target_arch = "wasm32"))]
        {
            ui.label("WebBluetooth is only available in the browser (wasm build).");
            ui.add_space(10.0);
        }
        
        if ui.button("Clear Readings").clicked() {
            let _ = self.handler.unbounded_send(HandlerMessage::ClearReadings);
        }
        
        ui.add_space(10.0);
        
        ui.group(|ui| {
            ui.label("Stats:");
            ui.label(format!("Readings: {}", state.readings.len()));
        });
    }
    
    fn draw_readings(&mut self, ui: &mut egui::Ui, state: &AppState) {
        ui.label(egui::RichText::new("Readings").heading());
        ui.separator();
        
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                if state.readings.is_empty() {
                    ui.label("No readings yet. Click 'Connect' to start.");
                } else {
                    for r in state.readings.iter().rev().take(20) {
                        self.show_reading(ui, r);
                    }
                    
                    if state.readings.len() > 20 {
                        ui.label(format!("... and {} more", state.readings.len() - 20));
                    }
                }
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
