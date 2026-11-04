//! All pill selectors share one surface and row treatment.
use super::*;

impl Overlay {
    pub(super) fn selection_menu(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if !self.model_menu_open && !self.prompt_menu_open && !self.profile_menu_open {
            return None;
        }
        let theme = self.theme;
        let mut menu = self
            .menu_surface("selection-menu")
            .on_mouse_down_out(cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                let bounds = if this.model_menu_open {
                    &this.model_bounds
                } else if this.profile_menu_open {
                    &this.profile_bounds
                } else {
                    &this.prompt_bounds
                };
                if !bounds.get().contains(&event.position) {
                    this.close_pill_menus();
                    cx.notify();
                }
            }));
        let mut rows = div()
            .id("pill-options")
            .flex()
            .flex_col()
            .gap(px(2.0))
            .max_h(px(224.0))
            .flex_shrink_0()
            .overflow_y_scroll();
        if self.model_menu_open {
            menu = menu.child(
                div()
                    .flex()
                    .items_center()
                    .flex_shrink_0()
                    .gap(px(5.0))
                    .p(px(2.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .children(self.model_search.clone()),
                    )
                    .child(
                        self.menu_row("refresh-models", false)
                            .relative()
                            .size(scaled(crate::ui::widgets::editor::DENSE_HEIGHT))
                            .flex_shrink_0()
                            .rounded(px(8.0))
                            .bg(crate::ui::widgets::blur::tint(
                                theme,
                                crate::ui::widgets::blur::Surface::Input,
                            ))
                            .child(crate::ui::widgets::material::dual_edge(theme, 8.0))
                            .px(px(0.0))
                            .justify_center()
                            .opacity(if self.catalog_loading { 0.45 } else { 1.0 })
                            .tooltip(move |_, cx| {
                                cx.new(|_| super::HoverLabel {
                                    text: "Refresh models".into(),
                                    theme,
                                })
                                .into()
                            })
                            .child(icons::icon(&self.theme, Icon::RefreshCw, 12.0))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if !this.catalog_loading {
                                    this.load_model_catalog(true, cx);
                                }
                                cx.notify();
                            })),
                    ),
            );
            let query = self
                .model_search
                .as_ref()
                .map(|e| e.read(cx).input.text().trim().to_lowercase())
                .unwrap_or_default();
            let choices = if self.catalog_choices.is_empty() {
                &self.chat_choices
            } else {
                &self.catalog_choices
            };
            let mut count = 0;
            for (index, (provider, model)) in choices.iter().enumerate() {
                let label = self
                    .managed_model_labels
                    .get(model)
                    .cloned()
                    .unwrap_or_else(|| {
                        self.locked_models
                            .iter()
                            .find(|m| {
                                m.id == *model
                                    || m.id.rsplit('/').next() == model.rsplit('/').next()
                            })
                            .map(|m| m.name.clone())
                            .unwrap_or_else(|| model.clone())
                    });
                let searchable = format!("{label} {provider} {model}").to_lowercase();
                if !query
                    .split_whitespace()
                    .all(|word| searchable.contains(word))
                {
                    continue;
                }
                count += 1;
                let active = self.model_provider.as_ref() == Some(provider)
                    && self.model.as_ref() == Some(model);
                let provider = provider.clone();
                let model = model.clone();
                rows = rows.child(
                    self.menu_row(("provider-choice", index), active)
                        .child(icons::model_icon(&theme, &provider, &model, 16.0))
                        .child(div().flex_1().min_w(px(0.0)).truncate().child(label))
                        .child(if provider == crate::managed::catalog::PROVIDER {
                            lennette_badge(&theme).into_any_element()
                        } else {
                            div()
                                .max_w(px(85.0))
                                .truncate()
                                .text_size(scaled(9.0))
                                .text_color(theme.muted_foreground)
                                .child(
                                    self.provider_names
                                        .get(&provider)
                                        .unwrap_or(&provider)
                                        .clone(),
                                )
                                .into_any_element()
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.model_provider = Some(provider.clone());
                            this.select_model(&model);
                            this.focus_composer = true;
                            cx.notify();
                        })),
                );
            }
            let locked: Vec<_> = if crate::managed::active().is_none() {
                self.locked_models
                    .iter()
                    .filter(|model| {
                        let text = format!("{} {}", model.name, model.id).to_lowercase();
                        query.split_whitespace().all(|word| text.contains(word))
                    })
                    .collect()
            } else {
                Vec::new()
            };
            if count == 0 && locked.is_empty() {
                rows = rows.child(
                    div()
                        .p(px(10.0))
                        .text_size(scaled(11.0))
                        .text_color(self.theme.muted_foreground)
                        .child(if choices.is_empty() {
                            if crate::managed::active().is_some() {
                                "Refresh to load your account's models."
                            } else {
                                "Connect an AI in Settings."
                            }
                        } else {
                            "No matching models."
                        }),
                );
            }
            if !locked.is_empty() {
                rows = rows.child(
                    div()
                        .pt(px(12.0))
                        .pb(px(5.0))
                        .px(px(8.0))
                        .text_size(scaled(10.0))
                        .text_color(theme.muted_foreground)
                        .child("Lennette models"),
                );
                for (index, model) in locked.into_iter().enumerate() {
                    let label = format!(
                        "{}{}",
                        model.name,
                        if model.plus_only { " · Plus" } else { "" }
                    );
                    rows = rows.child(div().id(("locked-managed-model", index))
                        .flex().items_center().gap(px(8.0)).px(px(8.0)).py(px(7.0))
                        .opacity(0.45).text_color(theme.muted_foreground).text_size(scaled(11.0))
                        .tooltip(move |_, cx| cx.new(|_| super::HoverLabel {
                            text: "Sign in with an active Lennette subscription to use this model.".into(), theme,
                        }).into())
                        .child(icons::model_icon(&theme, crate::managed::catalog::PROVIDER, &model.id, 16.0))
                        .child(div().flex_1().min_w(px(0.0)).truncate().child(label))
                        .child(lennette_badge(&theme))
                        .child(icons::icon(&theme, Icon::LockKeyhole, 12.0)));
                }
                rows = rows.child(
                    self.menu_row("connect-managed-models", false)
                        .child(icons::icon(&theme, Icon::UserRound, 14.0))
                        .child("Connect Lennette")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.close_pill_menus();
                            this.open_dashboard_at(
                                cx,
                                crate::features::dashboard::nav::Route::Settings,
                            );
                            cx.notify();
                        })),
                );
            }
            if let Some(status) = &self.catalog_status {
                menu = menu.child(
                    div()
                        .max_h(px(42.0))
                        .overflow_hidden()
                        .p(px(8.0))
                        .text_size(scaled(10.0))
                        .text_color(self.theme.muted_foreground)
                        .child(status.clone()),
                );
            }
        } else if self.profile_menu_open {
            rows = rows.child(self.profile_option(None, "No context".into(), 0, cx));
            for (index, profile) in self.profile_choices.iter().enumerate() {
                rows = rows.child(self.profile_option(
                    Some(profile.id.clone()),
                    profile.name.clone(),
                    index + 1,
                    cx,
                ));
            }
        } else {
            if !self
                .prompt_choices
                .iter()
                .any(|(_, name, _)| name == "General")
            {
                rows = rows.child(self.prompt_option(None, "General".into(), 0, cx));
            }
            for (index, (id, name, _)) in self.prompt_choices.iter().enumerate() {
                rows = rows.child(self.prompt_option(Some(*id), name.clone(), index + 1, cx));
            }
        }
        menu = menu.child(rows);
        let bounds = if self.model_menu_open {
            self.model_bounds.get()
        } else if self.profile_menu_open {
            self.profile_bounds.get()
        } else {
            self.prompt_bounds.get()
        };
        Some(
            gpui::deferred(
                gpui::anchored()
                    .position(gpui::point(bounds.left(), bounds.bottom() + px(6.0)))
                    .snap_to_window_with_margin(px(8.0))
                    .child(
                        div()
                            .w(px(if self.model_menu_open || self.prompt_menu_open {
                                310.0
                            } else {
                                190.0
                            }))
                            .child(crate::ui::widgets::motion::reveal(
                                menu,
                                (
                                    "pill-dropdown",
                                    if self.model_menu_open {
                                        0usize
                                    } else if self.profile_menu_open {
                                        1
                                    } else {
                                        2
                                    },
                                ),
                                self.runtime_settings.reduce_motion,
                                true,
                            )),
                    ),
            )
            .with_priority(10)
            .into_any_element(),
        )
    }
    fn profile_option(
        &self,
        id: Option<String>,
        name: String,
        index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = self.active_profile.as_ref().map(|p| &p.id) == id.as_ref();
        self.menu_row(("profile-option", index), active)
            .child(div().flex_1().min_w(px(0.0)).truncate().child(name))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select_profile(id.as_deref());
                cx.notify();
            }))
    }
    fn prompt_option(
        &self,
        id: Option<i64>,
        name: String,
        index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (icon, description) = mode_presentation(&name);
        self.menu_row(("prompt-option", index), self.effective_prompt_id() == id)
            .h(scaled(36.0))
            .child(icons::icon(&self.theme, icon, 16.0))
            .child(div().w(px(70.0)).flex_shrink_0().truncate().child(name))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate()
                    .text_size(scaled(10.0))
                    .text_color(self.theme.muted_foreground)
                    .child(description),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.active_prompt = id;
                this.prompt_menu_open = false;
                cx.notify();
            }))
    }
}

/// Hosted-provider badge stays recognizable without coloring model logos.
fn lennette_badge(theme: &Theme) -> gpui::Div {
    let color = if theme.foreground.r < 0.5 {
        gpui::rgb(0x3456c5)
    } else {
        gpui::rgb(0x9baeff)
    };
    div()
        .flex_shrink_0()
        .w(px(26.0))
        .h(px(18.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(3.0))
        .bg(Theme::with_alpha(color, 0.16))
        .text_color(color)
        .text_size(scaled(8.5))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child("LEN")
}

/// Short, stable descriptions complement both built-in and custom modes.
pub(super) fn mode_presentation(name: &str) -> (Icon, &'static str) {
    match name.trim().to_ascii_lowercase().as_str() {
        "general" => (Icon::Sparkles, "Everyday questions"),
        "interview" => (Icon::Briefcase, "Confident interview answers"),
        "coding" => (Icon::Code, "Code and technical problems"),
        "translate" | "translation" => (Icon::Languages, "Translate between languages"),
        "meeting" => (Icon::Users, "Contribute to the discussion"),
        _ => (Icon::SlidersHorizontal, "Your custom instructions"),
    }
}
