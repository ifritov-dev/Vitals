//! The battery category, from IOKit's power source description.

use std::ffi::{c_char, c_void, CStr};

use crate::model::{format, CategoryReading, Reading, Sensor};

extern "C" {
    fn IOPSCopyPowerSourcesInfo() -> *mut c_void;
    fn IOPSCopyPowerSourcesList(blob: *mut c_void) -> *mut c_void;
    fn IOPSGetPowerSourceDescription(blob: *mut c_void, source: *mut c_void) -> *mut c_void;
    fn CFArrayGetCount(array: *mut c_void) -> isize;
    fn CFArrayGetValueAtIndex(array: *mut c_void, index: isize) -> *mut c_void;
    fn CFRelease(cf: *mut c_void);
}

pub fn battery(smc: &[(String, f64)]) -> CategoryReading {
        let lookup = |key: &str| smc.iter().find(|(k, _)| k == key).map(|(_, v)| *v);

        unsafe {
            let blob = IOPSCopyPowerSourcesInfo();
            if blob.is_null() {
                return CategoryReading::empty();
            }
            let list = IOPSCopyPowerSourcesList(blob);
            if list.is_null() {
                CFRelease(blob);
                return CategoryReading::empty();
            }
            let count = CFArrayGetCount(list);
            if count == 0 {
                CFRelease(list);
                CFRelease(blob);
                return CategoryReading::empty();
            }
            let source = CFArrayGetValueAtIndex(list, 0);
            let description = IOPSGetPowerSourceDescription(blob, source);
            if description.is_null() {
                CFRelease(list);
                CFRelease(blob);
                return CategoryReading::empty();
            }

            // Both capacities are needed for a percentage; inventing a 0 for a
            // missing one would show a plausible charge level that is not real.
            let (Some(current_capacity), Some(max_capacity)) = (
                dict_int(description, "Current Capacity"),
                dict_int(description, "Max Capacity"),
            ) else {
                CFRelease(list);
                CFRelease(blob);
                return CategoryReading::empty();
            };
            let charging = dict_bool(description, "Is Charging").unwrap_or(false);
            let state = dict_string(description, "Power Source State").unwrap_or_default();
            let on_ac = state == "AC Power";
            let charge_percent = if max_capacity > 0 {
                current_capacity as f64 / max_capacity as f64 * 100.0
            } else {
                current_capacity as f64
            };

            let mut sensors = vec![Sensor {
                id: "battery.charge".into(),
                key: "charge".into(),
                label: "Charge".into(),
                reading: format::percent(charge_percent),
            }];

            // IOPS reports millivolts and milliamps, exactly like B0AV/B0AC,
            // so both sources go through the same division. IOPS wins: B0AC
            // sits at 0 on machines where only the power-source description
            // knows the real draw.
            let voltage = dict_int(description, "Voltage")
                .map(|millivolts| millivolts as f64 / 1000.0)
                .or_else(|| lookup("B0AV").map(|millivolts| millivolts / 1000.0));
            let amperage = dict_int(description, "Current")
                .map(|milliamps| milliamps as f64 / 1000.0)
                .or_else(|| lookup("B0AC").map(|milliamps| milliamps / 1000.0).filter(|amps| *amps != 0.0));
            if let (Some(volts), Some(amps)) = (voltage, amperage) {
                sensors.push(Sensor {
                    id: "battery.power".into(),
                    key: "B0AV×B0AC".into(),
                    label: "Power".into(),
                    reading: format::watts(volts * amps),
                });
                sensors.push(Sensor {
                    id: "battery.voltage".into(),
                    key: "B0AV".into(),
                    label: "Voltage".into(),
                    reading: format::voltage(volts),
                });
                sensors.push(Sensor {
                    id: "battery.current".into(),
                    key: "B0AC".into(),
                    label: "Current".into(),
                    reading: format::amperes(amps),
                });
            }

            let state_text = if charging {
                "Charging"
            } else if on_ac {
                "AC Power"
            } else {
                "On Battery"
            };
            sensors.push(Sensor {
                id: "battery.state".into(),
                key: "state".into(),
                label: "State".into(),
                reading: Reading::new(
                    if on_ac { 1.0 } else { 0.0 },
                    state_text.to_string(),
                    state_text.to_string(),
                ),
            });

            if !on_ac {
                if let Some(minutes) = dict_int(description, "Time to Empty") {
                    if minutes > 0 {
                        let text = format!("{}:{:02}", minutes / 60, minutes % 60);
                        sensors.push(Sensor {
                            id: "battery.remaining".into(),
                            key: "time".into(),
                            label: "Remaining".into(),
                            reading: Reading::new(minutes as f64, text.clone(), text),
                        });
                    }
                }
            }

            CFRelease(list);
            CFRelease(blob);

            CategoryReading {
                sensors,
                average: format::percent(charge_percent),
            }
        }
}


unsafe fn dict_value(dict: *mut c_void, key: &str) -> *mut c_void {
    // The IOPowerSources description is a CFDictionary; NSString keys are
    // toll-free bridged, so the objc runtime can build them.
    // +stringWithUTF8String: hands back an autoreleased string; releasing it
    // here is an over-release and a crash once the pool drains.
    let key_obj = crate::objc::nsstring(key);
    crate::objc::send_obj(dict as crate::objc::Obj, "objectForKey:", key_obj)
}

unsafe fn dict_int(dict: *mut c_void, key: &str) -> Option<isize> {
    let value = dict_value(dict, key);
    if value.is_null() {
        return None;
    }
    Some(crate::objc::send_int_result(value as crate::objc::Obj, "integerValue"))
}

unsafe fn dict_bool(dict: *mut c_void, key: &str) -> Option<bool> {
    let value = dict_value(dict, key);
    if value.is_null() {
        return None;
    }
    Some(crate::objc::send_bool_result(value as crate::objc::Obj, "boolValue"))
}

unsafe fn dict_string(dict: *mut c_void, key: &str) -> Option<String> {
    let value = dict_value(dict, key);
    if value.is_null() {
        return None;
    }
    let utf8 = crate::objc::send(value as crate::objc::Obj, "UTF8String") as *const c_char;
    if utf8.is_null() {
        return None;
    }
    Some(CStr::from_ptr(utf8).to_string_lossy().into_owned())
}
