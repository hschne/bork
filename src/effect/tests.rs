use super::*;

#[test]
fn constructors_and_json_enforce_the_same_ranges() {
    for value in [f32::NAN, f32::INFINITY, -0.1, 100.1] {
        assert!(Glitchiness::try_from(value).is_err());
    }
    for (entry, maximum) in [
        (100.0, 0.5),
        (-1.0, 0.5),
        (0.0, 1.1),
        (f32::NAN, 0.5),
        (0.0, f32::INFINITY),
    ] {
        assert!(Effect::new(true, entry, maximum).is_err());
    }
    let valid = serde_json::to_value(Interference::default()).unwrap();
    assert_eq!(
        serde_json::from_value::<Interference>(valid.clone()).unwrap(),
        Interference::default()
    );
    for (pointer, bad) in [("/tears/entry", 100.0), ("/colour/maximum", -0.1)] {
        let mut input = valid.clone();
        *input.pointer_mut(pointer).unwrap() = serde_json::json!(bad);
        assert!(
            serde_json::from_value::<Interference>(input).is_err(),
            "{pointer}"
        );
    }
    let mut unknown = valid;
    unknown["tears"]["typo"] = serde_json::json!(true);
    assert!(serde_json::from_value::<Interference>(unknown).is_err());
}

#[test]
fn disabled_channels_are_invisible() {
    let disabled = Effect::new(false, 0.0, 1.0).unwrap();
    let interference = Interference {
        tears: disabled,
        colour: disabled,
        bands: disabled,
        noise: disabled,
        ghost: disabled,
    };
    assert!(!interference.has_visible_effect_at(Glitchiness::MAXIMUM));
    assert!(Interference::default().has_visible_effect_at(Glitchiness::MAXIMUM));
    assert!(!Interference::default().has_visible_effect_at(Glitchiness::ZERO));
}

#[test]
fn actual_shader_constants_raise_burst_frequency_and_duration_separately() {
    let interference = Interference::default();
    let mut previous = [0.0, 0.0];
    for level in 0..=100 {
        let body = interference.shader_body(Glitchiness::try_from(level as f32).unwrap());
        let mut current = [0.0; 2];
        for (i, name) in ["FREQUENCY", "DURATION"].iter().enumerate() {
            let prefix = format!("const float {name} = ");
            current[i] = body
                .lines()
                .find_map(|line| line.strip_prefix(&prefix))
                .unwrap()
                .trim_end_matches(';')
                .parse::<f32>()
                .unwrap();
            assert!(current[i] >= previous[i]);
        }
        assert!(current[0] * current[1] > 0.0 && current[0] * current[1] < 1.0);
        previous = current;
    }
    assert!((previous[0] - 0.85).abs() < 0.000001);
    assert!((previous[1] - 1.1).abs() < 0.000001);
}

#[test]
fn seeded_adapter_preserves_time_and_monitor_identity() {
    let interference = Interference::default();
    let level = Glitchiness::try_from(75.0).unwrap();
    let shader = interference
        .hyprland_shader_at(level, 123.5, u32::MAX)
        .unwrap();
    assert!(shader.contains("uniform int wl_output;"));
    assert!(shader.contains("time + 123.5000, 4294967295u ^ uint(wl_output)"));
    assert_eq!(
        shader,
        interference
            .hyprland_shader_at(level, 123.5, u32::MAX)
            .unwrap()
    );
    assert_ne!(
        shader,
        interference.hyprland_shader_at(level, 123.5, 17).unwrap()
    );
    assert!(
        interference
            .hyprland_shader_at(level, f32::INFINITY, 0)
            .is_err()
    );
    assert!(interference.hyprland_shader_at(level, -1.0, 0).is_err());
    fresh_seed().unwrap();
}

#[test]
fn channel_strength_is_bounded_smooth_and_monotone() {
    let interference = Interference::default();
    for effect in [
        interference.tears,
        interference.colour,
        interference.bands,
        interference.noise,
        interference.ghost,
    ] {
        let mut previous = 0.0;
        for i in 0..=10000 {
            let value = effect.strength(Glitchiness::try_from(i as f32 / 100.0).unwrap());
            assert!(value >= previous && value <= effect.maximum);
            previous = value;
        }
        assert_eq!(
            effect.strength(Glitchiness::try_from(effect.entry).unwrap()),
            0.0
        );
        assert!(effect.strength(Glitchiness::try_from(effect.entry + 0.001).unwrap()) < 0.000001);
    }
    assert_eq!(
        Effect::new(false, 5.0, 0.7)
            .unwrap()
            .strength(Glitchiness::MAXIMUM),
        0.0
    );
}
