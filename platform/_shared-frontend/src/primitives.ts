// LiveBlock — vanilla TypeScript ports of the design's React primitives.
// Each function returns an HTMLElement so window TS can compose UIs without
// pulling in a framework.

const ICON_NS = "http://www.w3.org/2000/svg";

export interface LogoOptions { size?: number; radius?: number }

/** Curated bright-but-cohesive palette from primitives.jsx. */
const LOGO_PALETTE = [
  '#ff3b30','#ff9500','#ffcc00','#34c759','#00c7be','#30b0c7',
  '#0a84ff','#5e5ce6','#bf5af2','#ff375f','#ff6482','#ffd60a',
  '#32d74b','#64d2ff','#5ac8fa','#af52de','#ff453a','#ffd426',
  '#a8e10c','#ff6b35','#ec407a',
];

/** Deterministic 12×12 cells using the same LCG seed as primitives.jsx. */
function logoCells(): string[] {
  let seed = 67;
  const rnd = () => { seed = (seed * 9301 + 49297) % 233280; return seed / 233280; };
  const out: string[] = [];
  for (let i = 0; i < 144; i++) {
    out.push(LOGO_PALETTE[Math.floor(rnd() * LOGO_PALETTE.length)]);
  }
  return out;
}
const CELLS = logoCells();

export function logo(opts: LogoOptions = {}): SVGSVGElement {
  const size = opts.size ?? 64;
  const radius = opts.radius ?? 14;
  const rOut = (120 / size) * radius;
  const svg = document.createElementNS(ICON_NS, "svg");
  svg.setAttribute("viewBox", "0 0 120 120");
  svg.setAttribute("width", String(size));
  svg.setAttribute("height", String(size));
  svg.style.display = "block";

  const defs = document.createElementNS(ICON_NS, "defs");
  const clip = document.createElementNS(ICON_NS, "clipPath");
  const clipId = `lbclip-${Math.random().toString(36).slice(2, 8)}`;
  clip.setAttribute("id", clipId);
  const clipRect = document.createElementNS(ICON_NS, "rect");
  clipRect.setAttribute("x", "0"); clipRect.setAttribute("y", "0");
  clipRect.setAttribute("width", "120"); clipRect.setAttribute("height", "120");
  clipRect.setAttribute("rx", String(rOut));
  clip.appendChild(clipRect);
  defs.appendChild(clip);
  svg.appendChild(defs);

  const g = document.createElementNS(ICON_NS, "g");
  g.setAttribute("clip-path", `url(#${clipId})`);
  // Pixel field
  for (let y = 0; y < 12; y++) for (let x = 0; x < 12; x++) {
    const r = document.createElementNS(ICON_NS, "rect");
    r.setAttribute("x", String(x * 10));
    r.setAttribute("y", String(y * 10));
    r.setAttribute("width", "10"); r.setAttribute("height", "10");
    r.setAttribute("fill", CELLS[y * 12 + x]);
    g.appendChild(r);
  }
  // Black corner brackets
  const brackets = document.createElementNS(ICON_NS, "g");
  brackets.setAttribute("fill", "none");
  brackets.setAttribute("stroke", "#0c0a14");
  brackets.setAttribute("stroke-width", "7");
  brackets.setAttribute("stroke-linecap", "round");
  brackets.setAttribute("stroke-linejoin", "round");
  for (const d of [
    "M16 36 V26 Q16 16 26 16 H36",
    "M84 16 H94 Q104 16 104 26 V36",
    "M104 84 V94 Q104 104 94 104 H84",
    "M36 104 H26 Q16 104 16 94 V84",
  ]) {
    const p = document.createElementNS(ICON_NS, "path");
    p.setAttribute("d", d);
    brackets.appendChild(p);
  }
  g.appendChild(brackets);
  // Red record dot
  const ringOuter = document.createElementNS(ICON_NS, "circle");
  ringOuter.setAttribute("cx", "89"); ringOuter.setAttribute("cy", "31");
  ringOuter.setAttribute("r", "14"); ringOuter.setAttribute("fill", "#0c0a14");
  g.appendChild(ringOuter);
  const dot = document.createElementNS(ICON_NS, "circle");
  dot.setAttribute("cx", "89"); dot.setAttribute("cy", "31");
  dot.setAttribute("r", "10.5"); dot.setAttribute("fill", "#ff3b30");
  g.appendChild(dot);
  // Hairline rim
  const rim = document.createElementNS(ICON_NS, "rect");
  rim.setAttribute("x", "0.5"); rim.setAttribute("y", "0.5");
  rim.setAttribute("width", "119"); rim.setAttribute("height", "119");
  rim.setAttribute("rx", String(rOut - 0.5));
  rim.setAttribute("fill", "none");
  rim.setAttribute("stroke", "rgba(0,0,0,0.25)");
  rim.setAttribute("stroke-width", "1");
  g.appendChild(rim);
  svg.appendChild(g);
  svg.classList.add("lb-logo");
  return svg;
}

export function wordmark(size = 22): HTMLElement {
  const root = document.createElement("div");
  root.className = "lb-wordmark";
  root.style.fontSize = `${size}px`;
  const l = logo({ size: size + 12, radius: Math.round((size + 12) * 0.22) });
  root.appendChild(l);
  const text = document.createElement("span");
  text.innerHTML = `Live<span class="red">Block</span>er`;
  root.appendChild(text);
  return root;
}

export type SurfaceKind = "raised" | "raised-sm" | "inset" | "pressed" | "flat";

export function surface(opts: { kind?: SurfaceKind; radius?: number; padding?: number } = {}): HTMLElement {
  const el = document.createElement("div");
  el.style.borderRadius = `${opts.radius ?? 22}px`;
  el.style.padding = `${opts.padding ?? 20}px`;
  if (opts.kind === "inset")    el.classList.add("lb-inset");
  else if (opts.kind === "pressed") el.classList.add("lb-pressed");
  else if (opts.kind === "raised-sm") el.classList.add("lb-raised-sm");
  else if (opts.kind === "flat") {} // no shadow
  else el.classList.add("lb-raised");
  return el;
}

export type PillVariant = "default" | "block" | "train" | "ghost";

export function pill(opts: { label: string; variant?: PillVariant; size?: "sm" | "md" | "lg"; icon?: string; onClick?: () => void }): HTMLButtonElement {
  const b = document.createElement("button");
  b.className = "lb-pill";
  if (opts.variant === "block") b.classList.add("lb-block");
  if (opts.variant === "train") b.classList.add("lb-train");
  if (opts.variant === "ghost") b.classList.add("lb-ghost");
  if (opts.size === "sm") b.classList.add("lb-sm");
  if (opts.size === "lg") b.classList.add("lb-lg");
  if (opts.icon) {
    const ic = document.createElement("span");
    ic.textContent = opts.icon;
    ic.style.fontSize = "13px";
    b.appendChild(ic);
  }
  const t = document.createElement("span");
  t.textContent = opts.label;
  b.appendChild(t);
  if (opts.onClick) b.addEventListener("click", opts.onClick);
  return b;
}

export function toggle(opts: { value: boolean; accent?: "block" | "success" | "train" | "detect"; onChange?: (v: boolean) => void }): HTMLButtonElement {
  const b = document.createElement("button");
  b.className = "lb-toggle";
  b.dataset.on = String(opts.value);
  if (opts.accent) b.dataset.accent = opts.accent;
  const k = document.createElement("span");
  k.className = "knob";
  b.appendChild(k);
  b.addEventListener("click", () => {
    const next = b.dataset.on !== "true";
    b.dataset.on = String(next);
    opts.onChange?.(next);
  });
  return b;
}

export function statusDot(opts: { color?: "block" | "success" | "warn" | "detect"; pulse?: boolean } = {}): HTMLElement {
  const s = document.createElement("span");
  s.className = "lb-dot";
  if (opts.color === "success") s.classList.add("success");
  if (opts.color === "warn")    s.classList.add("warn");
  if (opts.color === "detect")  s.classList.add("detect");
  if (opts.pulse) s.classList.add("pulse");
  return s;
}

export function trafficLights(): HTMLElement {
  const t = document.createElement("div");
  t.className = "lb-traffic";
  for (const c of ["red", "yellow", "green"]) {
    const d = document.createElement("span");
    d.className = `dot ${c}`;
    t.appendChild(d);
  }
  return t;
}

export function lavenderBar(opts: { center?: HTMLElement[] } = {}): HTMLElement {
  const bar = document.createElement("div");
  bar.className = "lb-titlebar";
  bar.appendChild(trafficLights());
  const center = document.createElement("div");
  center.style.flex = "1";
  center.style.display = "flex";
  center.style.gap = "8px";
  center.style.justifyContent = "center";
  for (const el of opts.center ?? []) center.appendChild(el);
  bar.appendChild(center);
  const right = document.createElement("div");
  right.style.width = "60px";
  bar.appendChild(right);
  return bar;
}
