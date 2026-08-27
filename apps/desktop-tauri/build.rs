fn main() {
    ensure_placeholder_icon();
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "quit_rlogs",
            "show_combat_overlay",
            "set_combat_overlay_enabled",
            "hide_combat_overlay",
            "show_combat_overlay_if_requested",
            "set_combat_overlay_automatically_hidden",
            "combat_overlay_ready",
            "assign_hotkey",
        ]),
    ))
    .expect("could not build the rLogs Tauri application")
}

fn ensure_placeholder_icon() {
    let manifest_root = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory is unavailable"),
    );
    let icon_path = manifest_root.join("icons/icon.ico");
    let mut directory = ico::IconDir::new(ico::ResourceType::Icon);
    for size in [16, 32, 48, 64, 128, 256] {
        let image = ico::IconImage::from_rgba_data(size, size, placeholder_pixels(size));
        directory
            .add_entry(ico::IconDirEntry::encode(&image).expect("could not encode icon layer"));
    }
    let mut encoded = Vec::new();
    directory
        .write(&mut encoded)
        .expect("could not encode placeholder icon");
    if std::fs::read(&icon_path).ok().as_deref() == Some(encoded.as_slice()) {
        return;
    }
    std::fs::create_dir_all(icon_path.parent().expect("icon path has no parent"))
        .expect("could not create icon folder");
    std::fs::write(icon_path, encoded).expect("could not write placeholder icon");
}

fn placeholder_pixels(size: u32) -> Vec<u8> {
    let mut pixels = vec![0_u8; (size * size * 4) as usize];
    let edge = (size / 12).max(1);
    for y in 0..size {
        for x in 0..size {
            let index = ((y * size + x) * 4) as usize;
            let border = x < edge || y < edge || x >= size - edge || y >= size - edge;
            let slash_width = (size / 10).max(1);
            let slash_x = size.saturating_sub(1).saturating_sub(y);
            let slash = x.abs_diff(slash_x) <= slash_width;
            let (red, green, blue) = if border || slash {
                (8, 17, 27)
            } else {
                (91, 222, 211)
            };
            pixels[index] = red;
            pixels[index + 1] = green;
            pixels[index + 2] = blue;
            pixels[index + 3] = 255;
        }
    }
    pixels
}
