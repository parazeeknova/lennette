use super::*;
type View = crate::ui::dashboard::view::DashboardView;

pub(super) fn panel(view: &View, theme: &Theme, cx: &mut Context<View>) -> gpui::AnyElement {
    let signed_in = crate::managed::active().is_some();
    let mut content = section(
        theme,
        "Account & sync",
        "Use your own keys locally, or sign in for managed AI and sync across devices.",
    )
    .child(help_text(theme, &crate::managed::status()));
    if signed_in {
        content = content
            .child(row(
                theme,
                "Account usage",
                "View your current subscription and remaining usage",
                action(
                    "account-usage",
                    theme,
                    "Refresh usage",
                    cx,
                    |view: &mut View, cx, _| view.refresh_account_usage(cx),
                ),
            ))
            .child(row(
                theme,
                "Managed · $17.10/month",
                "300 meeting minutes, model-weighted AI credits and 1 GB storage",
                action(
                    "account-standard",
                    theme,
                    "Subscribe",
                    cx,
                    |view: &mut View, cx, _| view.open_account_billing(Some("managed"), cx),
                ),
            ))
            .child(row(
                theme,
                "Managed Plus · $40/month",
                "1,200 meeting minutes, more AI credits, premium models and 5 GB storage",
                action(
                    "account-plus",
                    theme,
                    "Subscribe",
                    cx,
                    |view: &mut View, cx, _| view.open_account_billing(Some("managed_plus"), cx),
                ),
            ))
            .child(row(
                theme,
                "Billing",
                "Manage your subscription, invoices or cancellation",
                action(
                    "account-portal",
                    theme,
                    "Open billing",
                    cx,
                    |view: &mut View, cx, _| view.open_account_billing(None, cx),
                ),
            ))
            .child(row(
                theme,
                "Sync conflicts",
                "Keep the queued edits on this device, or accept the saved cloud versions",
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
            ))
            .child(row(
                theme,
                "Local only",
                "Stop syncing and remove this device's saved sign-in",
                action(
                    "account-signout",
                    theme,
                    "Sign out",
                    cx,
                    |view: &mut View, cx, _| view.sign_out_account(cx),
                ),
            ));
        let mut models = div().flex().flex_wrap().gap(px(8.0));
        for (index, (id, label)) in [
            ("openai/gpt-6-luna", "Luna · 1×"),
            ("anthropic/claude-haiku-5-5", "Haiku · 1×"),
            ("deepseek/deepseek-v4.1-flash", "DeepSeek Flash · 3×"),
            ("google/gemini-3.5-flash-lite", "Gemini Lite · 5×"),
            ("google/gemini-3.8-flash", "Gemini Flash · 8×"),
            ("anthropic/claude-sonnet-5-5", "Sonnet · 20× · Plus"),
            ("openai/gpt-6.1-sol", "Sol · 20× · Plus"),
        ]
        .into_iter()
        .enumerate()
        {
            models = models.child(action(
                ("managed-model", index),
                theme,
                label,
                cx,
                move |view: &mut View, cx, _| {
                    view.select_managed_model(id);
                    cx.notify();
                },
            ));
        }
        content = content.child(row(
            theme,
            "Managed model",
            "Usage varies with context length and generated text. Premium models require Plus.",
            models,
        ));
    } else {
        content=content.child(row(theme,"Import existing history","Include this device's existing conversations, transcripts, prompts and profiles on first sign-in",switch("account-import",theme,view.account_import(),cx,|view:&mut View,cx,_|{view.toggle_account_import();cx.notify();})))
            .child(row(theme,"Sign in","Your browser will open. Provider keys and device settings stay local.",div().flex().gap(px(8.0))
                .child(action("account-google",theme,"Google",cx,|view:&mut View,cx,_|view.start_account_login("google",cx)))
                .child(action("account-github",theme,"GitHub",cx,|view:&mut View,cx,_|view.start_account_login("github",cx)))));
    }
    if let Some(note) = view.account_note() {
        content = content.child(help_text(theme, note));
    }
    content.into_any_element()
}
