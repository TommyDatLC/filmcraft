//! Sequence-space viewport mapping shared by rendering and preview gizmos.
use filmcraft_geom::Affine;
use filmcraft_project::{Project, Sequence, TrackItem, video_fx::DynamicZoomSettings};
use filmcraft_time::Tick;

pub fn matrix(seq: &Sequence, item: &TrackItem, time: Tick) -> Affine {
    let Some(effect) = item.effects.iter().find(|e| e.effect == "dynamic_zoom" && e.enabled) else { return Affine::IDENTITY };
    let Ok(settings) = DynamicZoomSettings::from_effect(effect) else { return Affine::IDENTITY };
    let span = item.duration.0 as i128 - seq.settings.frame_rate.frame_duration().0 as i128;
    let progress = if span > 0 { ((time.0 as i128 - item.start.0 as i128) as f64 / span as f64).clamp(0.0, 1.0) } else { 0.0 };
    let rect = settings.rect_at(progress);
    let scale = 1.0 / rect.width;
    let (w, h) = (seq.settings.width as f64, seq.settings.height as f64);
    Affine { a: scale, b: 0.0, c: 0.0, d: scale, e: w * (0.5 - rect.center.x * scale), f: h * (0.5 - rect.center.y * scale) }
}

/// Source pixels → sequence pixels, with the owning track's FX bypass respected.
pub fn clip_matrix(project: &Project, seq: &Sequence, item: &TrackItem, time: Tick, effects: bool) -> Option<Affine> {
    let basic = crate::motion_matrix(seq, item, crate::source_size(project, item.item)?, crate::source_par(project, item.item), item.effect_time_at(time));
    Some(stage_matrix(seq, item, time, effects).then_apply(&basic))
}

/// Sequence pixels before the automatic camera → final sequence pixels.
pub fn stage_matrix(seq: &Sequence, item: &TrackItem, time: Tick, effects: bool) -> Affine {
    let bypass = seq.find_item(item.id).and_then(|(id, _)| seq.track(id)).is_some_and(|t| t.video_effects_bypassed);
    if effects && !bypass { matrix(seq, item, time) } else { Affine::IDENTITY }
}

/// UI-only inspection image. Work on a private snapshot; project flags and exports stay intact.
pub fn edit_frame(
    project: &Project,
    sequence: filmcraft_project::ItemId,
    clip: filmcraft_project::ClipId,
    time: Tick,
    opts: crate::RenderOptions,
    sources: &dyn crate::SourceProvider,
) -> Option<crate::Image> {
    if opts.depth > crate::MAX_NEST_DEPTH {
        return None;
    }
    let original = project.sequence(sequence)?;
    let (track_id, item) = original.find_item(clip)?;
    if !item.enabled || !original.track(track_id)?.enabled || original.track(track_id)?.kind != filmcraft_project::TrackKind::Video {
        return None;
    }
    let time = time.clamp(item.start, (item.end() - Tick(1)).max(item.start));
    let mut preview = project.clone();
    let q = preview.sequence_mut(sequence)?;
    q.track_mut(track_id)?.effects.clear();
    q.find_item_mut(clip)?.1.effects.retain(|e| e.effect != "dynamic_zoom" && e.effect != "opacity");
    // Keep the other tracks as filter inputs (e.g. a clip's Track Matte), without compositing them.
    let q = preview.sequence(sequence)?;
    let item = q.find_item(clip)?.1;
    let tc =
        filmcraft_time::format_time(time, q.settings.frame_rate, q.settings.drop_frame, filmcraft_time::TimeDisplay::Timecode, q.settings.sample_rate as i64);
    let (w, h) = crate::output_size(q, opts.scale);
    let (mut image, _, _) = crate::item_layer_ex(&preview, q, item, time, opts, sources, crate::LayerWant { tc: &tc, allow_region: false })?.into_full(w, h);
    if opts.depth == 0 && !opts.working_output {
        crate::colorman::to_display(&mut image, &q.settings.color);
    }
    Some(image)
}
