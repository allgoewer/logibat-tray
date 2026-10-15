//! HID device arrival / removal notifications (CfgMgr32, no window needed).

use crate::worker::Trigger;
use std::ffi::c_void;
use std::sync::mpsc::Sender;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Register_Notification, CM_Unregister_Notification, CM_NOTIFY_ACTION,
    CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL, CM_NOTIFY_ACTION_DEVICEINTERFACEREMOVAL,
    CM_NOTIFY_EVENT_DATA, CM_NOTIFY_FILTER, CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE, CR_SUCCESS,
    HCMNOTIFICATION,
};
use windows_sys::Win32::Devices::HumanInterfaceDevice::GUID_DEVINTERFACE_HID;

/// Active registration. Unregisters on drop.
pub struct Registration {
    handle: HCMNOTIFICATION,
    context: *mut Sender<Trigger>,
}

impl Drop for Registration {
    fn drop(&mut self) {
        // SAFETY: handle came from CM_Register_Notification. After unregister
        // returns, no callback runs any more, so the context can be freed.
        unsafe {
            CM_Unregister_Notification(self.handle);
            drop(Box::from_raw(self.context));
        }
    }
}

unsafe extern "system" fn callback(
    _handle: HCMNOTIFICATION,
    context: *const c_void,
    action: CM_NOTIFY_ACTION,
    _data: *const CM_NOTIFY_EVENT_DATA,
    _size: u32,
) -> u32 {
    // SAFETY: context is the Sender boxed in `register`; it lives until unregister.
    let tx = unsafe { &*(context as *const Sender<Trigger>) };
    let trigger = match action {
        CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL => Some(Trigger::DeviceArrival),
        CM_NOTIFY_ACTION_DEVICEINTERFACEREMOVAL => Some(Trigger::DeviceRemoval),
        _ => None,
    };
    if let Some(t) = trigger {
        let _ = tx.send(t);
    }
    0 // ERROR_SUCCESS
}

/// Send [`Trigger::DeviceArrival`] / [`Trigger::DeviceRemoval`] for each HID
/// interface change. Returns `None` if the registration fails.
pub fn register(tx: Sender<Trigger>) -> Option<Registration> {
    let context = Box::into_raw(Box::new(tx));
    // SAFETY: zeroed is a valid CM_NOTIFY_FILTER; the fields are set below.
    let mut filter: CM_NOTIFY_FILTER = unsafe { std::mem::zeroed() };
    filter.cbSize = std::mem::size_of::<CM_NOTIFY_FILTER>() as u32;
    filter.FilterType = CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE;
    filter.u.DeviceInterface.ClassGuid = GUID_DEVINTERFACE_HID;

    let mut handle: HCMNOTIFICATION = std::ptr::null_mut();
    // SAFETY: all pointers are valid for the call; context stays alive until drop.
    let cr = unsafe {
        CM_Register_Notification(
            &filter,
            context as *const c_void,
            Some(callback),
            &mut handle,
        )
    };
    if cr != CR_SUCCESS {
        log::error!("CM_Register_Notification failed: 0x{cr:x}");
        // SAFETY: registration failed, so no callback holds the pointer.
        drop(unsafe { Box::from_raw(context) });
        return None;
    }
    Some(Registration { handle, context })
}
