// WebBluetooth functionality using web_sys types
use js_sys::Uint8Array;
use web_sys::BluetoothLeScanFilterInit;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{
    BluetoothDevice, BluetoothRemoteGattCharacteristic, BluetoothRemoteGattServer,
    BluetoothRemoteGattService,
};
use futures::channel::mpsc::{unbounded, UnboundedSender, UnboundedReceiver};
use crate::data_structure::*;

/// Event emitted by the Daly BTLE device wrapper.
#[derive(Clone, Debug)]
pub enum DalyBtleEvent {
    Received(Reading),
}

/// Minimal wrapper around a connected BluetoothDevice for Daly devices.
pub struct DalyBtleDevice {
    device: BluetoothDevice,
    server: BluetoothRemoteGattServer,
    control_char: Option<BluetoothRemoteGattCharacteristic>,
    // Sender for Rust-side events
    sender: UnboundedSender<DalyBtleEvent>,
    // Keep JS Closures alive for characteristic notifications
    js_listeners: Rc<RefCell<Vec<Closure<dyn FnMut(wasm_bindgen::JsValue)>>>>,
}

// legacy start_scan removed — app should call `open_device()` and store the returned device + receiver.
impl DalyBtleDevice {
    /// Write a raw command to the control characteristic if available.
    pub fn write(&self, command: &[u8]) {
        if let Some(ch) = &self.control_char {
            let u8a = Uint8Array::from(command);
            match ch.write_value_with_u8_array(&u8a) {
                Ok(promise) => {
                    wasm_bindgen_futures::spawn_local(async move {
                        match wasm_bindgen_futures::JsFuture::from(promise).await {
                            Ok(_) => web_sys::console::log_1(&JsValue::from_str("Write succeeded")),
                            Err(e) => web_sys::console::error_1(&e),
                        }
                    });
                }
                Err(e) => web_sys::console::error_1(&e),
            }
        } else {
            web_sys::console::log_1(&JsValue::from_str("No control characteristic to write to"));
        }
    }

    /// Helper: clone control characteristic if caller wants to store it separately.
    pub fn control_char_clone(&self) -> Option<BluetoothRemoteGattCharacteristic> {
        self.control_char.clone()
    }

    /// Move any internal JS Closures that keep notification callbacks alive into `dst` so the
    /// application can own them and ensure they are not dropped while JS still holds references.
    pub fn move_listeners_into(&mut self, dst: Rc<RefCell<Vec<Closure<dyn FnMut(wasm_bindgen::JsValue)>>>>) {
        let mut src = self.js_listeners.borrow_mut();
        let mut dst_borrow = dst.borrow_mut();
        dst_borrow.append(&mut src);
    }

    /// Convenience: send the Daly read command to the control characteristic.
    pub fn request_status(&self) {
        let frame = build_daly_read_command();
        self.write(&frame);
    }

    /// Return the device name if available.
    pub fn name(&self) -> Option<String> {
        self.device.name()
    }

    /// Disconnect the underlying GATT server if connected.
    pub fn disconnect(&self) {
        if let Some(server) = self.device.gatt() {
            if server.connected() {
                server.disconnect();
            }
        }
    }
}

/// Open a single Daly device by requesting the device and connecting GATT.
/// Returns (DalyBtleDevice, receiver) where the receiver yields `DalyBtleEvent`.
pub async fn open_device() -> Result<(DalyBtleDevice, UnboundedReceiver<DalyBtleEvent>), wasm_bindgen::JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let navigator = window.navigator();
    let bluetooth = navigator.bluetooth().ok_or_else(|| JsValue::from_str("Bluetooth not available"))?;

    let options = web_sys::RequestDeviceOptions::new();
    let filter = BluetoothLeScanFilterInit::new();
    filter.set_name_prefix("DL-");
    options.set_filters(&js_sys::Array::of1(&filter));

    let optional_services = js_sys::Array::new();
    optional_services.push(&JsValue::from_str("0000fff0-0000-1000-8000-00805f9b34fb"));
    optional_services.push(&JsValue::from_str("0000fff1-0000-1000-8000-00805f9b34fb"));
    optional_services.push(&JsValue::from_str("0000fff2-0000-1000-8000-00805f9b34fb"));
    options.set_optional_services(&optional_services);

    let device: BluetoothDevice = wasm_bindgen_futures::JsFuture::from(bluetooth.request_device(&options)).await?.unchecked_into();

    let gatt = device.gatt().ok_or_else(|| JsValue::from_str("No GATT server"))?;
    let server: BluetoothRemoteGattServer = wasm_bindgen_futures::JsFuture::from(gatt.connect()).await?.unchecked_into();

    let services_js = wasm_bindgen_futures::JsFuture::from(server.get_primary_services()).await?;
    let device_name = device.name().unwrap_or_default();

    let (tx, rx) = unbounded::<DalyBtleEvent>();
    let sender = tx.clone();
    let js_listeners = Rc::new(RefCell::new(Vec::new()));
    let mut control_char: Option<BluetoothRemoteGattCharacteristic> = None;

    let services = js_sys::Array::from(&services_js);

    for svc_val in services.iter() {
        let svc: BluetoothRemoteGattService = svc_val.unchecked_into();
        let service_uuid = svc.uuid();

        let chars_js = match wasm_bindgen_futures::JsFuture::from(svc.get_characteristics()).await {
            Ok(c) => c,
            Err(e) => { web_sys::console::error_1(&e); continue; }
        };

        let chars = js_sys::Array::from(&chars_js);
        for char_val in chars.iter() {
            let ch: BluetoothRemoteGattCharacteristic = char_val.unchecked_into();
            let char_uuid = ch.uuid();
            let props = ch.properties();

            // Store control characteristic (FFF2) if writable
            if char_uuid.to_lowercase().starts_with("0000fff2") && props.write() {
                control_char = Some(ch.clone());
                web_sys::console::log_1(&JsValue::from_str("Stored control characteristic (FFF2)"));
            }

            if props.notify() {
                // start notifications
                let _ = wasm_bindgen_futures::JsFuture::from(ch.start_notifications()).await;

                // Try initial read
                if let Some(initial_hex) = try_read_value(&ch).await {
                    web_sys::console::log_1(&JsValue::from_str(&format!(
                        "Initial read {} {} {} -> {}",
                        device_name, service_uuid, char_uuid, initial_hex
                    )));
                    let reading = Reading {
                        device: device_name.clone(),
                        service: service_uuid.clone(),
                        characteristic: char_uuid.clone(),
                        value_hex: initial_hex,
                        value_text: None,
                        ts: js_sys::Date::now(),
                    };
                    let _ = sender.unbounded_send(DalyBtleEvent::Received(reading));
                }

                // Create notification listener
                let device_name = device_name.clone();
                let service_uuid = service_uuid.clone();
                let char_uuid = char_uuid.clone();
                let sender_clone = sender.clone();

                let closure = Closure::wrap(Box::new(move |ev: JsValue| {
                    if let Some(hex) = extract_value_from_event(&ev) {
                        web_sys::console::log_1(&JsValue::from_str(&format!(
                            "Notification {} {} {} -> {}",
                            device_name, service_uuid, char_uuid, hex
                        )));
                        let reading = Reading {
                            device: device_name.clone(),
                            service: service_uuid.clone(),
                            characteristic: char_uuid.clone(),
                            value_hex: hex,
                            value_text: None,
                            ts: js_sys::Date::now(),
                        };
                        let _ = sender_clone.unbounded_send(DalyBtleEvent::Received(reading));
                    }
                }) as Box<dyn FnMut(_)>);

                ch.add_event_listener_with_callback("characteristicvaluechanged", closure.as_ref().unchecked_ref()).ok();
                js_listeners.borrow_mut().push(closure);
            } else if props.read() {
                if let Some(hex) = try_read_value(&ch).await {
                    web_sys::console::log_1(&JsValue::from_str(&format!(
                        "Read {} {} {} -> {}",
                        device_name, service_uuid, char_uuid, hex
                    )));
                    let reading = Reading {
                        device: device_name.clone(),
                        service: service_uuid.clone(),
                        characteristic: char_uuid.clone(),
                        value_hex: hex,
                        value_text: None,
                        ts: js_sys::Date::now(),
                    };
                    let _ = sender.unbounded_send(DalyBtleEvent::Received(reading));
                }
            }
        }
    }

    Ok((DalyBtleDevice {
        device,
        server,
        control_char,
        sender: tx,
        js_listeners,
    }, rx))
}

async fn try_read_value(ch: &BluetoothRemoteGattCharacteristic) -> Option<String> {
    let val = wasm_bindgen_futures::JsFuture::from(ch.read_value())
        .await
        .ok()?;
    Some(bytes_to_hex(&Uint8Array::new(&val)))
}

fn extract_value_from_event(ev: &JsValue) -> Option<String> {
    // Try target.value
    let mut value = js_sys::Reflect::get(ev, &JsValue::from_str("target"))
        .ok()
        .and_then(|t| js_sys::Reflect::get(&t, &JsValue::from_str("value")).ok())
        .filter(|v| !v.is_null() && !v.is_undefined());

    // Try ev.value
    if value.is_none() {
        value = js_sys::Reflect::get(ev, &JsValue::from_str("value"))
            .ok()
            .filter(|v| !v.is_null() && !v.is_undefined());
    }

    // Try ev.detail.value
    if value.is_none() {
        if let Ok(detail) = js_sys::Reflect::get(ev, &JsValue::from_str("detail")) {
            if !detail.is_null() {
                value = js_sys::Reflect::get(&detail, &JsValue::from_str("value"))
                    .ok()
                    .filter(|v| !v.is_null() && !v.is_undefined());
            }
        }
    }

    let value = value?;

    // Handle DataView
    if let Ok(buffer) = js_sys::Reflect::get(&value, &JsValue::from_str("buffer")) {
        if !buffer.is_null() {
            let byte_offset = js_sys::Reflect::get(&value, &JsValue::from_str("byteOffset"))
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as u32;
            let byte_length = js_sys::Reflect::get(&value, &JsValue::from_str("byteLength"))
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as u32;
            let arr = Uint8Array::new(&buffer);
            let view = arr.subarray(byte_offset, byte_offset + byte_length);
            return Some(bytes_to_hex(&view));
        }
    }

    // Fallback: try direct Uint8Array
    Some(bytes_to_hex(&Uint8Array::new(&value)))
}

fn bytes_to_hex(u8a: &Uint8Array) -> String {
    let mut hex = String::new();
    for i in 0..u8a.length() {
        if !hex.is_empty() {
            hex.push(' ');
        }
        hex.push_str(&format!("{:02x}", u8a.get_index(i)));
    }
    hex
}

fn crc16_ibm_with_init(data: &[u8], init: u16) -> u16 {
    let mut crc = init;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            crc = if (crc & 0x0001) != 0 {
                (crc >> 1) ^ 0xA001
            } else {
                crc >> 1
            };
        }
    }
    crc
}

fn build_daly_read_command() -> Vec<u8> {
    let mut frame = vec![0xD2, 0x03, 0x00, 0x00, 0x00, 0x3E];
    let crc = crc16_ibm_with_init(&frame, 0xFFFF);
    frame.push((crc & 0xFF) as u8);
    frame.push((crc >> 8) as u8);
    frame
}

// Note: legacy `write_control_command` removed. Use `DalyBtleDevice::request_status()` or
// `DalyBtleDevice::write()` directly when you have an owned device instance. If you still
// only have a control characteristic, you can write using the stored `control_char` value
// via `web_sys` APIs from the app — but prefer owning the device.
