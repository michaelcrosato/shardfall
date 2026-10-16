//! The shipped packs (`props::store::SHIPPED`) load with no asset files on disk, as in the
//! browser and a lone Windows exe: each KayKit figure, its body and its meshes come from the
//! build. Its own process, since it points the props root at an empty folder.

use pav_core::props;

#[test]
fn figures_load_from_the_build_alone() {
    let empty = std::env::temp_dir().join(format!("pav_no_assets_{}", std::process::id()));
    std::fs::create_dir_all(&empty).unwrap();
    // SAFETY: set before any other code in this test binary reads the environment.
    unsafe { std::env::set_var("PAV_ASSETS", &empty) };
    for leaf in ["knight", "barbarian", "mage", "rogue", "rogue_hooded"] {
        let name = format!("LIB/kaykit_adventurers/{leaf}");
        assert!(props::store::shipped(&name), "{name} ships");
        assert!(!props::store::embedded(&name), "{name} keeps its own tier");
        let figure = pav_core::figure::get(&name).unwrap_or_else(|e| panic!("{e}"));
        assert!(figure.segments().len() >= 12, "{name}: {} segments", figure.segments().len());
        assert!(pav_core::figure::check(&figure).is_empty(), "{name}: {:?}", pav_core::figure::check(&figure));
    }
    assert!(!props::store::shipped("LIB/kenney_mini_dungeon/barrel"), "other packs stay on disk");
    std::fs::remove_dir_all(&empty).ok();
}
