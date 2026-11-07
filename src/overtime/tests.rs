use super::*;

#[test]
fn missing_fields_inherit_defaults_and_labels_inherit_global_style() {
    let settings: crate::config::Settings = serde_json::from_str(
        r##"{"overtime":{"color":"#abcdef","labels":[{"text":"{remaining} / {total}"}]}}"##,
    )
    .unwrap();
    assert_eq!(settings.overtime.background, Background::default());
    assert_eq!(settings.overtime.font_family, "sans-serif");
    assert_eq!(settings.overtime.color, Color([171, 205, 239, 255]));
    assert_eq!(settings.overtime.labels[0].color, None);
    assert_eq!(settings.overtime.labels[0].position, [Offset::default(); 2]);
    assert_eq!(settings.overtime.labels[0].font_size, None);
    let encoded = serde_json::to_value(settings).unwrap();
    assert!(encoded["overtime"]["labels"][0].get("color").is_none());
    let default: Overtime = serde_json::from_str("{}").unwrap();
    assert_eq!(default, Overtime::default());
    assert_eq!(
        default
            .labels
            .iter()
            .map(|label| label.text.as_str())
            .collect::<Vec<_>>(),
        [
            "TRANSMISSION PAUSED",
            "TAKE A BREAK",
            "{remaining} / {total}"
        ]
    );
}

#[test]
fn nulls_invalid_values_and_unknown_keys_are_rejected() {
    for invalid in [
        r#"{"background":null}"#,
        r#"{"background":{"image":null}}"#,
        r#"{"font_family":null}"#,
        r#"{"font_size":0}"#,
        r#"{"font_size":257}"#,
        r#"{"color":"orange"}"#,
        r#"{"halign":"top"}"#,
        r#"{"valign":"left"}"#,
        r#"{"labels":[{"font_family":null}]}"#,
        r#"{"labels":[{"font_size":null}]}"#,
        r#"{"labels":[{"color":null}]}"#,
        r#"{"labels":[{"position":["101%",0]}]}"#,
        r#"{"labels":[{"position":["0",0]}]}"#,
        r#"{"labels":[{"position":[0]}]}"#,
        r#"{"labels":[{"text":"two\nlines"}]}"#,
        r#"{"labels":[{"typo":true}]}"#,
        r#"{"labels":null}"#,
        r#"{"typo":true}"#,
    ] {
        assert!(
            serde_json::from_str::<Overtime>(invalid).is_err(),
            "{invalid}"
        );
    }
    let labels = serde_json::json!({"labels":vec![serde_json::json!({"text":"test"}); 33]});
    assert!(serde_json::from_value::<Overtime>(labels).is_err());
}

#[test]
fn color_and_position_values_round_trip_without_null_sentinels() {
    for input in ["#aabbcc", "#aabbcc80"] {
        let color: Color = serde_json::from_value(serde_json::json!(input)).unwrap();
        assert_eq!(serde_json::to_value(color).unwrap(), input);
    }
    let position: [Offset; 2] = serde_json::from_str(r#"[12,"-10%"]"#).unwrap();
    assert_eq!(position, [Offset::Pixels(12.0), Offset::Percent(-10.0)]);
    assert_eq!(
        serde_json::to_value(position).unwrap(),
        serde_json::json!([12.0, "-10%"])
    );
}

#[test]
fn default_background_is_embedded_and_custom_paths_round_trip() {
    let background = Background::default();
    assert_eq!(background.image, None);
    let encoded = serde_json::to_value(&background).unwrap();
    assert!(encoded.get("image").is_none());
    assert_eq!(
        serde_json::from_value::<Background>(encoded).unwrap(),
        background
    );

    let custom = serde_json::json!({"image": "/tmp/custom.png"});
    let background: Background = serde_json::from_value(custom).unwrap();
    assert_eq!(background.image, Some(PathBuf::from("/tmp/custom.png")));
    assert_eq!(
        serde_json::to_value(background).unwrap()["image"],
        "/tmp/custom.png"
    );
}
