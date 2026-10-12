//! Writes every WGSL module the renderer compiles, exactly as compiled (the scene shader with
//! the material library in front), to `<dir>/<name>.wgsl`. `scripts/check-wgsl.sh` compiles
//! them in headless Chromium, whose Tint is stricter than naga.
//! `cargo run -p pav_render --example dump_wgsl -- <dir>`
fn main() -> std::io::Result<()> {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "out/wgsl".into()));
    std::fs::create_dir_all(&dir)?;
    for (name, source) in pav_render::renderer::WGSL_MODULES {
        let path = dir.join(format!("{name}.wgsl"));
        std::fs::write(&path, source)?;
        println!("{} ({} bytes)", path.display(), source.len());
    }
    Ok(())
}
