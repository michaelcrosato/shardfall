//! The tool registry. Add a tool: write a `fn(&mut Session, &Args) -> Result<Output>` and list
//! it in `TOOLS`. It is then available from the CLI, the REPL and MCP.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use pav_core::params::{self, ParamValue};
use serde_json::{Map, Value, json};

use crate::session::Session;

pub type Args = Map<String, Value>;

pub enum Output {
    Json(Value),
    /// A PNG image plus metadata (also saved to `path`).
    Image {
        png: Vec<u8>,
        path: Option<PathBuf>,
        meta: Value,
    },
}

pub struct Arg {
    pub name: &'static str,
    /// JSON schema type: string, number, integer, boolean.
    pub kind: &'static str,
    pub help: &'static str,
}

pub struct Tool {
    pub name: &'static str,
    pub help: &'static str,
    pub args: &'static [Arg],
    pub run: fn(&mut Session, &Args) -> Result<Output>,
}

pub(crate) const fn arg(name: &'static str, kind: &'static str, help: &'static str) -> Arg {
    Arg { name, kind, help }
}

pub static TOOLS: &[Tool] = &[
    Tool { name: "scenes", help: "List built-in scenes.", args: &[], run: t_scenes },
    Tool {
        name: "load",
        help: "Start a fresh simulation of a scene.",
        args: &[arg("scene", "string", "scene name (see `scenes`)"), arg("seed", "integer", "world seed (default 1)")],
        run: t_load,
    },
    Tool {
        name: "step",
        help: "Advance the simulation N ticks as fast as possible.",
        args: &[arg("ticks", "integer", "number of ticks (default 1)")],
        run: t_step,
    },
    Tool { name: "status", help: "Tick, time, entity/block counts and state hash.", args: &[], run: t_status },
    Tool {
        name: "entities",
        help: "List entities (id, name, position).",
        args: &[arg("name", "string", "only entities whose name contains this")],
        run: t_entities,
    },
    Tool {
        name: "params",
        help: "List tunable parameters (sim.*, camera.*, view.*) with values and ranges.",
        args: &[arg("prefix", "string", "only paths starting with this")],
        run: t_params,
    },
    Tool {
        name: "set",
        help: "Set a parameter: path=value.",
        args: &[arg("path", "string", "parameter path, e.g. camera.tilt"), arg("value", "string", "new value")],
        run: t_set,
    },
    Tool {
        name: "camera",
        help: "Apply a camera preset (no args lists them).",
        args: &[arg("preset", "string", "preset name")],
        run: t_camera,
    },
    Tool {
        name: "capture",
        help: "Render a screenshot (PNG) with the software/real GPU.",
        args: &[
            arg("out", "string", "output path (default out/capture-<tick>.png)"),
            arg("width", "integer", "default 960"),
            arg("height", "integer", "default 540"),
        ],
        run: t_capture,
    },
    Tool {
        name: "bench",
        help: "Measure simulation ticks per second.",
        args: &[arg("ticks", "integer", "ticks to run (default 600)")],
        run: t_bench,
    },
    Tool { name: "gpu", help: "Describe the GPU adapter used for captures.", args: &[], run: t_gpu },
    Tool {
        name: "player",
        help: "Player state: feet position, velocity, grounded, posture, climbing.",
        args: &[],
        run: t_player,
    },
    Tool {
        name: "input",
        help: "Drive the player: hold a move direction/buttons for N ticks (press = only on the first tick).",
        args: &[
            arg("move", "array", "[x, z] world direction, e.g. [1, 0] = east, [0, -1] = north"),
            arg("hold", "string", "held buttons, comma separated: jump,crouch,crawl,use,focus,interact,sprint,primary"),
            arg("press", "string", "buttons pressed on the first tick"),
            arg("aim", "array", "[x, y, z] aim point (bomb throws)"),
            arg("ticks", "integer", "ticks to run (default 1)"),
        ],
        run: t_input,
    },
    Tool {
        name: "spawn",
        help: "Spawn a prop.",
        args: &[
            arg("shape", "string", "box | sphere | capsule | cylinder | rounded_box"),
            arg("size", "array", "box: [hx,hy,hz] half extents; sphere: [r]; capsule/cylinder: [half_height, r]"),
            arg("pos", "array", "[x, y, z]"),
            arg("color", "string", "#rrggbb"),
            arg("body", "string", "dynamic (default) | fixed | kinematic | none"),
            arg("name", "string", "entity name"),
        ],
        run: t_spawn,
    },
    Tool { name: "despawn", help: "Remove an entity.", args: &[arg("id", "integer", "entity id")], run: t_despawn },
    Tool {
        name: "teleport",
        help: "Move an entity (default: the player; for characters pos = feet).",
        args: &[arg("id", "integer", "entity id (default player)"), arg("pos", "array", "[x, y, z]")],
        run: t_teleport,
    },
    Tool {
        name: "rewind",
        help: "Go back in time: ticks=N back, or to tick=T. Starts a new timeline.",
        args: &[arg("ticks", "integer", "ticks back"), arg("tick", "integer", "absolute tick")],
        run: t_rewind,
    },
    Tool {
        name: "snapshot_save",
        help: "Save the full state to a file.",
        args: &[arg("path", "string", "file path")],
        run: t_snap_save,
    },
    Tool { name: "snapshot_load", help: "Load a state file.", args: &[arg("path", "string", "file path")], run: t_snap_load },
    Tool {
        name: "record_save",
        help: "Save the inputs since the scene started as a replay file (JSON).",
        args: &[arg("path", "string", "file path")],
        run: t_record_save,
    },
    Tool { name: "rooms", help: "List rooms in the world (key, name, wing, bounds, entrance).", args: &[], run: t_rooms },
    Tool {
        name: "room",
        help: "Info card of a room (default: the one the player is in).",
        args: &[arg("key", "string", "room key")],
        run: t_room,
    },
    Tool {
        name: "guide",
        help: "The field guide: words the station guides use (term=, search=), or every room's ask-for-it phrases (asks=true).",
        args: &[
            arg("term", "string", "a word, e.g. bloom"),
            arg("search", "string", "find words mentioning this"),
            arg("asks", "boolean", "every room's ask-for-it phrases"),
        ],
        run: t_guide,
    },
    Tool {
        name: "goto",
        help: "Teleport the player into a room (room=key), to the plaza (room=hub) or to a position.",
        args: &[arg("room", "string", "room key or 'hub'"), arg("pos", "array", "[x, y, z] feet position")],
        run: t_goto,
    },
    Tool {
        name: "room_reset",
        help: "Rebuild a room from its definition (default: current room).",
        args: &[arg("key", "string", "room key")],
        run: t_room_reset,
    },
    Tool {
        name: "room_check",
        help: "Validate a room file without loading it. Returns errors with line/column.",
        args: &[arg("path", "string", "path to a .toml room file")],
        run: t_room_check,
    },
    Tool {
        name: "room_reload",
        help: "Re-read room files from the rooms directory and rebuild changed rooms (hot reload).",
        args: &[],
        run: t_room_reload,
    },
    Tool {
        name: "stream",
        help: "Streaming state: active/dormant regions. point=[x,y,z] adds an interest point, clear=true removes them.",
        args: &[arg("point", "array", "[x, y, z] extra interest point"), arg("clear", "boolean", "remove extra points")],
        run: t_stream,
    },
    Tool {
        name: "filmstrip",
        help: "Capture N frames, `every` ticks apart (optionally driving the player), tiled into one PNG.",
        args: &[
            arg("frames", "integer", "number of frames (default 8)"),
            arg("every", "integer", "ticks between frames (default 10)"),
            arg("columns", "integer", "tiles per row (default 4)"),
            arg("width", "integer", "frame width (default 320)"),
            arg("height", "integer", "frame height (default 180)"),
            arg("move", "array", "[x, z] move direction while recording"),
            arg("hold", "string", "held buttons while recording"),
            arg("press", "string", "buttons pressed at the start"),
            arg("out", "string", "output path"),
        ],
        run: t_filmstrip,
    },
    Tool {
        name: "camera_bench",
        help: "Render the current moment from several camera setups, tiled into one PNG.",
        args: &[
            arg("presets", "string", "comma-separated preset names or tilt angles (default: all presets)"),
            arg("columns", "integer", "tiles per row (default 4)"),
            arg("width", "integer", "tile width (default 320)"),
            arg("height", "integer", "tile height (default 180)"),
            arg("out", "string", "output path"),
        ],
        run: t_camera_bench,
    },
    Tool {
        name: "npcs",
        help: "Non-player characters and creatures: body plan, feet, velocity, brain, steps taken, flinch.",
        args: &[arg("name", "string", "only names starting with this")],
        run: t_npcs,
    },
    Tool {
        name: "signal",
        help: "Send a pad signal (spawners listening for it fire on the next tick), then step 1 tick.",
        args: &[arg("name", "string", "signal name, e.g. drop or clear")],
        run: t_signal,
    },
    Tool {
        name: "course",
        help: "Course state: current run (timer, gates, hits, falls), last result, best times, checkpoint.",
        args: &[],
        run: t_course,
    },
    Tool {
        name: "feel",
        help: "Feel metrics of the player: response ticks, time to top speed, stopping, turnaround, last jump.",
        args: &[],
        run: t_feel,
    },
    Tool {
        name: "audio_capture",
        help: "Run N ticks (optionally driving the player) and render the game's sounds to a .wav file.",
        args: &[
            arg("ticks", "integer", "ticks to run (default 180)"),
            arg("move", "array", "[x, z] move direction"),
            arg("hold", "string", "held buttons"),
            arg("press", "string", "buttons pressed on the first tick"),
            arg("aim", "array", "[x, y, z] aim point"),
            arg("out", "string", "output .wav path"),
        ],
        run: t_audio_capture,
    },
    Tool {
        name: "replay",
        help: "Rebuild the scene from a replay file, run all its inputs and verify the final state hash.",
        args: &[arg("path", "string", "file path")],
        run: t_replay,
    },
    // ---- Shardfall (game_tools.rs)
    Tool {
        name: "game",
        help: "Shardfall status: the hero (level, life, gold, skills), monsters by family and rarity, the wave.",
        args: &[],
        run: crate::game_tools::t_game,
    },
    Tool {
        name: "hero",
        help: "Change the hero: level, gold, xp to add, a skill on a bar slot (slot=0..5 skill=key), full heal.",
        args: &[
            arg("level", "integer", "set the level"),
            arg("gold", "integer", "set gold"),
            arg("xp", "number", "experience to add"),
            arg("slot", "integer", "bar slot 0-5 (with skill)"),
            arg("skill", "string", "skill key for the slot"),
            arg("heal", "boolean", "refill life, mana and potions"),
        ],
        run: crate::game_tools::t_hero,
    },
    Tool {
        name: "monster",
        help: "Spawn monsters of a family near the hero (or at pos). Lists families with no arguments.",
        args: &[
            arg("family", "string", "family key (game/monsters.toml)"),
            arg("level", "integer", "monster level (default: the hero's)"),
            arg("rarity", "string", "normal | magic | rare | unique"),
            arg("count", "integer", "how many (default 1)"),
            arg("pos", "array", "[x, y, z] feet position (default: 6 m in front of the hero)"),
            arg("aggro", "boolean", "start hunting the hero (default true)"),
        ],
        run: crate::game_tools::t_monster,
    },
    Tool {
        name: "autoplay",
        help: "Let a bot play for N seconds (fight, skills, potions, dodge telegraphs); reports kills, deaths, damage taken, xp, gold.",
        args: &[
            arg("seconds", "number", "how long (default 30)"),
            arg("goal", "array", "[x, y, z] to walk to when no monsters are near"),
        ],
        run: crate::game_tools::t_autoplay,
    },
    Tool {
        name: "skills",
        help: "Every skill (hero and monster) with its numbers, from game/skills.toml.",
        args: &[arg("key", "string", "only this skill")],
        run: crate::game_tools::t_skills,
    },
    Tool {
        name: "game_reload",
        help: "Re-read the game data folder (./game or $PAV_GAME) for live editing; reports errors.",
        args: &[],
        run: crate::game_tools::t_game_reload,
    },
    Tool {
        name: "loot_roll",
        help: "Roll items without playing (item level, rarity, slot, count): full tooltips, or a summary of many rolls (rarity spread, affix frequency, value ranges) to check loot tables.",
        args: &[
            arg("level", "integer", "item level (default the hero's or 10)"),
            arg("rarity", "string", "normal | magic | rare | unique (default: rolled)"),
            arg("slot", "string", "weapon | offhand | helmet | body | gloves | boots | belt | amulet | ring"),
            arg("count", "integer", "how many (default 5; more than 20 gives a summary)"),
            arg("bonus", "number", "item rarity bonus in percent"),
            arg("seed", "integer", "random seed (default 1)"),
        ],
        run: crate::game_tools::t_loot_roll,
    },
    Tool {
        name: "give",
        help: "Give the hero an item: a unique by key, or a rolled item (level, rarity, slot); optionally wear it at once.",
        args: &[
            arg("unique", "string", "unique key (game/uniques.toml)"),
            arg("base", "string", "item base key (game/items.toml) for a plain item"),
            arg("level", "integer", "item level (default the hero's)"),
            arg("rarity", "string", "normal | magic | rare | unique"),
            arg("slot", "string", "item slot"),
            arg("equip", "boolean", "wear it now"),
        ],
        run: crate::game_tools::t_give,
    },
    Tool {
        name: "inventory",
        help: "The hero's worn gear, bag and stash with every item described, gold, and the vendor's wares in town.",
        args: &[arg("brief", "boolean", "names only")],
        run: crate::game_tools::t_inventory,
    },
    Tool {
        name: "game_cmd",
        help: "Do a menu action as the player would (it rides in the input frame, so replays include it): pickup|equip|unequip|drop|sell|buy|stash|take|sell_all|bar|travel|use|auto_loot|sort with id/slot/skill/place/spot.",
        args: &[
            arg("do", "string", "the action"),
            arg("id", "integer", "item id (equip, drop, sell, buy, stash, take, pickup)"),
            arg(
                "slot",
                "string",
                "equip slot (weapon, offhand, helmet, body, gloves, boots, belt, amulet, ring1, ring2) or bar slot 0-5",
            ),
            arg("skill", "string", "skill key for bar"),
            arg("place", "string", "town | arena | lab | level for travel"),
            arg("depth", "integer", "depth for place=level"),
            arg("spot", "integer", "spot index for use (the way down, a cursed chest)"),
            arg("rarity", "integer", "sell_all up to / auto_loot from this rarity (0 normal .. 3 unique, 4 off)"),
        ],
        run: crate::game_tools::t_game_cmd,
    },
    Tool {
        name: "tree_map",
        help: "Draw the passive tree (and the first Astral rings) to a PNG: sectors coloured, notables/keystones/masteries ringed, skill nodes blue-ringed, the hero's allocation highlighted.",
        args: &[
            arg("size", "integer", "image size in pixels (default 1200)"),
            arg("rings", "integer", "Astral rings to include (default 2)"),
            arg("out", "string", "PNG path (default out/tree.png)"),
        ],
        run: crate::game_tools::t_tree_map,
    },
    Tool {
        name: "tree",
        help: "The hero's passive tree: points, what the allocation gives; find nodes (find=fire or find=keystone), take=NAME allocates the shortest path to a node, refund=, respec=true, mastery=NAME option=N.",
        args: &[
            arg("find", "string", "search node names and effects ('keystone' lists keystones)"),
            arg("take", "string", "node name or id: allocate the path to it"),
            arg("refund", "string", "node name or id to refund (gold)"),
            arg("respec", "boolean", "reset the whole tree (gold)"),
            arg("mastery", "string", "allocated mastery node name or id"),
            arg("option", "integer", "mastery option index"),
            arg("astral", "boolean", "include the endless rings in find"),
        ],
        run: crate::game_tools::t_tree,
    },
    Tool {
        name: "genome",
        help: "Grow a creature from a seed (Spore-style genome: body, parts, palette, archetype, skills, name); body=/archetype=/element= fix those; spawn=true puts it in the game.",
        args: &[
            arg("seed", "integer", "genome seed"),
            arg("level", "integer", "monster level (default 10)"),
            arg("body", "string", "biped | spider | lizard | beetle | blob"),
            arg("archetype", "string", "brute stalker spitter charger caster swarm bomber summoner tank"),
            arg("element", "string", "physical | fire | cold | lightning | poison"),
            arg(
                "parts",
                "string",
                "parts instead of rolled ones: horns antlers spikes crest tusks mandibles plates eyes orbs wings",
            ),
            arg("spawn", "boolean", "spawn it near the hero"),
        ],
        run: crate::game_tools::t_genome,
    },
    Tool {
        name: "bestiary",
        help: "Generate many genomes and summarise the spread (bodies, archetypes, elements, parts, distinct names) with examples: checks the generator's variety.",
        args: &[
            arg("count", "integer", "how many (default 20)"),
            arg("seed", "integer", "first seed"),
            arg("level", "integer", "level"),
            arg("body", "string", "fix the body"),
            arg("archetype", "string", "fix the archetype"),
            arg("element", "string", "fix the element"),
        ],
        run: crate::game_tools::t_bestiary,
    },
    Tool {
        name: "boss",
        help: "List the designed bosses, or spawn one (key=hollow_king) or a generated one (seed=N) near the hero.",
        args: &[
            arg("key", "string", "boss key"),
            arg("seed", "integer", "generated boss seed"),
            arg("level", "integer", "level"),
        ],
        run: crate::game_tools::t_boss,
    },
    Tool {
        name: "turntable",
        help: "Render a creature alone from several angles into one PNG: seed= (genome, with body/archetype/element), family=, boss=, or the hero; def= lays JSON puppet fields over it (author a creature: colours, proportions, parts, body plan). Reports its anatomy (height, length, parts, colours).",
        args: &[
            arg("seed", "integer", "genome seed"),
            arg("family", "string", "designed family"),
            arg("boss", "string", "boss key or gen:N"),
            arg("body", "string", "genome body"),
            arg("archetype", "string", "genome archetype"),
            arg("element", "string", "genome element"),
            arg("parts", "string", "genome parts, comma separated (horns,wings,...)"),
            arg(
                "def",
                "object",
                "puppet fields to lay over it, e.g. {\"parts\": [{\"kind\": \"wings\"}], \"shirt\": \"#3050a0\"}",
            ),
            arg("angles", "integer", "views (default 8)"),
            arg("size", "integer", "pixels per view (default 256)"),
            arg("out", "string", "PNG path"),
        ],
        run: crate::game_tools::t_turntable,
    },
    Tool {
        name: "animsheet",
        help: "Render a creature (the hero by default) through an action or a motion clip frame by frame into one PNG: skill= (or its first skill), any move= from anim/moves.toml, or clip= from the clip library.",
        args: &[
            arg("seed", "integer", "genome seed"),
            arg("family", "string", "designed family"),
            arg("boss", "string", "boss key"),
            arg("skill", "string", "skill to perform"),
            arg("move", "string", "any move by name (anim/moves.toml), at its own timing"),
            arg("hit", "number", "where the hit lands, 0..1 (move=)"),
            arg("side", "number", "-1 plays the move's alternate swing"),
            arg("clip", "string", "a motion clip (SET/Clip or Clip): played start to end"),
            arg("mirror", "boolean", "clip= mirrored left to right"),
            arg("upper", "boolean", "clip= on the upper body only"),
            arg("travel", "boolean", "clip= moving the body as it travels"),
            arg("def", "object", "puppet fields over the subject (JSON)"),
            arg("frames", "integer", "frames (default 8)"),
            arg("size", "integer", "pixels per frame"),
            arg("out", "string", "PNG path"),
        ],
        run: crate::game_tools::t_animsheet,
    },
    Tool {
        name: "clips",
        help: "The motion clip library (anim/*.json: animation translated from open libraries, as readable key poses): no args lists the sets; find=WORDS searches names, tags and descriptions; name=SET/Clip shows one clip as readable text with its source and license; set=NAME lists a set; load=FOLDER adds an on-disk library (load=cmu: every take of the CMU database; load=100style: its runs, backward, sideways and idle loops; load=local: what you translated from libraries that may not be shared). Play one with animsheet clip=.",
        args: &[
            arg("find", "string", "words to search for"),
            arg("name", "string", "SET/Clip (or Clip) to show"),
            arg("set", "string", "a set to list"),
            arg("load", "string", "an on-disk folder of sets under anim/ to add (cmu, 100style, local)"),
            arg("limit", "integer", "results (default 40)"),
        ],
        run: crate::anim_tools::t_clips,
    },
    Tool {
        name: "clip_import",
        help: "Translate an animation library into a clip set (anim/<set>.json: readable key poses fitted within tol mm of the capture, with credits and each clip's fit). from= .glb libraries (Rigify, Unreal-style or Mixamo rigs; comma separated, sources= their ids), .fbx files (binary: every animation stack a clip), .bvh takes (100STYLE, MotionBuilder (Mixamo, LaFAN1), Bandai Namco, Unreal or Rigify skeletons; at=FROM-TO, loop=true cuts the best cycle, way= the way it goes), or set files (my-3D2dge .js, readable .json) or a folder of them. catalog= (anim/catalogs/*.json) tags, describes and credits each clip; one with \"$pick\" and no from= cuts its moments out of a capture database (CMU, 100STYLE, Bandai Namco or LaFAN1, downloading the takes; the last two, non-commercial, into anim/local). list=true shows a file's rig, clips or joints.",
        args: &[
            arg("from", "string", "files (comma separated) or a folder of sets"),
            arg("catalog", "string", "catalog JSON (tags, descriptions, sources, skips, picks)"),
            arg("set", "string", "the set's name (MESH2MOTION, CMU ...)"),
            arg("sources", "string", "each library's source id, comma separated"),
            arg("rest", "string", "each library's rest-pose clip, comma separated"),
            arg(
                "rm",
                "string",
                "each .glb's root-motion twin, comma separated (loops take their speed from it; clips that travel are added as <clip>_RM)",
            ),
            arg("title", "string", "set title"),
            arg("credit", "string", "credit line (default: the catalog's sources and licenses)"),
            arg("clips", "string", "only these clips, comma separated"),
            arg("tol", "number", "key-pose budget in mm (default 30)"),
            arg("fps", "number", "sampling rate (default 30)"),
            arg("blade", "string", "clip names holding these keep a blade direction (default Sword)"),
            arg("at", "string", "a .bvh take's stretch, FROM-TO seconds"),
            arg("loop", "boolean", "a .bvh take loops: cut at its best cycle"),
            arg("min_cycle", "number", "a loop's shortest cycle in seconds (default 0.5)"),
            arg("way", "string", "a .bvh loop's stretch goes forward, back, left or right of where the hips face"),
            arg("log", "integer", "notes to show (default 60)"),
            arg("name", "string", "the clip's name (one .bvh take)"),
            arg("units", "number", "metres per .bvh unit (default: found from the legs)"),
            arg("list", "boolean", "show the file's rig and clips; write nothing"),
            arg("out", "string", "output file (or folder for a folder)"),
            arg("ledger", "string", "a takes ledger (.tsv) to copy beside a folder"),
        ],
        run: crate::anim_tools::t_clip_import,
    },
    Tool {
        name: "mocap",
        help: "Open motion-capture databases, translated on demand: no args describes them (CMU: 2,548 takes, free for all uses; 100STYLE: 100 walking and running styles, CC BY 4.0; Bandai Namco and LaFAN1, non-commercial: into anim/local; Mesh2Motion; Quaternius; Mixamo). find=WORDS searches takes (lib=cmu|100style); get=IDS downloads them (02_01, Zombie_FW, mesh2motion; cmu: the whole database); cut=TAKE fits one moment into a set: at=FROM-TO seconds, loop=true cuts the best cycle, name=, set= (default MOCAP, created or added to), tags=, desc=. survey=ledger measures every downloaded CMU take into anim/cmu/takes.tsv; survey=library translates them, a set a subject, into anim/cmu (subjects=5,13 for some).",
        args: &[
            arg("find", "string", "words to search takes for"),
            arg("lib", "string", "cmu, 100style or all (find)"),
            arg("get", "string", "takes to download, comma separated (cmu: every CMU take)"),
            arg("survey", "string", "ledger or library: the whole CMU database measured, or translated a set a subject"),
            arg("subjects", "string", "CMU subjects to survey, comma separated (default all)"),
            arg("cut", "string", "a take to cut a moment from (13_17, Zombie_FW)"),
            arg("at", "string", "FROM-TO seconds (default: where the take moves)"),
            arg("loop", "boolean", "cut at the best cycle and play in place"),
            arg("min_cycle", "number", "a loop's shortest cycle in seconds (a limp: 0.9)"),
            arg(
                "way",
                "string",
                "a loop's stretch goes forward, back, left or right of where the hips face (a sidestep take: one loop each way)",
            ),
            arg("name", "string", "the clip's name"),
            arg("set", "string", "the set it goes in (default MOCAP)"),
            arg("tags", "string", "tags, space separated"),
            arg("desc", "string", "what the body does"),
            arg("tol", "number", "key-pose budget in mm (default 30; survey=library 50)"),
            arg("limit", "integer", "results (default 30)"),
            arg("out", "string", "set file (default anim/<set>.json)"),
        ],
        run: crate::anim_tools::t_mocap,
    },
    Tool {
        name: "anim_reload",
        help: "Re-read anim/ from disk: the moves table (moves.toml) and the clip sets. Invalid files keep the old data and report the error.",
        args: &[],
        run: crate::anim_tools::t_anim_reload,
    },
    Tool {
        name: "anim_edit",
        help: "Create, copy, inspect, and edit readable animation clips. Edits are validated, saved to WORKSHOP (or WORKSHOP_LOCAL for local sources), and shown in the live preview. Source clips and credits are preserved. Use inspect first, then pass if_revision from its result to prevent stale edits. key changes only the specified pose channels. Undo and redo retain up to 32 changes. All actions work through the CLI, MCP, and live bridge.",
        args: &[
            arg("action", "string", "create | copy | inspect | key | delete_key | replace | retime | mirror | undo | redo"),
            arg("name", "string", "clip name, or WORKSHOP/Name (inspect also accepts any SET/Clip)"),
            arg("from", "string", "source SET/Clip for copy"),
            arg("duration", "number", "new clip duration in seconds (create)"),
            arg("loop", "boolean", "repeat the new clip (create)"),
            arg("time", "number", "key time in seconds (key or delete_key)"),
            arg("pose", "object", "key channels to change, e.g. {\"armR\":[80,20,50,25,0]} (key)"),
            arg("clip", "object", "complete clip data (replace); source credits are preserved"),
            arg("factor", "number", "duration multiplier (retime); below 1 is faster"),
            arg("if_revision", "string", "expected clip revision from inspect or the last edit"),
            arg("preview", "boolean", "select the accepted edit in the live preview (default true)"),
        ],
        run: crate::animation_tools::t_anim_edit,
    },
    Tool {
        name: "anim_preview",
        help: "View an animation in an isolated studio with the game's renderer. The game stays unchanged and resumes when the preview closes. Select clip=SET/Clip or move=NAME. Control play, pause, time, source-frame steps, speed, repeat, mirror, upper body, and travel. Same-name edits update without resetting the clock or camera. No args returns preview state; action=pose returns the sampled pose. capture and filmstrip show the studio through MCP images.",
        args: &[
            arg("clip", "string", "SET/Clip to view"),
            arg("move", "string", "procedural move to view"),
            arg("playing", "boolean", "true to play, false to pause"),
            arg("time", "number", "seek to this time in seconds"),
            arg("step", "integer", "step forward or backward this many source frames"),
            arg("speed", "number", "playback speed multiplier"),
            arg("repeat", "boolean", "repeat the preview"),
            arg("mirror", "boolean", "swap left and right"),
            arg("upper", "boolean", "apply the clip to the upper body only"),
            arg("travel", "boolean", "show the clip's root motion"),
            arg("side", "number", "procedural move side (-1 or 1)"),
            arg("hit", "number", "procedural move hit point as a fraction from 0 to 1"),
            arg(
                "action",
                "string",
                "status | open | play | pause | restart | pose | close | fit (pose returns joints; fit frames the full motion)",
            ),
            arg("close", "boolean", "close the studio and resume the game"),
        ],
        run: crate::preview_tools::t_anim_preview,
    },
    Tool {
        name: "level",
        help: "Shardfall levels: with depth= what that depth is (name, theme colours, mechanics, boss, monster level; to= for a range); with no args the level being played (features by kind with positions and state, spots, rooms seen, exit).",
        args: &[
            arg("depth", "integer", "a depth to describe (1-12 designed, then endless)"),
            arg("to", "integer", "describe depth..to"),
        ],
        run: crate::game_tools::t_level,
    },
    Tool {
        name: "levelmap",
        help: "Top-down PNG map of a level: the current one, or a fresh one built for depth= (and seed=). Rooms, corridors, every mechanic's pieces, monster packs, the boss, exit and portal.",
        args: &[
            arg("depth", "integer", "build this depth (default: the level being played)"),
            arg("seed", "integer", "layout seed for depth= (default 1)"),
            arg("size", "integer", "image size in pixels (default 900)"),
            arg("nav", "boolean", "overlay the navigation grid and the planned way to the exit"),
            arg("out", "string", "PNG path"),
        ],
        run: crate::game_tools::t_levelmap,
    },
    Tool {
        name: "go",
        help: "Travel at once: place=town|arena|lab|level (depth= for levels; unlocks the waypoint).",
        args: &[arg("place", "string", "town | arena | lab | level"), arg("depth", "integer", "depth for place=level")],
        run: crate::game_tools::t_go,
    },
    Tool {
        name: "goto_feature",
        help: "Put the hero next to a level feature: kind=shrine|keg|spikes|gate|wind|totem|lava|ice|crumble|well|chest|bubble|exit|portal (n= picks which one).",
        args: &[
            arg("kind", "string", "feature kind"),
            arg("n", "integer", "which one (default 0)"),
            arg("offset", "array", "[x, y, z] from it (default [0, 0, 3])"),
        ],
        run: crate::game_tools::t_goto_feature,
    },
    // ---- agent tools for building and judging content (agent_tools.rs)
    Tool {
        name: "see",
        help: "Screenshot with numbered marks on what matters (monsters, hero, townsfolk, loot, spots, level pieces; characters and named objects outside the game) plus a legend: what each number is, its kind, name, rarity, life, distance, world position.",
        args: &[
            arg("width", "integer", "default 1280"),
            arg("height", "integer", "default 720"),
            arg("max", "integer", "at most this many marks, nearest first (default 40)"),
            arg(
                "only",
                "string",
                "comma list of kinds: monster,boss,hero,townsfolk,loot,spot,feature,totem,keg,character,object",
            ),
            arg("out", "string", "PNG path"),
        ],
        run: crate::agent_tools::t_see,
    },
    Tool {
        name: "campaign",
        help: "Balance pass: the bot plays down through the levels (from= a depth, to= the last) for up to seconds= of game time and reports each finished level: minutes, kills, deaths, damage taken, hero level in/out, gold.",
        args: &[
            arg("from", "integer", "start at this depth (default: where the hero is)"),
            arg("to", "integer", "stop after this depth (default 12)"),
            arg("seconds", "number", "game seconds at most (default 1800)"),
            arg("hero_level", "integer", "set the hero's level first"),
            arg("wall", "number", "stop after this many real seconds and report (default 300)"),
        ],
        run: crate::agent_tools::t_campaign,
    },
    Tool {
        name: "theme_swatch",
        help: "PNG of every level theme's palette (floor, wall, pillar, accent, light, sky); depths=13-20 adds the blended palettes of those endless depths.",
        args: &[arg("depths", "string", "a range of endless depths, e.g. 13-20"), arg("out", "string", "PNG path")],
        run: crate::agent_tools::t_theme_swatch,
    },
    Tool {
        name: "look",
        help: "Looks and filter presets (the game's Look & Filters menu): name= a whole look, section= with preset= / on= (all, objects = characters and objects, environment) / enabled=, reset=true, compare=0.5 (filters only on the right). bench=all or a comma list of looks renders this moment under each, numbered. No args lists sections, presets and looks.",
        args: &[
            arg("name", "string", "a whole look (e.g. Pixel heroes, HD-2D, Game Boy world)"),
            arg("section", "string", "pixel, shading, outlines, palette, grading, scanlines, grain, screen, glow"),
            arg("preset", "string", "a preset of that section (e.g. Chunky, Ink, Game Boy, Noir)"),
            arg("on", "string", "the part of the scene that section's filter is on: all, objects, environment"),
            arg("enabled", "boolean", "switch that section on (true) or back to the scene's own settings (false)"),
            arg("reset", "boolean", "switch every section off"),
            arg("compare", "number", "filters only right of this screen fraction (0 = whole screen)"),
            arg("bench", "string", "all, or a comma list of looks (plus scene, current) to render side by side"),
            arg("columns", "integer", "bench tiles per row (default 3)"),
            arg("width", "integer", "bench tile width (default 400)"),
            arg("height", "integer", "bench tile height (default 225)"),
            arg("out", "string", "bench PNG path"),
        ],
        run: crate::agent_tools::t_look,
    },
];

pub fn find(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.name == name)
}

pub fn call(session: &mut Session, name: &str, args: &Args) -> Result<Output> {
    let tool = find(name).ok_or_else(|| anyhow!("unknown tool '{name}' (try `help`)"))?;
    (tool.run)(session, args)
}

/// JSON schema for a tool's arguments (for MCP).
pub fn schema(t: &Tool) -> Value {
    let mut props = Map::new();
    for a in t.args {
        props.insert(a.name.into(), json!({ "type": a.kind, "description": a.help }));
    }
    json!({ "type": "object", "properties": props })
}

pub(crate) fn get_u64(a: &Args, k: &str, default: u64) -> Result<u64> {
    match a.get(k) {
        None => Ok(default),
        Some(v) => v
            .as_u64()
            .or_else(|| v.as_f64().map(|f| f as u64))
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            .ok_or_else(|| anyhow!("argument '{k}' must be a non-negative integer")),
    }
}

pub(crate) fn get_str<'a>(a: &'a Args, k: &str) -> Option<&'a str> {
    a.get(k).and_then(|v| v.as_str())
}

pub(crate) fn get_f32(a: &Args, k: &str, default: f32) -> Result<f32> {
    match a.get(k) {
        None => Ok(default),
        Some(v) => v
            .as_f64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            .map(|f| f as f32)
            .ok_or_else(|| anyhow!("argument '{k}' must be a number")),
    }
}

pub(crate) fn get_bool(a: &Args, k: &str, default: bool) -> Result<bool> {
    match a.get(k) {
        None => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        Some(v) => match v.as_str().or_else(|| v.as_u64().map(|n| if n == 0 { "false" } else { "true" })) {
            Some("true" | "yes" | "1" | "on") => Ok(true),
            Some("false" | "no" | "0" | "off") => Ok(false),
            _ => Err(anyhow!("argument '{k}' must be true or false")),
        },
    }
}

fn t_scenes(_: &mut Session, _: &Args) -> Result<Output> {
    Ok(Output::Json(json!({
        "scenes": pav_core::scenes::SCENES.iter().map(|(n, d)| json!({"name": n, "about": d})).collect::<Vec<_>>(),
        "standalone_rooms": pav_core::scenes::names().into_iter().filter(|n| !pav_core::scenes::SCENES.iter().any(|s| s.0 == n)).collect::<Vec<_>>(),
    })))
}

fn t_load(s: &mut Session, a: &Args) -> Result<Output> {
    let scene = get_str(a, "scene").unwrap_or("test");
    let seed = get_u64(a, "seed", 1)?;
    let gpu = s.gpu.take();
    // Keep the caller's camera and view settings, minus any room's own view table.
    let (camera, view) = (s.camera_base_or_current(), s.view_base_or_current());
    let live = s.live.then(|| s.sim.config.clone());
    let look = std::mem::take(&mut s.look);
    *s = Session::new(scene, seed)?;
    s.gpu = gpu;
    s.look = look;
    if let Some(config) = live {
        // Inside the game: keep its tuning, and leave room cameras and views to it.
        s.sim.config = config;
        s.live = true;
    }
    s.reset_view(view);
    s.reset_camera(camera);
    t_status(s, a)
}

fn t_step(s: &mut Session, a: &Args) -> Result<Output> {
    let n = get_u64(a, "ticks", 1)?;
    let t = Instant::now();
    s.step(n);
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let mut st = status(s);
    st["elapsed_ms"] = json!((ms * 100.0).round() / 100.0);
    Ok(Output::Json(st))
}

fn status(s: &Session) -> Value {
    json!({
        "scene": s.sim.state.scene,
        "tick": s.sim.state.tick,
        "time": (s.sim.time() * 1000.0).round() / 1000.0,
        "tick_rate": s.sim.config.tick_rate.hz(),
        "entities": s.sim.state.entities.len(),
        "blocks": s.sim.state.statics.block_count(),
        "projectiles": s.sim.state.projectiles.list.len(),
        "hash": format!("{:016x}", s.sim.state_hash()),
    })
}

fn t_status(s: &mut Session, _: &Args) -> Result<Output> {
    Ok(Output::Json(status(s)))
}

fn t_entities(s: &mut Session, a: &Args) -> Result<Output> {
    let filter = get_str(a, "name").unwrap_or("");
    let list: Vec<Value> = s
        .sim
        .state
        .entities
        .iter()
        .filter(|e| e.name.contains(filter))
        .map(|e| {
            let p = e.pos;
            json!({"id": e.id.0, "name": e.name, "pos": [round3(p.x), round3(p.y), round3(p.z)], "body": e.body_kind})
        })
        .collect();
    Ok(Output::Json(json!(list)))
}

pub(crate) fn round3(x: f32) -> f64 {
    (x as f64 * 1000.0).round() / 1000.0
}

fn t_params(s: &mut Session, a: &Args) -> Result<Output> {
    let prefix = get_str(a, "prefix").unwrap_or("").to_string();
    let list: Vec<_> = params::list(&mut s.params()).into_iter().filter(|p| p.path.starts_with(&prefix)).collect();
    Ok(Output::Json(serde_json::to_value(list)?))
}

/// Parses CLI text into the most natural JSON value.
pub fn parse_value(v: &Value) -> ParamValue {
    match v {
        Value::Bool(b) => ParamValue::Bool(*b),
        Value::Number(n) => ParamValue::Float(n.as_f64().unwrap_or(0.0)),
        Value::String(t) => match serde_json::from_str::<Value>(t) {
            Ok(Value::Bool(b)) => ParamValue::Bool(b),
            Ok(Value::Number(n)) => ParamValue::Float(n.as_f64().unwrap_or(0.0)),
            _ => ParamValue::Text(t.clone()),
        },
        other => ParamValue::Text(other.to_string()),
    }
}

fn t_set(s: &mut Session, a: &Args) -> Result<Output> {
    let path = get_str(a, "path").context("missing 'path'")?.to_string();
    let value = a.get("value").context("missing 'value'")?;
    params::set(&mut s.params(), &path, parse_value(value)).map_err(|e| anyhow!(e))?;
    let now = params::get(&mut s.params(), &path);
    Ok(Output::Json(json!({ "path": path, "value": now })))
}

fn t_camera(s: &mut Session, a: &Args) -> Result<Output> {
    let presets = pav_view::CameraParams::PRESETS;
    match get_str(a, "preset") {
        None => Ok(Output::Json(json!(presets.iter().map(|p| p.0).collect::<Vec<_>>()))),
        Some(name) => {
            let p = presets.iter().find(|p| p.0.starts_with(name)).ok_or_else(|| anyhow!("unknown preset '{name}'"))?;
            s.camera.params = (p.1)();
            Ok(Output::Json(json!({ "preset": p.0 })))
        }
    }
}

fn t_capture(s: &mut Session, a: &Args) -> Result<Output> {
    let w = get_u64(a, "width", 960)? as u32;
    let h = get_u64(a, "height", 540)? as u32;
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        bail!("width/height must be 1..8192");
    }
    let path = PathBuf::from(get_str(a, "out").map(String::from).unwrap_or(format!("out/capture-{}.png", s.sim.state.tick)));
    let t = Instant::now();
    let rgba = s.render(w, h)?;
    let png = pav_render::capture::encode_png(w, h, &rgba)?;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, &png)?;
    let meta = json!({ "path": path, "width": w, "height": h, "tick": s.sim.state.tick, "render_ms": t.elapsed().as_millis() });
    Ok(Output::Image { png, path: Some(path), meta })
}

fn t_bench(s: &mut Session, a: &Args) -> Result<Output> {
    let n = get_u64(a, "ticks", 600)?.max(1);
    let t = Instant::now();
    s.step(n);
    let secs = t.elapsed().as_secs_f64();
    Ok(Output::Json(json!({
        "ticks": n,
        "seconds": (secs * 1000.0).round() / 1000.0,
        "ticks_per_second": (n as f64 / secs).round(),
        "realtime_factor": ((n as f64 / s.sim.config.tick_rate.hz() as f64) / secs * 10.0).round() / 10.0,
        "entities": s.sim.state.entities.len(),
    })))
}

fn t_gpu(s: &mut Session, _: &Args) -> Result<Output> {
    let g = s.gpu()?;
    Ok(Output::Json(json!({ "adapter": pav_render::gpu::describe(&g.headless.adapter.get_info()) })))
}

pub(crate) fn vec_arg(a: &Args, k: &str) -> Result<Option<Vec<f32>>> {
    match a.get(k) {
        None => Ok(None),
        Some(Value::Array(v)) => Ok(Some(v.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect())),
        Some(Value::String(t)) => Ok(Some(
            t.trim_matches(|c| c == '[' || c == ']')
                .split(',')
                .map(|x| x.trim().parse::<f32>())
                .collect::<Result<_, _>>()
                .map_err(|_| anyhow!("argument '{k}' must be numbers like [1, 2]"))?,
        )),
        Some(Value::Number(n)) => Ok(Some(vec![n.as_f64().unwrap_or(0.0) as f32])),
        _ => bail!("argument '{k}' must be an array of numbers"),
    }
}

fn buttons_arg(a: &Args, k: &str) -> Result<u32> {
    let Some(t) = get_str(a, k) else { return Ok(0) };
    let mut b = 0;
    for name in t.split(',').map(str::trim).filter(|n| !n.is_empty()) {
        b |= pav_core::input::buttons::from_name(name).ok_or_else(|| anyhow!("unknown button '{name}'"))?;
    }
    Ok(b)
}

pub fn player_json(s: &Session) -> Value {
    let Some(p) = s.sim.player() else { return json!(null) };
    let ch = p.character.as_ref().unwrap();
    let feet = p.pos - glam::Vec3::Y * ch.height() * 0.5;
    // The vehicle being driven: its name, speed and heading (0 = +Z/south, 90 = +X/east).
    let riding = ch.riding.and_then(|id| s.sim.state.entities.get(id)).map(|v| {
        let body = v.body.and_then(|b| s.sim.state.physics.bodies.get(b));
        let vel = body.map(|b| b.linvel()).unwrap_or_default();
        let fwd = body.map(|b| *b.rotation() * glam::Vec3::Z).unwrap_or(glam::Vec3::Z);
        json!({
            "id": v.id.0,
            "name": v.name,
            "pos": [round3(v.pos.x), round3(v.pos.y), round3(v.pos.z)],
            "vel": [round3(vel.x), round3(vel.y), round3(vel.z)],
            "speed": round3(vel.length()),
            "heading_deg": round3(fwd.x.atan2(fwd.z).to_degrees()),
        })
    });
    json!({
        "riding": riding,
        "id": p.id.0,
        "feet": [round3(feet.x), round3(feet.y), round3(feet.z)],
        "vel": [round3(ch.vel.x), round3(ch.vel.y), round3(ch.vel.z)],
        "grounded": ch.grounded,
        "posture": pav_core::params::ChoiceParam::name(ch.posture),
        "climbing": ch.climbing.is_some(),
        "hanging": ch.hang.map(|h| h.climb >= 0.0).map(|c| if c { "climbing up" } else { "hanging" }),
        "swimming": ch.swimming,
        "water_depth": round3(ch.water_depth),
        "rolling": ch.roll > 0.0,
        "stunned": ch.stun > 0.0,
        "model": pav_core::params::ChoiceParam::name(s.sim.config.movement.model),
        "facing_deg": round3(ch.facing.to_degrees()),
        "tick": s.sim.state.tick,
    })
}

fn t_player(s: &mut Session, _: &Args) -> Result<Output> {
    Ok(Output::Json(player_json(s)))
}

fn t_input(s: &mut Session, a: &Args) -> Result<Output> {
    let mv = vec_arg(a, "move")?.unwrap_or_default();
    let dir = glam::Vec2::new(mv.first().copied().unwrap_or(0.0), mv.get(1).copied().unwrap_or(0.0));
    let held = buttons_arg(a, "hold")?;
    let press = buttons_arg(a, "press")?;
    let aim = vec_arg(a, "aim")?.filter(|v| v.len() == 3).map(|v| glam::Vec3::new(v[0], v[1], v[2]));
    let n = get_u64(a, "ticks", 1)?.max(1);
    for i in 0..n {
        let f = pav_core::InputFrame {
            move_dir: dir.clamp_length_max(1.0),
            vertical: 0.0,
            aim,
            held: held | if i == 0 { press } else { 0 },
            pressed: if i == 0 { press } else { 0 },
            cmd: None,
        };
        s.sim.step(&f);
        s.sync_camera();
    }
    s.keep_events();
    Ok(Output::Json(player_json(s)))
}

fn t_spawn(s: &mut Session, a: &Args) -> Result<Output> {
    use pav_core::{BodyKind, Color, Shape, Spawn, Visual};
    let size = vec_arg(a, "size")?.unwrap_or_default();
    let g = |i: usize, d: f32| size.get(i).copied().unwrap_or(d);
    let shape = match get_str(a, "shape").unwrap_or("box") {
        "box" => Shape::Box { half: glam::Vec3::new(g(0, 0.4), g(1, g(0, 0.4)), g(2, g(0, 0.4))) },
        "rounded_box" => Shape::RoundedBox { half: glam::Vec3::new(g(0, 0.4), g(1, g(0, 0.4)), g(2, g(0, 0.4))), radius: 0.1 },
        "sphere" => Shape::Sphere { radius: g(0, 0.4) },
        "capsule" => Shape::Capsule { half_height: g(0, 0.3), radius: g(1, 0.25) },
        "cylinder" => Shape::Cylinder { half_height: g(0, 0.4), radius: g(1, 0.3) },
        other => bail!("unknown shape '{other}'"),
    };
    let pos = vec_arg(a, "pos")?.filter(|v| v.len() == 3).map(|v| glam::Vec3::new(v[0], v[1], v[2]));
    let pos = pos.unwrap_or_else(|| s.sim.state.focus + glam::Vec3::new(0.0, 3.0, 0.0));
    let body = match get_str(a, "body").unwrap_or("dynamic") {
        "dynamic" => BodyKind::Dynamic,
        "fixed" => BodyKind::Fixed,
        "kinematic" => BodyKind::Kinematic,
        "none" => BodyKind::None,
        other => bail!("unknown body '{other}'"),
    };
    let color = Color::try_hex(get_str(a, "color").unwrap_or("#e8704a")).context("color must be #rrggbb")?;
    let id = s.sim.spawn(Spawn::new(get_str(a, "name").unwrap_or("prop"), pos).visual(Visual::new(shape, color)).body(body));
    Ok(Output::Json(json!({ "id": id.0 })))
}

fn t_despawn(s: &mut Session, a: &Args) -> Result<Output> {
    let id = pav_core::EntityId(get_u64(a, "id", 0)? as u32);
    Ok(Output::Json(json!({ "removed": s.sim.despawn(id) })))
}

fn t_teleport(s: &mut Session, a: &Args) -> Result<Output> {
    let id = match a.get("id") {
        Some(_) => pav_core::EntityId(get_u64(a, "id", 0)? as u32),
        None => s.sim.state.player.context("no player")?,
    };
    let p = vec_arg(a, "pos")?.filter(|v| v.len() == 3).context("pos must be [x, y, z]")?;
    let ok = s.sim.set_position(id, glam::Vec3::new(p[0], p[1], p[2]));
    Ok(Output::Json(json!({ "moved": ok })))
}

fn t_rewind(s: &mut Session, a: &Args) -> Result<Output> {
    let now = s.sim.state.tick;
    let target = match (a.get("tick"), a.get("ticks")) {
        (Some(_), _) => get_u64(a, "tick", now)?,
        (None, Some(_)) => now.saturating_sub(get_u64(a, "ticks", 0)?),
        _ => bail!("give ticks=N (back) or tick=T"),
    };
    let oldest = s.sim.history.oldest_tick().unwrap_or(now);
    if !s.sim.rewind_to(target.max(oldest)) {
        bail!("nothing to rewind to");
    }
    s.sim.commit_rewind();
    let mut st = status(s);
    st["oldest_available"] = json!(oldest);
    Ok(Output::Json(st))
}

fn t_snap_save(s: &mut Session, a: &Args) -> Result<Output> {
    let path = get_str(a, "path").context("missing path")?;
    s.sim.save_state(std::path::Path::new(path))?;
    Ok(Output::Json(json!({ "saved": path, "tick": s.sim.state.tick })))
}

fn t_snap_load(s: &mut Session, a: &Args) -> Result<Output> {
    let path = get_str(a, "path").context("missing path")?;
    s.sim.load_state(std::path::Path::new(path))?;
    Ok(Output::Json(status(s)))
}

fn t_record_save(s: &mut Session, a: &Args) -> Result<Output> {
    let path = get_str(a, "path").context("missing path")?;
    let mut r = s.sim.recording.clone();
    r.params = pav_core::params::to_map(&mut s.params());
    r.final_hash = Some(format!("{:016x}", s.sim.state_hash()));
    if let Some(d) = std::path::Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(path, serde_json::to_string(&r)?)?;
    Ok(Output::Json(json!({ "saved": path, "ticks": r.ticks() })))
}

fn t_replay(s: &mut Session, a: &Args) -> Result<Output> {
    let path = get_str(a, "path").context("missing path")?;
    let r: pav_core::history::Replay = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let gpu = s.gpu.take();
    *s = Session::new(&r.scene, r.seed)?;
    s.gpu = gpu;
    let unknown = pav_core::params::apply_map(&mut s.params(), &r.params);
    for f in r.iter() {
        s.sim.step(f);
    }
    s.keep_events();
    let hash = format!("{:016x}", s.sim.state_hash());
    Ok(Output::Json(json!({
        "ticks": r.ticks(),
        "hash": hash,
        "expected": r.final_hash,
        "matches": r.final_hash.as_deref().map(|h| h == hash),
        "unknown_params": unknown,
    })))
}

fn slot_json(r: &pav_core::world::RoomSlot) -> Value {
    json!({
        "key": r.key,
        "name": r.def.name,
        "wing": r.def.wing,
        "built": r.built,
        "min": [round3(r.min.x), round3(r.min.z)],
        "max": [round3(r.max.x), round3(r.max.z)],
        "inside": [round3(r.inside.x), round3(r.inside.y), round3(r.inside.z)],
        "primary_device": r.def.primary_device,
        "movement_model": r.def.movement_model,
    })
}

fn t_rooms(s: &mut Session, _: &Args) -> Result<Output> {
    let w = &s.sim.state.world;
    Ok(Output::Json(json!({
        "rooms": w.rooms.iter().map(slot_json).collect::<Vec<_>>(),
        "current": w.current_room.and_then(|i| w.rooms.get(i as usize)).map(|r| r.key.clone()),
        "errors": w.errors,
    })))
}

fn t_room(s: &mut Session, a: &Args) -> Result<Output> {
    let w = &s.sim.state.world;
    let slot = match get_str(a, "key") {
        Some(k) => w.room(k).with_context(|| format!("no room '{k}'"))?,
        None => w.current_room.and_then(|i| w.rooms.get(i as usize)).context("the player is not in a room")?,
    };
    let mut v = slot_json(slot);
    v["about"] = json!(slot.def.about);
    v["try"] = json!(slot.def.try_list);
    v["params"] = serde_json::to_value(&slot.def.params)?;
    v["camera"] = serde_json::to_value(&slot.def.camera)?;
    if !slot.def.learn.is_empty() {
        v["learn"] = serde_json::to_value(&slot.def.learn)?;
    }
    let pads: Vec<Value> = slot.def.pads().into_iter().map(|(l, n)| json!({ "label": l, "note": n })).collect();
    if !pads.is_empty() {
        v["pads"] = json!(pads);
    }
    Ok(Output::Json(v))
}

/// The field guide: words (`term=`, `search=`), or every room's "ask for it" phrases.
fn t_guide(_s: &mut Session, a: &Args) -> Result<Output> {
    use pav_core::guide;
    let term_json = |t: &guide::Term| json!({ "key": t.key, "name": t.name, "text": t.text, "see": t.see });
    if let Some(k) = get_str(a, "term") {
        let t = guide::term(k).with_context(|| format!("no term '{k}' (try search=)"))?;
        return Ok(Output::Json(term_json(t)));
    }
    if let Some(q) = get_str(a, "search") {
        return Ok(Output::Json(json!(guide::search(q).into_iter().map(term_json).collect::<Vec<_>>())));
    }
    if a.get("asks").and_then(|v| v.as_bool()).unwrap_or(false) {
        let (defs, _) = pav_core::room::parse_all(&pav_core::room::load_sources(None));
        let rooms: Vec<Value> = defs
            .iter()
            .filter(|(_, d)| !d.learn.ask.is_empty())
            .map(|(k, d)| json!({ "room": k, "name": d.name, "ask": d.learn.ask }))
            .collect();
        return Ok(Output::Json(json!(rooms)));
    }
    Ok(Output::Json(json!({
        "terms": guide::terms().iter().map(|t| t.key.as_str()).collect::<Vec<_>>(),
        "hint": "term=<key> explains one, search=<text> finds some, asks=true lists every room's ask-for-it phrases",
    })))
}

fn t_goto(s: &mut Session, a: &Args) -> Result<Output> {
    if let Some(r) = get_str(a, "room") {
        if r == "hub" {
            let pid = s.sim.state.player.context("no player")?;
            s.sim.set_position(pid, glam::Vec3::new(0.0, 0.0, 6.0));
        } else if !s.sim.teleport_to_room(r) {
            bail!("unknown room '{r}' (or not a world scene; use `load scene=world`)");
        }
    } else if let Some(p) = vec_arg(a, "pos")?.filter(|v| v.len() == 3) {
        let pid = s.sim.state.player.context("no player")?;
        s.sim.state.world.interest.push(glam::Vec3::new(p[0], p[1], p[2]));
        s.sim.update_streaming(usize::MAX);
        s.sim.state.world.interest.pop();
        s.sim.set_position(pid, glam::Vec3::new(p[0], p[1], p[2]));
    } else {
        bail!("give room=<key> or pos=[x,y,z]");
    }
    s.step(2);
    Ok(Output::Json(player_json(s)))
}

fn t_room_reset(s: &mut Session, a: &Args) -> Result<Output> {
    let w = &s.sim.state.world;
    let id = match get_str(a, "key") {
        Some(k) => w.room(k).with_context(|| format!("no room '{k}'"))?.id,
        None => w.current_room.context("the player is not in a room")?,
    };
    s.sim.reset_room(id);
    Ok(Output::Json(json!({ "reset": id })))
}

fn t_room_check(_: &mut Session, a: &Args) -> Result<Output> {
    let path = get_str(a, "path").context("missing path")?;
    let text = std::fs::read_to_string(path)?;
    Ok(Output::Json(match pav_core::room::RoomDef::parse(&text) {
        Ok(d) => json!({ "ok": true, "name": d.name, "size": d.layout.extent(), "objects": d.objects.len() }),
        Err(e) => json!({ "ok": false, "error": e }),
    }))
}

fn t_room_reload(s: &mut Session, _: &Args) -> Result<Output> {
    let dir = pav_core::room::rooms_dir();
    let (defs, errors) = pav_core::room::parse_all(&pav_core::room::load_sources(dir.as_deref()));
    let n = reload_rooms(&mut s.sim, defs);
    Ok(Output::Json(json!({ "dir": dir, "reloaded": n, "errors": errors })))
}

/// Applies freshly parsed room definitions: changed rooms are rebuilt, new/removed rooms
/// trigger a pavilion re-layout. Returns how many rooms changed.
pub fn reload_rooms(sim: &mut pav_core::Sim, defs: Vec<(String, pav_core::room::RoomDef)>) -> usize {
    if !sim.state.world.enabled {
        // Standalone room scene: rebuild it if it changed.
        let Some(slot) = sim.state.world.rooms.first().cloned() else { return 0 };
        if let Some((_, d)) = defs.into_iter().find(|(k, _)| *k == slot.key) {
            if serde_json::to_string(&d).ok() != serde_json::to_string(&*slot.def).ok() {
                let mut fresh = pav_core::Sim::empty(sim.state.seed);
                fresh.config = sim.config.clone();
                pav_core::scenes::build_standalone_room(&mut fresh, &slot.key, d);
                fresh.state.scene = sim.state.scene.clone();
                *sim = fresh;
                return 1;
            }
        }
        return 0;
    }
    let same_set = defs.len() == sim.state.world.rooms.len() && defs.iter().all(|(k, _)| sim.state.world.room(k).is_some());
    if !same_set {
        sim.rebuild_pavilion(defs);
        return 1;
    }
    let mut n = 0;
    for (k, d) in defs {
        let old = sim.state.world.room(&k).map(|r| r.def.clone());
        if let Some(old) = old {
            if serde_json::to_string(&d).ok() != serde_json::to_string(&*old).ok() {
                sim.replace_room(&k, d);
                n += 1;
            }
        }
    }
    n
}

fn t_stream(s: &mut Session, a: &Args) -> Result<Output> {
    if a.get("clear").and_then(|v| v.as_bool()).unwrap_or(false) {
        s.sim.state.world.interest.clear();
    }
    if let Some(p) = vec_arg(a, "point")?.filter(|v| v.len() == 3) {
        s.sim.state.world.interest.push(glam::Vec3::new(p[0], p[1], p[2]));
        s.sim.update_streaming(usize::MAX);
    }
    let st = &s.sim.state.statics;
    let fmt = |k: &pav_core::statics::RegionKey| format!("{k:?}");
    Ok(Output::Json(json!({
        "active": st.chunks.keys().map(fmt).collect::<Vec<_>>(),
        "dormant": st.dormant.keys().map(fmt).collect::<Vec<_>>(),
        "dormant_entities": s.sim.state.world.dormant_entities.values().map(|v| v.len()).sum::<usize>(),
        "interest": s.sim.state.world.interest.iter().map(|p| [p.x, p.y, p.z]).collect::<Vec<_>>(),
    })))
}

fn t_filmstrip(s: &mut Session, a: &Args) -> Result<Output> {
    let frames = get_u64(a, "frames", 8)?.clamp(1, 64) as usize;
    let every = get_u64(a, "every", 10)?.max(1);
    let cols = get_u64(a, "columns", 4)? as u32;
    let w = get_u64(a, "width", 320)? as u32;
    let h = get_u64(a, "height", 180)? as u32;
    let mv = vec_arg(a, "move")?.unwrap_or_default();
    let dir = glam::Vec2::new(mv.first().copied().unwrap_or(0.0), mv.get(1).copied().unwrap_or(0.0));
    let held = buttons_arg(a, "hold")?;
    let press = buttons_arg(a, "press")?;
    let mut shots = Vec::with_capacity(frames);
    for i in 0..frames {
        shots.push(s.render(w, h)?);
        if i + 1 < frames {
            for t in 0..every {
                let first = i == 0 && t == 0;
                let f = pav_core::InputFrame {
                    move_dir: dir.clamp_length_max(1.0),
                    held: held | if first { press } else { 0 },
                    pressed: if first { press } else { 0 },
                    ..Default::default()
                };
                s.sim.step(&f);
                s.sync_camera();
            }
            s.keep_events();
        }
    }
    let (tw, th, px) = pav_render::capture::tile_frames(&shots, w, h, cols);
    let png = pav_render::capture::encode_png(tw, th, &px)?;
    let path = PathBuf::from(get_str(a, "out").map(String::from).unwrap_or(format!("out/filmstrip-{}.png", s.sim.state.tick)));
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, &png)?;
    let meta = json!({ "path": path, "frames": frames, "every_ticks": every, "size": [tw, th], "tick": s.sim.state.tick });
    Ok(Output::Image { png, path: Some(path), meta })
}

fn t_audio_capture(s: &mut Session, a: &Args) -> Result<Output> {
    let n = get_u64(a, "ticks", 180)?.max(1);
    let mv = vec_arg(a, "move")?.unwrap_or_default();
    let dir = glam::Vec2::new(mv.first().copied().unwrap_or(0.0), mv.get(1).copied().unwrap_or(0.0));
    let held = buttons_arg(a, "hold")?;
    let press = buttons_arg(a, "press")?;
    let aim = vec_arg(a, "aim")?.filter(|v| v.len() == 3).map(|v| glam::Vec3::new(v[0], v[1], v[2]));
    s.keep_events();
    let dt = s.sim.dt();
    let mut events = Vec::new();
    for i in 0..n {
        let f = pav_core::InputFrame {
            move_dir: dir.clamp_length_max(1.0),
            aim,
            held: held | if i == 0 { press } else { 0 },
            pressed: if i == 0 { press } else { 0 },
            ..Default::default()
        };
        s.sim.step(&f);
        s.sync_camera();
        for e in s.sim.drain_events() {
            events.push((i as f32 * dt, e));
        }
    }
    let listener = pav_audio::Listener { pos: s.sim.state.focus, right: glam::Vec3::X };
    let duration = n as f32 * dt + 1.5;
    let samples = pav_audio::render_events(&events, duration, 44100.0, &listener);
    let path = PathBuf::from(get_str(a, "out").map(String::from).unwrap_or(format!("out/audio-{}.wav", s.sim.state.tick)));
    pav_audio::write_wav(&path, &samples, 44100)?;
    let peak = samples.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    let kinds: std::collections::BTreeMap<String, usize> = events.iter().fold(Default::default(), |mut m, (_, e)| {
        let k = serde_json::to_value(e)
            .ok()
            .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(String::from))
            .unwrap_or_default();
        *m.entry(k).or_default() += 1;
        m
    });
    Ok(Output::Json(json!({ "path": path, "seconds": duration, "events": kinds, "peak": (peak * 1000.0).round() / 1000.0 })))
}

fn t_course(s: &mut Session, _: &Args) -> Result<Output> {
    let c = &s.sim.state.courses;
    Ok(Output::Json(json!({
        "run": c.hud(s.sim.state.tick, s.sim.dt()),
        "last": c.last,
        "best": c.best,
        "checkpoint": c.checkpoint.map(|(p, _)| [round3(p.x), round3(p.y), round3(p.z)]),
        "message": c.message.as_ref().map(|m| &m.0),
    })))
}

fn t_feel(s: &mut Session, _: &Args) -> Result<Output> {
    let f = s.sim.state.feel.report;
    let ms = s.sim.dt() * 1000.0;
    Ok(Output::Json(json!({
        "model": pav_core::params::ChoiceParam::name(s.sim.config.movement.model),
        "response_ticks": f.response_ticks,
        "response_ms": (f.response_ticks as f32 * ms).round(),
        "speed": round3(f.speed),
        "top_speed": round3(f.top_speed),
        "accel_ms": f.accel_ms.round(),
        "stop_ms": f.stop_ms.round(),
        "stop_dist": round3(f.stop_dist),
        "turn_ms": f.turn_ms.round(),
        "jump_height": round3(f.jump_height),
        "air_ms": f.air_ms.round(),
        "jump_dist": round3(f.jump_dist),
    })))
}

fn t_camera_bench(s: &mut Session, a: &Args) -> Result<Output> {
    use pav_view::CameraParams;
    let w = get_u64(a, "width", 320)? as u32;
    let h = get_u64(a, "height", 180)? as u32;
    let cols = get_u64(a, "columns", 4)? as u32;
    let base = s.camera.params.clone();
    let list: Vec<(String, CameraParams)> = match get_str(a, "presets") {
        None => CameraParams::PRESETS.iter().map(|(n, f)| (n.to_string(), f())).collect(),
        Some(spec) => spec
            .split(',')
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .map(|t| match t.parse::<f32>() {
                Ok(tilt) => Ok((format!("{tilt}°"), CameraParams { tilt, ..base.clone() })),
                Err(_) => CameraParams::PRESETS
                    .iter()
                    .find(|(n, _)| n.starts_with(t))
                    .map(|(n, f)| (n.to_string(), f()))
                    .ok_or_else(|| anyhow!("unknown preset '{t}'")),
            })
            .collect::<Result<_>>()?,
    };
    let mut shots = Vec::with_capacity(list.len());
    for (name, mut p) in list.iter().cloned() {
        if !name.starts_with("isometric") {
            p.yaw = base.yaw;
        }
        s.camera.params = p;
        shots.push(s.render(w, h)?);
    }
    s.camera.params = base;
    let (tw, th, px) = pav_render::capture::tile_frames(&shots, w, h, cols);
    let png = pav_render::capture::encode_png(tw, th, &px)?;
    let path = PathBuf::from(get_str(a, "out").map(String::from).unwrap_or(format!("out/camera-bench-{}.png", s.sim.state.tick)));
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, &png)?;
    let names: Vec<&str> = list.iter().map(|(n, _)| n.as_str()).collect();
    let meta = json!({ "path": path, "tiles": names, "size": [tw, th] });
    Ok(Output::Image { png, path: Some(path), meta })
}

fn t_npcs(s: &mut Session, a: &Args) -> Result<Output> {
    let prefix = get_str(a, "name").unwrap_or("");
    let player = s.sim.state.player;
    let r = |v: f32| (v * 1000.0).round() / 1000.0;
    let list: Vec<Value> = s
        .sim
        .state
        .entities
        .iter()
        .filter(|e| Some(e.id) != player && e.name.starts_with(prefix))
        .filter_map(|e| {
            let ch = e.character.as_ref()?;
            let feet = e.pos - glam::Vec3::Y * ch.height() * 0.5;
            let body = ch.puppet.as_ref().map(|d| d.body).unwrap_or(s.sim.config.puppet.body);
            Some(json!({
                "id": e.id.0,
                "name": e.name,
                "body": pav_core::params::ChoiceParam::name(body),
                "feet": [r(feet.x), r(feet.y), r(feet.z)],
                "vel": [r(ch.vel.x), r(ch.vel.y), r(ch.vel.z)],
                "facing_deg": r(ch.facing.to_degrees()),
                "grounded": ch.grounded,
                "ai": e.ai.as_ref().map(|a| serde_json::to_value(&a.def).unwrap_or_default()),
                "steps": ch.rig.as_ref().map(|r| r.steps),
                "flinch": r(ch.anim.hit_side.abs() + ch.anim.hit_fwd.abs()),
                // Guards: alert meter (1 = spotted) and whether they see the player this tick.
                "alert": e.ai.as_ref().filter(|a| matches!(a.def, pav_core::ai::AiDef::Guard { .. })).map(|a| r(a.alert)),
                "sees_player": e.ai.as_ref().filter(|a| matches!(a.def, pav_core::ai::AiDef::Guard { .. })).map(|a| a.sees),
            }))
        })
        .collect();
    Ok(Output::Json(json!(list)))
}

fn t_signal(s: &mut Session, a: &Args) -> Result<Output> {
    let name = get_str(a, "name").context("give name=<signal>")?.to_string();
    s.sim.state.signals.push(name.clone());
    s.step(1);
    Ok(Output::Json(json!({ "sent": name, "entities": s.sim.state.entities.len() })))
}
