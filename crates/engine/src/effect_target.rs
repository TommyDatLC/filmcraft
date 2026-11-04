//! Effect-stack addressing without synthetic clip identifiers.
use crate::{EngineError, Result};
use filmcraft_project::{ClipId, EffectInstance, Sequence, TrackId, TrackKind};
use filmcraft_time::Tick;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EffectTarget {
    Clip(ClipId),
    VideoTrack(TrackId),
}

pub fn effects(seq: &Sequence, target: EffectTarget) -> Result<&[EffectInstance]> {
    match target {
        EffectTarget::Clip(id) => seq.find_item(id).map(|(_, it)| it.effects.as_slice()),
        EffectTarget::VideoTrack(id) => seq.track(id).filter(|t| t.kind == TrackKind::Video).map(|t| t.effects.as_slice()),
    }
    .ok_or_else(|| EngineError::Other("effect target does not exist".into()))
}

pub fn effects_mut(seq: &mut Sequence, target: EffectTarget) -> Result<&mut Vec<EffectInstance>> {
    let id = match target {
        EffectTarget::Clip(id) => seq.find_item(id).map(|(track, _)| track),
        EffectTarget::VideoTrack(id) => seq.track(id).filter(|t| t.kind == TrackKind::Video).map(|t| t.id),
    }
    .ok_or_else(|| EngineError::Other("effect target does not exist".into()))?;
    if seq.track(id).is_some_and(|t| t.locked) {
        return Err(EngineError::Other("effect target is on a locked track".into()));
    }
    match target {
        EffectTarget::Clip(id) => seq.find_item_mut(id).map(|(_, it)| &mut it.effects),
        EffectTarget::VideoTrack(id) => seq.track_mut(id).map(|t| &mut t.effects),
    }
    .ok_or_else(|| EngineError::Other("effect target does not exist".into()))
}

/// Clip keys retain source-time semantics; track keys are sequence ticks.
pub fn effect_time(seq: &Sequence, target: EffectTarget, timeline: Tick) -> Result<Tick> {
    if timeline.0.unsigned_abs() > filmcraft_project::ProjectView::MAX_PLAYHEAD.0 as u64 {
        return Err(EngineError::Other("effect time is outside supported bounds".into()));
    }
    match target {
        EffectTarget::Clip(id) => {
            let (_, it) = seq.find_item(id).ok_or_else(|| EngineError::Other("clip does not exist".into()))?;
            let limit = filmcraft_project::ProjectView::MAX_PLAYHEAD.0 as f64;
            let speed = it.speed.abs();
            let scaled = (((timeline.0 as i128 - it.start.0 as i128) as f64) * speed).round();
            let span = (it.duration.0 as f64 * speed).round();
            if !speed.is_finite() || !scaled.is_finite() || !span.is_finite() || scaled.abs() > limit || span.abs() > limit {
                return Err(EngineError::Other("clip effect time is outside supported bounds".into()));
            }
            let mapped = if let Some(hold) = it.frame_hold.filter(|_| it.hold_filters) {
                hold.0 as i128
            } else if it.reverse {
                it.source_in.0 as i128 + span as i128 - scaled as i128 - 1
            } else {
                it.source_in.0 as i128 + scaled as i128
            };
            if mapped.unsigned_abs() > filmcraft_project::ProjectView::MAX_PLAYHEAD.0 as u128 {
                return Err(EngineError::Other("clip effect time is outside supported bounds".into()));
            }
            i64::try_from(mapped).map(Tick).map_err(|_| EngineError::Other("clip effect time overflow".into()))
        }
        EffectTarget::VideoTrack(_) => {
            effects(seq, target)?;
            Ok(timeline)
        }
    }
}

impl Serialize for crate::EffectSel {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut wire = serializer.serialize_struct("EffectSel", 3)?;
        match self.target {
            EffectTarget::Clip(id) => wire.serialize_field("clip", &id)?,
            EffectTarget::VideoTrack(id) => wire.serialize_field("track", &id)?,
        }
        wire.serialize_field("effect", &self.effect)?;
        wire.serialize_field("instance", &self.instance)?;
        wire.end()
    }
}
impl<'de> Deserialize<'de> for crate::EffectSel {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            clip: Option<ClipId>,
            track: Option<TrackId>,
            target: Option<EffectTarget>,
            effect: String,
            instance: usize,
        }
        let wire = Wire::deserialize(deserializer)?;
        let target = match (wire.clip, wire.track, wire.target) {
            (Some(id), None, None) => EffectTarget::Clip(id),
            (None, Some(id), None) => EffectTarget::VideoTrack(id),
            (None, None, Some(target)) => target,
            _ => return Err(serde::de::Error::custom("need exactly one effect target")),
        };
        Ok(Self { target, effect: wire.effect, instance: wire.instance })
    }
}

/// Geometry-stage definitions reject malformed data before a project edit commits.
pub(crate) fn validate_geometry(effect: &EffectInstance) -> Result<()> {
    if effect.effect == "dynamic_zoom" {
        filmcraft_project::video_fx::DynamicZoomSettings::from_effect(effect).map_err(|why| crate::commands::bad("dynamic_zoom", why))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hostile_clip_time_mapping_returns_an_error_without_panicking() {
        let mut session = crate::Session::default();
        session.execute("file.openDemoProject", json!({})).unwrap();
        let id = session.active_sequence().unwrap().video_tracks[0].items[0].id;
        session
            .edit_sequence("fast clip", |q, _, _| {
                q.find_item_mut(id).unwrap().1.speed = 1000.0;
                Ok(())
            })
            .unwrap();
        let q = session.active_sequence().unwrap();
        let result = std::panic::catch_unwind(|| effect_time(q, EffectTarget::Clip(id), filmcraft_project::ProjectView::MAX_PLAYHEAD));
        assert!(result.is_ok(), "hostile mapping must not panic");
        assert!(result.unwrap().is_err(), "overflow must be rejected");
    }
}
