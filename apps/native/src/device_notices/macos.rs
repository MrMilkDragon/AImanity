use super::{Banner, PhoneNotification};
use crate::macos_accessibility::attribute;
use core_foundation::{
    array::CFArray,
    base::{CFType, CFTypeRef, TCFType},
    string::CFString,
};
use objc2_app_kit::NSRunningApplication;
use objc2_foundation::NSString;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCreateApplication(pid: libc::pid_t) -> CFTypeRef;
}

fn string(node: &CFType, key: &'static str) -> String {
    attribute(node, key)
        .and_then(|v| v.downcast_into::<CFString>())
        .map_or_else(String::new, |s| s.to_string())
}
fn children(node: &CFType, key: &'static str) -> Vec<CFType> {
    attribute(node, key)
        .and_then(|v| v.downcast_into::<CFArray>())
        .map_or_else(Vec::new, |items| {
            items
                .iter()
                .map(|p| unsafe { CFType::wrap_under_get_rule(*p) })
                .collect()
        })
}

pub(super) fn visible() -> Result<Vec<Banner>, String> {
    objc2::rc::autoreleasepool(|_| snapshot())
}

fn snapshot() -> Result<Vec<Banner>, String> {
    let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(
        "com.apple.notificationcenterui",
    ));
    let Some(app) = apps.firstObject() else {
        return Ok(Vec::new());
    };
    let ptr = unsafe { AXUIElementCreateApplication(app.processIdentifier()) };
    if ptr.is_null() {
        return Err("Notification Center unavailable".into());
    }
    let center = unsafe { CFType::wrap_under_create_rule(ptr) };
    let mut nodes = children(&center, "AXWindows");
    let mut result = Vec::new();
    while let Some(node) = nodes.pop() {
        let id = string(&node, "AXIdentifier");
        if id == "AXNotificationListItems" {
            continue;
        }
        let subrole = string(&node, "AXSubrole");
        if matches!(
            subrole.as_str(),
            "AXNotificationCenterBanner" | "AXNotificationCenterAlert"
        ) {
            let mut title = String::new();
            let mut body = Vec::new();
            let mut fields = children(&node, "AXChildren");
            fields.reverse();
            while let Some(field) = fields.pop() {
                match string(&field, "AXIdentifier").as_str() {
                    "title" => title = string(&field, "AXValue"),
                    "subtitle" | "body" => {
                        let value = string(&field, "AXValue");
                        if !value.is_empty() {
                            body.push(value);
                        }
                    }
                    _ => fields.extend(children(&field, "AXChildren").into_iter().rev()),
                }
            }
            let label = string(&node, "AXDescription");
            let app_name = label
                .split_once(&title)
                .filter(|_| !title.is_empty())
                .map(|(name, _)| name.trim_matches([' ', ',']).to_owned())
                .unwrap_or_default();
            if body.is_empty() && title.is_empty() && !label.is_empty() {
                body.push(label);
            }
            if !title.is_empty() || !body.is_empty() {
                let text = body.join("\n");
                result.push(Banner {
                    key: format!("{id}\n{app_name}\n{title}\n{text}"),
                    notice: PhoneNotification {
                        event_id: uuid::Uuid::parse_str(&id)
                            .map(|id| id.to_string())
                            .unwrap_or_default(),
                        app_name,
                        title,
                        text,
                        ..Default::default()
                    },
                });
            }
        } else {
            nodes.extend(children(&node, "AXChildren"));
        }
    }
    Ok(result)
}
