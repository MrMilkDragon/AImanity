use super::receiver_dex::{Class, emit_for};

const HOME: &str = "Ldev/aiphone/home/HomeActivity;";
const SCROLL: &str = "Ldev/aiphone/home/HomeScroll;";
const RECEIVER: &str = "Landroid/content/BroadcastReceiver;";
const CONTEXT: &str = "Landroid/content/Context;";
const INTENT: &str = "Landroid/content/Intent;";
const VIEW: &str = "Landroid/view/View;";
const MOTION: &str = "Landroid/view/MotionEvent;";
const INSETS: &str = "Landroid/view/WindowInsets;";
const RESOLVE: &str = "Landroid/content/pm/ResolveInfo;";
const RUNNABLE: &str = "Ljava/lang/Runnable;";
const OBJECT: &str = "Ljava/lang/Object;";
const VOID: (&str, &[&str]) = ("V", &[]);
const OWNER: (&str, &str, u32) = ("owner", HOME, 2);
const RUN: (&str, (&str, &[&str]), u32) = ("run", VOID, 1);

pub(super) fn emit() -> Vec<u8> {
    emit_for(
        &[
            Class {
                name: HOME,
                parent: "Landroid/app/Activity;",
                constructor_args: &[],
                interfaces: &[RUNNABLE],
                fields: &[
                    ("apps", "Ljava/util/ArrayList;", 2),
                    ("root", "Landroid/widget/LinearLayout;", 2),
                    ("scroller", SCROLL, 2),
                    ("handler", "Landroid/os/Handler;", 2),
                    ("packages", RECEIVER, 2),
                    ("page", "I", 2),
                    ("dragging", "Z", 2),
                ],
                natives: &[
                    ("onCreate", ("V", &["Landroid/os/Bundle;"]), 1),
                    ("onResume", VOID, 4),
                    ("onNewIntent", ("V", &[INTENT]), 4),
                    ("onBackPressed", VOID, 1),
                    ("onDestroy", VOID, 4),
                    RUN,
                ],
            },
            Class {
                name: "Ldev/aiphone/home/HomePackages;",
                parent: RECEIVER,
                constructor_args: &[],
                interfaces: &[],
                fields: &[OWNER],
                natives: &[("onReceive", ("V", &[CONTEXT, INTENT]), 1)],
            },
            Class {
                name: "Ldev/aiphone/home/HomeInsets;",
                parent: OBJECT,
                constructor_args: &[],
                interfaces: &["Landroid/view/View$OnApplyWindowInsetsListener;"],
                fields: &[OWNER],
                natives: &[("onApplyWindowInsets", (INSETS, &[VIEW, INSETS]), 1)],
            },
            Class {
                name: SCROLL,
                parent: "Landroid/widget/HorizontalScrollView;",
                constructor_args: &["Landroid/content/Context;"],
                interfaces: &[RUNNABLE],
                fields: &[
                    OWNER,
                    ("screen", "I", 2),
                    ("pageSize", "I", 2),
                    ("settle", "Z", 2),
                ],
                natives: &[("onTouchEvent", ("Z", &[MOTION]), 1), RUN],
            },
            Class {
                name: "Ldev/aiphone/home/HomeIcon;",
                parent: OBJECT,
                constructor_args: &[],
                interfaces: &[
                    "Landroid/view/View$OnClickListener;",
                    "Landroid/view/View$OnTouchListener;",
                    RUNNABLE,
                ],
                fields: &[
                    OWNER,
                    ("app", RESOLVE, 2),
                    ("box", VIEW, 2),
                    ("image", VIEW, 2),
                    ("x", "F", 2),
                    ("y", "F", 2),
                    ("armed", "Z", 2),
                    ("moved", "Z", 2),
                ],
                natives: &[
                    ("onClick", ("V", &[VIEW]), 1),
                    ("onTouch", ("Z", &[VIEW, MOTION]), 1),
                    RUN,
                ],
            },
            Class {
                name: "Ldev/aiphone/home/HomeGrid;",
                parent: OBJECT,
                constructor_args: &[],
                interfaces: &["Landroid/view/View$OnDragListener;"],
                fields: &[
                    OWNER,
                    ("index", "I", 2),
                    ("columns", "I", 2),
                    ("rows", "I", 2),
                    ("screen", "I", 2),
                    ("count", "I", 2),
                ],
                natives: &[("onDrag", ("Z", &[VIEW, "Landroid/view/DragEvent;"]), 1)],
            },
            Class {
                name: "Ldev/aiphone/home/HomeMenu;",
                parent: OBJECT,
                constructor_args: &[],
                interfaces: &["Landroid/view/MenuItem$OnMenuItemClickListener;"],
                fields: &[OWNER, ("app", RESOLVE, 2), ("action", "I", 2)],
                natives: &[("onMenuItemClick", ("Z", &["Landroid/view/MenuItem;"]), 1)],
            },
            Class {
                name: "Ldev/aiphone/home/HomeSort;",
                parent: OBJECT,
                constructor_args: &[],
                interfaces: &["Ljava/util/Comparator;"],
                fields: &[("manager", "Landroid/content/pm/PackageManager;", 2)],
                natives: &[("compare", ("I", &[OBJECT, OBJECT]), 1)],
            },
        ],
        "aimanity_home",
    )
}
