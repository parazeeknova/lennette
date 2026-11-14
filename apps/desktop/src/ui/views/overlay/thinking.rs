use super::*;
use lennette_protocol::thinking::{effort_label, reasoning_efforts};

impl Overlay {
    pub(super) fn thinking_levels(&self) -> &'static [&'static str] {
        if self.model_provider.as_deref() == Some(crate::managed::catalog::PROVIDER)
            && let Some(client) = crate::managed::active()
        {
            return crate::managed::catalog::reasoning_efforts(
                &client,
                self.model.as_deref().unwrap_or_default(),
            );
        }
        if matches!(
            self.model_provider.as_deref(),
            Some("gemini" | "merge-gateway" | "lennette-managed")
        ) {
            reasoning_efforts(self.model.as_deref().unwrap_or_default())
        } else {
            &[]
        }
    }

    pub(super) fn current_thinking_effort(&self) -> Option<&'static str> {
        if self.thinking_levels().is_empty() {
            return None;
        }
        let levels = self.thinking_levels();
        self.thinking_effort
            .as_deref()
            .and_then(|effort| levels.iter().copied().find(|level| *level == effort))
            .or_else(|| levels.first().copied())
    }

    pub(super) fn thinking_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let label = effort_label(self.current_thinking_effort().unwrap_or_default());
        let theme = self.theme;
        div()
            .id("thinking-select")
            .occlude()
            .relative()
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(4.0))
            .h(px(26.0))
            .px(px(6.0))
            .rounded_full()
            .bg(self.pill_selector_fill(self.thinking_menu_open))
            .hover(|style| style.bg(self.surfaces().accent()))
            .cursor_pointer()
            .text_size(scaled(11.0))
            .text_color(theme.foreground)
            .tooltip(move |_, cx| {
                cx.new(|_| HoverLabel {
                    text: "Thinking effort · defaults to the fastest level".into(),
                    theme,
                })
                .into()
            })
            .child(icons::icon(&theme, Icon::Brain, 13.0))
            .child(label)
            .child(icons::icon(&theme, Icon::ChevronDown, 10.0))
            .child(menu_probe(self.thinking_bounds.clone(), cx))
            .on_click(cx.listener(|this, _, _, cx| {
                let open = !this.thinking_menu_open;
                this.close_pill_menus();
                this.thinking_menu_open = open;
                cx.notify();
            }))
    }

    pub(super) fn toggle_top_pill(&mut self) {
        self.close_pill_menus();
        let hidden = !self.runtime_settings.hide_top_pill;
        self.runtime_settings.hide_top_pill = hidden;
        if let Some(db) = self.services.connect()
            && let Err(error) = crate::db::settings::set(
                db.connection(),
                "hide_top_pill",
                if hidden { "true" } else { "false" },
            )
        {
            self.chat_error = Some(format!("Could not save pill visibility: {error}"));
        }
    }
}
