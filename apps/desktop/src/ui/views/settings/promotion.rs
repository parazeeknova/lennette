//! Shared Settings header promotion, independent of the selected sub-tab.
use super::*;
type View = crate::ui::dashboard::view::DashboardView;

pub(super) fn banner(view: &View, theme: &Theme, cx: &mut Context<View>) -> impl IntoElement {
    let plus = view
        .account_usage()
        .is_some_and(|usage| usage["subscription"]["plan"] == "managed_plus");
    let connected = crate::managed::active().is_some();
    let (title, description, label) = if plus {
        (
            "Your workspace, everywhere.",
            "Live sync, cloud storage and managed AI. Everything connected to your account.",
            "Manage plan",
        )
    } else if connected {
        (
            "Make room for more.",
            "More AI usage, transcription and cloud storage, with live sync across devices.",
            "Upgrade plan",
        )
    } else {
        (
            "Unlock more with Lennette.",
            "Managed AI, live sync and cloud storage, ready across your devices.",
            "Explore plans",
        )
    };
    let material = theme.promotion();
    let blue = material.background;
    let white = material.foreground;
    div()
        .relative()
        .flex_1()
        .min_w(px(0.0))
        .flex()
        .flex_col()
        .justify_between()
        .gap(px(16.0))
        .p(px(18.0))
        .rounded(px(radius::LG))
        .bg(blue)
        .text_color(white)
        .child(crate::ui::widgets::material::dual_edge(
            material,
            radius::LG,
        ))
        .child(
            div()
                .flex()
                .items_start()
                .gap(px(12.0))
                .child(gpui::img(icons::app_icon()).size(px(40.0)).flex_shrink_0())
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex()
                        .flex_col()
                        .gap(px(5.0))
                        .child(
                            div()
                                .text_size(scaled(17.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(scaled(11.5))
                                .line_height(scaled(17.0))
                                .child(description),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .justify_end()
                .gap(px(8.0))
                .when(!connected, |row| {
                    row.child(account::login_button(
                        "promotion-google",
                        "google",
                        &material,
                        view.account_busy(),
                        cx,
                    ))
                })
                .child(action(
                    "settings-upgrade-banner",
                    &material,
                    label,
                    cx,
                    |_: &mut View, cx, _| account::open_website(cx, "account"),
                )),
        )
}
