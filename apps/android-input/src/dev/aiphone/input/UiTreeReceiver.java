package dev.aiphone.input;

import android.app.Activity;
import android.app.NotificationManager;
import android.content.BroadcastReceiver;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.util.Base64;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityManager;
import java.nio.charset.StandardCharsets;

/** DUMP-protected observations of the current screen and input services. */
public final class UiTreeReceiver extends BroadcastReceiver {
    @Override public void onReceive(Context context, Intent intent) {
        setResultCode(Activity.RESULT_CANCELED);
        if ("dev.aiphone.input.READ_ACCESS".equals(intent.getAction())) {
            String service = intent.getStringExtra("service");
            final boolean ready;
            if ("notification".equals(service)) {
                ready = context.getSystemService(NotificationManager.class)
                    .isNotificationListenerAccessGranted(new ComponentName(context, PhoneNotifications.class));
            } else if ("attention".equals(service)) {
                ComponentName attention = new ComponentName(context, PhoneAttention.class);
                ready = context.getSystemService(AccessibilityManager.class).getInstalledAccessibilityServiceList()
                    .stream().anyMatch(info -> attention.equals(ComponentName.unflattenFromString(info.getId())));
            } else return;
            setResultData(service + ":" + ready);
            setResultCode(Activity.RESULT_OK);
            return;
        }
        String request=intent.getStringExtra("request_id");
        if(!"dev.aiphone.input.DUMP_UI".equals(intent.getAction())||request==null||!request.matches("[a-f0-9]{32}"))return;
        AccessibilityNodeInfo root=null;
        try {
            root=PhoneAttention.activeRoot();
            if(root==null)return;
            byte[] xml=UiHierarchy.capture(root).getBytes(StandardCharsets.UTF_8);
            if(xml.length>262144)return;
            setResultData(request+":"+Base64.encodeToString(xml,Base64.NO_WRAP));
            setResultCode(Activity.RESULT_OK);
        } catch(Exception unavailable) {
            // An incomplete or unavailable hierarchy must fall back to another source.
        } finally { if(root!=null)root.recycle(); }
    }
}
