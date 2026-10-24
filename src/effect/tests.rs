use super::*;
#[test]
fn constructors_and_json_enforce_the_same_ranges() {
    for value in [f32::NAN, f32::INFINITY, -0.1, 100.1] {
        assert!(Glitchiness::try_from(value).is_err());
    }
    for value in [f32::NAN, f32::INFINITY, 1.0, 5.1] {
        assert!(GraceCurve::try_from(value).is_err());
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
    let valid = serde_json::to_value(Settings::default()).unwrap();
    assert_eq!(
        serde_json::from_value::<Settings>(valid.clone()).unwrap(),
        Settings::default()
    );
    for (pointer, bad) in [
        ("/curve", 0.0),
        ("/tears/entry", 100.0),
        ("/colour/maximum", -0.1),
    ] {
        let mut input = valid.clone();
        *input.pointer_mut(pointer).unwrap() = serde_json::json!(bad);
        assert!(
            serde_json::from_value::<Settings>(input).is_err(),
            "{pointer}"
        );
    }
    let mut unknown = valid;
    unknown["tears"]["typo"] = serde_json::json!(true);
    assert!(serde_json::from_value::<Settings>(unknown).is_err());
}

#[test]
fn rejected_edits_preserve_valid_tuning_and_disabled_effects_are_invisible() {
    let mut settings = Settings::default();
    let original = settings.clone();
    assert!(settings.set_curve(0.0).is_err());
    assert_eq!(settings, original);
    for kind in EffectKind::ALL {
        settings.set_effect(kind, Effect::new(false, 0.0, 1.0).unwrap());
    }
    assert!(!settings.has_visible_effect_at(Glitchiness::MAXIMUM));
    assert!(original.has_visible_effect_at(Glitchiness::MAXIMUM));
    assert!(!original.has_visible_effect_at(Glitchiness::ZERO));
}

#[test]
fn actual_shader_constants_raise_burst_frequency_and_duration_separately() {
    let settings = Settings::default();
    let mut previous = [0.0, 0.0];
    for level in 0..=100 {
        let body = settings.shader_body(Glitchiness::try_from(level as f32).unwrap());
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
    let settings = Settings::default();
    let shader = settings
        .hyprland_shader_at(Glitchiness::try_from(75.0).unwrap(), 123.5, u32::MAX)
        .unwrap();
    assert!(shader.contains("uniform int wl_output;"));
    assert!(shader.contains("time + 123.5000, 4294967295u ^ uint(wl_output)"));
    assert_eq!(
        shader,
        settings
            .hyprland_shader_at(Glitchiness::try_from(75.0).unwrap(), 123.5, u32::MAX)
            .unwrap()
    );
    assert_ne!(
        shader,
        settings
            .hyprland_shader_at(Glitchiness::try_from(75.0).unwrap(), 123.5, 17)
            .unwrap()
    );
    assert!(
        settings
            .hyprland_shader_at(Glitchiness::try_from(75.0).unwrap(), f32::INFINITY, 0)
            .is_err()
    );
    assert!(
        settings
            .hyprland_shader_at(Glitchiness::try_from(75.0).unwrap(), -1.0, 0)
            .is_err()
    );
    fresh_seed().unwrap();
}

#[test]
fn bounded_smooth_monotonic_and_toggle() {
    let mut s = Settings::default();
    for kind in EffectKind::ALL {
        let e = s.effect(kind);
        let mut previous = 0.0;
        for i in 0..=10000 {
            let v = e.strength(Glitchiness::try_from(i as f32 / 100.0).unwrap());
            assert!(v >= previous && v <= e.maximum());
            previous = v;
        }
        assert_eq!(e.strength(Glitchiness::try_from(e.entry()).unwrap()), 0.0);
        assert!(e.strength(Glitchiness::try_from(e.entry() + 0.001).unwrap()) < 0.000001);
    }
    s.set_effect(EffectKind::Tears, Effect::new(false, 5.0, 0.7).unwrap());
    assert_eq!(
        s.effect(EffectKind::Tears).strength(Glitchiness::MAXIMUM),
        0.0
    );
    assert!(
        s.hyprland_shader(Glitchiness::MAXIMUM)
            .unwrap()
            .contains("bork")
    );
    assert!(Glitchiness::try_from(f32::NAN).is_err());
    assert!(s.set_curve(f32::INFINITY).is_err());
}
