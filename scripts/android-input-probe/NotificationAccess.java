package dev.aiphone.input.probe;

import android.accessibilityservice.AccessibilityServiceInfo;
import android.content.ComponentName;
import android.os.IBinder;
import android.os.Process;
import java.lang.reflect.Method;
import java.util.List;

/** Root-only observation of the bundled input services for Android user 0. */
public final class NotificationAccess {
    private static Object service(String name, Class<?> api) throws ReflectiveOperationException {
        IBinder binder = (IBinder) Class.forName("android.os.ServiceManager")
            .getMethod("getService", String.class).invoke(null, name);
        return Class.forName(api.getName() + "$Stub").getMethod("asInterface", IBinder.class)
            .invoke(null, binder);
    }

    private static boolean attentionInstalled() throws ReflectiveOperationException {
        Class<?> api = Class.forName("android.view.accessibility.IAccessibilityManager");
        Object slice = api.getMethod("getInstalledAccessibilityServiceList", int.class)
            .invoke(service("accessibility", api), 0);
        Method getList = slice.getClass().getMethod("getList");
        getList.setAccessible(true);
        List<?> installed = (List<?>) getList.invoke(slice);
        ComponentName component = new ComponentName("dev.aiphone.input", "dev.aiphone.input.PhoneAttention");
        for (Object entry : installed) {
            AccessibilityServiceInfo info = (AccessibilityServiceInfo) entry;
            if (component.equals(ComponentName.unflattenFromString(info.getId()))) return true;
        }
        return false;
    }

    public static void main(String[] args) {
        try {
            if (Process.myUid() != 0) throw new IllegalStateException();
            if (args.length == 1 && "attention-installed".equals(args[0])) {
                System.out.println(attentionInstalled());
                return;
            }
            if (args.length != 0) throw new IllegalStateException();
            Class<?> api = Class.forName("android.app.INotificationManager");
            Object granted = api.getMethod("isNotificationListenerAccessGrantedForUser",
                ComponentName.class, int.class).invoke(service("notification", api),
                new ComponentName("dev.aiphone.input", "dev.aiphone.input.PhoneNotifications"), 0);
            if (!(granted instanceof Boolean)) throw new IllegalStateException();
            System.out.println(((Boolean) granted).booleanValue() ? "true" : "false");
        } catch (Throwable error) {
            // No Binder/provider message or other user fields cross this boundary.
            System.err.println("Android input service observation is unknown");
            System.exit(1);
        }
    }
}
