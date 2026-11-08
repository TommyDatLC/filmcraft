//! Shift+Space effect search, backed by the same `effects.apply` command as the Effects panel.

use egui::{Frame, Id, Key, Margin, Modifiers, RichText, Stroke, vec2};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{Dialog, FilmcraftApp};
use filmcraft_engine::EffectTarget;
use filmcraft_project::{ClipId, EffectDef, EffectKind, TrackKind};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchState {
    pub query: String,
    pub selected: usize,
    pub message: String,
    #[serde(skip)]
    pub focus_requested: bool,
}

pub fn open(app: &mut FilmcraftApp) -> Result<(), String> {
    if app.ui.fullscreen_program {
        return Err("Exit Cinema mode to search effects".into());
    }
    if app.dialog.is_some() && app.dialog != Some(Dialog::EffectSearch) {
        return Err("Close the current dialog before searching effects".into());
    }
    app.ui.effect_search = SearchState { focus_requested: true, ..Default::default() };
    app.dialog = Some(Dialog::EffectSearch);
    Ok(())
}

fn results(query: &str, recent: &[String], track: bool) -> Vec<&'static EffectDef> {
    let query = query.trim().to_lowercase();
    let mut defs: Vec<_> = filmcraft_project::effect_defs()
        .iter()
        .filter(|def| !def.intrinsic && !filmcraft_project::graphic::is_layer_id(def.id))
        .filter(|def| !track || filmcraft_project::video_fx::supports_track(def))
        .filter(|def| {
            query.split_whitespace().take(16).all(|word| {
                crate::i18n::matches_query(def.name, word)
                    || def.id.contains(word)
                    || def.category.iter().any(|folder| crate::i18n::matches_query(folder, word))
            })
        })
        .collect();
    defs.sort_by_key(|def| (recent.iter().take(10).position(|id| id == def.id).unwrap_or(usize::MAX), def.name.to_lowercase()));
    defs
}

pub(crate) fn track_target(app: &FilmcraftApp) -> Option<filmcraft_project::TrackId> {
    match app.session.state.effect_controls_target {
        Some(EffectTarget::VideoTrack(id)) => app.session.active_sequence()?.track(id).filter(|t| t.kind == TrackKind::Video).map(|t| t.id),
        _ => None,
    }
}

/// Quick search uses the explicit inspector target or selected clips.
pub(crate) fn apply(app: &mut FilmcraftApp, def: &EffectDef) -> Result<(), String> {
    let params = if let Some(track) = track_target(app) {
        if !filmcraft_project::video_fx::supports_track(def) {
            return Err("This effect is not available on video tracks".into());
        }
        json!({"effect":def.id,"track":track.0})
    } else {
        let clips = selected_targets(app, def);
        if clips.is_empty() {
            return Err(tl!("Select compatible clips to apply this effect.").into());
        }
        json!({"effect":def.id,"clips":clips.iter().map(|c| c.0).collect::<Vec<_>>()})
    };
    app.session.execute("effects.apply", params).map(|_| ()).map_err(|e| e.to_string())
}

fn selected_targets(app: &FilmcraftApp, def: &EffectDef) -> Vec<ClipId> {
    let Some(sequence) = app.session.active_sequence() else { return Vec::new() };
    let audio = matches!(def.kind, EffectKind::Audio | EffectKind::AudioTransition);
    let kind = if audio { TrackKind::Audio } else { TrackKind::Video };
    // The normal Effects panel lets a selected A/V picture receive an audio effect on its linked
    // sound. Preserve that behavior, but never fall back to an unrelated clip at the playhead.
    let links: Vec<_> = if def.kind == EffectKind::Audio {
        app.session
            .state
            .selection
            .iter()
            .filter_map(|id| sequence.find_item(*id))
            .filter(|(track, _)| sequence.track(*track).is_some_and(|track| track.kind == TrackKind::Video))
            .filter_map(|(_, item)| item.link)
            .collect()
    } else {
        Vec::new()
    };
    sequence
        .all_tracks()
        .filter(|track| track.kind == kind && !track.locked)
        .flat_map(|track| &track.items)
        .filter(|item| app.session.state.selection.contains(&item.id) || item.link.is_some_and(|link| links.contains(&link)))
        .map(|item| item.id)
        .collect()
}

pub fn show(app: &mut FilmcraftApp, ctx: &egui::Context) -> bool {
    let t = app.tokens;
    let mut draft = app.ui.effect_search.clone();
    let recent = app.session.prefs.recent_effects.clone();
    let mut keep = true;
    let mut apply = None;
    let query_id = Id::unique("effect-search-query");
    let modal = egui::Modal::new(Id::unique("effect-search-modal"))
        .frame(Frame::new().fill(t.panel_bg).stroke(Stroke::new(1.0, t.separator)).corner_radius(10).inner_margin(Margin::same(20)))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(200.0, 580.0));
            ui.horizontal(|ui| {
                ui.heading(tl!("Search effects"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let close = ui.button("×").on_hover_text(tl!("Close"));
                    app.auto.add_response("effectSearch.close", &close, "Close");
                    if close.clicked() {
                        keep = false;
                    }
                });
            });
            ui.add_space(8.0);
            // Consume list navigation before the text editor can interpret arrows or Enter.
            let (up, down, enter) = ctx.input_mut(|input| {
                (
                    input.consume_key(Modifiers::NONE, Key::ArrowUp),
                    input.consume_key(Modifiers::NONE, Key::ArrowDown),
                    input.consume_key(Modifiers::NONE, Key::Enter),
                )
            });
            let input =
                ui.add(egui::TextEdit::singleline(&mut draft.query).id(query_id).hint_text(tl!("Search effects")).char_limit(256).desired_width(f32::INFINITY));
            app.auto.add_response("effectSearch.query", &input, "Search effects");
            if draft.focus_requested {
                input.request_focus();
                draft.focus_requested = false;
            }
            if input.changed() {
                draft.selected = 0;
                draft.message.clear();
            }
            let defs = results(&draft.query, &recent, track_target(app).is_some());
            let last = defs.len().saturating_sub(1);
            draft.selected = draft.selected.min(last);
            if up {
                draft.selected = draft.selected.saturating_sub(1);
            }
            if down {
                draft.selected = draft.selected.saturating_add(1).min(last);
            }
            if enter {
                apply = defs.get(draft.selected).map(|def| def.id);
            }
            ui.add_space(10.0);
            if defs.is_empty() {
                ui.label(tl!("No effects found."));
            }
            let recent_count =
                if draft.query.trim().is_empty() { defs.iter().take_while(|def| recent.iter().take(10).any(|id| id == def.id)).count() } else { 0 };
            egui::ScrollArea::vertical().id_salt("effect-search-results").max_height(320.0).auto_shrink([false, false]).show(ui, |ui| {
                for (index, def) in defs.iter().enumerate() {
                    if index == 0 && recent_count > 0 {
                        let header = ui.label(RichText::new(tl!("Recently used")).color(t.text_dim).small());
                        app.auto.add("effectSearch.recent", header.rect, "Recently used");
                    }
                    if index == recent_count && draft.query.trim().is_empty() {
                        if index > 0 {
                            ui.add_space(8.0);
                        }
                        ui.label(RichText::new(tl!("Effects")).color(t.text_dim).small());
                    }
                    let label = format!(
                        "{}   ·   {}",
                        crate::i18n::t(def.name),
                        def.category.iter().map(|folder| crate::i18n::t(folder)).collect::<Vec<_>>().join(" / ")
                    );
                    let row = ui.add_sized(vec2(ui.available_width(), 30.0), egui::Button::selectable(index == draft.selected, label));
                    app.auto.add_response(&format!("effectSearch.item.{}", def.id), &row, def.name);
                    if index == draft.selected && (up || down || input.changed()) {
                        row.scroll_to_me(Some(egui::Align::Center));
                    }
                    if row.clicked() {
                        draft.selected = index;
                        apply = Some(def.id);
                    }
                }
            });
            ui.separator();
            if app.session.state.selection.is_empty() && track_target(app).is_none() && draft.message.is_empty() {
                ui.label(RichText::new(tl!("Select clips to apply an effect.")).color(t.text_dim));
            }
            if !draft.message.is_empty() {
                let message = ui.label(RichText::new(&draft.message).color(t.text));
                app.auto.add("effectSearch.message", message.rect, &draft.message);
            }
            ui.label(RichText::new("↑ ↓   ·   Enter   ·   Esc").small().color(t.text_dim));
        });
    if let Some(effect) = apply
        && let Some(def) = filmcraft_project::effect::find_effect(effect)
    {
        match self::apply(app, def) {
            Ok(()) => keep = false,
            Err(error) => draft.message = error,
        }
    }
    let keep = keep && !modal.should_close();
    if !keep {
        ctx.memory_mut(|memory| memory.surrender_focus(query_id));
    }
    app.ui.effect_search = draft;
    keep
}
