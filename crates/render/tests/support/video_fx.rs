#![allow(dead_code)]
use filmcraft_frame::{AudioBuffer, VideoFrame};
use filmcraft_media::{FrameRequest, Generator, MediaInfo, MediaSource, SharedSource, generators::GeneratorSource};
use filmcraft_project::{
    ClipId, EffectInstance, ItemId, ItemKind, Label, MediaClip, MediaRef, ParamValue, Project, SequenceSettings, Track, TrackItem, TrackKind,
    resolve_auto_points,
};
use filmcraft_render::{Image, RenderOptions, SourceMap, render_sequence};
use filmcraft_time::{FrameRate, Tick, TimeRange};
use std::sync::Arc;

struct Pattern {
    base: GeneratorSource,
    background: bool,
}
impl MediaSource for Pattern {
    fn info(&self) -> &MediaInfo {
        self.base.info()
    }
    fn audio(&self, start: i64, frames: usize, rate: u32) -> filmcraft_media::Result<AudioBuffer> {
        self.base.audio(start, frames, rate)
    }
    fn video_frame(&self, req: FrameRequest) -> filmcraft_media::Result<Arc<VideoFrame>> {
        let w = (320.0 * req.scale).round().max(2.0) as u32;
        let h = (180.0 * req.scale).round().max(2.0) as u32;
        let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
        for y in 0..h {
            for x in 0..w {
                let (u, v) = (x as f32 / w as f32, y as f32 / h as f32);
                let color = if self.background {
                    [25 + (u * 30.0) as u8, 60 + (v * 55.0) as u8, 35, 255]
                } else if (0.375..0.625).contains(&u) && (0.33..0.67).contains(&v) {
                    [240, 140, 30, 255]
                } else {
                    [0, 0, 0, 255]
                };
                pixels.extend(color);
            }
        }
        Ok(Arc::new(VideoFrame::rgba8(w, h, pixels)))
    }
}

pub struct VideoFxFixture {
    pub project: Project,
    pub sequence: ItemId,
    pub sources: SourceMap,
    pub motion: ClipId,
}
impl VideoFxFixture {
    pub fn gradient_and_black_motion() -> Self {
        let mut f = Self { project: Project::new("Video FX"), sequence: ItemId(0), sources: SourceMap::default(), motion: ClipId(0) };
        let settings = SequenceSettings { width: 320, height: 180, frame_rate: FrameRate::FPS_60, ..Default::default() };
        f.sequence = f.project.new_sequence("Test", settings, 3, 0, None);
        for background in [true, false] {
            let base = GeneratorSource::new(Generator::BlackVideo, 320, 180, FrameRate::FPS_60, FrameRate::FPS_60.tick_of(1200));
            let info = base.info().clone();
            let id = f.project.add_item(
                "pattern",
                Label::Iris,
                ItemKind::Media(Box::new(MediaClip {
                    media: MediaRef::Generator(Generator::BlackVideo),
                    info,
                    interpret: Default::default(),
                    mark_in: None,
                    mark_out: None,
                    markers: Vec::new(),
                    offline: false,
                    proxy: None,
                    identity: None,
                })),
                None,
            );
            f.sources.0.insert(id, Arc::new(Pattern { base, background }) as SharedSource);
            let mut it = f
                .project
                .make_track_item(id, TrackKind::Video, Tick::ZERO, TimeRange::new(Tick::ZERO, FrameRate::FPS_60.tick_of(240)), FrameRate::FPS_60)
                .unwrap();
            for effect in &mut it.effects {
                resolve_auto_points(effect, (320, 180), (320, 180));
            }
            if !background {
                set(it.effect_mut("opacity").unwrap(), "blend", ParamValue::Choice(7));
                f.motion = it.id;
            }
            f.project.sequence_mut(f.sequence).unwrap().video_tracks[usize::from(!background)].items.push(it);
        }
        f
    }
    pub fn track(&mut self) -> &mut Track {
        &mut self.project.sequence_mut(self.sequence).unwrap().video_tracks[1]
    }
    pub fn clip(&mut self) -> &mut TrackItem {
        self.project.sequence_mut(self.sequence).unwrap().find_item_mut(self.motion).unwrap().1
    }
    pub fn frame(&self, frame: i64, scale: f32) -> Image {
        render_sequence(&self.project, self.sequence, FrameRate::FPS_60.tick_of(frame), RenderOptions { scale, ..Default::default() }, &self.sources)
    }
    pub fn planned(&self, frame: i64, scale: f32) -> Image {
        filmcraft_render::plan::execute_cpu(&filmcraft_render::plan::plan_frame(
            &self.project,
            self.sequence,
            FrameRate::FPS_60.tick_of(frame),
            RenderOptions { scale, ..Default::default() },
            filmcraft_render::plan::DeviceCaps::ALL,
            &self.sources,
        ))
    }
}
pub fn effect(id: &str) -> EffectInstance {
    filmcraft_project::find_effect(id).unwrap().instance()
}
pub fn set(e: &mut EffectInstance, p: &str, v: ParamValue) {
    e.params.get_mut(p).unwrap().value = v;
}
pub fn blur(amount: f64) -> EffectInstance {
    let mut e = effect("gaussian_blur");
    set(&mut e, "blurriness", ParamValue::Float(amount));
    e
}
pub fn max_abs_difference(a: &Image, b: &Image) -> f32 {
    assert_eq!((a.w, a.h), (b.w, b.h));
    a.px.iter().zip(&b.px).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max)
}
pub fn valid_alpha(image: &Image) {
    assert!(image.px.iter().all(|v| v.is_finite()));
    for p in image.px.chunks_exact(4) {
        assert!((0.0..=1.00001).contains(&p[3]));
        if p[3] == 0.0 {
            assert!(p[..3].iter().all(|v| v.abs() < 1e-6));
        }
    }
}
