//! Pure operations for the animation editor. Each operation returns a checked copy, so a bad
//! request cannot change a clip that the renderer or the game currently uses.

use serde_json::Value;

use crate::clips::{self, Clip, Key};

/// Authoring limits keep accidental values from producing unbounded work in clip tools.
pub const MAX_DURATION: f32 = 600.0;
pub const MAX_KEYS: usize = 4096;
const MAX_VALUE: f32 = 1_000_000.0;
const TIME_EPSILON: f32 = 0.000_1;

/// A neutral pose in the engine's readable clip format.
pub fn standing_key(time: f32) -> Key {
    Key {
        t: time,
        hips: [0.0, 0.0, 100.0],
        arm_l: [0.0, 0.0, -100.0, 10.0, 0.0],
        arm_r: [0.0, 0.0, -100.0, 10.0, 0.0],
        leg_l: [0.0, 0.0, -100.0, 5.0, 0.0],
        leg_r: [0.0, 0.0, -100.0, 5.0, 0.0],
        ..Default::default()
    }
}

/// A new clip starts and ends in the neutral pose.
pub fn create(name: &str, duration: f32, looping: bool) -> Result<Clip, String> {
    let clip = Clip {
        clip: name.into(),
        dur: duration,
        looping,
        src: "AUTHORED".into(),
        keys: vec![standing_key(0.0), standing_key(duration)],
        ..Default::default()
    };
    checked(clip)
}

/// Make an editable copy of an imported clip. Some captures include an interpolation key
/// just beyond their duration. Replace that tail with the pose the existing player shows at
/// the exact end; all playable times and the duration retain their meaning.
pub fn copy_clip(clip: &Clip, name: &str) -> Result<Clip, String> {
    let mut next = clip.clone();
    next.clip = name.into();
    if !next.keys.is_empty() && next.dur.is_finite() && next.dur > 0.0 && next.keys.iter().any(|k| k.t > next.dur) {
        let mut end = next.key_at_clamped(next.dur);
        end.t = next.dur;
        next.keys.retain(|k| k.t < next.dur - 0.000_9);
        next.keys.push(end);
    }
    checked(next)
}

/// Check values before they enter the live library. Limits apply to authored clips; old
/// imported sets retain their existing parser and do not change when the editor is added.
pub fn validate(clip: &Clip) -> Result<(), String> {
    if clip.clip.trim().is_empty() {
        return Err("the clip needs a name".into());
    }
    if !clip.dur.is_finite() || !(0.001..=MAX_DURATION).contains(&clip.dur) {
        return Err(format!("duration must be from 0.001 to {MAX_DURATION} seconds"));
    }
    if clip.keys.is_empty() || clip.keys.len() > MAX_KEYS {
        return Err(format!("a clip needs from 1 to {MAX_KEYS} keys"));
    }
    if clip.speed.is_some_and(|speed| !speed.is_finite() || speed <= 0.0 || speed > MAX_VALUE) {
        return Err("speed must be a finite number above 0 and at most 1000000".into());
    }
    let mut before = None;
    for (i, key) in clip.keys.iter().enumerate() {
        if !key.t.is_finite() || key.t < 0.0 || key.t > clip.dur + TIME_EPSILON {
            return Err(format!("key {i}: time must be from 0 to {} seconds", clip.dur));
        }
        if before.is_some_and(|t| key.t - t < 0.000_9) {
            return Err(format!("key {i}: times must increase by at least 0.001 seconds"));
        }
        before = Some(key.t);
        for (name, values) in [
            ("hips", &key.hips[..]),
            ("body", &key.body[..]),
            ("chest", &key.chest[..]),
            ("head", &key.head[..]),
            ("armL", &key.arm_l[..]),
            ("armR", &key.arm_r[..]),
            ("legL", &key.leg_l[..]),
            ("legR", &key.leg_r[..]),
        ] {
            values_check(i, name, values)?;
        }
        for (name, values) in [
            ("shL", key.sh_l.as_ref().map(|v| &v[..])),
            ("shR", key.sh_r.as_ref().map(|v| &v[..])),
            ("footL", key.foot_l.as_ref().map(|v| &v[..])),
            ("footR", key.foot_r.as_ref().map(|v| &v[..])),
            ("blade", key.blade.as_ref().map(|v| &v[..])),
            ("root", key.root.as_ref().map(|v| &v[..])),
        ] {
            if let Some(values) = values {
                values_check(i, name, values)?;
            }
        }
        for (name, limb) in [("armL", key.arm_l), ("armR", key.arm_r), ("legL", key.leg_l), ("legR", key.leg_r)] {
            if limb[..3].iter().all(|v| v.abs() < 1e-6) {
                return Err(format!("key {i}: {name} needs a nonzero direction"));
            }
            if !(0.0..=180.0).contains(&limb[3]) {
                return Err(format!("key {i}: {name} bend must be from 0 to 180 degrees"));
            }
        }
    }
    Ok(())
}

fn values_check(index: usize, channel: &str, values: &[f32]) -> Result<(), String> {
    if values.iter().any(|v| !v.is_finite() || v.abs() > MAX_VALUE) {
        return Err(format!("key {index}: {channel} needs finite numbers from -1000000 to 1000000"));
    }
    Ok(())
}

/// Sort the keys and check the exact text that will be saved. The format keeps milliseconds,
/// so this also detects times that become equal when they are written to disk.
pub fn checked(mut clip: Clip) -> Result<Clip, String> {
    clip.keys.sort_by(|a, b| a.t.total_cmp(&b.t));
    validate(&clip)?;
    let clip: Clip = serde_json::from_str(&clip.text()).map_err(|e| format!("the clip does not read back: {e}"))?;
    validate(&clip)?;
    Ok(clip)
}

/// Sample the end pose without wrapping a loop back to its start.
pub fn key_at(clip: &Clip, time: f32) -> Result<Key, String> {
    if !time.is_finite() || time < 0.0 || time > clip.dur + TIME_EPSILON {
        return Err(format!("time must be from 0 to {} seconds", clip.dur));
    }
    validate(clip)?;
    let mut key = clip.key_at_clamped(time);
    key.t = time.min(clip.dur);
    Ok(key)
}

/// Add or replace a key. Unspecified channels keep the sampled pose; an empty object inserts
/// that pose unchanged. An optional channel can be cleared with null. Unknown names are errors.
pub fn apply_key(clip: &Clip, time: f32, patch: &Value) -> Result<Clip, String> {
    let patch = patch.as_object().ok_or("pose must be a JSON object of channel arrays")?;
    let mut value = serde_json::to_value(key_at(clip, time)?).map_err(|e| e.to_string())?;
    for (name, channel) in patch {
        let (min, max, optional) = match name.as_str() {
            "hips" | "body" | "chest" | "head" => (3, 3, false),
            "armL" | "armR" | "legL" | "legR" => (3, 5, false),
            "shL" | "shR" | "root" => (2, 2, true),
            "footL" | "footR" | "blade" => (3, 3, true),
            _ => return Err(format!("unknown pose channel '{name}'; use the names in the legend")),
        };
        if !(optional && channel.is_null()) {
            let numbers = channel.as_array().ok_or_else(|| format!("{name} must be an array of {min} to {max} numbers"))?;
            if numbers.len() < min || numbers.len() > max || numbers.iter().any(|n| !n.is_number()) {
                return Err(format!("{name} must have {min} to {max} numbers"));
            }
        }
        value[name] = channel.clone();
    }
    let key: Key = serde_json::from_value(value).map_err(|e| format!("invalid pose: {e}"))?;
    let mut next = clip.clone();
    // A request within half a millisecond addresses the same stored key.
    if let Some(index) = next.keys.iter().position(|k| (k.t - time).abs() < 0.000_5) {
        next.keys[index] = key;
    } else {
        next.keys.push(key);
    }
    checked(next)
}

/// Remove one key at the given stored time. A clip must retain at least one pose.
pub fn delete_key(clip: &Clip, time: f32) -> Result<Clip, String> {
    if !time.is_finite() {
        return Err("time must be finite".into());
    }
    let index = clip.keys.iter().position(|k| (k.t - time).abs() < 0.000_5).ok_or_else(|| format!("no key at {time} seconds"))?;
    let mut next = clip.clone();
    next.keys.remove(index);
    checked(next)
}

/// Scale duration and key times. A walking clip's speed scales the other way, so the new
/// duration still covers the same distance.
pub fn retime(clip: &Clip, factor: f32) -> Result<Clip, String> {
    if !factor.is_finite() || factor <= 0.0 {
        return Err("factor must be a finite number above 0".into());
    }
    let mut next = clip.clone();
    next.dur *= factor;
    for key in &mut next.keys {
        key.t *= factor;
    }
    next.speed = next.speed.map(|speed| speed / factor);
    checked(next)
}

/// Swap left and right using the same operation as clip playback.
pub fn mirrored(clip: &Clip) -> Result<Clip, String> {
    let mut next = clip.clone();
    next.keys = next.keys.iter().map(clips::mirror).collect();
    checked(next)
}

/// A stable content revision for optimistic edits. This is a change token, not a security hash.
pub fn revision(clip: &Clip) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in clip.text().bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn partial_key_edits_keep_the_sampled_pose_and_close_a_loop_explicitly() {
        let start = create("Reach", 2.0, true).unwrap();
        let a = apply_key(&start, 1.0, &json!({"hips": [0, 0, 60], "armR": [100, 0, 0, 30, 10]})).unwrap();
        let b = apply_key(&a, 0.5, &json!({"head": [30, 0, 0]})).unwrap();
        let k = &b.keys[1];
        assert_eq!(k.hips, [0.0, 0.0, 80.0]);
        assert_eq!(k.head, [30.0, 0.0, 0.0]);
        assert_eq!(k.arm_l, start.keys[0].arm_l);
        let inserted = apply_key(&b, 0.25, &json!({})).unwrap();
        assert_eq!(inserted.keys.len(), b.keys.len() + 1);
        assert_eq!(inserted.keys[1].hips, [0.0, 0.0, 90.0]);
        let c = apply_key(&b, 2.0, &json!({"hips": [0, 0, 50]})).unwrap();
        assert_eq!(key_at(&c, 2.0).unwrap().hips[2], 50.0);
        assert_eq!(c.key_at(2.0).hips[2], 100.0, "the player still wraps loops");
    }

    #[test]
    fn failed_edits_leave_the_input_intact() {
        let c = create("Hold", 1.0, false).unwrap();
        let before = revision(&c);
        for patch in [json!({"armRigth": [0, 0, 1]}), json!({"armR": [0, 0, 0]}), json!({"armR": [1, 0, 0, 181]})] {
            assert!(apply_key(&c, 0.5, &patch).is_err());
        }
        assert!(apply_key(&c, 2.0, &json!({"head": [0, 0, 0]})).is_err());
        assert!(retime(&c, 0.0).is_err());
        assert_eq!(revision(&c), before);
        let mut broken = c.clone();
        broken.keys[0].hips[0] = f32::NAN;
        assert!(validate(&broken).is_err());
        broken = c;
        broken.keys[1].t = 0.000_3;
        assert!(checked(broken).is_err(), "the readable format cannot keep sub-millisecond keys");
    }

    #[test]
    fn mirror_and_retime_preserve_motion_and_body_lengths() {
        let mut c = create("Punch", 2.0, false).unwrap();
        c.speed = Some(120.0);
        c = apply_key(&c, 1.0, &json!({"armR": [100, 0, 0, 20, 10], "root": [50, 10]})).unwrap();
        assert_eq!(mirrored(&mirrored(&c).unwrap()).unwrap(), c);
        let slow = retime(&c, 2.0).unwrap();
        assert_eq!(slow.dur, 4.0);
        assert_eq!(slow.speed, Some(60.0));
        assert_eq!(slow.key_at(2.0).arm_r, c.key_at(1.0).arm_r);
        let def = crate::puppet::PuppetDef::default();
        let sk = clips::skel(&def, &slow.key_at(2.0), true);
        assert!(sk.head.is_finite());
        assert!(((sk.hand[1] - sk.elbow[1]).length() - def.arm_length * def.scale * 0.5).abs() < 1e-3);
    }

    #[test]
    fn an_imported_tail_key_does_not_prevent_an_editable_copy() {
        let mut original = create("Capture", 2.0, true).unwrap();
        original.keys[1].t = 2.02;
        original.keys[1].hips[2] = 50.0;
        assert!(validate(&original).is_err());
        let copied = copy_clip(&original, "Copy").unwrap();
        assert_eq!(copied.dur, original.dur);
        assert_eq!(copied.keys.last().unwrap().t, 2.0);
        for time in [0.0, 0.5, 1.0, 1.9, 2.0] {
            assert!((copied.key_at_clamped(time).hips[2] - original.key_at_clamped(time).hips[2]).abs() < 0.001);
        }
    }
}
