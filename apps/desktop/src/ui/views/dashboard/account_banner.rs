use super::*;
use crate::ui::controls::action;

impl DashboardView {
    pub(super) fn account_banner(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if crate::managed::active().is_some()
            || self.dashboard.route() == super::super::nav::Route::Settings
        {
            return None;
        }
        let theme = self.theme;
        Some(div()
            .w_full()
            .mb(px(24.0))
            .p(px(24.0))
            .rounded(px(radius::LG))
            .border_1()
            .border_color(theme.control_border(true))
            .bg(theme.fill(theme.accent))
            .flex()
            .flex_col()
            .gap(px(16.0))
            .child(div().flex().items_center().gap(px(10.0))
                .child(icons::icon(&theme, Icon::Cloud, 22.0))
                .child(div().text_size(scaled(11.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child("YOUR LENNETTE WORKSPACE")))
            .child(div().text_size(scaled(25.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme.foreground).whitespace_normal().child("Connect your Lennette account."))
            .child(div().text_size(scaled(13.0)).text_color(theme.muted_foreground).whitespace_normal().child("Already have a managed subscription? Connect it here for AI answers, live transcription, and meeting history across your devices. No provider keys to configure."))
            .child(div().flex().flex_wrap().gap(px(16.0))
                .child(benefit(&theme, Icon::Sparkles, "AI & transcription included"))
                .child(benefit(&theme, Icon::RefreshCw, "Live cross-device sync"))
                .child(benefit(&theme, Icon::FolderOpen, "History & attachments together")))
            .child(div().flex().flex_wrap().items_center().gap(px(12.0))
                .child(action("banner-connect-account",&theme,"Connect account",cx,|view:&mut Self,cx,_| {
                    if let Some(editor) = view.settings_search.as_ref() {
                        editor.update(cx, |editor,cx| {editor.input.clear();cx.notify();});
                    }
                    view.navigate_with(super::super::nav::Route::Settings,cx);
                }))
                .child(action("banner-view-plans",&theme,"View plans",cx,|_:&mut Self,cx,_| {
                    let website=std::env::var("LENNETTE_WEBSITE_URL").unwrap_or_else(|_|"https://lennette.itssingularity.com".into());
                    cx.open_url(&format!("{}/pricing",website.trim_end_matches('/')));
                })))
            .child(div().text_size(scaled(11.0)).text_color(theme.muted_foreground).whitespace_normal().child("Managed features require a paid plan. You choose whether to import existing local history when connecting."))
            .into_any_element())
    }
}

fn benefit(theme: &Theme, icon: Icon, label: &'static str) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .text_size(scaled(11.0))
        .text_color(theme.foreground)
        .child(icons::icon(theme, icon, 14.0))
        .child(label)
}
