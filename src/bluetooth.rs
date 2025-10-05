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

pub fn start_scan(
    readings: Rc<RefCell<Vec<crate::app::Reading>>>,
    listeners: Rc<RefCell<Vec<Closure<dyn FnMut(JsValue)>>>>,
    control_char: Rc<RefCell<Option<BluetoothRemoteGattCharacteristic>>>,
) {
    wasm_bindgen_futures::spawn_local(async move {
        let window = web_sys::window().expect("no window");
        let navigator = window.navigator();
        let bluetooth = match navigator.bluetooth() {
            Some(bt) => bt,
            None => {
                web_sys::console::error_1(&JsValue::from_str("Bluetooth not available"));
                return;
            }
        };

        // Request device with Daly service UUIDs
        let options = web_sys::RequestDeviceOptions::new();
        let filter = BluetoothLeScanFilterInit::new();
        filter.set_name_prefix("DL-");
        options.set_filters(&js_sys::Array::of1(&filter));

        let optional_services = js_sys::Array::new();
        optional_services.push(&JsValue::from_str("0000fff0-0000-1000-8000-00805f9b34fb"));
        optional_services.push(&JsValue::from_str("0000fff1-0000-1000-8000-00805f9b34fb"));
        optional_services.push(&JsValue::from_str("0000fff2-0000-1000-8000-00805f9b34fb"));
        options.set_optional_services(&optional_services);

        let device: BluetoothDevice =
            match wasm_bindgen_futures::JsFuture::from(bluetooth.request_device(&options)).await {
                Ok(d) => d.unchecked_into(),
                Err(e) => {
                    web_sys::console::error_1(&e);
                    return;
                }
            };

        let gatt = match device.gatt() {
            Some(g) => g,
            None => {
                web_sys::console::error_1(&JsValue::from_str("No GATT server"));
                return;
            }
        };

        let server: BluetoothRemoteGattServer =
            match wasm_bindgen_futures::JsFuture::from(gatt.connect()).await {
                Ok(s) => s.unchecked_into(),
                Err(e) => {
                    web_sys::console::error_1(&e);
                    return;
                }
            };

        let services_js =
            match wasm_bindgen_futures::JsFuture::from(server.get_primary_services()).await {
                Ok(s) => s,
                Err(e) => {
                    web_sys::console::error_1(&e);
                    return;
                }
            };

        let device_name = device.name().unwrap_or_default();
        let services = js_sys::Array::from(&services_js);

        for svc_val in services.iter() {
            let svc: BluetoothRemoteGattService = svc_val.unchecked_into();
            let service_uuid = svc.uuid();

            let chars_js =
                match wasm_bindgen_futures::JsFuture::from(svc.get_characteristics()).await {
                    Ok(c) => c,
                    Err(e) => {
                        web_sys::console::error_1(&e);
                        continue;
                    }
                };

            let chars = js_sys::Array::from(&chars_js);
            for char_val in chars.iter() {
                let ch: BluetoothRemoteGattCharacteristic = char_val.unchecked_into();
                let char_uuid = ch.uuid();
                let props = ch.properties();

                // Store control characteristic (FFF2) if writable
                if char_uuid.to_lowercase().starts_with("0000fff2") && props.write() {
                    control_char.borrow_mut().replace(ch.clone());
                    web_sys::console::log_1(&JsValue::from_str(
                        "Stored control characteristic (FFF2)",
                    ));
                }

                if props.notify() {
                    handle_notify_characteristic(
                        &ch,
                        &device_name,
                        &service_uuid,
                        &char_uuid,
                        readings.clone(),
                        listeners.clone(),
                    )
                    .await;
                } else if props.read() {
                    handle_read_characteristic(
                        &ch,
                        &device_name,
                        &service_uuid,
                        &char_uuid,
                        readings.clone(),
                    )
                    .await;
                }
            }
        }
    });
}

async fn handle_notify_characteristic(
    ch: &BluetoothRemoteGattCharacteristic,
    device_name: &str,
    service_uuid: &str,
    char_uuid: &str,
    readings: Rc<RefCell<Vec<crate::app::Reading>>>,
    listeners: Rc<RefCell<Vec<Closure<dyn FnMut(JsValue)>>>>,
) {
    // Start notifications
    let _ = wasm_bindgen_futures::JsFuture::from(ch.start_notifications()).await;

    // Try initial read
    if let Some(initial_hex) = try_read_value(ch).await {
        web_sys::console::log_1(&JsValue::from_str(&format!(
            "Initial read {} {} {} -> {}",
            device_name, service_uuid, char_uuid, initial_hex
        )));
        readings.borrow_mut().push(crate::app::Reading {
            device: device_name.to_string(),
            service: service_uuid.to_string(),
            characteristic: char_uuid.to_string(),
            value_hex: initial_hex,
            value_text: None,
            ts: js_sys::Date::now(),
        });
    }

    // Create notification listener
    let device_name = device_name.to_string();
    let service_uuid = service_uuid.to_string();
    let char_uuid = char_uuid.to_string();
    let readings_clone = readings.clone();

    let closure = Closure::wrap(Box::new(move |ev: JsValue| {
        if let Some(hex) = extract_value_from_event(&ev) {
            web_sys::console::log_1(&JsValue::from_str(&format!(
                "Notification {} {} {} -> {}",
                device_name, service_uuid, char_uuid, hex
            )));
            readings_clone.borrow_mut().push(crate::app::Reading {
                device: device_name.clone(),
                service: service_uuid.clone(),
                characteristic: char_uuid.clone(),
                value_hex: hex,
                value_text: None,
                ts: js_sys::Date::now(),
            });
        }
    }) as Box<dyn FnMut(_)>);

    ch.add_event_listener_with_callback(
        "characteristicvaluechanged",
        closure.as_ref().unchecked_ref(),
    )
    .ok();
    listeners.borrow_mut().push(closure);
}

async fn handle_read_characteristic(
    ch: &BluetoothRemoteGattCharacteristic,
    device_name: &str,
    service_uuid: &str,
    char_uuid: &str,
    readings: Rc<RefCell<Vec<crate::app::Reading>>>,
) {
    if let Some(hex) = try_read_value(ch).await {
        web_sys::console::log_1(&JsValue::from_str(&format!(
            "Read {} {} {} -> {}",
            device_name, service_uuid, char_uuid, hex
        )));
        readings.borrow_mut().push(crate::app::Reading {
            device: device_name.to_string(),
            service: service_uuid.to_string(),
            characteristic: char_uuid.to_string(),
            value_hex: hex,
            value_text: None,
            ts: js_sys::Date::now(),
        });
    }
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

pub fn write_control_command(control_char: Rc<RefCell<Option<BluetoothRemoteGattCharacteristic>>>) {
    wasm_bindgen_futures::spawn_local(async move {
        let Some(ch) = control_char.borrow().clone() else {
            web_sys::console::log_1(&JsValue::from_str("No control characteristic stored"));
            return;
        };

        let char_uuid = ch.uuid();
        web_sys::console::log_1(&JsValue::from_str(&format!(
            "Writing to characteristic: {}",
            char_uuid
        )));

        let frame = build_daly_read_command();
        let hex = frame
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<Vec<_>>()
            .join(" ");
        web_sys::console::log_1(&JsValue::from_str(&format!("Control frame: {}", hex)));

        let u8a = Uint8Array::from(frame.as_slice());
        match ch.write_value_with_u8_array(&u8a) {
            Ok(promise) => match wasm_bindgen_futures::JsFuture::from(promise).await {
                Ok(_) => web_sys::console::log_1(&JsValue::from_str("Write succeeded")),
                Err(e) => web_sys::console::error_1(&e),
            },
            Err(e) => web_sys::console::error_1(&e),
        }
    });
}
