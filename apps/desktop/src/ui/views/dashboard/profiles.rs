//! Searchable profile cards and a focused editor for reusable conversation context.
use super::*;
use crate::db::profiles::{self as store, Profile};
use crate::ui::widgets::editor::Editor;

pub(super) struct ProfileDraft {
    id: String,
    is_new: bool,
    name: gpui::Entity<Editor>,
    personal: gpui::Entity<Editor>,
    meeting: gpui::Entity<Editor>,
    observers: Vec<gpui::Subscription>,
    section: usize,
    preview: bool,
    discard: bool,
    original: [String; 3],
}

impl ProfileDraft {
    fn dirty(&self, cx: &gpui::App) -> bool {
        [&self.name, &self.personal, &self.meeting]
            .iter()
            .zip(&self.original)
            .any(|(editor, original)| editor.read(cx).input.text() != original)
    }

    pub(super) fn sync_theme(&self, theme: Theme, cx: &mut gpui::App) {
        for editor in [&self.name, &self.personal, &self.meeting] {
            Editor::sync_theme(editor, theme, cx);
        }
    }
}

impl DashboardView {
    pub(super) fn init_profile_search(&mut self, cx: &mut Context<Self>) {
        let search = cx.new(|cx| {
            let mut editor = Editor::new("", self.theme, cx);
            editor.compact = true;
            editor.search = true;
            editor.placeholder = "Search profiles...";
            editor
        });
        self.profile_observer = Some(cx.observe(&search, |this, _, cx| {
            this.profile_delete = None;
            cx.notify();
        }));
        self.profile_search = Some(search);
    }

    fn edit_profile(
        &mut self,
        profile: Option<Profile>,
        cx: &mut Context<Self>,
        window: &mut Window,
    ) {
        let is_new = profile.is_none();
        let profile = profile.unwrap_or(Profile {
            id: uuid::Uuid::new_v4().to_string(),
            name: String::new(),
            personal_context: String::new(),
            meeting_context: String::new(),
        });
        self.profile_edit = Some(ProfileDraft {
            observers: Vec::new(),
            section: 0,
            preview: false,
            discard: false,
            original: [
                profile.name.clone(),
                profile.personal_context.clone(),
                profile.meeting_context.clone(),
            ],
            id: profile.id,
            is_new,
            name: cx.new(|cx| {
                let mut editor = Editor::new(&profile.name, self.theme, cx);
                editor.compact = true;
                editor.placeholder = "For example: Product interview";
                editor
            }),
            personal: cx.new(|cx| {
                let mut editor = Editor::new(&profile.personal_context, self.theme, cx);
                editor.min_height = 420.0;
                editor.max_chars = Some(store::MAX_CONTEXT_CHARS);
                editor.placeholder =
                    "Paste your background, experience, projects and relevant examples…";
                editor
            }),
            meeting: cx.new(|cx| {
                let mut editor = Editor::new(&profile.meeting_context, self.theme, cx);
                editor.min_height = 420.0;
                editor.max_chars = Some(store::MAX_MEETING_CONTEXT_CHARS);
                editor.placeholder =
                    "Add the agenda, participants, goals, facts, constraints and desired outcome…";
                editor
            }),
        });
        if let Some(draft) = &mut self.profile_edit {
            for editor in [&draft.name, &draft.personal, &draft.meeting] {
                draft
                    .observers
                    .push(cx.observe(editor, |_, _, cx| cx.notify()));
            }
            draft.name.read(cx).focus(window);
        }
        self.profile_delete = None;
        self.settings.error = None;
    }

    fn save_profile(&mut self, cx: &mut Context<Self>) {
        let Some(db) = &self.database else {
            self.settings.error = Some("The local database is unavailable.".into());
            return;
        };
        let Some(draft) = &self.profile_edit else {
            return;
        };
        let profile = Profile {
            id: draft.id.clone(),
            name: draft.name.read(cx).input.text().into(),
            personal_context: draft.personal.read(cx).input.text().into(),
            meeting_context: draft.meeting.read(cx).input.text().into(),
        };
        match store::save(db.connection(), &profile) {
            Ok(()) => {
                self.profile_edit = None;
                self.settings.error = None;
                self.dashboard
                    .navigate(super::super::nav::Route::Profiles, Some(db));
                if let Some(search) = &self.profile_search {
                    search.update(cx, |editor, cx| {
                        editor.input.set_text("");
                        cx.notify();
                    });
                }
            }
            Err(error) => self.settings.error = Some(error),
        }
    }

    fn delete_profile(&mut self, id: &str) {
        let Some(db) = &self.database else {
            self.settings.error = Some("The local database is unavailable.".into());
            return;
        };
        match store::delete(db.connection(), id) {
            Ok(()) => {
                self.profile_delete = None;
                self.settings.error = None;
                self.dashboard
                    .navigate(super::super::nav::Route::Profiles, Some(db));
            }
            Err(error) => self.settings.error = Some(error.to_string()),
        }
    }

    pub(super) fn profiles_body(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = self.theme;
        let surfaces = self.surfaces();
        let available = (f32::from(window.viewport_size().width) - 48.0).clamp(300.0, 960.0);
        let mut body = div()
            .flex()
            .flex_col()
            .w_full()
            .min_w(px(0.0))
            .gap(px(24.0))
            .text_color(theme.foreground);
        if self.profile_edit.is_some() {
            body = body.child(self.page_heading(
                "Context studio",
                "Shape what Lennette knows before your next conversation.",
            ));
        }
        if let Some(error) = &self.settings.error {
            body = body.child(crate::ui::controls::warning(&theme, error));
        }
        if let Some(draft) = &self.profile_edit {
            let dirty = draft.dirty(cx);
            let mut tabs = div().flex().flex_wrap().gap(px(8.0));
            for (section, label) in [(0, "Your background"), (1, "Meeting brief")] {
                let label = if draft.section == section {
                    if section == 0 {
                        "● Your background"
                    } else {
                        "● Meeting brief"
                    }
                } else {
                    label
                };
                tabs = tabs.child(action_button(
                    ("context-section", section),
                    &theme,
                    label,
                    cx,
                    move |this, cx, window| {
                        if let Some(draft) = &mut this.profile_edit {
                            draft.section = section;
                            if !draft.preview {
                                let editor = if section == 0 {
                                    &draft.personal
                                } else {
                                    &draft.meeting
                                };
                                editor.read(cx).focus(window);
                            }
                        }
                        cx.notify();
                    },
                ));
            }
            let (editor, title, hint, limit) = if draft.section == 0 {
                (
                    &draft.personal,
                    "Your background",
                    "Capture your role, experience and concrete examples. Keep reusable facts here.",
                    store::MAX_CONTEXT_CHARS,
                )
            } else {
                (
                    &draft.meeting,
                    "Meeting brief",
                    "Set the agenda, participants, goals, constraints and the outcome you want.",
                    store::MAX_MEETING_CONTEXT_CHARS,
                )
            };
            let text = editor.read(cx).input.text().to_owned();
            let count = text.chars().count();
            let mut workspace = div()
                .flex()
                .flex_col()
                .w_full()
                .min_w(px(0.0))
                .gap(px(14.0))
                .p(px(18.0))
                .rounded(px(14.0))
                .border_1()
                .border_color(theme.control_border(false))
                .bg(surfaces.card())
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .justify_between()
                        .gap(px(10.0))
                        .child(tabs)
                        .child(action_button(
                            "context-preview",
                            &theme,
                            if draft.preview {
                                "Back to editing"
                            } else {
                                "Preview"
                            },
                            cx,
                            |this, cx, _| {
                                if let Some(draft) = &mut this.profile_edit {
                                    draft.preview = !draft.preview;
                                }
                                cx.notify();
                            },
                        )),
                )
                .child(
                    div()
                        .text_size(scaled(15.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .child(
                    div()
                        .text_size(scaled(12.0))
                        .text_color(theme.muted_foreground)
                        .whitespace_normal()
                        .child(hint),
                );
            if draft.preview {
                for (heading, content) in [
                    ("Your background", draft.personal.read(cx).input.text()),
                    ("Meeting brief", draft.meeting.read(cx).input.text()),
                ] {
                    workspace = workspace.child(
                        div()
                            .text_size(scaled(13.0))
                            .font_weight(FontWeight::MEDIUM)
                            .child(heading),
                    );
                    workspace = workspace.child(crate::ui::overlay::render_markdown(
                        &theme,
                        surfaces,
                        if content.trim().is_empty() {
                            "_No context added yet._"
                        } else {
                            content
                        },
                    ));
                }
            } else {
                if text.trim().is_empty() {
                    workspace = workspace.child(action_button("context-outline", &theme, "Start with a guided outline", cx, |this, cx, window| {
                        if let Some(draft) = &this.profile_edit {
                            let (editor, outline) = if draft.section == 0 {
                                (&draft.personal, "## Role & background\n\n## Experience & strengths\n\n## Projects & examples\n\n## Preferences\n")
                            } else {
                                (&draft.meeting, "## Goal & desired outcome\n\n## Participants & roles\n\n## Agenda\n\n## Key facts & references\n\n## Constraints & open questions\n")
                            };
                            editor.update(cx, |editor, cx| {
                                // Only seed an empty field; existing context is never replaced.
                                if editor.input.is_empty() { editor.input.set_text(outline); }
                                editor.focus(window);
                                cx.notify();
                            });
                        }
                    }));
                }
                workspace = workspace.child(editor.clone()).child(
                    div()
                        .flex()
                        .flex_wrap()
                        .justify_between()
                        .gap(px(8.0))
                        .text_size(scaled(11.0))
                        .text_color(theme.muted_foreground)
                        .child(format!(
                            "{count} / {limit} characters · {} words",
                            text.split_whitespace().count()
                        ))
                        .child("Markdown headings and lists supported"),
                );
                if count >= limit {
                    workspace = workspace.child(crate::ui::controls::warning(
                        &theme,
                        "Character limit reached. Shorten this section before adding more.",
                    ));
                }
            }
            let mut form = div().flex().flex_col().w_full().min_w(px(0.0)).gap(px(18.0))
                .child(div().flex().flex_wrap().items_center().justify_between().gap(px(12.0))
                    .child(div().text_size(scaled(15.0)).font_weight(FontWeight::SEMIBOLD)
                        .child(if draft.is_new { "New profile" } else { "Edit profile" }))
                    .child(div().text_size(scaled(11.0)).text_color(theme.muted_foreground)
                        .child(if dirty { "Unsaved changes" } else { "No unsaved changes" }))
                    .child(primary("save-profile", "Save profile", Icon::Check, theme, cx, |this, cx, _| this.save_profile(cx))))
                .child(div().flex().flex_col().gap(px(8.0)).w_full().max_w(px(420.0))
                    .child(div().text_size(scaled(12.0)).font_weight(FontWeight::MEDIUM).child("Profile name"))
                    .child(draft.name.clone()))
                .child(workspace)
                .child(div().text_size(scaled(11.0)).text_color(theme.muted_foreground).whitespace_normal()
                    .child("Choose this profile before a meeting or chat. Both sections accompany every answer. Saved edits apply to the next answer. Longer briefs can increase response time and usage. Your draft stays here when you visit another page."));
            if draft.discard && dirty {
                form = form.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(10.0))
                        .child("Discard your unsaved changes?")
                        .child(action_button(
                            "keep-profile",
                            &theme,
                            "Keep editing",
                            cx,
                            |this, cx, _| {
                                if let Some(draft) = &mut this.profile_edit {
                                    draft.discard = false;
                                }
                                cx.notify();
                            },
                        ))
                        .child(action_button(
                            "discard-profile",
                            &theme,
                            "Discard changes",
                            cx,
                            |this, cx, _| {
                                this.profile_edit = None;
                                this.settings.error = None;
                                cx.notify();
                            },
                        )),
                );
            } else {
                form = form.child(action_button(
                    "cancel-profile",
                    &theme,
                    "Back to profiles",
                    cx,
                    |this, cx, _| {
                        if this
                            .profile_edit
                            .as_ref()
                            .is_some_and(|draft| draft.dirty(cx))
                        {
                            if let Some(draft) = &mut this.profile_edit {
                                draft.discard = true;
                            }
                        } else {
                            this.profile_edit = None;
                            this.settings.error = None;
                        }
                        cx.notify();
                    },
                ));
            }
            return body.child(form).into_any_element();
        }
        let selected = self
            .database
            .as_ref()
            .and_then(|db| store::selected(db.connection()).ok().flatten());
        body = body
            .child(
                self.collection_header(
                    "Context studio",
                    "Your background. Their agenda. A prepared assistant for every conversation.",
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .w_full()
                        .gap(px(12.0))
                        .child(
                            div()
                                .w(px(280.0))
                                .max_w_full()
                                .min_w(px(0.0))
                                .children(self.profile_search.clone()),
                        )
                        .child(primary(
                            "create-profile",
                            "New profile",
                            Icon::Plus,
                            theme,
                            cx,
                            |this, cx, window| this.edit_profile(None, cx, window),
                        ))
                        .into_any_element(),
                ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .p(px(14.0))
                    .rounded(px(12.0))
                    .border_1()
                    .border_color(theme.control_border(false))
                    .bg(surfaces.input())
                    .child(icons::icon(&theme, Icon::UserRound, 18.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .flex()
                            .flex_col()
                            .gap(px(3.0))
                            .child(
                                div()
                                    .text_size(scaled(10.5))
                                    .text_color(theme.muted_foreground)
                                    .child("ACTIVE PROFILE · USED IN YOUR NEXT ANSWER"),
                            )
                            .child(
                                div().text_size(scaled(12.0)).truncate().child(
                                    selected.as_ref().map(|p| p.name.clone()).unwrap_or_else(
                                        || "Choose a profile below to get started".into(),
                                    ),
                                ),
                            ),
                    )
                    .children(selected.as_ref().map(|_| {
                        action_button(
                            "clear-profile-selection",
                            &theme,
                            "Clear selection",
                            cx,
                            |this, _, _| {
                                if let Some(db) = &this.database {
                                    this.settings.error =
                                        store::select(db.connection(), None).err();
                                }
                            },
                        )
                    })),
            );
        let query = self
            .profile_search
            .as_ref()
            .map(|search| search.read(cx).input.text().trim().to_lowercase())
            .unwrap_or_default();
        let PageData::Profiles(profiles) = self.dashboard.data() else {
            return body
                .child(self.collection_empty(
                    Icon::UserRound,
                    "Your profiles will appear here",
                    "Add background once and reuse it in interviews, meetings and chats.",
                ))
                .into_any_element();
        };
        let matching = profiles
            .iter()
            .filter(|profile| {
                let text = format!(
                    "{} {} {}",
                    profile.name, profile.personal_context, profile.meeting_context
                )
                .to_lowercase();
                query.split_whitespace().all(|word| text.contains(word))
            })
            .collect::<Vec<_>>();
        if matching.is_empty() {
            return body
                .child(if profiles.is_empty() {
                    self.collection_empty(
                        Icon::UserRound,
                        "Your profiles will appear here",
                        "Create your first profile, then choose it in the overlay.",
                    )
                } else {
                    self.collection_empty(
                        Icon::Search,
                        "No matching profiles",
                        "Try a name, role or topic from your saved context.",
                    )
                })
                .into_any_element();
        }
        body = body.child(
            div()
                .text_size(scaled(11.0))
                .text_color(theme.muted_foreground)
                .child(format!(
                    "{} {}",
                    matching.len(),
                    if matching.len() == 1 {
                        "profile"
                    } else {
                        "profiles"
                    }
                )),
        );
        let mut grid = div().flex().flex_col().w_full().gap(px(18.0));
        for profile in matching {
            let active = selected
                .as_ref()
                .is_some_and(|selected| selected.id == profile.id);
            let editing = profile.clone();
            let duplicate = profile.clone();
            let using = profile.id.clone();
            let deleting = profile.id.clone();
            let confirming = self.profile_delete.as_ref() == Some(&profile.id);
            let mut card = div()
                .flex()
                .flex_col()
                .w_full()
                .min_w(px(0.0))
                .gap(px(12.0))
                .p(px(20.0))
                .rounded(px(16.0))
                .border_1()
                .border_color(theme.control_border(active))
                .bg(if active {
                    surfaces.input()
                } else {
                    surfaces.card()
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .size(px(30.0))
                                .rounded(px(9.0))
                                .bg(surfaces.muted())
                                .child(icons::icon(&theme, Icon::UserRound, 15.0)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .text_size(scaled(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .child(profile.name.clone()),
                        )
                        .children(active.then(|| {
                            div()
                                .flex_shrink_0()
                                .text_size(scaled(10.0))
                                .text_color(theme.muted_foreground)
                                .child("Selected")
                        })),
                )
                .child({
                    let mut sections = div().flex().w_full().min_w(px(0.0)).gap(px(16.0));
                    if available < 760.0 {
                        sections = sections.flex_col();
                    }
                    sections
                        .child(context_preview(
                            theme,
                            "ABOUT YOU",
                            &profile.personal_context,
                        ))
                        .child(context_preview(
                            theme,
                            "MEETING BRIEF",
                            &profile.meeting_context,
                        ))
                });
            if confirming {
                card = card.child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_shrink_0()
                        .gap(px(7.0))
                        .child(div().text_size(scaled(11.0)).child("Delete this profile?"))
                        .child(
                            div()
                                .flex()
                                .gap(px(6.0))
                                .child(action_button(
                                    gpui::ElementId::Name(
                                        format!("confirm-profile-{deleting}").into(),
                                    ),
                                    &theme,
                                    "Delete",
                                    cx,
                                    move |this, _, _| this.delete_profile(&deleting),
                                ))
                                .child(action_button(
                                    "cancel-profile-delete",
                                    &theme,
                                    "Cancel",
                                    cx,
                                    |this, _, _| this.profile_delete = None,
                                )),
                        ),
                );
            } else {
                card = card.child(
                    div()
                        .flex()
                        .flex_shrink_0()
                        .items_center()
                        .justify_between()
                        .gap(px(6.0))
                        .child(action_button(
                            gpui::ElementId::Name(format!("use-profile-{using}").into()),
                            &theme,
                            if active { "Deselect" } else { "Use profile" },
                            cx,
                            move |this, _, _| {
                                this.profile_delete = None;
                                if let Some(db) = &this.database {
                                    this.settings.error = store::select(
                                        db.connection(),
                                        if active { None } else { Some(&using) },
                                    )
                                    .err();
                                }
                            },
                        ))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.0))
                                .child(action_button(
                                    gpui::ElementId::Name(
                                        format!("edit-profile-{}", profile.id).into(),
                                    ),
                                    &theme,
                                    "Edit",
                                    cx,
                                    move |this, cx, window| {
                                        this.edit_profile(Some(editing.clone()), cx, window)
                                    },
                                ))
                                .child(action_button(
                                    gpui::ElementId::Name(
                                        format!("duplicate-profile-{}", profile.id).into(),
                                    ),
                                    &theme,
                                    "Duplicate",
                                    cx,
                                    move |this, cx, window| {
                                        let mut copy = duplicate.clone();
                                        copy.id = uuid::Uuid::new_v4().to_string();
                                        copy.name = format!("{} copy", copy.name);
                                        this.edit_profile(Some(copy), cx, window);
                                        if let Some(draft) = &mut this.profile_edit {
                                            draft.is_new = true;
                                        }
                                    },
                                ))
                                .child(
                                    div()
                                        .id(gpui::ElementId::Name(
                                            format!("delete-profile-{deleting}").into(),
                                        ))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .size(px(26.0))
                                        .flex_shrink_0()
                                        .rounded(px(7.0))
                                        .cursor_pointer()
                                        .hover(|style| style.bg(theme.menu_hover()))
                                        .child(icons::icon(&theme, Icon::Trash2, 13.0))
                                        .tooltip(move |_, cx| {
                                            cx.new(|_| super::NavTooltip {
                                                text: "Delete profile".into(),
                                                theme,
                                            })
                                            .into()
                                        })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.profile_delete = Some(deleting.clone());
                                            cx.notify();
                                        })),
                                ),
                        ),
                );
            }
            grid = grid.child(card);
        }
        body.child(grid).into_any_element()
    }
}

fn context_preview(theme: Theme, title: &'static str, text: &str) -> impl IntoElement {
    let excerpt = text.chars().take(240).collect::<String>();
    let excerpt = if text.chars().count() > 240 {
        format!("{excerpt}…")
    } else {
        excerpt
    };
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.0))
        .gap(px(8.0))
        .p(px(14.0))
        .rounded(px(10.0))
        .bg(theme.surfaces(1.0).muted())
        .child(
            div()
                .text_size(scaled(10.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.muted_foreground)
                .child(format!(
                    "{title} · {} words",
                    text.split_whitespace().count()
                )),
        )
        .child(
            div()
                .text_size(scaled(12.0))
                .line_height(scaled(19.0))
                .whitespace_normal()
                .text_color(theme.foreground)
                .child(if excerpt.trim().is_empty() {
                    "No context yet. Edit this profile to add it.".to_owned()
                } else {
                    excerpt
                }),
        )
}

fn primary(
    id: &'static str,
    label: &'static str,
    icon: Icon,
    theme: Theme,
    cx: &mut Context<DashboardView>,
    handler: impl Fn(&mut DashboardView, &mut Context<DashboardView>, &mut Window) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .h(scaled(38.0))
        .px(px(12.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(theme.control_border(true))
        .bg(Theme::with_alpha(theme.primary, 0.12))
        .text_color(theme.primary)
        .text_size(scaled(12.0))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(|style| style.bg(Theme::with_alpha(theme.primary, 0.20)))
        .child(icons::icon_colored(theme.primary, icon, 14.0))
        .child(label)
        .on_click(cx.listener(move |this, _, window, cx| {
            handler(this, cx, window);
            cx.notify();
        }))
}
