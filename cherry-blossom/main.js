/* =========================================================================
   Sakura Drift — generative cherry blossom scene
   Vanilla JS + Canvas 2D. No dependencies, no network, all procedural.

   Structure
   1. Utilities & noise
   2. Scene state (theme, wind, motion)
   3. World build: hill, tree skeletons, blossoms, ground petals, water
   4. Render passes: sky, sun/moon, stars, mist, hill, trees, water, petals
   5. Interaction: gust, theme, wind, pause, resize, reduced motion
   ========================================================================= */
(() => {
  'use strict';

  /* ---------------------------------------------------------------------
     1. Utilities
     --------------------------------------------------------------------- */
  const TAU = Math.PI * 2;
  const rand = (a = 0, b = 1) => a + Math.random() * (b - a);
  const pick = (arr) => arr[(Math.random() * arr.length) | 0];
  const clamp = (v, a, b) => (v < a ? a : v > b ? b : v);
  const lerp = (a, b, t) => a + (b - a) * t;
  const smooth = (t) => t * t * (3 - 2 * t);

  // Cheap deterministic value noise for organic motion.
  const perm = new Uint8Array(512);
  (() => {
    const p = new Uint8Array(256);
    for (let i = 0; i < 256; i++) p[i] = i;
    let seed = 1337;
    const next = () => ((seed = (seed * 1664525 + 1013904223) >>> 0) / 4294967296);
    for (let i = 255; i > 0; i--) {
      const j = (next() * (i + 1)) | 0;
      const t = p[i]; p[i] = p[j]; p[j] = t;
    }
    for (let i = 0; i < 512; i++) perm[i] = p[i & 255];
  })();
  const noise1 = (x) => {
    const xi = Math.floor(x), xf = x - xi;
    const a = perm[xi & 255], b = perm[(xi + 1) & 255];
    return lerp(a, b, smooth(xf)) / 127.5 - 1;
  };

  /* ---------------------------------------------------------------------
     2. Scene state
     --------------------------------------------------------------------- */
  const reduceMotionQuery = window.matchMedia('(prefers-reduced-motion: reduce)');

  const WIND = {
    calm:   { name: 'Calm',   strength: 0.22, petalRate: 7,  sway: 0.5 },
    breeze: { name: 'Breeze', strength: 1.0,  petalRate: 17, sway: 1.0 },
    storm:  { name: 'Storm',  strength: 2.3,  petalRate: 34, sway: 1.7 },
  };

  const state = {
    theme: 'day',           // 'day' | 'night'
    windKey: 'breeze',
    paused: false,
    reducedMotion: reduceMotionQuery.matches,
    gust: 0,                // 0..1 decaying gust envelope
    gustAt: 0,
    transition: 1,          // theme blend: 1 = fully day
    time: 0,
    wind: 0,                // smoothed horizontal wind force
    petalBudget: 0,         // cap for perf
  };

  const THEMES = {
    day: {
      sky: [[0, '#f9d9df'], [0.42, '#f3dfe3'], [0.74, '#fbeae6'], [1, '#f6d6cf']],
      sunGlow: 'rgba(255, 232, 196, 0.85)',
      sun: '#fff3dc',
      hillFar: '#c9b1b8',
      hillMid: '#a4878f',
      hill: '#6f5862',
      hillRim: 'rgba(255, 214, 214, 0.35)',
      water: ['#e7c9cb', '#c9a2ad', '#8f7182'],
      waterGlow: 'rgba(255, 226, 214, 0.55)',
      bark: '#4a3339',
      barkLit: '#6b4b52',
      blossomCore: '#e7708f',
      blossomMid: '#f7a7ba',
      blossomLight: '#ffd2de',
      blossomPale: '#fff0f4',
      groundPetal: 'rgba(238, 150, 170, 0.85)',
      mist: 'rgba(255, 246, 244, 0.75)',
      firefly: 0,
      star: 0,
    },
    night: {
      sky: [[0, '#0b0a1f'], [0.45, '#1c1436'], [0.76, '#3a2143'], [1, '#5a2c45']],
      sunGlow: 'rgba(214, 226, 255, 0.35)',
      sun: '#eef2ff',
      hillFar: '#2a1f3c',
      hillMid: '#211830',
      hill: '#120d1c',
      hillRim: 'rgba(255, 190, 210, 0.22)',
      water: ['#2a1b36', '#170f24', '#0a0713'],
      waterGlow: 'rgba(255, 206, 222, 0.30)',
      bark: '#140e18',
      barkLit: '#2c2033',
      blossomCore: '#a63d63',
      blossomMid: '#d9769a',
      blossomLight: '#f6b7c9',
      blossomPale: '#ffe3ec',
      groundPetal: 'rgba(214, 112, 146, 0.7)',
      mist: 'rgba(206, 200, 255, 0.16)',
      firefly: 1,
      star: 1,
    },
  };

  // Blend two colour sets numerically for the day→night transition.
  function mixColor(a, b, t) {
    if (a === b) return a;
    const pa = parseHex(a), pb = parseHex(b);
    return `rgba(${Math.round(lerp(pa[0], pb[0], t))},${Math.round(lerp(pa[1], pb[1], t))},${Math.round(lerp(pa[2], pb[2], t))},${lerp(pa[3], pb[3], t).toFixed(3)})`;
  }
  const hexCache = new Map();
  function parseHex(c) {
    let v = hexCache.get(c);
    if (v) return v;
    if (c[0] === '#') {
      const h = c.slice(1);
      v = [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16), 1];
    } else {
      const m = c.match(/rgba?\(([^)]+)\)/);
      const parts = m ? m[1].split(',').map(Number) : [0, 0, 0, 1];
      v = [parts[0], parts[1], parts[2], parts[3] ?? 1];
    }
    hexCache.set(c, v);
    return v;
  }

  // Live palette resolved from the current transition value.
  const palette = {};
  function resolvePalette() {
    const t = 1 - state.transition; // 0 = day, 1 = night
    const d = THEMES.day, n = THEMES.night;
    palette.sky = d.sky.map((stop, i) => [stop[0], mixColor(stop[1], n.sky[i][1], t)]);
    for (const key of ['sunGlow', 'sun', 'hillFar', 'hillMid', 'hill', 'hillRim', 'bark', 'barkLit',
      'blossomCore', 'blossomMid', 'blossomLight', 'blossomPale', 'groundPetal', 'mist', 'waterGlow']) {
      palette[key] = mixColor(d[key], n[key], t);
    }
    palette.water = d.water.map((c, i) => mixColor(c, n.water[i], t));
    palette.firefly = lerp(d.firefly, n.firefly, t);
    palette.star = lerp(d.star, n.star, t);
  }

  /* ---------------------------------------------------------------------
     Canvas setup
     --------------------------------------------------------------------- */
  const canvas = document.getElementById('scene');
  const ctx = canvas.getContext('2d', { alpha: false });
  let W = 0, H = 0, DPR = 1;
  let horizonY = 0, groundY = 0;

  const world = {
    trees: [],
    blossoms: [],
    groundPetals: [],
    waterLines: [],
    mistBands: [],
    fireflies: [],
    stars: [],
    hillPoints: [],
    distantTrees: [],
  };

  /* ---------------------------------------------------------------------
     3. World build
     --------------------------------------------------------------------- */
  function mulberry32(seed) {
    return function () {
      seed |= 0; seed = (seed + 0x6D2B79F5) | 0;
      let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }

  /**
   * Recursively grow a branch into the skeleton.
   * Each branch stores its parent index, length, angle, thickness and depth
   * so it can sway coherently at draw time.
   */
  function growBranch(rng, list, parent, depth, cfg) {
    const parentBranch = parent >= 0 ? list[parent] : null;
    const angle = parentBranch
      ? parentBranch.angle + (rng() - 0.5) * cfg.spread
      : -Math.PI / 2 + (rng() - 0.5) * 0.12;
    const len = parentBranch ? parentBranch.len * (0.66 + rng() * 0.16) : cfg.trunkLen;
    const width = parentBranch ? Math.max(0.8, parentBranch.width * (0.62 + rng() * 0.12)) : cfg.trunkWidth;

    const b = {
      parent,
      depth,
      angle,
      len,
      width,
      sway: 0.35 + depth * 0.42 + rng() * 0.3,
      phase: rng() * TAU,
      curve: (rng() - 0.5) * 0.5,
      fork: [],
      tip: false,
    };
    const index = list.length;
    list.push(b);
    if (parentBranch) parentBranch.fork.push(index);

    if (depth >= cfg.maxDepth) {
      b.tip = true;
      b.width = Math.max(0.55, b.width);
      return index;
    }

    // Two or three children; near the tips bias toward twos for finer sprays.
    const kids = depth < 2 ? 2 + (rng() < 0.55 ? 1 : 0) : 2 + (rng() < 0.25 ? 1 : 0);
    for (let i = 0; i < kids; i++) {
      growBranch(rng, list, index, depth + 1, cfg);
    }
    return index;
  }

  function buildTree(seed, opts) {
    const rng = mulberry32(seed);
    const branches = [];
    growBranch(rng, branches, -1, 0, {
      trunkLen: opts.trunkLen,
      trunkWidth: opts.trunkWidth,
      maxDepth: opts.maxDepth,
      spread: opts.spread,
    });

    // Walk the skeleton once to compute rest positions, then place blossoms.
    const tree = {
      x: opts.x,
      baseY: opts.baseY,
      scale: opts.scale,
      flip: opts.flip ? -1 : 1,
      depth: opts.depth,             // 0 = foreground
      branches,
      points: [],                    // rest-space endpoints per branch
      blossoms: [],
      lean: opts.lean || 0,
      phase: rng() * TAU,
    };

    // Rest positions (without sway) — used for blossom anchors.
    const px = new Float32Array(branches.length);
    const py = new Float32Array(branches.length);
    const ex = new Float32Array(branches.length);
    const ey = new Float32Array(branches.length);
    for (let i = 0; i < branches.length; i++) {
      const b = branches[i];
      const startX = b.parent >= 0 ? ex[b.parent] : 0;
      const startY = b.parent >= 0 ? ey[b.parent] : 0;
      px[i] = startX; py[i] = startY;
      ex[i] = startX + Math.cos(b.angle) * b.len;
      ey[i] = startY + Math.sin(b.angle) * b.len;
    }
    tree.rest = { px, py, ex, ey };

    // Blossom clusters on outer branches.
    for (let i = 0; i < branches.length; i++) {
      const b = branches[i];
      if (b.depth < Math.max(2, opts.maxDepth - 2)) continue;
      const density = b.depth === opts.maxDepth ? 4 : 2;
      for (let k = 0; k < density; k++) {
        const t = rand(0.35, 1.0);
        const bx = lerp(px[i], ex[i], t) + rand(-9, 9);
        const by = lerp(py[i], ey[i], t) + rand(-9, 9);
        tree.blossoms.push({
          branch: i,
          ox: bx, oy: by,
          r: rand(4.2, 9.5) * (b.depth === opts.maxDepth ? 1 : 0.85),
          rot: rand(0, TAU),
          hue: Math.random(),
          phase: rand(0, TAU),
          cluster: (Math.random() * 5) | 0,
        });
      }
    }
    return tree;
  }

  function buildWorld() {
    const rng = mulberry32(90210);
    const isNarrow = W < 760;

    horizonY = H * (isNarrow ? 0.70 : 0.68);
    groundY = H * 0.80;

    world.trees = [];
    world.blossoms = [];

    // Tree layout: one dominant tree left of centre, a smaller companion
    // right, plus a far silhouette for depth. On narrow screens we keep
    // one hero tree and a smaller companion tucked behind it.
    const layout = isNarrow
      ? [
          { x: W * 0.30, trunkLen: Math.min(H * 0.24, W * 0.20), trunkWidth: 11, maxDepth: 6, spread: 1.05, scale: 1, flip: false, lean: 0.04, depth: 0 },
          { x: W * 0.86, trunkLen: Math.min(H * 0.16, W * 0.13), trunkWidth: 7, maxDepth: 5, spread: 1.15, scale: 1, flip: true, lean: -0.06, depth: 1 },
        ]
      : [
          { x: W * 0.25, trunkLen: Math.min(H * 0.30, 210), trunkWidth: 15, maxDepth: 7, spread: 0.92, scale: 1, flip: false, lean: 0.05, depth: 0 },
          { x: W * 0.79, trunkLen: Math.min(H * 0.24, 165), trunkWidth: 10, maxDepth: 6, spread: 1.1, scale: 1, flip: true, lean: -0.07, depth: 1 },
          { x: W * 0.55, trunkLen: Math.min(H * 0.15, 95), trunkWidth: 6, maxDepth: 5, spread: 1.25, scale: 1, flip: false, lean: 0.1, depth: 2 },
        ];

    layout.forEach((cfg, idx) => {
      const tree = buildTree(4100 + idx * 977, { ...cfg, baseY: groundY + (cfg.depth === 0 ? 8 : cfg.depth * 6) });
      world.trees.push(tree);
      for (const bl of tree.blossoms) world.blossoms.push(bl);
    });

    // Sorted back-to-front so foreground trees overlap the distant ones.
    world.trees.sort((a, b) => b.depth - a.depth);

    // Ground petal carpet — soft drifts of fallen petals around the trunks.
    world.groundPetals = [];
    const carpetCount = isNarrow ? 260 : 520;
    for (let i = 0; i < carpetCount; i++) {
      const t = rng();
      const cx = lerp(W * 0.06, W * 0.94, t) + (rng() - 0.5) * W * 0.12;
      // Denser near the tree bases.
      const near = Math.exp(-Math.pow((t - 0.3) * 4, 2)) + Math.exp(-Math.pow((t - 0.8) * 5, 2)) * 0.8;
      if (rng() > 0.25 + near * 0.6) continue;
      world.groundPetals.push({
        x: cx,
        y: rand(horizonY + 10, H + 10),
        r: rand(1.1, 2.8),
        rot: rand(0, TAU),
        a: rand(0.25, 0.9),
        hue: rng(),
      });
    }

    // Water ripples: thin highlights across the lake.
    world.waterLines = [];
    for (let i = 0; i < (isNarrow ? 26 : 46); i++) {
      world.waterLines.push({
        x: rand(0, W),
        y: rand(horizonY + 4, H),
        len: rand(20, W * 0.18),
        w: rand(0.6, 1.8),
        phase: rand(0, TAU),
        speed: rand(0.2, 0.7),
      });
    }

    // Drifting mist bands near the horizon.
    world.mistBands = [];
    for (let i = 0; i < 6; i++) {
      world.mistBands.push({
        x: rand(-W * 0.3, W),
        y: horizonY - rand(10, 70),
        w: rand(W * 0.35, W * 0.75),
        h: rand(18, 46),
        speed: rand(4, 12) * (rng() < 0.5 ? -1 : 1),
        a: rand(0.35, 0.8),
      });
    }

    // Fireflies (visible at night).
    world.fireflies = [];
    for (let i = 0; i < (isNarrow ? 22 : 40); i++) {
      world.fireflies.push({
        x: rand(0, W),
        y: rand(horizonY - H * 0.18, groundY + 20),
        r: rand(0.9, 2.1),
        phase: rand(0, TAU),
        speed: rand(0.3, 1),
        drift: rand(10, 40),
      });
    }

    // Stars.
    world.stars = [];
    for (let i = 0; i < (isNarrow ? 90 : 160); i++) {
      world.stars.push({
        x: rand(0, W),
        y: rand(0, horizonY * 0.85),
        r: rand(0.4, 1.4),
        phase: rand(0, TAU),
        twinkle: rand(0.6, 2.2),
      });
    }

    // Distant tree line silhouette on the far shore.
    world.distantTrees = [];
    let x = -20;
    while (x < W + 20) {
      const w = rand(18, 46);
      world.distantTrees.push({ x, w, h: rand(14, 42) });
      x += w * rand(0.55, 0.9);
    }

    // Rolling hill silhouette just under the tree bases.
    world.hillPoints = [];
    const steps = 42;
    for (let i = 0; i <= steps; i++) {
      const t = i / steps;
      world.hillPoints.push({
        x: t * W,
        y: horizonY + 6 + Math.sin(t * Math.PI * 2.2 + 1.3) * 10 + Math.sin(t * 7.1) * 4 + Math.pow(t - 0.5, 2) * 26,
      });
    }

    // Perf budget for petals scales with viewport area.
    const area = W * H;
    state.petalBudget = clamp(Math.round(area / 9000), 90, 340);
  }

  /* ---------------------------------------------------------------------
     Sway model: per-branch angle offset from wind + gust + noise
     --------------------------------------------------------------------- */
  function branchSway(branch, tree, time, windForce) {
    const t = time;
    const base = Math.sin(t * 0.9 + branch.phase + tree.phase) * 0.012 * branch.sway;
    const ripple = Math.sin(t * 1.7 + branch.phase * 2.1 + tree.phase) * 0.008 * branch.sway;
    const bend = windForce * 0.03 * branch.sway * (0.6 + 0.4 * Math.sin(t * 0.55 + tree.phase));
    const noise = noise1(t * 0.35 + branch.phase * 0.2) * 0.01 * branch.sway;
    return (base + ripple + noise) * tree.flip * -1 + bend * tree.flip * -1 + tree.lean * 0.15;
  }

  /**
   * Compute current joint positions for a tree.
   * Returns Float32Arrays reused each frame to avoid GC churn.
   */
  const jointCache = new WeakMap();
  function computeJoints(tree, time, windForce) {
    const n = tree.branches.length;
    let cache = jointCache.get(tree);
    if (!cache || cache.x.length !== n) {
      cache = { x: new Float32Array(n), y: new Float32Array(n), sx: new Float32Array(n), sy: new Float32Array(n) };
      jointCache.set(tree, cache);
    }
    const { x, y, sx, sy } = cache;
    const rest = tree.rest;
    for (let i = 0; i < n; i++) {
      const b = tree.branches[i];
      const parentX = b.parent >= 0 ? x[b.parent] : tree.x;
      const parentY = b.parent >= 0 ? y[b.parent] : tree.baseY;
      sx[i] = parentX; sy[i] = parentY;
      const sway = branchSway(b, tree, time, windForce);
      const a = b.angle + sway;
      const len = b.len * (1 + windForce * 0.012 * b.sway);
      x[i] = parentX + Math.cos(a) * len;
      y[i] = parentY + Math.sin(a) * len;
    }
    return cache;
  }

  /* ---------------------------------------------------------------------
     Petals
     --------------------------------------------------------------------- */
  const petals = [];

  function spawnPetal(fromGust, anchor) {
    let sx, sy;
    if (anchor) {
      sx = anchor.x; sy = anchor.y;
    } else if (world.blossoms.length && Math.random() < 0.75) {
      // Release from a blossom on a random tree.
      const tree = pick(world.trees);
      const bl = pick(tree.blossoms);
      const j = jointCache.get(tree);
      if (j) {
        sx = bl.px; sy = bl.py;
      } else {
        sx = rand(0, W); sy = rand(0, H * 0.5);
      }
    } else {
      sx = rand(-20, W + 20);
      sy = rand(-40, H * 0.55);
    }
    const depth = rand(0.35, 1);
    petals.push({
      x: sx, y: sy,
      z: depth,
      size: rand(4.5, 10) * (0.6 + depth * 0.7),
      rot: rand(0, TAU),
      spin: rand(-2.4, 2.4),
      flip: rand(0, TAU),
      flipSpeed: rand(1.2, 3.4),
      phase: rand(0, TAU),
      swayAmp: rand(12, 46),
      fall: rand(14, 34) * (0.55 + depth * 0.75),
      drift: rand(-12, 18),
      tone: Math.random(),
      landed: false,
      life: 0,
      gustBoost: fromGust ? rand(0.5, 1) : 0,
    });
    if (petals.length > state.petalBudget * 1.6) petals.splice(0, petals.length - state.petalBudget * 1.6 | 0);
  }

  function updatePetals(dt, time, windForce) {
    const target = state.reducedMotion ? 0 : Math.round(WIND[state.windKey].petalRate * (1 + state.gust * 2.5) * (W * H) / 900000);
    const wanted = clamp(target, 8, state.petalBudget);
    const spawnAcc = Math.min(3, Math.ceil(wanted / 40)) || 1;
    for (let i = 0; i < spawnAcc; i++) {
      if (petals.length < wanted && Math.random() < 0.9) spawnPetal(state.gust > 0.2);
    }

    const wind = windForce * 26;
    for (let i = petals.length - 1; i >= 0; i--) {
      const p = petals[i];
      p.life += dt;
      const flutter = Math.sin(time * 2.2 + p.phase) * p.swayAmp * 0.5;
      const flutter2 = Math.sin(time * 3.7 + p.phase * 1.7) * 6 * p.z;
      p.x += (p.drift + wind * (0.5 + p.z)) * dt + flutter * dt;
      p.y += p.fall * (1 + state.gust * 0.9) * dt;
      p.x += flutter2 * dt;
      p.rot += p.spin * dt;
      p.flip += p.flipSpeed * dt;

      if (p.y > H + 30 || p.x < -80 || p.x > W + 80) {
        petals.splice(i, 1);
      }
    }
    return petals.length;
  }

  /* ---------------------------------------------------------------------
     4. Render passes
     --------------------------------------------------------------------- */
  function drawSky(time) {
    const g = ctx.createLinearGradient(0, 0, 0, horizonY + 40);
    for (const [stop, color] of palette.sky) g.addColorStop(stop, color);
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, W, H);

    // Stars (night only).
    if (palette.star > 0.01) {
      for (const s of world.stars) {
        const tw = 0.55 + 0.45 * Math.sin(time * s.twinkle + s.phase);
        ctx.globalAlpha = palette.star * tw;
        ctx.fillStyle = '#fff7ec';
        ctx.beginPath();
        ctx.arc(s.x, s.y, s.r, 0, TAU);
        ctx.fill();
      }
      ctx.globalAlpha = 1;
    }

    // Sun / moon.
    const isNight = state.transition < 0.5;
    const sunX = W * (isNight ? 0.78 : 0.72);
    const sunY = horizonY * (isNight ? 0.30 : 0.36);
    const sunR = Math.min(W, H) * (isNight ? 0.045 : 0.055);

    const glow = ctx.createRadialGradient(sunX, sunY, sunR * 0.2, sunX, sunY, sunR * 6.5);
    glow.addColorStop(0, palette.sunGlow);
    glow.addColorStop(1, 'rgba(255,255,255,0)');
    ctx.fillStyle = glow;
    ctx.fillRect(0, 0, W, horizonY + 60);

    if (isNight) {
      // Crescent moon: full disc with a background-coloured bite taken out.
      ctx.fillStyle = palette.sun;
      ctx.beginPath();
      ctx.arc(sunX, sunY, sunR, 0, TAU);
      ctx.fill();
      ctx.fillStyle = palette.sky[1][1];
      ctx.beginPath();
      ctx.arc(sunX - sunR * 0.45, sunY - sunR * 0.2, sunR * 0.92, 0, TAU);
      ctx.fill();
    } else {
      ctx.fillStyle = palette.sun;
      ctx.beginPath();
      ctx.arc(sunX, sunY, sunR, 0, TAU);
      ctx.fill();
    }

    // Distant tree line on the far shore.
    ctx.fillStyle = palette.hillFar;
    for (const t of world.distantTrees) {
      ctx.beginPath();
      ctx.ellipse(t.x, horizonY - t.h * 0.15, t.w * 0.6, t.h * 0.55, 0, Math.PI, TAU);
      ctx.fill();
    }
    ctx.fillRect(0, horizonY - 4, W, 8);

    // Mist bands drifting across the horizon.
    for (const m of world.mistBands) {
      m.x += m.speed * 0.016 * (0.6 + state.wind * 0.5);
      if (m.speed > 0 && m.x > W + 40) m.x = -m.w;
      if (m.speed < 0 && m.x < -m.w - 40) m.x = W;
      const mg = ctx.createRadialGradient(m.x + m.w / 2, m.y, 0, m.x + m.w / 2, m.y, m.w / 2);
      mg.addColorStop(0, palette.mist);
      mg.addColorStop(1, 'rgba(255,255,255,0)');
      ctx.globalAlpha = 0.5 * m.a;
      ctx.fillStyle = mg;
      ctx.beginPath();
      ctx.ellipse(m.x + m.w / 2, m.y, m.w / 2, m.h, 0, 0, TAU);
      ctx.fill();
    }
    ctx.globalAlpha = 1;
  }

  function drawHill(time) {
    // Mid ground band.
    ctx.fillStyle = palette.hillMid;
    ctx.beginPath();
    ctx.moveTo(0, H);
    for (const p of world.hillPoints) ctx.lineTo(p.x, p.y - 26);
    ctx.lineTo(W, H);
    ctx.closePath();
    ctx.fill();

    // Main hill.
    ctx.fillStyle = palette.hill;
    ctx.beginPath();
    ctx.moveTo(0, H);
    ctx.lineTo(0, world.hillPoints[0].y);
    for (const p of world.hillPoints) ctx.lineTo(p.x, p.y);
    ctx.lineTo(W, H);
    ctx.closePath();
    ctx.fill();

    // Soft rim light along the crest.
    ctx.strokeStyle = palette.hillRim;
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    world.hillPoints.forEach((p, i) => (i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y)));
    ctx.stroke();

    // Fallen petal carpet.
    for (const gp of world.groundPetals) {
      const shade = gp.hue < 0.5 ? palette.blossomMid : palette.groundPetal;
      ctx.globalAlpha = gp.a * 0.55;
      ctx.fillStyle = shade;
      ctx.beginPath();
      ctx.ellipse(gp.x, gp.y, gp.r, gp.r * 0.62, gp.rot, 0, TAU);
      ctx.fill();
    }
    ctx.globalAlpha = 1;
  }

  function drawWater(time) {
    // Water occupies the lower band of the scene; the hill overlaps it,
    // so we clip to the region under the hill line.
    ctx.save();
    ctx.beginPath();
    ctx.moveTo(0, H);
    for (let i = world.hillPoints.length - 1; i >= 0; i--) {
      const p = world.hillPoints[i];
      ctx.lineTo(p.x, p.y + 1);
    }
    ctx.lineTo(W, H);
    ctx.closePath();
    ctx.clip();

    const g = ctx.createLinearGradient(0, horizonY, 0, H);
    g.addColorStop(0, palette.water[0]);
    g.addColorStop(0.5, palette.water[1]);
    g.addColorStop(1, palette.water[2]);
    ctx.fillStyle = g;
    ctx.fillRect(0, horizonY, W, H - horizonY);

    // Sun/moon reflection column.
    const isNight = state.transition < 0.5;
    const rx = W * (isNight ? 0.78 : 0.72);
    const refl = ctx.createLinearGradient(0, horizonY, 0, H);
    refl.addColorStop(0, palette.waterGlow);
    refl.addColorStop(1, 'rgba(255,255,255,0)');
    ctx.fillStyle = refl;
    ctx.beginPath();
    ctx.moveTo(rx - 18, horizonY);
    ctx.lineTo(rx + 18, horizonY);
    ctx.lineTo(rx + W * 0.14, H);
    ctx.lineTo(rx - W * 0.14, H);
    ctx.closePath();
    ctx.fill();

    // Ripples.
    ctx.lineCap = 'round';
    for (const rl of world.waterLines) {
      const wave = Math.sin(time * rl.speed + rl.phase);
      ctx.globalAlpha = 0.12 + 0.18 * (0.5 + 0.5 * wave);
      ctx.strokeStyle = '#ffffff';
      ctx.lineWidth = rl.w;
      ctx.beginPath();
      const off = wave * 6;
      ctx.moveTo(rl.x + off, rl.y);
      ctx.quadraticCurveTo(rl.x + rl.len * 0.5 + off, rl.y + 1.5, rl.x + rl.len + off, rl.y);
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
    ctx.restore();
  }

  function drawBranch(branch, i, cache, tree) {
    const x0 = cache.sx[i], y0 = cache.sy[i], x1 = cache.x[i], y1 = cache.y[i];
    const b = branch;
    ctx.lineWidth = b.width * tree.scale;
    ctx.strokeStyle = b.depth < 2 ? palette.bark : palette.barkLit;
    ctx.beginPath();
    ctx.moveTo(x0, y0);
    const mx = (x0 + x1) / 2 + Math.sin(b.angle + b.curve) * b.len * 0.12;
    const my = (y0 + y1) / 2 - Math.cos(b.angle + b.curve) * b.len * 0.12;
    ctx.quadraticCurveTo(mx, my, x1, y1);
    ctx.stroke();
  }

  function drawBlossom(x, y, r, tone, rot, depthAlpha) {
    // Five-petal silhouette with a lighter core; tone picks the pink mix.
    const light = tone > 0.72 ? palette.blossomPale : tone > 0.4 ? palette.blossomLight : palette.blossomMid;
    ctx.save();
    ctx.translate(x, y);
    ctx.rotate(rot);
    ctx.globalAlpha = depthAlpha;
    ctx.fillStyle = light;
    for (let k = 0; k < 5; k++) {
      const a = (k / 5) * TAU;
      ctx.beginPath();
      ctx.ellipse(Math.cos(a) * r * 0.42, Math.sin(a) * r * 0.42, r * 0.46, r * 0.36, a, 0, TAU);
      ctx.fill();
    }
    ctx.fillStyle = palette.blossomCore;
    ctx.globalAlpha = depthAlpha * 0.8;
    ctx.beginPath();
    ctx.arc(0, 0, r * 0.26, 0, TAU);
    ctx.fill();
    ctx.restore();
  }

  function drawTree(tree, time, windForce) {
    const cache = computeJoints(tree, time, windForce);
    const depthAlpha = tree.depth === 0 ? 1 : tree.depth === 1 ? 0.82 : 0.66;

    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    ctx.globalAlpha = depthAlpha;

    // Trunk first (thick), then branches from thick to thin.
    for (let d = 0; d < 8; d++) {
      for (let i = 0; i < tree.branches.length; i++) {
        if (tree.branches[i].depth === d) drawBranch(tree.branches[i], i, cache, tree);
      }
    }

    // Blossom clusters — anchored to their branch joints.
    let bxSum = 0, bySum = 0, n = 0;
    const jitter = Math.sin(time * 0.8 + tree.phase) * 1.2 * (1 + windForce * 0.4);
    for (const bl of tree.blossoms) {
      const i = bl.branch;
      // Re-derive the blossom's position by moving it with its branch joint.
      const b = tree.branches[i];
      const restMidX = (tree.rest.px[i] + tree.rest.ex[i]) / 2;
      const restMidY = (tree.rest.py[i] + tree.rest.ey[i]) / 2;
      const curMidX = (cache.sx[i] + cache.x[i]) / 2;
      const curMidY = (cache.sy[i] + cache.y[i]) / 2;
      const dx = bl.ox - restMidX, dy = bl.oy - restMidY;
      // Rotate the offset a little with the branch sway for parallax.
      const swayAmt = (b.angle + Math.sin(time + b.phase) * 0.02) - b.angle;
      const rx = dx * Math.cos(swayAmt * 6) - dy * Math.sin(swayAmt * 6);
      const ry = dx * Math.sin(swayAmt * 6) + dy * Math.cos(swayAmt * 6);
      const px = curMidX + rx + jitter;
      const py = curMidY + ry;
      bl.px = px; bl.py = py;
      bxSum += px; bySum += py; n++;
      const breathe = 1 + Math.sin(time * 1.4 + bl.phase) * 0.04;
      drawBlossom(px, py, bl.r * breathe, bl.hue, bl.rot + Math.sin(time * 0.6 + bl.phase) * 0.08, depthAlpha);
    }
    if (n) { tree.centerX = bxSum / n; tree.centerY = bySum / n; }
    ctx.globalAlpha = 1;
  }

  function drawPetal(p) {
    const s = p.size;
    const flip = Math.cos(p.flip);
    const tone = p.tone > 0.7 ? palette.blossomPale : p.tone > 0.35 ? palette.blossomLight : palette.blossomMid;
    ctx.save();
    ctx.translate(p.x, p.y);
    ctx.rotate(p.rot);
    ctx.scale(1, clamp(flip, 0.12, 1));
    ctx.globalAlpha = 0.55 + p.z * 0.45;
    ctx.fillStyle = tone;

    // Notched petal: two arcs meeting at the centre with a small cleft.
    ctx.beginPath();
    ctx.moveTo(0, -s * 0.6);
    ctx.bezierCurveTo(s * 0.62, -s * 0.55, s * 0.58, s * 0.35, 0, s * 0.62);
    ctx.bezierCurveTo(-s * 0.58, s * 0.35, -s * 0.62, -s * 0.55, 0, -s * 0.6);
    ctx.fill();

    // Soft highlight along one edge for a waxy feel.
    if (p.z > 0.6) {
      ctx.globalAlpha *= 0.5;
      ctx.strokeStyle = palette.blossomPale;
      ctx.lineWidth = 0.8;
      ctx.beginPath();
      ctx.moveTo(-s * 0.2, -s * 0.4);
      ctx.quadraticCurveTo(-s * 0.45, 0, -s * 0.1, s * 0.4);
      ctx.stroke();
    }
    ctx.restore();
  }

  function drawFireflies(time) {
    if (palette.firefly < 0.01) return;
    for (const f of world.fireflies) {
      const x = f.x + Math.sin(time * f.speed + f.phase) * f.drift;
      const y = f.y + Math.cos(time * f.speed * 0.8 + f.phase * 1.3) * f.drift * 0.5;
      const pulse = 0.35 + 0.65 * Math.pow(0.5 + 0.5 * Math.sin(time * 1.6 + f.phase), 2);
      const g = ctx.createRadialGradient(x, y, 0, x, y, f.r * 9);
      g.addColorStop(0, `rgba(255, 240, 190, ${0.85 * pulse * palette.firefly})`);
      g.addColorStop(0.3, `rgba(255, 214, 120, ${0.35 * pulse * palette.firefly})`);
      g.addColorStop(1, 'rgba(255, 200, 120, 0)');
      ctx.fillStyle = g;
      ctx.beginPath();
      ctx.arc(x, y, f.r * 9, 0, TAU);
      ctx.fill();
      ctx.fillStyle = `rgba(255, 252, 230, ${pulse * palette.firefly})`;
      ctx.beginPath();
      ctx.arc(x, y, f.r, 0, TAU);
      ctx.fill();
    }
  }

  /* ---------------------------------------------------------------------
     5. Main loop
     --------------------------------------------------------------------- */
  let last = performance.now();
  let countTimer = 0;
  const countEl = document.getElementById('petal-count');

  function frame(now) {
    const dt = Math.min(0.05, (now - last) / 1000);
    last = now;

    if (!state.paused) state.time += dt;
    const time = state.time;

    // Smooth wind toward target strength plus gust envelope.
    const targetWind = WIND[state.windKey].strength + state.gust * 3.2;
    state.wind += (targetWind - state.wind) * Math.min(1, dt * 1.8);
    state.gust *= Math.exp(-dt * 0.9);
    if (state.gust < 0.002) state.gust = 0;

    // Theme blend.
    const goal = state.theme === 'night' ? 0 : 1;
    if (state.transition !== goal) {
      state.transition += (goal - state.transition) * Math.min(1, dt * 1.6);
      if (Math.abs(goal - state.transition) < 0.004) state.transition = goal;
      resolvePalette();
    }

    // Wind sways everything; in reduced motion we keep it nearly still.
    const windForce = state.reducedMotion ? state.wind * 0.15 : state.wind;
    const renderTime = state.reducedMotion ? time * 0.15 : time;

    drawSky(renderTime);
    drawHill(renderTime);
    drawWater(renderTime);
    for (const tree of world.trees) drawTree(tree, renderTime, windForce);

    const live = state.paused ? petals.length : updatePetals(dt, renderTime, windForce);
    for (const p of petals) drawPetal(p);

    drawFireflies(renderTime);

    // Petal counter (throttled).
    countTimer += dt;
    if (countTimer > 0.25) {
      countTimer = 0;
      countEl.textContent = String(live);
    }

    requestAnimationFrame(frame);
  }

  /* ---------------------------------------------------------------------
     Interaction
     --------------------------------------------------------------------- */
  const btnWind = document.getElementById('btn-wind');
  const btnTheme = document.getElementById('btn-theme');
  const btnMotion = document.getElementById('btn-motion');
  const motionLabel = document.getElementById('motion-label');
  const windSegs = Array.from(document.querySelectorAll('.wind__seg'));

  function triggerGust(strength = 1) {
    state.gust = Math.max(state.gust, strength);
    state.gustAt = performance.now();
    // A gust throws an extra burst of petals loose.
    if (!state.reducedMotion && !state.paused) {
      const burst = 26 + Math.round(strength * 22);
      for (let i = 0; i < burst; i++) spawnPetal(true);
    }
    btnWind.setAttribute('aria-pressed', 'true');
    clearTimeout(triggerGust._t);
    triggerGust._t = setTimeout(() => btnWind.setAttribute('aria-pressed', 'false'), 700);
  }

  function setTheme(theme) {
    state.theme = theme;
    document.body.dataset.theme = theme;
    document.documentElement.dataset.theme = theme;
    btnTheme.setAttribute('aria-pressed', String(theme === 'night'));
    const label = btnTheme.querySelector('.btn__label--long');
    label.textContent = theme === 'night' ? 'Day' : 'Night';
    btnTheme.querySelector('.btn__label--short').textContent = label.textContent;
  }

  function setWind(key) {
    state.windKey = key;
    for (const seg of windSegs) seg.setAttribute('aria-checked', String(seg.dataset.wind === key));
    if (key === 'storm') triggerGust(0.6);
  }

  function setPaused(paused) {
    state.paused = paused;
    btnMotion.setAttribute('aria-pressed', String(paused));
    motionLabel.textContent = paused ? 'Resume' : 'Pause';
    // Swap the icon between play and pause.
    btnMotion.querySelector('svg').innerHTML = paused
      ? '<path d="M8 5v14l11-7z" />'
      : '<path d="M9 5v14" /><path d="M15 5v14" />';
    last = performance.now();
  }

  btnWind.addEventListener('click', () => triggerGust(1));
  btnTheme.addEventListener('click', () => setTheme(state.theme === 'night' ? 'day' : 'night'));
  btnMotion.addEventListener('click', () => setPaused(!state.paused));
  windSegs.forEach((seg) => seg.addEventListener('click', () => setWind(seg.dataset.wind)));

  // Radiogroup arrow-key navigation.
  document.querySelector('.wind').addEventListener('keydown', (e) => {
    const idx = windSegs.findIndex((s) => s.dataset.wind === state.windKey);
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown') {
      e.preventDefault();
      const next = windSegs[(idx + 1) % windSegs.length];
      setWind(next.dataset.wind);
      next.focus();
    } else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') {
      e.preventDefault();
      const prev = windSegs[(idx - 1 + windSegs.length) % windSegs.length];
      setWind(prev.dataset.wind);
      prev.focus();
    }
  });

  // Click/tap anywhere on the scene (not the UI) for a gust.
  canvas.addEventListener('pointerdown', (e) => {
    if (e.button !== undefined && e.button !== 0) return;
    triggerGust(0.85);
  });

  window.addEventListener('keydown', (e) => {
    if (e.target instanceof HTMLElement && /^(INPUT|TEXTAREA|SELECT)$/.test(e.target.tagName)) return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const k = e.key.toLowerCase();
    if (k === 'g' || k === ' ') { e.preventDefault(); triggerGust(1); }
    else if (k === 'n') setTheme(state.theme === 'night' ? 'day' : 'night');
    else if (k === 'm') setPaused(!state.paused);
    else if (k === '1') setWind('calm');
    else if (k === '2') setWind('breeze');
    else if (k === '3') setWind('storm');
  });

  reduceMotionQuery.addEventListener?.('change', (e) => {
    state.reducedMotion = e.matches;
  });

  /* ---------------------------------------------------------------------
     Resize (debounced) — rebuilds procedural geometry
     --------------------------------------------------------------------- */
  function resize() {
    DPR = Math.min(window.devicePixelRatio || 1, 2);
    W = window.innerWidth;
    H = window.innerHeight;
    canvas.width = Math.round(W * DPR);
    canvas.height = Math.round(H * DPR);
    canvas.style.width = W + 'px';
    canvas.style.height = H + 'px';
    ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
    jointCache.clear?.();
    buildWorld();
    petals.length = 0;
    resolvePalette();
  }

  let resizeTimer = 0;
  window.addEventListener('resize', () => {
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(resize, 140);
  });

  // Boot.
  // Read the preference directly at boot, then apply the paused UI before the
  // first frame so the control reads "Resume" and no petals spawn.
  state.reducedMotion = reduceMotionQuery.matches;
  if (state.reducedMotion) setPaused(true);
  resolvePalette();
  resize();
  // Seed a few petals so the scene is alive on first paint.
  for (let i = 0; i < 30; i++) {
    spawnPetal(false);
    const p = petals[petals.length - 1];
    p.y = rand(-H * 0.2, H * 0.7);
  }
  requestAnimationFrame((t) => { last = t; frame(t); });

  // Small debug surface for automated checks.
  window.__sakura = { state, petals, world, triggerGust, setTheme, setWind, setPaused };
})();
