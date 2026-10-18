//! Lucid 的 Windows TSF 壳。
//!
//! 这个 DLL 只处理 COM/TSF、当前文档读写和安全校验。AI 请求、配置读取和会话路由
//! 都由 LucidServer.exe 完成，TSF 不会在按键回调里启动 HTTP 客户端。

#![cfg_attr(windows, allow(non_snake_case))]

mod identity;
mod ipc;
mod keys;
mod session;
mod suggestion;
#[cfg(windows)]
mod win32;

#[cfg(windows)]
mod document;
#[cfg(windows)]
mod text_service;

pub use identity::{CLSID_TEXT_SERVICE, LANG_EN_US, PROFILE_GUID};
pub use keys::{KeyAction, action_for_virtual_key};
pub use session::{Edit, InputSession, KeyDisposition, PendingSuggestion};

#[cfg(windows)]
pub use text_service::active_object_count;

#[cfg(windows)]
mod exports {
    use windows::core::{GUID, HRESULT, IUnknown, Interface};

    use crate::text_service::{ClassFactory, active_object_count};

    fn service_clsid() -> GUID {
        "{7C4E2A11-6B0D-4E58-9A31-1D5C8B0E4F27}"
            .try_into()
            .unwrap_or_else(|_| GUID::zeroed())
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "system" fn DllGetClassObject(
        class_id: *const GUID,
        interface_id: *const GUID,
        object: *mut *mut core::ffi::c_void,
    ) -> HRESULT {
        if object.is_null() {
            return HRESULT(-2147024809);
        }
        unsafe {
            object.write(core::ptr::null_mut());
            if class_id.is_null() || *class_id != service_clsid() {
                return HRESULT(-2147221231);
            }
            let factory: IUnknown = ClassFactory.into();
            factory.query(interface_id, object)
        }
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn DllCanUnloadNow() -> HRESULT {
        if active_object_count() == 0 {
            HRESULT(0)
        } else {
            HRESULT(1)
        }
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn DllRegisterServer() -> HRESULT {
        HRESULT(-2147467263)
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn DllUnregisterServer() -> HRESULT {
        HRESULT(-2147467263)
    }
}
