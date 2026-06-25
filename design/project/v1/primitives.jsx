// LiveBlocker neumorphic primitives + the brand Logo.
// All components consume tokens.css custom properties so the Tweaks panel
// can dial neumorphism intensity & density at runtime.

const LB_FONT = "var(--lb-font-ui)";

// ── Logo ──────────────────────────────────────────────────────
// Pixel-grid square + corner brackets + red record dot, riffing on the
// figma sketch. Uses a deterministic palette of bright pixels so it reads
// as "the screen" being framed by ScreenCaptureKit.
function Logo({ size = 64, radius = 14 }) {
  // Curated bright-but-cohesive palette — flat colors only, no gradients.
  // Pulled to feel like a tasteful screen, not a TV-static toy.
  const palette = [
    '#ff3b30', '#ff9500', '#ffcc00', '#34c759', '#00c7be', '#30b0c7',
    '#0a84ff', '#5e5ce6', '#bf5af2', '#ff375f', '#ff6482', '#ffd60a',
    '#32d74b', '#64d2ff', '#5ac8fa', '#af52de', '#ff453a', '#ffd426',
    '#a8e10c', '#ff6b35', '#ec407a',
  ];
  const uid = React.useId();
  const cells = [];
  let seed = 67;
  const rnd = () => { seed = (seed * 9301 + 49297) % 233280; return seed / 233280; };
  for (let y = 0; y < 12; y++) for (let x = 0; x < 12; x++) {
    cells.push({ x, y, c: palette[Math.floor(rnd() * palette.length)] });
  }
  const rOut = 120 / size * radius;
  return (
    <div style={{ width: size, height: size, position: 'relative', display: 'inline-block',
      filter: `drop-shadow(0 ${size * 0.025}px ${size * 0.06}px rgba(20,15,40,0.18))`,
    }}>
      <svg viewBox="0 0 120 120" width={size} height={size} style={{ display: 'block' }}>
        <defs>
          <clipPath id={`lbclip-${uid}`}>
            <rect x="0" y="0" width="120" height="120" rx={rOut} />
          </clipPath>
        </defs>
        <g clipPath={`url(#lbclip-${uid})`}>
          {/* flat pixel field */}
          {cells.map((c, i) => (
            <rect key={i} x={c.x * 10} y={c.y * 10} width="10" height="10" fill={c.c} />
          ))}

          {/* flat black camera brackets — all four corners */}
          <g fill="none" stroke="#0c0a14" strokeWidth="7" strokeLinecap="round" strokeLinejoin="round">
            <path d="M16 36 V26 Q16 16 26 16 H36" />
            <path d="M84 16 H94 Q104 16 104 26 V36" />
            <path d="M104 84 V94 Q104 104 94 104 H84" />
            <path d="M36 104 H26 Q16 104 16 94 V84" />
          </g>

          {/* flat red record dot with flat black ring (top-right) */}
          <circle cx="89" cy="31" r="14" fill="#0c0a14" />
          <circle cx="89" cy="31" r="10.5" fill="#ff3b30" />

          {/* hairline rim for crispness */}
          <rect x="0.5" y="0.5" width="119" height="119" rx={rOut - 0.5}
            fill="none" stroke="rgba(0,0,0,0.25)" strokeWidth="1" />
        </g>
      </svg>
    </div>
  );
}

// ── Wordmark ──────────────────────────────────────────────────
function Wordmark({ size = 22 }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
      <Logo size={size + 12} radius={Math.round((size + 12) * 0.22)} />
      <div className="lb-display" style={{ fontSize: size, fontWeight: 700, color: 'var(--lb-ink)', letterSpacing: '-0.03em' }}>
        Live<span style={{ color: 'var(--lb-block)' }}>Block</span>er
      </div>
    </div>
  );
}

// ── Surfaces ──────────────────────────────────────────────────
function Surface({ children, radius = 22, padding = 20, kind = 'raised', style = {}, ...rest }) {
  const cls = kind === 'inset' ? 'lb-inset' : kind === 'pressed' ? 'lb-pressed' : kind === 'sm' ? 'lb-raised-sm' : 'lb-raised';
  return (
    <div className={cls} style={{ borderRadius: radius, padding, ...style }} {...rest}>
      {children}
    </div>
  );
}

// ── Neumorphic pill button ────────────────────────────────────
// Variants: 'default', 'block' (red-orange filled), 'train' (aubergine filled), 'ghost'
function Pill({ children, variant = 'default', size = 'md', icon, onClick, active = false, style = {} }) {
  const padY = size === 'sm' ? 6 : size === 'lg' ? 12 : 9;
  const padX = size === 'sm' ? 12 : size === 'lg' ? 22 : 16;
  const fs = size === 'sm' ? 12 : size === 'lg' ? 15 : 13;

  let bg, color, shadow, border;
  if (variant === 'block') {
    bg = 'linear-gradient(180deg,#ff5d3f 0%, #ee3b1f 100%)';
    color = '#fff';
    shadow = '4px 4px 10px rgba(184,40,12,0.35), -3px -3px 8px #ffffff, inset 0 1px 0 rgba(255,255,255,0.45)';
    border = '0.5px solid rgba(120,20,0,0.4)';
  } else if (variant === 'train') {
    bg = 'linear-gradient(180deg,#4a2a4d 0%, #2e1631 100%)';
    color = '#f3e8ff';
    shadow = '4px 4px 10px rgba(20,10,30,0.30), -3px -3px 8px #ffffff, inset 0 1px 0 rgba(255,255,255,0.18)';
    border = '0.5px solid rgba(0,0,0,0.4)';
  } else if (variant === 'ghost') {
    bg = 'transparent';
    color = 'var(--lb-ink-2)';
    shadow = 'none';
    border = '0.5px solid transparent';
  } else {
    bg = 'var(--lb-bg)';
    color = 'var(--lb-ink)';
    shadow = active
      ? 'inset 4px 4px 8px var(--lb-shadow-dark), inset -3px -3px 6px var(--lb-shadow-light)'
      : '4px 4px 10px var(--lb-shadow-dark), -3px -3px 8px var(--lb-shadow-light)';
    border = '0.5px solid rgba(255,255,255,0.5)';
  }

  return (
    <button onClick={onClick}
      className="lb"
      style={{
        display: 'inline-flex', alignItems: 'center', gap: 7,
        padding: `${padY}px ${padX}px`,
        fontSize: fs, fontWeight: 600, letterSpacing: '-0.005em',
        background: bg, color, borderRadius: 999, border,
        boxShadow: shadow, cursor: 'pointer',
        transition: 'box-shadow .15s, transform .08s',
        ...style,
      }}>
      {icon && <span style={{ display: 'inline-flex' }}>{icon}</span>}
      {children}
    </button>
  );
}

// ── Neumorphic toggle ─────────────────────────────────────────
function Toggle({ value, onChange, accent = 'var(--lb-block)' }) {
  return (
    <button onClick={() => onChange(!value)}
      style={{
        position: 'relative', width: 46, height: 26, borderRadius: 999,
        border: '0.5px solid rgba(0,0,0,0.06)',
        background: 'var(--lb-bg)',
        boxShadow: 'inset 3px 3px 6px var(--lb-shadow-dark), inset -3px -3px 6px var(--lb-shadow-light)',
        cursor: 'pointer', padding: 0, transition: 'background .15s',
      }}>
      <span style={{
        position: 'absolute', top: 3, left: value ? 23 : 3, width: 20, height: 20, borderRadius: '50%',
        background: value ? accent : 'var(--lb-bg)',
        boxShadow: value
          ? `0 2px 4px rgba(0,0,0,0.25), inset 0 1px 0 rgba(255,255,255,0.4)`
          : '2px 2px 5px var(--lb-shadow-dark), -2px -2px 5px var(--lb-shadow-light)',
        transition: 'left .18s cubic-bezier(.3,.7,.4,1), background .15s',
      }} />
    </button>
  );
}

// ── Neumorphic slider ─────────────────────────────────────────
function Slider({ value, min = 0, max = 100, onChange, accent = 'var(--lb-block)', width = '100%' }) {
  const ref = React.useRef(null);
  const pct = ((value - min) / (max - min)) * 100;
  const handle = (e) => {
    const r = ref.current.getBoundingClientRect();
    const p = Math.max(0, Math.min(1, (e.clientX - r.left) / r.width));
    onChange(min + p * (max - min));
  };
  const onDown = (e) => {
    handle(e);
    const move = (ev) => handle(ev);
    const up = () => { window.removeEventListener('pointermove', move); window.removeEventListener('pointerup', up); };
    window.addEventListener('pointermove', move); window.addEventListener('pointerup', up);
  };
  return (
    <div ref={ref} onPointerDown={onDown}
      style={{ width, height: 22, position: 'relative', cursor: 'pointer', touchAction: 'none' }}>
      <div style={{
        position: 'absolute', top: 8, left: 0, right: 0, height: 6, borderRadius: 999,
        background: 'var(--lb-bg)',
        boxShadow: 'inset 2px 2px 4px var(--lb-shadow-dark), inset -2px -2px 4px var(--lb-shadow-light)',
      }} />
      <div style={{
        position: 'absolute', top: 8, left: 0, width: `${pct}%`, height: 6, borderRadius: 999,
        background: accent, boxShadow: '0 1px 2px rgba(0,0,0,0.15)',
      }} />
      <div style={{
        position: 'absolute', top: 0, left: `calc(${pct}% - 11px)`, width: 22, height: 22, borderRadius: '50%',
        background: 'var(--lb-bg)',
        boxShadow: '3px 3px 6px var(--lb-shadow-dark), -3px -3px 6px var(--lb-shadow-light), inset 0 1px 0 rgba(255,255,255,0.5)',
        border: '0.5px solid rgba(0,0,0,0.05)',
      }} />
    </div>
  );
}

// ── Inset field (search / value display) ──────────────────────
function Field({ children, icon, placeholder, style = {} }) {
  return (
    <div className="lb-inset" style={{
      display: 'flex', alignItems: 'center', gap: 8,
      padding: '8px 14px', borderRadius: 999, fontSize: 13,
      color: 'var(--lb-ink-2)', ...style,
    }}>
      {icon}
      {children || <span style={{ color: 'var(--lb-ink-faint)' }}>{placeholder}</span>}
    </div>
  );
}

// ── Stat chip / dot ───────────────────────────────────────────
function Dot({ color = 'var(--lb-block)', size = 8, pulse = false }) {
  return (
    <span style={{
      display: 'inline-block', width: size, height: size, borderRadius: '50%', background: color,
      boxShadow: `0 0 0 ${size/2}px ${color}33${pulse ? ',0 0 0 8px ' + color + '14' : ''}`,
    }} />
  );
}

// ── Traffic lights (custom — to match the sketch) ─────────────
function TrafficLights() {
  return (
    <div style={{ display: 'flex', gap: 8 }}>
      {['#19c332','#febc2e','#ff5e57'].map((c) => (
        <div key={c} style={{ width: 13, height: 13, borderRadius: '50%', background: c, boxShadow: 'inset 0 0 0 0.5px rgba(0,0,0,0.15), 0 1px 1px rgba(255,255,255,0.5)' }} />
      ))}
    </div>
  );
}

// ── Lavender title bar (matches the sketch's purple band) ─────
function LavenderBar({ children, leftPills, height = 56 }) {
  return (
    <div style={{
      height, padding: '0 14px',
      background: 'linear-gradient(180deg,#cdcce8 0%, #b9b8d8 100%)',
      borderTopLeftRadius: 'inherit', borderTopRightRadius: 'inherit',
      display: 'flex', alignItems: 'center', gap: 12,
      boxShadow: 'inset 0 -1px 0 rgba(0,0,0,0.07), inset 0 1px 0 rgba(255,255,255,0.4)',
    }}>
      <TrafficLights />
      <div style={{ flex: 1, display: 'flex', gap: 8, justifyContent: 'center' }}>
        {leftPills}
      </div>
      <div style={{ width: 60 }} />
    </div>
  );
}

// ── Tiny SVG icon kit (line, 1.6px) ───────────────────────────
const Icon = {
  block: (s = 16) => (
    <svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round">
      <circle cx="8" cy="8" r="6" />
      <path d="M3.8 3.8l8.4 8.4" />
    </svg>
  ),
  marker: (s = 16) => (
    <svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <rect x="2.5" y="2.5" width="11" height="11" rx="1.5" strokeDasharray="2 2" />
      <circle cx="8" cy="8" r="1.6" fill="currentColor" stroke="none" />
    </svg>
  ),
  fullscreen: (s = 16) => (
    <svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round">
      <path d="M2 6V2h4M14 6V2h-4M2 10v4h4M14 10v4h-4" />
    </svg>
  ),
  ml: (s = 16) => (
    <svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round">
      <circle cx="4" cy="4" r="1.5" /><circle cx="12" cy="4" r="1.5" />
      <circle cx="4" cy="12" r="1.5" /><circle cx="12" cy="12" r="1.5" />
      <circle cx="8" cy="8" r="2" />
      <path d="M5.4 5.4L6.5 6.5M10.6 5.4L9.5 6.5M5.4 10.6L6.5 9.5M10.6 10.6L9.5 9.5" />
    </svg>
  ),
  settings: (s = 16) => (
    <svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="8" cy="8" r="2.2" />
      <path d="M8 1.5v2M8 12.5v2M14.5 8h-2M3.5 8h-2M12.6 3.4L11.2 4.8M4.8 11.2L3.4 12.6M12.6 12.6L11.2 11.2M4.8 4.8L3.4 3.4" />
    </svg>
  ),
  pause: (s = 16) => (
    <svg width={s} height={s} viewBox="0 0 16 16" fill="currentColor"><rect x="4" y="3" width="3" height="10" rx="1" /><rect x="9" y="3" width="3" height="10" rx="1" /></svg>
  ),
  play: (s = 16) => (
    <svg width={s} height={s} viewBox="0 0 16 16" fill="currentColor"><path d="M4 3l9 5-9 5z" /></svg>
  ),
  rec: (s = 14) => (
    <svg width={s} height={s} viewBox="0 0 14 14"><circle cx="7" cy="7" r="5" fill="#ff4a2e" /></svg>
  ),
  search: (s = 14) => (
    <svg width={s} height={s} viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round">
      <circle cx="6" cy="6" r="4" /><path d="M9 9l3 3" />
    </svg>
  ),
  plus: (s = 14) => (
    <svg width={s} height={s} viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round">
      <path d="M7 2v10M2 7h10" />
    </svg>
  ),
  trash: (s = 14) => (
    <svg width={s} height={s} viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="M2.5 4h9M5 4V2.5h4V4M3.5 4l.5 8h6l.5-8" />
    </svg>
  ),
  drag: (s = 14) => (
    <svg width={s} height={s} viewBox="0 0 14 14" fill="currentColor">
      <circle cx="5" cy="3" r="1" /><circle cx="9" cy="3" r="1" />
      <circle cx="5" cy="7" r="1" /><circle cx="9" cy="7" r="1" />
      <circle cx="5" cy="11" r="1" /><circle cx="9" cy="11" r="1" />
    </svg>
  ),
  check: (s = 14) => (
    <svg width={s} height={s} viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M3 7.5L6 10.5 11 4.5" />
    </svg>
  ),
  shield: (s = 16) => (
    <svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinejoin="round">
      <path d="M8 1.5L2.5 3.5v4.2c0 3.4 2.4 5.6 5.5 6.8 3.1-1.2 5.5-3.4 5.5-6.8V3.5L8 1.5z"/>
    </svg>
  ),
  cpu: (s = 16) => (
    <svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
      <rect x="4" y="4" width="8" height="8" rx="1.2" />
      <rect x="6" y="6" width="4" height="4" rx="0.5" />
      <path d="M6 1.5v2M10 1.5v2M6 12.5v2M10 12.5v2M1.5 6h2M1.5 10h2M12.5 6h2M12.5 10h2" />
    </svg>
  ),
  arrow: (s = 14) => (
    <svg width={s} height={s} viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <path d="M2 7h10M8 3l4 4-4 4" />
    </svg>
  ),
};

// ── Pixel-tile background pattern ─────────────────────────────
// Used as a subtle texture for "screen pixels" sections. Renders a
// 8-wide grid of muted colored squares, very low opacity by default.
function PixelTile({ size = 8, cellsX = 24, cellsY = 12, opacity = 0.18, palette }) {
  const colors = palette || ['#ff4a2e','#ffd23a','#2bbf6c','#3da6ff','#7c5cff','#ff5da7','#ff8a3d','#1ec8c8','#a8e10c','#dfe0ec','#b8b9cb','#ecedf5'];
  let s = 91;
  const rnd = () => { s = (s * 9301 + 49297) % 233280; return s / 233280; };
  const cells = [];
  for (let y = 0; y < cellsY; y++) for (let x = 0; x < cellsX; x++) cells.push({ x, y, c: colors[Math.floor(rnd() * colors.length)] });
  return (
    <svg viewBox={`0 0 ${cellsX*size} ${cellsY*size}`} width="100%" height="100%"
      preserveAspectRatio="none" style={{ display: 'block', opacity }}>
      {cells.map((c, i) => <rect key={i} x={c.x*size} y={c.y*size} width={size} height={size} fill={c.c} />)}
    </svg>
  );
}

Object.assign(window, { Logo, Wordmark, Surface, Pill, Toggle, Slider, Field, Dot, TrafficLights, LavenderBar, Icon, PixelTile });
