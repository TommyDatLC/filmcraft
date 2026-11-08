//! Filters on an isolated sequence-sized layer, before the clip's opacity/blend.
use crate::{Image, RenderOptions, effects::FxCtx, vfx::FxEnv};
use filmcraft_geom::Affine;
use filmcraft_project::{EffectInstance, ItemId, Project, Sequence, Track};
use filmcraft_time::{Tick, TimeDisplay, format_time};
use rayon::prelude::*;

pub(crate) fn clip_filters_enabled(track: &Track, opts: RenderOptions) -> bool {
    opts.effects && !track.video_effects_bypassed
}
pub(crate) fn has_stack(track: &Track, opts: RenderOptions) -> bool {
    clip_filters_enabled(track, opts) && track.effects.iter().any(|e| e.enabled && e.def().is_some_and(filmcraft_project::video_fx::supports_track))
}

struct TrackEnv<'a> {
    seq: &'a Sequence,
    track: &'a Track,
    time: Tick,
    scale: f32,
}
impl FxEnv for TrackEnv<'_> {
    fn frame(&self, _: f64, _: &EffectInstance) -> Option<Image> {
        None
    }
    fn source_frame(&self, _: f64, _: f32) -> Option<Image> {
        None
    }
    fn track(&self, _: usize) -> Option<Image> {
        None
    }
    fn auxiliary_frame(&self, _: ItemId) -> Option<Image> {
        None
    }
    fn layer_to_output(&self) -> Affine {
        Affine::IDENTITY
    }
    fn output_scale(&self) -> f32 {
        self.scale
    }
    fn clip_seconds(&self) -> f64 {
        self.seq.duration().seconds()
    }
    fn clip_offset(&self) -> f64 {
        self.time.seconds()
    }
    fn frame_rate(&self) -> f64 {
        self.seq.settings.frame_rate.as_f64()
    }
    fn media_timecode(&self) -> String {
        format_time(self.time, self.seq.settings.frame_rate, self.seq.settings.drop_frame, TimeDisplay::Timecode, self.seq.settings.sample_rate as i64)
    }
    fn file_name(&self) -> String {
        self.track.name.clone()
    }
    fn sequence_name(&self) -> String {
        String::new()
    }
    fn sequence_size(&self) -> (u32, u32) {
        (self.seq.settings.width, self.seq.settings.height)
    }
    fn source_size(&self) -> (u32, u32) {
        self.sequence_size()
    }
    fn clip_key(&self) -> u64 {
        self.track.id.0
    }
}

pub(crate) fn apply_track_stack(image: &mut Image, project: &Project, track: &Track, seq: &Sequence, t: Tick, opts: RenderOptions) {
    if !has_stack(track, opts) {
        return;
    }
    let env = TrackEnv { seq, track, time: t, scale: opts.scale };
    let cx = FxCtx {
        t,
        px_scale: opts.scale,
        seconds: t.seconds(),
        timecode: "",
        clip_name: &track.name,
        project: Some(project),
        env: Some(&env),
        frame_rate: seq.settings.frame_rate.as_f64(),
        working: seq.settings.color.working,
    };
    for effect in track.effects.iter().filter(|e| e.enabled && e.def().is_some_and(filmcraft_project::video_fx::supports_track)) {
        if filmcraft_media::cancel::cancelled() {
            return;
        }
        crate::mask::apply_effect(image, effect, &cx);
    }
}

/// Coverage is prepared at the actual decode size, then transformed with the
/// same matrix as the image. Recomputing it from nominal source dimensions can
/// shift a feather at reduced preview resolutions.
pub(crate) fn apply_clip_opacity_after_track(image: &mut Image, coverage: Option<Image>) {
    let Some(coverage) = coverage else { return };
    if (image.w, image.h) != (coverage.w, coverage.h) {
        return;
    }
    image.px.par_chunks_mut(4).zip(coverage.px.par_chunks(4)).for_each(|(pixel, mask)| {
        let alpha = mask.get(3).copied().unwrap_or(0.0).clamp(0.0, 1.0);
        for value in pixel {
            *value *= alpha;
        }
    });
}
