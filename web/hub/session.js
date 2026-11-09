/*! aihub-session 1.4.0 — AI Hub Session Protocol v1 client. Paste it into your game unmodified. */
(function (root) {
  'use strict';
  if (root.AIHub) return;

  var VERSION = '1.4.0';
  var HELLO_EVERY_MS = 500;
  var HELLO_TIMEOUT_MS = 10000;
  var PROGRESS_EVERY_MS = 1000;
  /** The hub's welcome may change these; a newer hub never needs a newer copy of this file for it. */
  var limits = { maxBytes: 16384, ratePerS: 20, burst: 40 };

  var framed = false;
  try {
    framed = root.parent && root.parent !== root;
  } catch (e) {
    framed = true;
  }
  var search = String(root.location && root.location.search);
  var flagged = /[?&]aihub=/.test(search);
  var seedArg = /[?&]aihub-seed=(\d+)/.exec(search);

  var hub = {
    version: VERSION,
    protocol: 1,
    /** True when the hub launched this page for a session (?aihub=1 inside a frame). */
    active: !!(flagged && framed),
    me: null,
    host: null,
    isHost: false,
    players: [],
    mode: null,
    seed: 0,
    limitMs: 0,
    round: 0,
    /** True when the hub shows the round's splash and collects every player's ready: call ready() once loaded, with no prompt. */
    splash: false,
    /** True when the hub seeded Math.random for this round (side by side): every copy draws the same numbers. */
    seeded: false,
  };

  /** A storage that lives and dies with the page: the Storage methods, and items as properties too. */
  function memoryStorage() {
    var data = Object.create(null);
    var api = {
      getItem: function (k) {
        k = String(k);
        return k in data ? data[k] : null;
      },
      setItem: function (k, v) {
        data[String(k)] = String(v);
      },
      removeItem: function (k) {
        delete data[String(k)];
      },
      clear: function () {
        for (var k in data) delete data[k];
      },
      key: function (i) {
        var keys = Object.keys(data);
        return i >= 0 && i < keys.length ? keys[i] : null;
      },
    };
    if (typeof Proxy !== 'function') return api;
    return new Proxy(api, {
      get: function (t, k) {
        if (k === 'length') return Object.keys(data).length;
        if (typeof k === 'symbol' || k in t) return t[k];
        return k in data ? data[k] : undefined;
      },
      set: function (t, k, v) {
        if (typeof k !== 'symbol') data[k] = String(v);
        return true;
      },
      has: function (t, k) {
        return k in data || k in t;
      },
      deleteProperty: function (t, k) {
        delete data[k];
        return true;
      },
      ownKeys: function () {
        return Object.keys(data);
      },
      getOwnPropertyDescriptor: function (t, k) {
        return k in data ? { value: data[k], writable: true, enumerable: true, configurable: true } : undefined;
      },
    });
  }

  /** mulberry32: numbers in [0, 1), the same sequence everywhere for the same 32-bit start. */
  function generator(a) {
    return function () {
      a = (a + 0x6d2b79f5) >>> 0;
      var t = a;
      t = Math.imul(t ^ (t >>> 15), t | 1);
      t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }

  // A fair start: in a session the game gets an empty storage of its own, so
  // every round starts as a first launch (no saved unlocks, upgrades or runs),
  // and nothing the round saves touches the player's own saves.
  if (hub.active) {
    for (var s = 0, stores = ['localStorage', 'sessionStorage']; s < stores.length; s++) {
      try {
        Object.defineProperty(root, stores[s], { configurable: true, value: memoryStorage() });
      } catch (e) {
        warn('could not give the game a fresh ' + stores[s]);
      }
    }
  }

  // Side by side: before any of the game's own scripts run, Math.random becomes
  // a generator seeded by the hub, so every copy builds the same start.
  var realRandom = Math.random;
  var seededRandom = null;
  if (hub.active && seedArg) {
    Math.random = seededRandom = generator(Number(seedArg[1]) >>> 0);
    hub.seeded = true;
  }

  // In a session the hub owns fullscreen: a game that asks for it gets the
  // hub's whole page, so the scoreboard and the hub's cards stay on top. The
  // hub says when its page goes in and out, so the game's own toggle and its
  // fullscreenchange listeners work as they always did.
  var fullscreenOn = false;
  if (hub.active && root.Element && root.document) {
    var asks = function (on) {
      return function () {
        post('fullscreen', { on: on });
        return Promise.resolve();
      };
    };
    var proto = root.Element.prototype;
    proto.requestFullscreen = proto.webkitRequestFullscreen = asks(true);
    root.document.exitFullscreen = root.document.webkitExitFullscreen = asks(false);
    var shown = function () {
      return fullscreenOn ? root.document.documentElement || null : null;
    };
    try {
      Object.defineProperty(root.document, 'fullscreenElement', { configurable: true, get: shown });
      Object.defineProperty(root.document, 'webkitFullscreenElement', { configurable: true, get: shown });
    } catch (e) {}
  }

  /**
   * No hub after all: play on, with Math.random the page's own again. The
   * empty storage stays: the game has read it already, and what it writes
   * must never land on the player's real saves.
   */
  function standAlone() {
    hub.active = false;
    if (seededRandom && Math.random === seededRandom) Math.random = realRandom;
    hub.seeded = false;
  }

  var origin = '*';
  var welcomed = false;
  var ended = false;
  var connecting = null;
  var goAt = null;
  var sentReady = false;
  var reported = false;
  var handlers = {};
  var pending = {};
  var tokens = limits.burst;
  var refilled = now();
  var probeFn = null;
  var lastProgress = -Infinity;
  var nextProgress = null;
  var progressTimer = null;

  function now() {
    return root.performance && root.performance.now ? root.performance.now() : Date.now();
  }

  function warn(text) {
    if (root.console) root.console.warn('[AIHub] ' + text);
  }

  function own(o, k) {
    return Object.prototype.hasOwnProperty.call(o, k);
  }

  /** This client's own fields, which no welcome may overwrite. */
  var KEEP = { aihub: 1, type: 1, version: 1, protocol: 1, active: 1, seeded: 1 };

  /** A plain JSON copy, or undefined when it is not JSON. */
  function json(value) {
    try {
      var text = JSON.stringify(value);
      return text === undefined ? undefined : JSON.parse(text);
    } catch (e) {
      return undefined;
    }
  }

  function post(type, fields) {
    var msg = { aihub: 1, type: type };
    for (var key in fields) if (own(fields, key)) msg[key] = fields[key];
    try {
      root.parent.postMessage(msg, origin);
    } catch (e) {
      warn('could not reach the hub: ' + e.message);
    }
  }

  function emit(type, args) {
    var list = handlers[type];
    if (!list || !list.length || pending[type]) {
      (pending[type] = pending[type] || []).push(args);
      return;
    }
    for (var i = 0; i < list.length; i++) {
      try {
        list[i].apply(null, args);
      } catch (e) {
        setTimeout(function () {
          throw e;
        });
      }
    }
  }

  function onMessage(event) {
    if (event.source !== root.parent) return;
    var m = event.data;
    if (!m || m.aihub !== 1 || typeof m.type !== 'string') return;
    if (m.type === 'welcome') {
      if (welcomed) return;
      welcomed = true;
      origin = event.origin && event.origin !== 'null' ? event.origin : '*';
      // Every field the hub sends is kept, so a newer hub can add one without a newer client.
      for (var key in m) {
        if (!own(m, key) || KEEP[key] || typeof hub[key] === 'function') continue;
        hub[key] = m[key];
      }
      hub.players = (m.players || []).slice();
      hub.seed = m.seed >>> 0;
      hub.isHost = m.me === m.host;
      hub.splash = m.splash === true;
      if (m.limits) {
        for (var l in limits) if (own(limits, l) && Number(m.limits[l]) > 0) limits[l] = Number(m.limits[l]);
        tokens = limits.burst;
      }
      if (connecting) connecting.resolve(info());
      return;
    }
    if (m.type === 'fullscreen') {
      fullscreenOn = m.on === true;
      try {
        root.document.dispatchEvent(new root.Event('fullscreenchange'));
      } catch (e) {}
      return;
    }
    if (!welcomed || ended) return;
    if (m.type === 'start') {
      goAt = now() + Number(m.in || 0);
      emit('start', [{ in: Number(m.in || 0) }]);
    } else if (m.type === 'message') {
      emit('message', [m.data, m.from]);
    } else if (m.type === 'leave') {
      hub.players = hub.players.filter(function (p) {
        return p.id !== m.id;
      });
      emit('leave', [{ id: m.id, reason: m.reason }]);
    } else if (m.type === 'time-up') {
      emit('time-up', [{ graceMs: m.graceMs }]);
    } else if (m.type === 'end') {
      ended = true;
      if (progressTimer) clearTimeout(progressTimer);
      emit('end', [{ reason: m.reason, standings: m.standings || null }]);
    } else {
      // An event this client has never heard of: hand it on as it came.
      var detail = {};
      for (var k in m) if (own(m, k) && k !== 'aihub' && k !== 'type') detail[k] = m[k];
      emit(m.type, [detail]);
    }
  }

  function info() {
    var out = {};
    for (var key in hub) if (own(hub, key) && typeof hub[key] !== 'function' && !KEEP[key]) out[key] = hub[key];
    out.players = hub.players.slice();
    return out;
  }

  /** Join the session. Resolves with the round's info, or null when there is no hub: play normally. */
  hub.connect = function (opts) {
    if (connecting) return connecting.promise;
    var done;
    var promise = new Promise(function (resolve) {
      done = resolve;
    });
    connecting = { promise: promise, resolve: done };
    if (!hub.active) {
      done(null);
      return promise;
    }
    opts = opts || {};
    root.addEventListener('message', onMessage);
    var started = now();
    var hello = function () {
      if (welcomed) return;
      if (now() - started > HELLO_TIMEOUT_MS) {
        standAlone();
        warn('no hub answered; playing normally');
        done(null);
        return;
      }
      post('hello', { v: 1, client: VERSION, mode: opts.mode || null, seats: opts.seats || null, card: opts.card === true });
      setTimeout(hello, HELLO_EVERY_MS);
    };
    hello();
    return promise;
  };

  /** Everything is loaded: at once when info.splash, otherwise from the player's ready gesture. */
  hub.ready = function () {
    if (!welcomed || sentReady || ended) return;
    sentReady = true;
    post('ready', {});
  };

  /** On the game's own card (connect({ card: true })): this player is 'ready', or asks for 'more' time (again takes it back). */
  hub.answer = function (what) {
    if (!welcomed || ended || goAt !== null) return;
    if (what !== 'ready' && what !== 'more') return warn('answer is ready or more');
    post('answer', { answer: what });
  };

  hub.on = function (type, fn) {
    (handlers[type] = handlers[type] || []).push(fn);
    if (pending[type] && !pending[type].flushing) {
      pending[type].flushing = true;
      setTimeout(function () {
        var queued = pending[type];
        delete pending[type];
        for (var i = 0; i < queued.length; i++) emit(type, queued[i]);
      });
    }
    return function () {
      var list = handlers[type] || [];
      var at = list.indexOf(fn);
      if (at !== -1) list.splice(at, 1);
    };
  };

  /** Send JSON data to every other player, or to one with {to: id}. {reliable: false} may drop or reorder. */
  hub.send = function (data, opts) {
    if (!welcomed || ended) return false;
    opts = opts || {};
    var text;
    try {
      text = JSON.stringify(data);
    } catch (e) {
      warn('send: data is not JSON');
      return false;
    }
    if (text === undefined) return false;
    if (text.length > limits.maxBytes) {
      warn('send: message over ' + limits.maxBytes + ' bytes dropped');
      return false;
    }
    var t = now();
    tokens = Math.min(limits.burst, tokens + ((t - refilled) / 1000) * limits.ratePerS);
    refilled = t;
    if (tokens < 1) {
      warn('send: over ' + limits.ratePerS + ' messages a second, dropped');
      return false;
    }
    tokens -= 1;
    var fields = { data: JSON.parse(text), reliable: opts.reliable !== false };
    if (opts.to !== undefined && opts.to !== null) fields.to = opts.to;
    post('send', fields);
    return true;
  };

  /** Milliseconds since the synchronized start: negative during the countdown, -Infinity before start. */
  hub.time = function () {
    return goAt === null ? -Infinity : now() - goAt;
  };

  /** A seeded random number generator (0 ≤ n < 1), the same sequence on every device for the same label. */
  hub.rng = function (label) {
    var h = 2166136261;
    var s = String(label === undefined ? '' : label);
    for (var i = 0; i < s.length; i++) {
      h ^= s.charCodeAt(i);
      h = Math.imul(h, 16777619);
    }
    return generator((hub.seed ^ h) >>> 0);
  };

  /** A number the hub can rank: finite, given as a number or as text. Not null, '' or true, which JSON or Number would make 0. */
  function finite(v) {
    return (typeof v === 'number' || (typeof v === 'string' && v.trim() !== '')) && isFinite(v);
  }

  /**
   * The whole outcome as plain JSON, field by field: the hub checks and trims
   * it, so new optional fields need no new client. A field that is not JSON
   * (a cycle, a BigInt) is left out, not the whole outcome.
   */
  function outcomeOf(outcome) {
    if (!outcome || typeof outcome !== 'object' || Array.isArray(outcome)) {
      warn('the outcome is not a plain object');
      return null;
    }
    var copy = {};
    for (var key in outcome) {
      if (!own(outcome, key)) continue;
      var value = json(outcome[key]);
      if (value !== undefined) copy[key] = value;
      else if (outcome[key] !== undefined) warn('the outcome\'s ' + key + ' is not JSON: left out');
    }
    return copy;
  }

  /** Race and side by side: report your own outcome once, {time: ms} (lower wins) or {score: n} (higher wins), with optional stats and awards. */
  hub.result = function (outcome) {
    if (!welcomed || ended || reported) return;
    var fields = outcomeOf(outcome);
    if (!fields) return;
    var given = outcome || {};
    delete fields.time;
    delete fields.score;
    if (finite(given.time)) fields.time = Number(given.time);
    if (finite(given.score)) fields.score = Number(given.score);
    if (!own(fields, 'time') && !own(fields, 'score')) return warn('result needs a finite time or score');
    reported = true;
    if (progressTimer) clearTimeout(progressTimer);
    post('result', fields);
  };

  /** Versus and co-op: the host reports the standings once, best first, ties grouped: [[2], [0, 3], [1]]. */
  hub.finish = function (outcome) {
    if (!welcomed || ended || reported) return;
    if (!hub.isHost) return warn('finish is for the host only');
    var fields = outcomeOf(outcome);
    if (!fields) return;
    if (!Array.isArray(fields.standings)) return warn('finish needs standings');
    reported = true;
    post('finish', fields);
  };

  function sendProgress() {
    progressTimer = null;
    if (!nextProgress || ended || reported) return;
    lastProgress = now();
    post('progress', nextProgress);
    nextProgress = null;
  }

  /** Your standing during play, for the hub's scoreboard: {score, label}. Call it as often as you like; the hub hears it about once a second. */
  hub.progress = function (p) {
    if (!welcomed || ended || reported || !p || !finite(p.score)) return;
    nextProgress = { score: Number(p.score) };
    if (p.label != null) nextProgress.label = String(p.label).slice(0, 24);
    var wait = lastProgress + PROGRESS_EVERY_MS - now();
    if (wait <= 0) sendProgress();
    else if (!progressTimer) progressTimer = setTimeout(sendProgress, wait);
  };

  /** For the harness, in or out of a session: fn() → { scene, menu, world, input }, read with inspect(). */
  hub.probe = function (fn) {
    probeFn = typeof fn === 'function' ? fn : null;
  };

  hub.inspect = function () {
    if (!probeFn) return null;
    try {
      return JSON.parse(JSON.stringify(probeFn()));
    } catch (e) {
      return { error: String((e && e.message) || e) };
    }
  };

  /** The game cannot run here (no WebGL, failed load): the hub skips it. */
  hub.fail = function (message) {
    if (!hub.active) return;
    post('error', { message: String(message || 'error').slice(0, 500), fatal: true });
  };

  root.AIHub = hub;
})(typeof window !== 'undefined' ? window : globalThis);
