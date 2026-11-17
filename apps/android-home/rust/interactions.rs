use super::{
    APP, OWNER, SCROLL, VIEW, activity_info, add, apps, density, dp, handler, label, linear_params,
    package, post, protected, reload, save_order,
};
use crate::jni::{Arg, Env, Obj, RawEnv, args};

pub(super) fn icon(env: Env, home: Obj, app: Obj, manager: Obj, density: f32) -> Obj {
    let layout = env.new(
        c"android/widget/LinearLayout",
        c"(Landroid/content/Context;)V",
        args![home],
    );
    let name = label(env, app, manager);
    env.void(layout, c"setOrientation", c"(I)V", args![1]);
    env.void(layout, c"setGravity", c"(I)V", args![49]);
    env.void(
        layout,
        c"setPadding",
        c"(IIII)V",
        args![0, dp(density, 8), 0, 0],
    );
    env.void(
        layout,
        c"setContentDescription",
        c"(Ljava/lang/CharSequence;)V",
        args![name],
    );
    env.void(layout, c"setFocusable", c"(Z)V", args![1]);
    env.void(layout, c"setClickable", c"(Z)V", args![1]);
    let image = env.new(
        c"android/widget/ImageView",
        c"(Landroid/content/Context;)V",
        args![home],
    );
    let drawable = env.obj(
        app,
        c"loadIcon",
        c"(Landroid/content/pm/PackageManager;)Landroid/graphics/drawable/Drawable;",
        args![manager],
    );
    env.void(
        image,
        c"setImageDrawable",
        c"(Landroid/graphics/drawable/Drawable;)V",
        args![drawable],
    );
    let scale = env.static_field(
        c"android/widget/ImageView$ScaleType",
        c"FIT_CENTER",
        c"Landroid/widget/ImageView$ScaleType;",
    );
    env.void(
        image,
        c"setScaleType",
        c"(Landroid/widget/ImageView$ScaleType;)V",
        args![scale],
    );
    env.void(image, c"setImportantForAccessibility", c"(I)V", args![2]);
    add(
        env,
        layout,
        image,
        linear_params(env, dp(density, 58), dp(density, 58)),
    );
    let text = env.new(
        c"android/widget/TextView",
        c"(Landroid/content/Context;)V",
        args![home],
    );
    env.void(
        text,
        c"setText",
        c"(Ljava/lang/CharSequence;)V",
        args![name],
    );
    env.void(text, c"setTextColor", c"(I)V", args![-1]);
    env.void(text, c"setTextSize", c"(F)V", args![12.0f32]);
    env.void(text, c"setGravity", c"(I)V", args![17]);
    env.void(text, c"setMaxLines", c"(I)V", args![2]);
    env.void(
        text,
        c"setPadding",
        c"(IIII)V",
        args![dp(density, 2), dp(density, 8), dp(density, 2), 0],
    );
    env.void(text, c"setImportantForAccessibility", c"(I)V", args![2]);
    add(env, layout, text, linear_params(env, -1, -2));
    let listener = env.new(c"dev/aiphone/home/HomeIcon", c"()V", &[]);
    env.set(listener, c"owner", OWNER, home);
    env.set(listener, c"app", APP, app);
    env.set(listener, c"box", VIEW, layout);
    env.set(listener, c"image", VIEW, image);
    env.void(
        layout,
        c"setOnClickListener",
        c"(Landroid/view/View$OnClickListener;)V",
        args![listener],
    );
    env.void(
        layout,
        c"setOnTouchListener",
        c"(Landroid/view/View$OnTouchListener;)V",
        args![listener],
    );
    layout
}

fn open(env: Env, home: Obj, app: Obj) {
    let activity = activity_info(env, app);
    let component = env.new(
        c"android/content/ComponentName",
        c"(Ljava/lang/String;Ljava/lang/String;)V",
        args![
            env.get(activity, c"packageName", c"Ljava/lang/String;"),
            env.get(activity, c"name", c"Ljava/lang/String;")
        ],
    );
    let intent = super::launch_intent(env);
    env.obj(
        intent,
        c"setComponent",
        c"(Landroid/content/ComponentName;)Landroid/content/Intent;",
        args![component],
    );
    env.obj(
        intent,
        c"addFlags",
        c"(I)Landroid/content/Intent;",
        args![0x1020_0000],
    );
    env.void(
        home,
        c"startActivity",
        c"(Landroid/content/Intent;)V",
        args![intent],
    );
    if env.exception() {
        reload(env, home);
    }
}

fn menu(env: Env, home: Obj, app: Obj, anchor: Obj) {
    let popup = env.new(
        c"android/widget/PopupMenu",
        c"(Landroid/content/Context;Landroid/view/View;)V",
        args![home, anchor],
    );
    let items = env.obj(popup, c"getMenu", c"()Landroid/view/Menu;", &[]);
    let removable = !protected(env, package(env, app));
    for (action, title) in [c"打开", c"应用信息", c"卸载"]
        .into_iter()
        .take(if removable { 3 } else { 2 })
        .enumerate()
    {
        let item = env.obj(
            items,
            c"add",
            c"(Ljava/lang/CharSequence;)Landroid/view/MenuItem;",
            args![env.string(title)],
        );
        let callback = env.new(c"dev/aiphone/home/HomeMenu", c"()V", &[]);
        env.set(callback, c"owner", OWNER, home);
        env.set(callback, c"app", APP, app);
        env.set_int(callback, c"action", action as i32);
        env.obj(
            item,
            c"setOnMenuItemClickListener",
            c"(Landroid/view/MenuItem$OnMenuItemClickListener;)Landroid/view/MenuItem;",
            args![callback],
        );
    }
    env.void(popup, c"show", c"()V", &[]);
}

fn disallow_intercept(env: Env, view: Obj, disallow: i32) {
    let parent = env.obj(view, c"getParent", c"()Landroid/view/ViewParent;", &[]);
    env.void(
        parent,
        c"requestDisallowInterceptTouchEvent",
        c"(Z)V",
        args![disallow],
    );
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_home_HomeIcon_onClick(raw: RawEnv, listener: Obj, _: Obj) {
    let env = Env(raw);
    open(
        env,
        env.get(listener, c"owner", OWNER),
        env.get(listener, c"app", APP),
    );
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_home_HomeIcon_run(raw: RawEnv, listener: Obj) {
    let env = Env(raw);
    let view = env.get(listener, c"box", VIEW);
    env.set_bool(listener, c"armed", 1);
    disallow_intercept(env, view, 1);
    env.boolean(view, c"performHapticFeedback", c"(I)Z", args![0]);
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_home_HomeIcon_onTouch(
    raw: RawEnv,
    listener: Obj,
    view: Obj,
    event: Obj,
) -> u8 {
    let env = Env(raw);
    let home = env.get(listener, c"owner", OWNER);
    let handler = handler(env, home);
    let action = env.int(event, c"getActionMasked", c"()I", &[]);
    if action == 0 {
        env.set_float(listener, c"x", env.float(event, c"getX", c"()F", &[]));
        env.set_float(listener, c"y", env.float(event, c"getY", c"()F", &[]));
        env.set_bool(listener, c"armed", 0);
        env.set_bool(listener, c"moved", 0);
        let delay = env.static_int(
            c"android/view/ViewConfiguration",
            c"getLongPressTimeout",
            c"()I",
            &[],
        );
        env.boolean(
            handler,
            c"postDelayed",
            c"(Ljava/lang/Runnable;J)Z",
            args![listener, i64::from(delay)],
        );
        env.void(view, c"setAlpha", c"(F)V", args![0.72f32]);
        return 1;
    }
    if action == 3 {
        disallow_intercept(env, view, 0);
        env.void(
            handler,
            c"removeCallbacks",
            c"(Ljava/lang/Runnable;)V",
            args![listener],
        );
        env.void(view, c"setAlpha", c"(F)V", args![1.0f32]);
        return 1;
    }
    let x = env.float(event, c"getX", c"()F", &[]) - env.get_float(listener, c"x");
    let y = env.float(event, c"getY", c"()F", &[]) - env.get_float(listener, c"y");
    let distance = x * x + y * y;
    let density = density(env, home);
    let close = dp(density, 8);
    let far = dp(density, 12);
    let armed = env.get_bool(listener, c"armed") != 0;
    if action == 2 {
        if armed && distance > (close * close) as f32 {
            env.void(
                handler,
                c"removeCallbacks",
                c"(Ljava/lang/Runnable;)V",
                args![listener],
            );
            env.set_bool(listener, c"moved", 1);
            env.void(view, c"setAlpha", c"(F)V", args![1.0f32]);
            let app = env.get(listener, c"app", APP);
            let data = env.static_obj(
                c"android/content/ClipData",
                c"newPlainText",
                c"(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Landroid/content/ClipData;",
                args![env.string(c"app"), package(env, app)],
            );
            let shadow = env.new(
                c"android/view/View$DragShadowBuilder",
                c"(Landroid/view/View;)V",
                args![env.get(listener, c"image", VIEW)],
            );
            let started = env.boolean(view, c"startDragAndDrop",
                c"(Landroid/content/ClipData;Landroid/view/View$DragShadowBuilder;Ljava/lang/Object;I)Z",
                args![data, shadow, app, 0]);
            env.set_bool(home, c"dragging", started);
        } else if !armed && distance > (far * far) as f32 {
            env.void(
                handler,
                c"removeCallbacks",
                c"(Ljava/lang/Runnable;)V",
                args![listener],
            );
        }
    } else if action == 1 {
        disallow_intercept(env, view, 0);
        env.void(
            handler,
            c"removeCallbacks",
            c"(Ljava/lang/Runnable;)V",
            args![listener],
        );
        env.void(view, c"setAlpha", c"(F)V", args![1.0f32]);
        if env.get_bool(listener, c"moved") == 0 {
            if armed {
                menu(env, home, env.get(listener, c"app", APP), view);
            } else if distance < (far * far) as f32 {
                env.boolean(view, c"performClick", c"()Z", &[]);
            }
        }
    }
    1
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_home_HomeMenu_onMenuItemClick(
    raw: RawEnv,
    listener: Obj,
    _: Obj,
) -> u8 {
    let env = Env(raw);
    let home = env.get(listener, c"owner", OWNER);
    let app = env.get(listener, c"app", APP);
    let action = env.get_int(listener, c"action");
    if action == 0 {
        open(env, home, app);
    } else {
        let prefix = env.string(c"package:");
        let uri = env.obj(
            prefix,
            c"concat",
            c"(Ljava/lang/String;)Ljava/lang/String;",
            args![package(env, app)],
        );
        let uri = env.static_obj(
            c"android/net/Uri",
            c"parse",
            c"(Ljava/lang/String;)Landroid/net/Uri;",
            args![uri],
        );
        let action = if action == 1 {
            c"android.settings.APPLICATION_DETAILS_SETTINGS"
        } else {
            c"android.intent.action.DELETE"
        };
        let intent = env.new(
            c"android/content/Intent",
            c"(Ljava/lang/String;Landroid/net/Uri;)V",
            args![env.string(action), uri],
        );
        env.void(
            home,
            c"startActivity",
            c"(Landroid/content/Intent;)V",
            args![intent],
        );
    }
    1
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_home_HomeGrid_onDrag(
    raw: RawEnv,
    grid: Obj,
    _: Obj,
    event: Obj,
) -> u8 {
    let env = Env(raw);
    let home = env.get(grid, c"owner", OWNER);
    match env.int(event, c"getAction", c"()I", &[]) {
        1 => {
            return u8::from(env.instance_of(
                env.obj(event, c"getLocalState", c"()Ljava/lang/Object;", &[]),
                c"android/content/pm/ResolveInfo",
            ));
        }
        2 => {
            let count = env.get_int(grid, c"count");
            if count > 1 {
                let screen = env.get_int(grid, c"screen");
                let x = env.float(event, c"getX", c"()F", &[]);
                let edge = dp(density(env, home), 24);
                let page = env.get_int(home, c"page");
                let next = if x > (screen - edge) as f32 && page < count - 1 {
                    page + 1
                } else if x < edge as f32 && page > 0 {
                    page - 1
                } else {
                    page
                };
                if next != page {
                    env.set_int(home, c"page", next);
                    env.void(
                        env.get(home, c"scroller", SCROLL),
                        c"smoothScrollTo",
                        c"(II)V",
                        args![next * screen, 0],
                    );
                }
            }
        }
        3 => {
            let apps = apps(env, home);
            let item = env.obj(event, c"getLocalState", c"()Ljava/lang/Object;", &[]);
            let screen = env.get_int(grid, c"screen");
            let columns = env.get_int(grid, c"columns");
            let rows = env.get_int(grid, c"rows");
            let density = density(env, home);
            let x = env.float(event, c"getX", c"()F", &[]);
            let y = env.float(event, c"getY", c"()F", &[]);
            let column = (((x - dp(density, 8) as f32)
                / ((screen - dp(density, 16)) as f32 / columns as f32))
                as i32)
                .max(0)
                .min(columns - 1);
            let row = ((y / dp(density, 106) as f32) as i32).max(0).min(rows - 1);
            let destination = env.get_int(grid, c"index") * rows * columns + row * columns + column;
            env.boolean(apps, c"remove", c"(Ljava/lang/Object;)Z", args![item]);
            let index = destination.min(env.int(apps, c"size", c"()I", &[]));
            env.void(apps, c"add", c"(ILjava/lang/Object;)V", args![index, item]);
            save_order(env, home, apps);
        }
        4 => {
            env.set_bool(home, c"dragging", 0);
            post(env, handler(env, home), home);
        }
        _ => {}
    }
    1
}
