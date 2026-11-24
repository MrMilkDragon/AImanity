use core_foundation::{
    base::{CFRange, CFType, CFTypeRef, TCFType},
    string::{CFString, CFStringRef},
};
use std::{ffi::c_void, ptr};

const AX_CF_RANGE: u32 = 4;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCreateSystemWide() -> CFTypeRef;
    fn AXUIElementCopyAttributeValue(
        node: CFTypeRef,
        key: CFStringRef,
        result: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementIsAttributeSettable(node: CFTypeRef, key: CFStringRef, result: *mut u8) -> i32;
    fn AXUIElementSetAttributeValue(node: CFTypeRef, key: CFStringRef, value: CFTypeRef) -> i32;
    fn AXUIElementCopyParameterizedAttributeValue(
        node: CFTypeRef,
        key: CFStringRef,
        parameter: CFTypeRef,
        result: *mut CFTypeRef,
    ) -> i32;
    fn AXValueGetValue(value: CFTypeRef, kind: u32, result: *mut c_void) -> bool;
    fn AXValueCreate(kind: u32, value: *const c_void) -> CFTypeRef;
}

pub(super) fn attribute(node: &CFType, name: &'static str) -> Option<CFType> {
    let key = CFString::from_static_string(name);
    let mut value = ptr::null();
    let status = unsafe {
        AXUIElementCopyAttributeValue(node.as_CFTypeRef(), key.as_concrete_TypeRef(), &mut value)
    };
    (status == 0 && !value.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(value) })
}

pub(super) fn insert_text(text: &str) -> Result<bool, String> {
    if text.is_empty() {
        return Ok(true);
    }
    let desktop = unsafe { CFType::wrap_under_create_rule(AXUIElementCreateSystemWide()) };
    let Some(editor) = attribute(&desktop, "AXFocusedUIElement") else {
        return Ok(false);
    };
    let selected = CFString::from_static_string("AXSelectedText");
    let node = editor.as_CFTypeRef();
    let mut writable = 0;
    if unsafe {
        AXUIElementIsAttributeSettable(node, selected.as_concrete_TypeRef(), &mut writable)
    } != 0
        || writable == 0
    {
        return Ok(false);
    }
    let Some(selection) = attribute(&editor, "AXSelectedTextRange") else {
        return Ok(false);
    };
    let mut range = CFRange::init(0, 0);
    if !unsafe {
        AXValueGetValue(
            selection.as_CFTypeRef(),
            AX_CF_RANGE,
            ptr::from_mut(&mut range).cast(),
        )
    } {
        return Ok(false);
    }
    let read_key = CFString::from_static_string("AXStringForRange");
    let read = |range: &CFType| -> Option<CFString> {
        let mut result = ptr::null();
        let status = unsafe {
            AXUIElementCopyParameterizedAttributeValue(
                node,
                read_key.as_concrete_TypeRef(),
                range.as_CFTypeRef(),
                &mut result,
            )
        };
        (status == 0 && !result.is_null())
            .then(|| unsafe { CFType::wrap_under_create_rule(result) })?
            .downcast_into::<CFString>()
    };
    // Prove that this editor supports range reads before changing the selection.
    range.length = 0;
    let ax_range = |range: &CFRange| unsafe {
        CFType::wrap_under_create_rule(AXValueCreate(AX_CF_RANGE, ptr::from_ref(range).cast()))
    };
    if read(&ax_range(&range)).is_none() {
        return Ok(false);
    }
    let value = CFString::new(text);
    let status = unsafe {
        AXUIElementSetAttributeValue(node, selected.as_concrete_TypeRef(), value.as_CFTypeRef())
    };
    if status != 0 {
        return Err(format!("macOS editor rejected text input ({status})"));
    }
    range.length = value.char_len();
    if read(&ax_range(&range)).as_ref() != Some(&value) {
        return Err(
            "macOS editor did not confirm the inserted text. Observe before continuing.".into(),
        );
    }
    Ok(true)
}
