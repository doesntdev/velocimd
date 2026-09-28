use eframe::egui::{self, Color32, ImageSource, Rect, Vec2, load::SizeHint, pos2, vec2};
use std::collections::HashSet;
use velocimd::{
    commands::Command,
    icons::{Icon, compact_icon_button, icon_button, paint_icon},
    theme::ThemeConfig,
};

fn all_icons() -> Vec<Icon> {
    let mut icons: Vec<_> = Command::all()
        .iter()
        .copied()
        .map(Icon::for_command)
        .collect();
    icons.extend([Icon::Plus, Icon::Check]);
    icons
}

#[test]
fn every_command_has_a_distinct_embedded_icon() {
    let mut uris = HashSet::new();
    let mut sources = HashSet::new();
    for icon in all_icons() {
        let ImageSource::Bytes { uri, bytes } = icon.source() else {
            panic!("{icon:?} must not require a runtime asset file or network request");
        };
        assert!(uri.ends_with(".svg"));
        assert!(uris.insert(uri.to_string()), "duplicate icon URI: {icon:?}");
        assert!(sources.insert(bytes.to_vec()), "duplicate art: {icon:?}");
        let source = std::str::from_utf8(&bytes).unwrap();
        for contract in [
            "viewBox=\"0 0 24 24\"",
            "stroke=\"#fff\"",
            "stroke-width=\"1.75\"",
            "stroke-linecap=\"round\"",
            "stroke-linejoin=\"round\"",
            "fill=\"none\"",
        ] {
            assert!(source.contains(contract), "{icon:?} is missing {contract}");
        }
    }
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icons/toolbar");
    let asset_count = std::fs::read_dir(directory)
        .unwrap()
        .map(Result::unwrap)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "svg"))
        .count();
    assert_eq!(
        uris.len(),
        asset_count,
        "unmapped asset or missing command icon"
    );
}

#[test]
fn icons_rasterize_without_clipping_at_compact_normal_and_hidpi_sizes() {
    for icon in all_icons() {
        let ImageSource::Bytes { bytes, .. } = icon.source() else {
            unreachable!();
        };
        for size in [12, 14, 18, 27, 36] {
            let image = egui_extras::image::load_svg_bytes_with_size(
                &bytes,
                SizeHint::Size {
                    width: size,
                    height: size,
                    maintain_aspect_ratio: true,
                },
                &Default::default(),
            )
            .unwrap_or_else(|error| panic!("{icon:?} at {size}px: {error}"));
            let size = size as usize;
            assert_eq!(image.size, [size, size]);
            assert!(
                image.pixels.iter().any(|pixel| pixel.a() > 127),
                "empty icon: {icon:?}"
            );
            for (index, pixel) in image.pixels.iter().enumerate() {
                // Premultiplied white can be tinted for any built-in or custom theme.
                assert_eq!(pixel.r(), pixel.a(), "nonwhite source: {icon:?}");
                assert_eq!(pixel.g(), pixel.a());
                assert_eq!(pixel.b(), pixel.a());
                if index < size
                    || index >= size * (size - 1)
                    || index % size == 0
                    || index % size == size - 1
                {
                    assert!(
                        pixel.a() < 128,
                        "{icon:?} clipped at {size}px, pixel {index}"
                    );
                }
            }
        }
    }
}

#[test]
fn embedded_texture_loads_synchronously_and_is_reused() {
    let ctx = egui::Context::default();
    egui_extras::install_image_loaders(&ctx);
    for scale in [1.0, 1.5, 2.0] {
        ctx.set_pixels_per_point(scale);
        let _ = ctx.run_ui(Default::default(), |_| {});
        for icon in all_icons() {
            let image = egui::Image::new(icon.source()).fit_to_exact_size(Vec2::splat(18.0));
            let first = image.load_for_size(&ctx, Vec2::splat(18.0)).unwrap();
            let second = image.load_for_size(&ctx, Vec2::splat(18.0)).unwrap();
            let (
                egui::load::TexturePoll::Ready { texture: first },
                egui::load::TexturePoll::Ready { texture: second },
            ) = (first, second)
            else {
                panic!("embedded icon pending: {icon:?}");
            };
            assert_eq!(first.id, second.id, "texture cache miss: {icon:?}");
            // SizedTexture.size reports the SVG's intrinsic size, not its raster size.
            let textures = ctx.tex_manager();
            let textures = textures.read();
            assert_eq!(
                textures.meta(first.id).unwrap().size,
                [(18.0 * scale) as usize; 2]
            );
        }
    }
}

#[test]
fn painting_uses_theme_tint_and_keeps_nonsquare_slots_square() {
    let ctx = egui::Context::default();
    egui_extras::install_image_loaders(&ctx);
    for theme in [ThemeConfig::default_dark(), ThemeConfig::default_light()] {
        theme.apply_to(&ctx);
        let color = ctx.global_style().visuals.selection.stroke.color;
        let output = ctx.run_ui(Default::default(), |root| {
            egui::CentralPanel::default().show_inside(root, |ui| {
                paint_icon(
                    ui,
                    Icon::Sun,
                    Rect::from_min_size(pos2(10.0, 10.0), vec2(22.0, 18.0)),
                    color,
                );
            });
        });
        let images: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect) if rect.brush.is_some() && rect.fill == color => Some(rect),
                _ => None,
            })
            .collect();
        assert_eq!(images.len(), 1, "missing tinted icon texture");
        let bounds = images[0].rect;
        assert_eq!(bounds.size(), Vec2::splat(18.0));
        assert_eq!(bounds.center(), pos2(21.0, 19.0));
        assert_ne!(color, Color32::WHITE);
    }
}

#[test]
fn buttons_preserve_hit_targets_and_selected_icon_accent() {
    let ctx = egui::Context::default();
    egui_extras::install_image_loaders(&ctx);
    ThemeConfig::default_dark().apply_to(&ctx);
    let accent = ctx.global_style().visuals.selection.stroke.color;
    let output = ctx.run_ui(Default::default(), |root| {
        egui::CentralPanel::default().show_inside(root, |ui| {
            assert_eq!(
                icon_button(ui, Icon::Edit, true, "Edit".into()).rect.size(),
                vec2(30.0, 28.0)
            );
            assert_eq!(
                compact_icon_button(ui, Icon::X, "Close".into()).rect.size(),
                Vec2::splat(20.0)
            );
        });
    });
    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Rect(rect) if rect.brush.is_some() && rect.fill == accent)));
}
