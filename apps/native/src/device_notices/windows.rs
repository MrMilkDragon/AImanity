use super::{Banner, PhoneNotification};
use windows::Win32::{
    System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    },
    UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationCondition, IUIAutomationElement, TreeScope,
        TreeScope_Children, TreeScope_Descendants, UIA_TextControlTypeId,
    },
};
use windows_sys::Win32::{
    Foundation::CloseHandle,
    System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    },
};

fn shell_window(pid: i32) -> bool {
    let Ok(pid) = u32::try_from(pid) else {
        return false;
    };
    let mut buffer = [0u16; 512];
    let mut count = buffer.len() as u32;
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return false;
        }
        let ok = QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut count);
        CloseHandle(process);
        if ok == 0 {
            return false;
        }
    }
    let path = String::from_utf16_lossy(&buffer[..count as usize]);
    path.rsplit('\\').next().is_some_and(|s| {
        ["ShellExperienceHost.exe", "ShellHost.exe", "explorer.exe"]
            .iter()
            .any(|name| s.eq_ignore_ascii_case(name))
    })
}

fn elements(
    node: &IUIAutomationElement,
    scope: TreeScope,
    any: &IUIAutomationCondition,
) -> windows::core::Result<Vec<IUIAutomationElement>> {
    unsafe {
        let list = node.FindAll(scope, any)?;
        (0..list.Length()?).map(|n| list.GetElement(n)).collect()
    }
}
fn toast(node: &IUIAutomationElement) -> bool {
    unsafe {
        node.CurrentAutomationId()
            .is_ok_and(|id| id.to_string().contains("ToastView"))
            || node.CurrentClassName().is_ok_and(|s| s == "ToastView")
    }
}
fn snapshot() -> windows::core::Result<Vec<Banner>> {
    unsafe {
        let ui: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
        let any = ui.CreateTrueCondition()?;
        let root = ui.GetRootElement()?;
        let mut result = Vec::new();
        for window in elements(&root, TreeScope_Children, &any)? {
            let pid = window.CurrentProcessId().unwrap_or_default();
            if !shell_window(pid) {
                continue;
            }
            let banners = if toast(&window) {
                vec![window]
            } else if window
                .CurrentClassName()
                .is_ok_and(|s| s == "Windows.UI.Core.CoreWindow")
            {
                elements(&window, TreeScope_Descendants, &any)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(toast)
                    .collect()
            } else {
                continue;
            };
            for node in banners {
                if node.CurrentIsOffscreen().is_ok_and(|b| b.as_bool()) {
                    continue;
                }
                let mut lines = Vec::new();
                for el in elements(&node, TreeScope_Descendants, &any).unwrap_or_default() {
                    if el.CurrentControlType().ok() == Some(UIA_TextControlTypeId)
                        && let Ok(s) = el.CurrentName()
                    {
                        let s = s.to_string();
                        if !s.is_empty() && lines.last() != Some(&s) {
                            lines.push(s);
                        }
                    }
                }
                let label = node
                    .CurrentName()
                    .map(|n| n.to_string())
                    .unwrap_or_default();
                let title = if lines.is_empty() {
                    label
                } else {
                    lines.remove(0)
                };
                let text = lines.join("\n");
                if title.is_empty() && text.is_empty() {
                    continue;
                }
                let key = format!(
                    "{pid}/{}\n{title}\n{text}",
                    node.CurrentAutomationId().unwrap_or_default()
                );
                result.push(Banner {
                    key,
                    notice: PhoneNotification {
                        app_name: "Windows".into(),
                        title,
                        text,
                        ..Default::default()
                    },
                });
            }
        }
        Ok(result)
    }
}
pub(super) fn visible() -> windows::core::Result<Vec<Banner>> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
    }
    let result = snapshot();
    unsafe {
        CoUninitialize();
    }
    result
}
