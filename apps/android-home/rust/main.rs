#![no_std]

mod interactions;
mod jni;

use core::panic::PanicInfo;
use core::ptr::null_mut;
use jni::{Arg, Env, Obj, RawEnv, args};

const ACTIVITY: &core::ffi::CStr = c"android/app/Activity";
const OWNER: &core::ffi::CStr = c"Ldev/aiphone/home/HomeActivity;";
const VIEW: &core::ffi::CStr = c"Landroid/view/View;";
const APP: &core::ffi::CStr = c"Landroid/content/pm/ResolveInfo;";
const MANAGER: &core::ffi::CStr = c"Landroid/content/pm/PackageManager;";
const SCROLL: &core::ffi::CStr = c"Ldev/aiphone/home/HomeScroll;";
const HANDLER: &core::ffi::CStr = c"Landroid/os/Handler;";
const ROOT: &core::ffi::CStr = c"Landroid/widget/LinearLayout;";
const SETTINGS: &core::ffi::CStr = c"com.android.settings";
const FILES: &core::ffi::CStr = c"com.android.documentsui";

#[panic_handler]
fn panic(_: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

fn apps(env: Env, home: Obj) -> Obj {
    env.get(home, c"apps", c"Ljava/util/ArrayList;")
}

fn manager(env: Env, home: Obj) -> Obj {
    env.obj(
        home,
        c"getPackageManager",
        c"()Landroid/content/pm/PackageManager;",
        &[],
    )
}

fn metrics(env: Env, home: Obj) -> Obj {
    let resources = env.obj(
        home,
        c"getResources",
        c"()Landroid/content/res/Resources;",
        &[],
    );
    let metrics = env.obj(
        resources,
        c"getDisplayMetrics",
        c"()Landroid/util/DisplayMetrics;",
        &[],
    );
    env.release(resources);
    metrics
}

fn dp(density: f32, pixels: u32) -> i32 {
    (density * pixels as f32 + 0.5) as i32
}

fn density(env: Env, home: Obj) -> f32 {
    let metrics = metrics(env, home);
    let density = env.get_float(metrics, c"density");
    env.release(metrics);
    density
}

fn activity_info(env: Env, app: Obj) -> Obj {
    env.get(app, c"activityInfo", c"Landroid/content/pm/ActivityInfo;")
}

fn package(env: Env, app: Obj) -> Obj {
    let activity = activity_info(env, app);
    let package = env.get(activity, c"packageName", c"Ljava/lang/String;");
    env.release(activity);
    package
}

fn label(env: Env, app: Obj, manager: Obj) -> Obj {
    env.obj(
        app,
        c"loadLabel",
        c"(Landroid/content/pm/PackageManager;)Ljava/lang/CharSequence;",
        args![manager],
    )
}

fn equals(env: Env, value: Obj, literal: &core::ffi::CStr) -> bool {
    let string = env.string(literal);
    let same = env.boolean(value, c"equals", c"(Ljava/lang/Object;)Z", args![string]) != 0;
    env.release(string);
    same
}

fn protected(env: Env, key: Obj) -> bool {
    equals(env, key, SETTINGS) || equals(env, key, FILES)
}

fn post(env: Env, view: Obj, task: Obj) {
    env.boolean(view, c"post", c"(Ljava/lang/Runnable;)Z", args![task]);
}

fn handler(env: Env, home: Obj) -> Obj {
    env.get(home, c"handler", HANDLER)
}

fn launch_intent(env: Env) -> Obj {
    let intent = env.new(
        c"android/content/Intent",
        c"(Ljava/lang/String;)V",
        args![env.string(c"android.intent.action.MAIN")],
    );
    env.release(env.obj(
        intent,
        c"addCategory",
        c"(Ljava/lang/String;)Landroid/content/Intent;",
        args![env.string(c"android.intent.category.LAUNCHER")],
    ));
    intent
}

fn preferences(env: Env, home: Obj) -> Obj {
    env.obj(
        home,
        c"getPreferences",
        c"(I)Landroid/content/SharedPreferences;",
        args![0],
    )
}

fn save_order(env: Env, home: Obj, apps: Obj) {
    let builder = env.new(c"java/lang/StringBuilder", c"()V", &[]);
    for index in 0..env.int(apps, c"size", c"()I", &[]) {
        let app = env.obj(apps, c"get", c"(I)Ljava/lang/Object;", args![index]);
        let key = package(env, app);
        if index != 0 {
            env.release(env.obj(
                builder,
                c"append",
                c"(C)Ljava/lang/StringBuilder;",
                args![124],
            ));
        }
        env.release(env.obj(
            builder,
            c"append",
            c"(Ljava/lang/String;)Ljava/lang/StringBuilder;",
            args![key],
        ));
        env.release(key);
        env.release(app);
    }
    let editor = env.obj(
        preferences(env, home),
        c"edit",
        c"()Landroid/content/SharedPreferences$Editor;",
        &[],
    );
    env.release(env.obj(
        editor,
        c"putString",
        c"(Ljava/lang/String;Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;",
        args![
            env.string(c"order"),
            env.obj(builder, c"toString", c"()Ljava/lang/String;", &[])
        ],
    ));
    env.void(editor, c"apply", c"()V", &[]);
}

fn reload(env: Env, home: Obj) {
    if env.get_bool(home, c"dragging") != 0 || !env.push() {
        return;
    }
    let pm = manager(env, home);
    let found = env.obj(
        pm,
        c"queryIntentActivities",
        c"(Landroid/content/Intent;I)Ljava/util/List;",
        args![launch_intent(env), 0],
    );
    let sort = env.new(c"dev/aiphone/home/HomeSort", c"()V", &[]);
    env.void(found, c"sort", c"(Ljava/util/Comparator;)V", args![sort]);
    let unique = env.new(c"java/util/LinkedHashMap", c"()V", &[]);
    let owner = env.obj(home, c"getPackageName", c"()Ljava/lang/String;", &[]);
    for index in 0..env.int(found, c"size", c"()I", &[]) {
        if !env.push() {
            env.pop();
            return;
        }
        let app = env.obj(found, c"get", c"(I)Ljava/lang/Object;", args![index]);
        let activity = activity_info(env, app);
        let key = env.get(activity, c"packageName", c"Ljava/lang/String;");
        let info = env.get(
            activity,
            c"applicationInfo",
            c"Landroid/content/pm/ApplicationInfo;",
        );
        let system = env.get_int(info, c"flags") & 1 != 0;
        if env.boolean(key, c"equals", c"(Ljava/lang/Object;)Z", args![owner]) == 0
            && !equals(env, key, c"dev.aiphone.input")
            && (!system || protected(env, key))
        {
            env.obj(
                unique,
                c"putIfAbsent",
                c"(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
                args![key, app],
            );
        }
        env.pop();
    }
    let order = env.obj(
        preferences(env, home),
        c"getString",
        c"(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        args![env.string(c"order"), env.string(c"")],
    );
    let keys = env.obj(
        order,
        c"split",
        c"(Ljava/lang/String;)[Ljava/lang/String;",
        args![env.string(c"\\|")],
    );
    let apps = apps(env, home);
    env.void(apps, c"clear", c"()V", &[]);
    for index in 0..env.array_size(keys) {
        let key = env.array_at(keys, index);
        let app = env.obj(
            unique,
            c"remove",
            c"(Ljava/lang/Object;)Ljava/lang/Object;",
            args![key],
        );
        if !app.is_null() {
            env.boolean(apps, c"add", c"(Ljava/lang/Object;)Z", args![app]);
            env.release(app);
        }
        env.release(key);
    }
    let values = env.obj(unique, c"values", c"()Ljava/util/Collection;", &[]);
    let added = env.new(
        c"java/util/ArrayList",
        c"(Ljava/util/Collection;)V",
        args![values],
    );
    env.set(sort, c"manager", MANAGER, pm);
    env.void(added, c"sort", c"(Ljava/util/Comparator;)V", args![sort]);
    env.boolean(apps, c"addAll", c"(Ljava/util/Collection;)Z", args![added]);
    save_order(env, home, apps);
    env.pop();
    render(env, home);
}

fn linear_params(env: Env, width: i32, height: i32) -> Obj {
    env.new(
        c"android/widget/LinearLayout$LayoutParams",
        c"(II)V",
        args![width, height],
    )
}

fn add(env: Env, parent: Obj, view: Obj, params: Obj) {
    env.void(
        parent,
        c"addView",
        c"(Landroid/view/View;Landroid/view/ViewGroup$LayoutParams;)V",
        args![view, params],
    );
}

fn render(env: Env, home: Obj) {
    if !env.push() {
        return;
    }
    let root = env.get(home, c"root", ROOT);
    let scroller = env.new(
        c"dev/aiphone/home/HomeScroll",
        c"(Landroid/content/Context;)V",
        args![home],
    );
    env.set(home, c"scroller", SCROLL, scroller);
    env.void(root, c"removeAllViews", c"()V", &[]);
    let metrics = metrics(env, home);
    let density = env.get_float(metrics, c"density");
    let screen = env.get_int(metrics, c"widthPixels")
        - env.int(root, c"getPaddingLeft", c"()I", &[])
        - env.int(root, c"getPaddingRight", c"()I", &[]);
    let height = env.get_int(metrics, c"heightPixels")
        - env.int(root, c"getPaddingTop", c"()I", &[])
        - env.int(root, c"getPaddingBottom", c"()I", &[])
        - dp(density, 24);
    let columns = ((screen as f32 / dp(density, 82) as f32) as i32).clamp(4, 8);
    let rows = ((height as f32 / dp(density, 106) as f32) as i32).clamp(1, 5);
    let page_size = columns * rows;
    let apps = apps(env, home);
    let size = env.int(apps, c"size", c"()I", &[]);
    let count = ((size + page_size - 1) as f32 / page_size as f32) as i32;
    let count = count.max(1);
    env.set_int(home, c"page", env.get_int(home, c"page").min(count - 1));
    env.set(scroller, c"owner", OWNER, home);
    env.set_int(scroller, c"screen", screen);
    env.set_int(scroller, c"pageSize", page_size);
    env.void(
        scroller,
        c"setHorizontalScrollBarEnabled",
        c"(Z)V",
        args![0],
    );
    env.void(scroller, c"setOverScrollMode", c"(I)V", args![2]);
    env.void(scroller, c"setFillViewport", c"(Z)V", args![1]);
    let pages = env.new(
        c"android/widget/LinearLayout",
        c"(Landroid/content/Context;)V",
        args![home],
    );
    add(
        env,
        scroller,
        pages,
        env.new(
            c"android/widget/FrameLayout$LayoutParams",
            c"(II)V",
            args![-2, -1],
        ),
    );
    let pm = manager(env, home);
    for page in 0..count {
        if !env.push() {
            env.pop();
            return;
        }
        let grid = env.new(
            c"android/widget/GridLayout",
            c"(Landroid/content/Context;)V",
            args![home],
        );
        env.void(grid, c"setColumnCount", c"(I)V", args![columns]);
        env.void(grid, c"setRowCount", c"(I)V", args![rows]);
        env.void(
            grid,
            c"setPadding",
            c"(IIII)V",
            args![dp(density, 8), 0, dp(density, 8), 0],
        );
        add(env, pages, grid, linear_params(env, screen, -1));
        for index in page * page_size..size.min((page + 1) * page_size) {
            if !env.push() {
                env.pop();
                env.pop();
                return;
            }
            let app = env.obj(apps, c"get", c"(I)Ljava/lang/Object;", args![index]);
            let icon = interactions::icon(env, home, app, pm, density);
            let params = env.new(c"android/widget/GridLayout$LayoutParams", c"()V", &[]);
            env.set_int(params, c"width", (screen - dp(density, 16)) / columns);
            env.set_int(params, c"height", dp(density, 106));
            add(env, grid, icon, params);
            env.pop();
        }
        let listener = env.new(c"dev/aiphone/home/HomeGrid", c"()V", &[]);
        env.set(listener, c"owner", OWNER, home);
        env.set_int(listener, c"index", page);
        env.set_int(listener, c"columns", columns);
        env.set_int(listener, c"rows", rows);
        env.set_int(listener, c"screen", screen);
        env.set_int(listener, c"count", count);
        env.void(
            grid,
            c"setOnDragListener",
            c"(Landroid/view/View$OnDragListener;)V",
            args![listener],
        );
        env.pop();
    }
    add(
        env,
        root,
        scroller,
        env.new(
            c"android/widget/LinearLayout$LayoutParams",
            c"(IIF)V",
            args![-1, 0, 1.0f32],
        ),
    );
    if count > 1 {
        let dots = env.new(
            c"android/widget/TextView",
            c"(Landroid/content/Context;)V",
            args![home],
        );
        let marks = env.new(
            c"java/lang/StringBuilder",
            c"(Ljava/lang/String;)V",
            args![env.string(c"●")],
        );
        let mark = env.string(c"  ·");
        for _ in 1..count {
            env.release(env.obj(
                marks,
                c"append",
                c"(Ljava/lang/String;)Ljava/lang/StringBuilder;",
                args![mark],
            ));
        }
        env.void(
            dots,
            c"setText",
            c"(Ljava/lang/CharSequence;)V",
            args![env.obj(marks, c"toString", c"()Ljava/lang/String;", &[])],
        );
        env.void(dots, c"setTextColor", c"(I)V", args![0xffb4bbc5u32 as i32]);
        env.void(dots, c"setGravity", c"(I)V", args![17]);
        env.void(dots, c"setTextSize", c"(F)V", args![12.0f32]);
        env.void(dots, c"setImportantForAccessibility", c"(I)V", args![2]);
        add(env, root, dots, linear_params(env, -1, dp(density, 24)));
    }
    post(env, scroller, scroller);
    env.pop();
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_home_HomeActivity_onCreate(raw: RawEnv, home: Obj, state: Obj) {
    let env = Env(raw);
    env.super_void(
        home,
        ACTIVITY,
        c"onCreate",
        c"(Landroid/os/Bundle;)V",
        args![state],
    );
    let window = env.obj(home, c"getWindow", c"()Landroid/view/Window;", &[]);
    env.void(window, c"setDecorFitsSystemWindows", c"(Z)V", args![0]);
    env.void(window, c"setStatusBarColor", c"(I)V", args![0]);
    env.void(window, c"setNavigationBarColor", c"(I)V", args![0]);
    let root = env.new(
        c"android/widget/LinearLayout",
        c"(Landroid/content/Context;)V",
        args![home],
    );
    env.set(home, c"root", ROOT, root);
    let looper = env.static_obj(
        c"android/os/Looper",
        c"getMainLooper",
        c"()Landroid/os/Looper;",
        &[],
    );
    env.set(
        home,
        c"handler",
        HANDLER,
        env.new(
            c"android/os/Handler",
            c"(Landroid/os/Looper;)V",
            args![looper],
        ),
    );
    env.set(
        home,
        c"apps",
        c"Ljava/util/ArrayList;",
        env.new(c"java/util/ArrayList", c"()V", &[]),
    );
    env.void(root, c"setOrientation", c"(I)V", args![1]);
    let orientation = env.static_field(
        c"android/graphics/drawable/GradientDrawable$Orientation",
        c"TL_BR",
        c"Landroid/graphics/drawable/GradientDrawable$Orientation;",
    );
    let colors = env.ints(&[
        0xff253749u32 as i32,
        0xff111925u32 as i32,
        0xff0b1019u32 as i32,
    ]);
    let background = env.new(
        c"android/graphics/drawable/GradientDrawable",
        c"(Landroid/graphics/drawable/GradientDrawable$Orientation;[I)V",
        args![orientation, colors],
    );
    env.void(
        root,
        c"setBackground",
        c"(Landroid/graphics/drawable/Drawable;)V",
        args![background],
    );
    let insets = env.new(c"dev/aiphone/home/HomeInsets", c"()V", &[]);
    env.set(insets, c"owner", OWNER, home);
    env.void(
        root,
        c"setOnApplyWindowInsetsListener",
        c"(Landroid/view/View$OnApplyWindowInsetsListener;)V",
        args![insets],
    );
    env.void(
        home,
        c"setContentView",
        c"(Landroid/view/View;)V",
        args![root],
    );
    let filter = env.new(c"android/content/IntentFilter", c"()V", &[]);
    for action in [
        c"android.intent.action.PACKAGE_ADDED",
        c"android.intent.action.PACKAGE_REMOVED",
        c"android.intent.action.PACKAGE_CHANGED",
    ] {
        env.void(
            filter,
            c"addAction",
            c"(Ljava/lang/String;)V",
            args![env.string(action)],
        );
    }
    env.void(
        filter,
        c"addDataScheme",
        c"(Ljava/lang/String;)V",
        args![env.string(c"package")],
    );
    let packages = env.new(c"dev/aiphone/home/HomePackages", c"()V", &[]);
    env.set(packages, c"owner", OWNER, home);
    env.set(
        home,
        c"packages",
        c"Landroid/content/BroadcastReceiver;",
        packages,
    );
    env.obj(home, c"registerReceiver",
        c"(Landroid/content/BroadcastReceiver;Landroid/content/IntentFilter;I)Landroid/content/Intent;",
        args![packages, filter, 2]);
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_home_HomeActivity_onResume(raw: RawEnv, home: Obj) {
    let env = Env(raw);
    env.super_void(home, ACTIVITY, c"onResume", c"()V", &[]);
    reload(env, home);
}

fn reset_scroll(env: Env, home: Obj) {
    let scroll = env.get(home, c"scroller", SCROLL);
    if !scroll.is_null() {
        env.void(scroll, c"smoothScrollTo", c"(II)V", args![0, 0]);
    }
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_home_HomeActivity_onNewIntent(
    raw: RawEnv,
    home: Obj,
    intent: Obj,
) {
    let env = Env(raw);
    env.super_void(
        home,
        ACTIVITY,
        c"onNewIntent",
        c"(Landroid/content/Intent;)V",
        args![intent],
    );
    env.set_int(home, c"page", 0);
    reset_scroll(env, home);
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_home_HomeActivity_onBackPressed(raw: RawEnv, home: Obj) {
    reset_scroll(Env(raw), home);
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_home_HomeActivity_onDestroy(raw: RawEnv, home: Obj) {
    let env = Env(raw);
    env.void(
        home,
        c"unregisterReceiver",
        c"(Landroid/content/BroadcastReceiver;)V",
        args![env.get(home, c"packages", c"Landroid/content/BroadcastReceiver;")],
    );
    env.void(
        handler(env, home),
        c"removeCallbacksAndMessages",
        c"(Ljava/lang/Object;)V",
        args![null_mut()],
    );
    env.super_void(home, ACTIVITY, c"onDestroy", c"()V", &[]);
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_home_HomeActivity_run(raw: RawEnv, home: Obj) {
    render(Env(raw), home);
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_home_HomePackages_onReceive(
    raw: RawEnv,
    receiver: Obj,
    _: Obj,
    _: Obj,
) {
    let env = Env(raw);
    reload(env, env.get(receiver, c"owner", OWNER));
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_home_HomeInsets_onApplyWindowInsets(
    raw: RawEnv,
    listener: Obj,
    view: Obj,
    insets: Obj,
) -> Obj {
    let env = Env(raw);
    let home = env.get(listener, c"owner", OWNER);
    let density = density(env, home);
    let bars = env.static_int(
        c"android/view/WindowInsets$Type",
        c"systemBars",
        c"()I",
        &[],
    );
    let safe = env.obj(
        insets,
        c"getInsets",
        c"(I)Landroid/graphics/Insets;",
        args![bars],
    );
    let left = env.get_int(safe, c"left");
    let top = env.get_int(safe, c"top") + dp(density, 28);
    let right = env.get_int(safe, c"right");
    let bottom = env.get_int(safe, c"bottom") + dp(density, 16);
    let changed = env.int(view, c"getPaddingLeft", c"()I", &[]) != left
        || env.int(view, c"getPaddingTop", c"()I", &[]) != top
        || env.int(view, c"getPaddingRight", c"()I", &[]) != right
        || env.int(view, c"getPaddingBottom", c"()I", &[]) != bottom;
    if changed {
        env.void(
            view,
            c"setPadding",
            c"(IIII)V",
            args![left, top, right, bottom],
        );
        if !env.get(home, c"scroller", SCROLL).is_null() {
            post(env, handler(env, home), home);
        }
    }
    insets
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_home_HomeScroll_onTouchEvent(
    raw: RawEnv,
    scroll: Obj,
    event: Obj,
) -> u8 {
    let env = Env(raw);
    let handled = env.super_bool(
        scroll,
        c"android/widget/HorizontalScrollView",
        c"onTouchEvent",
        c"(Landroid/view/MotionEvent;)Z",
        args![event],
    );
    if matches!(env.int(event, c"getActionMasked", c"()I", &[]), 1 | 3) {
        env.set_bool(scroll, c"settle", 1);
        post(env, scroll, scroll);
    }
    handled
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_home_HomeScroll_run(raw: RawEnv, scroll: Obj) {
    let env = Env(raw);
    let home = env.get(scroll, c"owner", OWNER);
    if env.get_bool(scroll, c"settle") != 0 {
        env.set_bool(scroll, c"settle", 0);
        let width = env.int(scroll, c"getWidth", c"()I", &[]);
        let offset = env.int(scroll, c"getScrollX", c"()I", &[]);
        let size = env.int(apps(env, home), c"size", c"()I", &[]);
        let end = ((size - 1) as f32 / env.get_int(scroll, c"pageSize") as f32) as i32;
        let page = ((offset as f32 / width as f32 + 0.5) as i32)
            .max(0)
            .min(end);
        env.set_int(home, c"page", page);
        env.void(scroll, c"smoothScrollTo", c"(II)V", args![page * width, 0]);
    } else {
        let page = env.get_int(home, c"page");
        env.void(
            scroll,
            c"scrollTo",
            c"(II)V",
            args![page * env.get_int(scroll, c"screen"), 0],
        );
    }
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_home_HomeSort_compare(
    raw: RawEnv,
    sorter: Obj,
    a: Obj,
    b: Obj,
) -> i32 {
    let env = Env(raw);
    let pm = env.get(sorter, c"manager", MANAGER);
    if pm.is_null() {
        let a = env.get(activity_info(env, a), c"name", c"Ljava/lang/String;");
        let b = env.get(activity_info(env, b), c"name", c"Ljava/lang/String;");
        env.int(a, c"compareTo", c"(Ljava/lang/String;)I", args![b])
    } else {
        let a = env.obj(label(env, a, pm), c"toString", c"()Ljava/lang/String;", &[]);
        let b = env.obj(label(env, b, pm), c"toString", c"()Ljava/lang/String;", &[]);
        env.int(
            a,
            c"compareToIgnoreCase",
            c"(Ljava/lang/String;)I",
            args![b],
        )
    }
}
