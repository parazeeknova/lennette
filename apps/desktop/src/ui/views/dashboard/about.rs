//! About is a compact product overview, followed by updates and useful links.
use super::*;
use gpui::prelude::FluentBuilder;

impl DashboardView {
    pub(super) fn about_body(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = self.theme;
        let narrow = window.viewport_size().width < px(1000.0);
        let updates = crate::ui::widgets::panels::panel(&theme)
            .flex_1().min_w(px(0.0)).p(px(20.0)).flex().flex_col().gap(px(14.0))
            .child(section_heading(&theme, Icon::Download, "Your version", "Keep Lennette up to date."))
            .child(div().flex().items_center().gap(px(8.0))
                .child(div().text_size(scaled(23.0)).font_weight(FontWeight::SEMIBOLD).child(format!("v{}", env!("CARGO_PKG_VERSION"))))
                .child(div().px(px(8.0)).py(px(3.0)).rounded_full().bg(theme.muted)
                    .text_size(scaled(10.0)).text_color(theme.muted_foreground)
                    .child(std::env::consts::OS)))
            .child(div().text_size(scaled(11.5)).line_height(scaled(18.0)).text_color(theme.muted_foreground)
                .child(self.update_status.clone().unwrap_or_else(|| "Check for the latest release and download the installer for your device.".into())))
            .child(div().flex().flex_wrap().gap(px(8.0))
                .child(action_button("about-check-updates", &theme,
                    if self.update_task.is_some() { "Checking…" } else { "Check for updates" }, cx,
                    |this, cx, _| { if this.update_task.is_none() { this.check_updates(cx); } }))
                .children(self.update_available.then(|| action_button("about-open-update", &theme, "Download update", cx,
                    |_, cx, _| cx.open_url("https://github.com/parazeeknova/lennette/releases/latest")))));
        let resources = crate::ui::widgets::panels::panel(&theme)
            .flex_1()
            .min_w(px(0.0))
            .p(px(20.0))
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(section_heading(
                &theme,
                Icon::Compass,
                "Explore & get help",
                "Everything you need in one place.",
            ))
            .child(resource_link(
                "about-website",
                &theme,
                Icon::Globe,
                "Lennette website",
                "Plans, downloads and tutorials",
                "https://lennette.itssingularity.com",
                cx,
            ))
            .child(resource_link(
                "about-github",
                &theme,
                Icon::Code,
                "Source & releases",
                "Browse the project on GitHub",
                "https://github.com/parazeeknova/lennette",
                cx,
            ))
            .child(resource_link(
                "about-contact",
                &theme,
                Icon::Mail,
                "Contact us",
                "harsh@itssingularity.com",
                "mailto:harsh@itssingularity.com",
                cx,
            ));

        div().w_full().min_w(px(0.0)).flex().flex_col().gap(px(22.0))
            .child(self.page_heading("About Lennette", "A little context about the app behind your meetings."))
            .child(crate::ui::widgets::panels::panel(&theme).p(px(24.0)).flex().items_center().gap(px(20.0))
                .child(img(icons::app_icon()).size(px(72.0)).flex_shrink_0().object_fit(gpui::ObjectFit::Contain))
                .child(div().flex_1().min_w(px(0.0)).flex().flex_col().gap(px(9.0))
                    .child(div().text_size(scaled(25.0)).font_weight(FontWeight::SEMIBOLD).child("One less thing to think about."))
                    .child(div().text_size(scaled(12.5)).line_height(scaled(20.0)).text_color(theme.muted_foreground)
                        .child("Follow the conversation, bring the right context, and find the words for what comes next. Lennette keeps transcripts, AI answers and your workspace together."))))
            .child(div().flex().gap(px(20.0)).when(narrow, |row| row.flex_col()).child(updates).child(resources))
            .child(crate::ui::widgets::panels::panel(&theme).p(px(20.0)).flex().flex_col().gap(px(18.0))
                .child(section_heading(&theme, Icon::ShieldCheck, "Your workspace, your choice", "Use your own providers or connect a Lennette account."))
                .child(info_row(&theme, Icon::HardDrive, "Local mode", "Keep history on this device and use your own AI and transcription keys. Keys stay in the encrypted local vault."))
                .child(info_row(&theme, Icon::Cloud, "Connected mode", "A managed subscription adds hosted AI, live sync and cloud storage for text and attachments. Lennette does not store raw meeting audio."))
                .child(info_row(&theme, Icon::Eye, "Context you control", "AI and transcription requests send the relevant text, attachments or audio to the providers that process them.")))
            .child(div().flex().flex_wrap().items_center().justify_between().gap(px(12.0)).pt(px(2.0))
                .child(div().flex().flex_col().gap(px(4.0))
                    .child(div().text_size(scaled(11.5)).font_weight(FontWeight::MEDIUM).child("Made by Singularity Works"))
                    .child(div().text_size(scaled(10.5)).text_color(theme.muted_foreground).child("Built with Rust and GPUI for Windows, macOS and Linux.")))
                .child(div().flex().flex_wrap().gap(px(8.0))
                    .child(action_button("about-studio", &theme, "Singularity Works", cx, |_, cx, _| cx.open_url("https://itssingularity.com")))
                    .child(action_button("about-creator", &theme, "Creator", cx, |_, cx, _| cx.open_url("https://przknv.cc")))))
            .into_any_element()
    }
}

fn section_heading(
    theme: &Theme,
    icon: Icon,
    title: &'static str,
    description: &'static str,
) -> gpui::Div {
    div()
        .flex()
        .items_start()
        .gap(px(10.0))
        .child(icons::icon(theme, icon, 17.0))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(scaled(14.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .child(
                    div()
                        .text_size(scaled(11.0))
                        .text_color(theme.muted_foreground)
                        .child(description),
                ),
        )
}

fn info_row(
    theme: &Theme,
    icon: Icon,
    title: &'static str,
    description: &'static str,
) -> gpui::Div {
    div()
        .flex()
        .items_start()
        .gap(px(12.0))
        .child(
            div()
                .flex_shrink_0()
                .size(px(30.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(9.0))
                .bg(theme.muted)
                .child(icons::icon(theme, icon, 15.0)),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(scaled(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .child(title),
                )
                .child(
                    div()
                        .text_size(scaled(11.5))
                        .line_height(scaled(18.0))
                        .text_color(theme.muted_foreground)
                        .child(description),
                ),
        )
}

fn resource_link(
    id: &'static str,
    theme: &Theme,
    icon: Icon,
    title: &'static str,
    description: &'static str,
    url: &'static str,
    cx: &mut Context<DashboardView>,
) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(10.0))
        .cursor_pointer()
        .child(icons::icon(theme, icon, 15.0))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(px(3.0))
                .child(
                    div()
                        .text_size(scaled(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .child(title),
                )
                .child(
                    div()
                        .text_size(scaled(10.5))
                        .text_color(theme.muted_foreground)
                        .child(description),
                ),
        )
        .child(icons::icon(theme, Icon::ArrowUpRight, 13.0))
        .on_click(cx.listener(move |_, _, _, cx| cx.open_url(url)))
}
