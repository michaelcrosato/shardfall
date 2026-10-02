//! The game registry: add your game's module and one line to `GAMES`.
use pavlite::sim::GameDef;

mod arena;
mod platformer;
mod template;

pub static GAMES: &[GameDef] = &[
    GameDef {
        name: "template",
        about: "Smallest complete game: walk around, push crates, collect the coins.",
        make: || Box::new(template::Template::default()),
    },
    GameDef {
        name: "platformer",
        about: "Side-view platformer: jump gaps and spikes, stomp blobs, ride the lift, reach the goal.",
        make: || Box::new(platformer::Platformer::default()),
    },
    GameDef {
        name: "arena",
        about: "Top-down shooter: survive waves of runners, shooters and brutes (mouse aims, hold fire).",
        make: || Box::new(arena::Arena::default()),
    },
];

/// A registered game by name (tests).
#[cfg(test)]
pub fn def(name: &str) -> &'static GameDef {
    GAMES.iter().find(|g| g.name == name).expect("registered game")
}

#[cfg(test)]
mod tests {
    use pavlite::input::Input;
    use pavlite::sim::Sim;
    use pavlite::tools::{Output, Session, parse_args};
    use serde_json::Value;

    use super::*;

    fn call(s: &mut Session, line: &str) -> Value {
        let words: Vec<String> = line.split(' ').map(String::from).collect();
        match s.call(&words[0], &parse_args(&words[1..])) {
            Ok(Output::Json(v)) => v,
            Ok(Output::Image { png, meta }) => {
                assert!(png.len() > 1000, "{line}: tiny image");
                meta
            }
            Err(e) => panic!("{line}: {e}"),
        }
    }

    fn step_bot(sim: &mut Sim, ticks: u64) {
        for _ in 0..ticks {
            let input = sim.game.bot(&sim.world).unwrap_or_default();
            sim.step(&input);
        }
    }

    #[test]
    fn every_game_runs_and_draws() {
        for g in GAMES {
            let mut sim = Sim::new(g, 3);
            step_bot(&mut sim, 240);
            let img = pavlite::view::snapshot(&sim.world, sim.game.as_ref(), 160, 90, 1, None);
            let mut colours: Vec<u32> = img.px.clone();
            colours.sort_unstable();
            colours.dedup();
            assert!(colours.len() > 20, "{}: only {} colours", g.name, colours.len());
            assert!(sim.world.player().is_some(), "{} has no player", g.name);
        }
    }

    #[test]
    fn rewind_and_replay_repeat_exactly() {
        for g in GAMES {
            let mut sim = Sim::new(g, 7);
            step_bot(&mut sim, 150);
            let mid = sim.hash();
            step_bot(&mut sim, 150);
            let end = sim.hash();
            assert_eq!(sim.rewind(150), 150);
            assert_eq!(sim.hash(), mid, "{}: rewind", g.name);
            // The same inputs again reach the same end.
            let replay = {
                let mut again = Sim::new(g, 7);
                step_bot(&mut again, 300);
                again.replay()
            };
            let mut fresh = Sim::new(g, 7);
            for input in replay.iter() {
                fresh.step(input);
            }
            assert_eq!(fresh.hash(), end, "{}: replay", g.name);
        }
    }

    #[test]
    fn npcs_stand_still_unless_driven() {
        let mut sim = Sim::new(def("template"), 1);
        let npc = sim.world.spawn(pavlite::entity::Spawn::character("npc", glam::Vec3::new(5.5, 0.0, 3.5)));
        sim.run(10, &Input::default());
        let start = sim.world.get(npc).unwrap().pos;
        // The game never drives it, so it must not keep walking on an old input.
        sim.world.drive(npc, Input { move_dir: glam::Vec2::X, ..Default::default() });
        sim.run(60, &Input::default());
        assert!(sim.world.get(npc).unwrap().pos.distance(start) < 0.05);
    }

    #[test]
    fn tools_work_end_to_end() {
        let dir = std::env::temp_dir().join(format!("pav-test-{}", std::process::id()));
        let out = |name: &str| dir.join(name).display().to_string();
        let mut s = Session::new(GAMES);
        assert!(call(&mut s, "help").as_array().unwrap().len() > 20);
        assert_eq!(call(&mut s, "games").as_array().unwrap().len(), GAMES.len());
        assert!(s.call("status", &Default::default()).is_err(), "no game loaded yet");
        call(&mut s, "load game=template seed=2");
        // Unknown arguments are errors, not silently ignored.
        let e = s.call("capture", &parse_args(&["mark=true".into()])).err().unwrap();
        assert!(e.contains("unknown argument 'mark'") && e.contains("marks"), "{e}");
        let st = call(&mut s, "input toward=[3.5,0,2.5] ticks=120");
        assert!(st["events"].as_array().unwrap().iter().any(|e| e.as_str().unwrap().contains("enter coin")), "{st}");
        assert_eq!(call(&mut s, "status")["status"]["score"], 1);
        let shot = call(&mut s, &format!("capture marks=true width=320 height=180 out={}", out("a.png")));
        assert!(shot["marks"].as_array().unwrap().len() >= 3);
        call(&mut s, &format!("filmstrip frames=4 every=5 width=160 height=90 move=[1,0] out={}", out("b.png")));
        let map = call(&mut s, "ascii radius=6");
        assert!(map["map"].as_array().unwrap().iter().any(|r| r.as_str().unwrap().contains('@')));
        assert!(call(&mut s, "entities kind=crate")["count"].as_u64().unwrap() == 2);
        call(&mut s, "entity");
        assert!(call(&mut s, "params prefix=movement").as_array().unwrap().len() > 10);
        assert_eq!(call(&mut s, "set path=movement.speed value=9")["value"], 9.0);
        assert!(s.call("set", &parse_args(&["path=movement.nope".into(), "value=1".into()])).is_err());
        let id = call(&mut s, "spawn kind=ball shape=sphere size=0.4 color=#ff0000")["id"].as_u64().unwrap();
        call(&mut s, "step ticks=30");
        call(&mut s, &format!("teleport id={id} pos=[5,3,3]"));
        assert_eq!(call(&mut s, &format!("despawn id={id}"))["despawned"], true);
        call(&mut s, "spawn kind=guard character=true color=#20a040");
        call(&mut s, "snapshot name=x");
        call(&mut s, "input move=[0,1] ticks=30");
        assert_eq!(call(&mut s, "restore name=x")["restored"], "x");
        assert!(call(&mut s, "rewind ticks=30").get("note").is_some(), "restore starts a new history");
        let t = call(&mut s, "status")["tick"].as_u64().unwrap();
        call(&mut s, "step ticks=60");
        assert_eq!(call(&mut s, "rewind ticks=60")["tick"].as_u64().unwrap(), t);
        assert!(call(&mut s, &format!("record path={}", out("x.json"))).get("warning").is_some());
        // Inputs alone reproduce a session exactly.
        call(&mut s, "load game=template seed=5");
        call(&mut s, "input move=[1,0.3] hold=jump ticks=50");
        call(&mut s, "input toward=[6.5,0,4.5] ticks=90");
        let rec = call(&mut s, &format!("record path={}", out("r.json")));
        assert!(rec.get("warning").is_none());
        let rep = call(&mut s, &format!("replay path={}", out("r.json")));
        assert_eq!(rep["match"], true, "{rec} {rep}");
        call(&mut s, "load game=platformer");
        assert!(
            call(&mut s, "ascii plane=xy radius=8")["map"].as_array().unwrap().iter().any(|r| r.as_str().unwrap().contains('#'))
        );
        assert!(call(&mut s, "autoplay seconds=5")["ticks"].as_u64().unwrap() == 300);
        assert!(call(&mut s, "bench ticks=60 frames=1 width=160 height=90")["ticks_per_sec"].as_f64().unwrap() > 60.0);
        let lv = call(&mut s, "level_check text=name=\"empty\"");
        assert_eq!(lv["blocks"], 0);
        assert_eq!(lv["name"], "empty");
        assert!(s.call("level_check", &parse_args(&["text=[[layer]]\nmapp=1".into()])).is_err());
        let _ = std::fs::remove_dir_all(dir);
        let _ = Input::default();
    }
}
