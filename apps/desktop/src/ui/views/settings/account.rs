use super::*;
type View = crate::ui::dashboard::view::DashboardView;

pub(super) fn panel(view: &View, theme: &Theme, cx: &mut Context<View>) -> gpui::AnyElement {
    let connected = crate::managed::active().is_some();
    let mut card = div()
        .relative()
        .w_full()
        .p(px(24.0))
        .rounded(px(radius::LG))
        .bg(theme.card)
        .child(crate::ui::widgets::material::dual_edge(*theme, radius::LG))
        .flex()
        .flex_col()
        .gap(px(20.0));
    card = card.child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(icons::icon(theme, Icon::Cloud, 22.0))
                    .child(
                        div()
                            .text_size(scaled(17.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Lennette account"),
                    ),
            )
            .child(
                div()
                    .px(px(10.0))
                    .py(px(5.0))
                    .rounded_full()
                    .bg(theme.muted)
                    .text_size(scaled(10.0))
                    .text_color(theme.muted_foreground)
                    .child(if connected {
                        "Connected"
                    } else {
                        "Local workspace"
                    }),
            ),
    );
    if connected {
        card=card.child(copy(theme,"Your workspace, connected.","Manage your subscription, connected sign-in methods, and devices in your browser. Your history stays available on this device."))
            .child(help_text(theme,&crate::managed::status()));
        if let Some(usage) = view.account_usage() {
            let plan = match usage["subscription"]["plan"].as_str() {
                Some("managed_plus") => "Managed Plus",
                Some("managed") => "Managed",
                _ => "No paid subscription",
            };
            card = card.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .p(px(16.0))
                    .rounded(px(radius::MD))
                    .bg(theme.muted)
                    .child(
                        div()
                            .text_size(scaled(13.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(plan),
                    )
                    .child(meter(
                        theme,
                        "AI credits",
                        usage["ai_used_microdollars"].as_f64().unwrap_or(0.0) / 1000.0,
                        usage["limits"]["ai_microdollars"].as_f64().unwrap_or(0.0) / 1000.0,
                        "credits",
                    ))
                    .child(meter(
                        theme,
                        "Transcription",
                        usage["transcription_seconds"].as_f64().unwrap_or(0.0) / 60.0,
                        usage["limits"]["transcription_seconds"]
                            .as_f64()
                            .unwrap_or(0.0)
                            / 60.0,
                        "min",
                    ))
                    .child(meter(
                        theme,
                        "Synced storage",
                        usage["storage_bytes"].as_f64().unwrap_or(0.0) / 1e9,
                        usage["limits"]["storage_bytes"].as_f64().unwrap_or(0.0) / 1e9,
                        "GB",
                    )),
            );
        }
        card=card.child(div().flex().flex_wrap().gap(px(8.0))
            .child(action("account-dashboard",theme,"Manage account & plan",cx,|_:&mut View,cx,_|open_website(cx,"account")))
            .child(action("account-refresh",theme,if view.account_busy(){"Updating…"}else{"Refresh usage"},cx,|view:&mut View,cx,_|view.refresh_account_usage(cx)))
            .child(action("account-signout",theme,"Disconnect device",cx,|view:&mut View,cx,_|view.sign_out_account(cx))))
            .child(help_text(theme,"Disconnecting stops sync on this device. It does not delete local history or cancel your subscription."));
        if crate::managed::status().to_lowercase().contains("conflict") {
            card = card.child(row(
                theme,
                "Resolve sync conflicts",
                "Choose which version to keep",
                div()
                    .flex()
                    .gap(px(8.0))
                    .child(action(
                        "conflicts-local",
                        theme,
                        "Keep local",
                        cx,
                        |view: &mut View, cx, _| view.resolve_account_conflicts(true, cx),
                    ))
                    .child(action(
                        "conflicts-cloud",
                        theme,
                        "Use cloud",
                        cx,
                        |view: &mut View, cx, _| view.resolve_account_conflicts(false, cx),
                    )),
            ));
        }
    } else {
        card=card.child(copy(theme,"Your meetings. One workspace.","Connect your Lennette subscription for managed AI and live transcription, with conversations and attachments synced across devices."))
            .child(div().flex().flex_wrap().gap(px(16.0))
                .child(feature(theme,Icon::Sparkles,"AI without API keys"))
                .child(feature(theme,Icon::RefreshCw,"Live sync"))
                .child(feature(theme,Icon::FolderOpen,"History & attachments")))
            .child(div().border_t_1().border_color(theme.border).pt(px(16.0)).child(row(theme,"Bring your history along","Optional: import existing conversations, transcripts, prompts, and profiles on first connection.",switch("account-import",theme,view.account_import(),cx,|view:&mut View,cx,_|{view.toggle_account_import();cx.notify();}))))
            .child(div().flex().flex_wrap().gap(px(8.0))
                .child(action("account-google",theme,if view.account_busy(){"Connecting…"}else{"Continue with Google"},cx,|view:&mut View,cx,_|view.start_account_login("google",cx)))
                .child(action("account-github",theme,"Continue with GitHub",cx,|view:&mut View,cx,_|view.start_account_login("github",cx)))
                .child(action("account-plans",theme,"View plans",cx,|_:&mut View,cx,_|open_website(cx,"pricing"))))
            .child(help_text(theme,"Sign in securely in your browser. Creating an account is free; managed AI requires a paid plan. Provider keys and device settings stay local."));
    }
    if let Some(note) = view.account_note() {
        card = card.child(warning(theme, note));
    }
    card.into_any_element()
}
fn copy(theme: &Theme, title: &str, description: &str) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .text_size(scaled(15.0))
                .font_weight(FontWeight::MEDIUM)
                .child(title.to_owned()),
        )
        .child(
            div()
                .text_size(scaled(12.0))
                .text_color(theme.muted_foreground)
                .whitespace_normal()
                .child(description.to_owned()),
        )
}
fn feature(theme: &Theme, icon: Icon, label: &'static str) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .text_size(scaled(11.0))
        .text_color(theme.muted_foreground)
        .child(icons::icon(theme, icon, 14.0))
        .child(label)
}
fn meter(theme: &Theme, label: &str, used: f64, limit: f64, unit: &str) -> impl IntoElement {
    let fraction = if limit > 0.0 {
        (used / limit).clamp(0.0, 1.0)
    } else {
        0.0
    };
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .flex()
                .justify_between()
                .text_size(scaled(11.0))
                .child(label.to_owned())
                .child(format!("{used:.1} / {limit:.1} {unit}")),
        )
        .child(
            div()
                .h(px(5.0))
                .rounded_full()
                .bg(theme.input)
                .overflow_hidden()
                .child(
                    div()
                        .h_full()
                        .w(gpui::relative(fraction as f32))
                        .rounded_full()
                        .bg(theme.primary_fill()),
                ),
        )
}
fn open_website(cx: &mut Context<View>, path: &str) {
    let website = std::env::var("LENNETTE_WEBSITE_URL")
        .unwrap_or_else(|_| "https://lennette.itssingularity.com".into());
    cx.open_url(&format!("{}/{path}", website.trim_end_matches('/')));
}
