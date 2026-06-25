// LiveBlocker v4 — humanized primitives.
// Sans-serif first; mono reserved for live numbers / hotkeys / code.
// Every component exposed via window.* at the bottom.

// ── Logo / brand ──────────────────────────────────────────────
// A solid block with a hollow corner — "shape behind a window".
function Logo({ size = 40, accent = 'var(--accent)' }) {
  return (
    <svg width={size} height={size} viewBox="0 0 40 40" style={{ display: 'block' }}>
      <defs>
        <linearGradient id="lb-logo-grad" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stopColor="#ff6a55" />
          <stop offset="100%" stopColor="#e23a25" />
        </linearGradient>
      </defs>
      <rect x="0" y="0" width="40" height="40" rx="9" fill="url(#lb-logo-grad)" />
      {/* corner cut-out — the "blocked region" */}
      <rect x="20" y="6" width="14" height="14" rx="3" fill="var(--bg)" />
      {/* sight ticks */}
      <rect x="6"  y="6"  width="6" height="2" fill="rgba(255,255,255,0.85)" />
      <rect x="6"  y="6"  width="2" height="6" fill="rgba(255,255,255,0.85)" />
      <rect x="6"  y="32" width="6" height="2" fill="rgba(255,255,255,0.85)" />
      <rect x="6"  y="28" width="2" height="6" fill="rgba(255,255,255,0.85)" />
      <rect x="28" y="32" width="6" height="2" fill="rgba(255,255,255,0.85)" />
      <rect x="32" y="28" width="2" height="6" fill="rgba(255,255,255,0.85)" />
    </svg>
  );
}

function Wordmark({ size = 16, showLogo = true, color }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 9 }}>
      {showLogo && <Logo size={size + 8} />}
      <span style={{
        fontFamily: 'var(--font-sans)',
        fontWeight: 700,
        fontSize: size,
        letterSpacing: '-0.018em',
        color: color || 'var(--ink-1)',
        lineHeight: 1,
      }}>
        Live<span style={{ color: 'var(--accent)' }}>Blocker</span>
      </span>
    </div>
  );
}

// ── Status dot (with optional ring pulse) ─────────────────────
function Dot({ color = 'var(--success)', size = 8, pulse = false, ring = false }) {
  return (
    <span style={{ position:'relative', display:'inline-flex', width:size, height:size, alignItems:'center', justifyContent:'center' }}>
      {ring && (
        <span style={{
          position:'absolute', width:size, height:size, borderRadius:'50%',
          background: color, animation: 'pulse-ring 1.6s ease-out infinite', opacity: 0.5,
        }}/>
      )}
      <span className={pulse ? 'pulse' : ''} style={{
        width: size, height: size, borderRadius: '50%', background: color,
        boxShadow: `0 0 0 3px ${color}1a`,
      }}/>
    </span>
  );
}

// ── Pill / chip ───────────────────────────────────────────────
function Pill({ children, tone = 'neutral', icon, dot, size = 'md', style }) {
  const tones = {
    neutral: { bg:'var(--surface-3)',  fg:'var(--ink-2)',     bd:'var(--line)' },
    accent:  { bg:'var(--accent-soft)', fg:'var(--accent)',   bd:'transparent' },
    success: { bg:'var(--success-soft)',fg:'var(--success)',  bd:'transparent' },
    info:    { bg:'var(--info-soft)',   fg:'var(--info)',     bd:'transparent' },
    ml:      { bg:'var(--ml-soft)',     fg:'var(--ml)',       bd:'transparent' },
    warn:    { bg:'var(--warn-soft)',   fg:'var(--warn)',     bd:'transparent' },
    ghost:   { bg:'transparent',        fg:'var(--ink-3)',    bd:'var(--line-2)' },
  };
  const t = tones[tone] || tones.neutral;
  const sm = size === 'sm';
  return (
    <span style={{
      display:'inline-flex', alignItems:'center', gap: 6,
      padding: sm ? '2px 7px' : '4px 10px',
      borderRadius:'var(--r-pill)',
      background:t.bg, color:t.fg, border:`1px solid ${t.bd}`,
      fontSize: sm ? 11 : 12, fontWeight:600, letterSpacing:'-0.005em',
      lineHeight:1, whiteSpace:'nowrap', ...style,
    }}>
      {dot && <Dot color={t.fg} size={6} pulse={dot==='pulse'} />}
      {icon}{children}
    </span>
  );
}

// ── Button ────────────────────────────────────────────────────
// variants: primary | secondary | ghost | danger | accent
function Btn({ children, icon, iconRight, kbd, variant='secondary', size='md', full, onClick, disabled, style }) {
  const sizes = {
    sm: { h:28, px:11, fs:12, gap:6, kbd:10 },
    md: { h:34, px:14, fs:13, gap:8, kbd:11 },
    lg: { h:40, px:18, fs:14, gap:9, kbd:12 },
  };
  const s = sizes[size];
  const styles = {
    primary:   { bg:'var(--accent)',     fg:'#fff',                bd:'var(--accent)' },
    secondary: { bg:'var(--surface-3)',  fg:'var(--ink-1)',        bd:'var(--line-2)' },
    ghost:     { bg:'transparent',       fg:'var(--ink-2)',        bd:'transparent'   },
    outline:   { bg:'transparent',       fg:'var(--ink-1)',        bd:'var(--line-2)' },
    accent:    { bg:'var(--accent-soft)',fg:'var(--accent)',       bd:'transparent'   },
  };
  const v = styles[variant] || styles.secondary;
  return (
    <button onClick={onClick} disabled={disabled} style={{
      display:'inline-flex', alignItems:'center', justifyContent: full ? 'center' : 'flex-start', gap: s.gap,
      height: s.h, padding: `0 ${s.px}px`, width: full ? '100%' : 'auto',
      background: v.bg, color: v.fg,
      border: `1px solid ${v.bd}`, borderRadius:'var(--r-3)',
      fontSize: s.fs, fontWeight: 600, letterSpacing:'-0.005em',
      fontFamily:'var(--font-sans)', cursor: disabled ? 'not-allowed' : 'pointer', opacity: disabled ? 0.5 : 1,
      transition:'background 0.12s ease, transform 0.08s ease', lineHeight:1, ...style,
    }}>
      {icon && <span style={{display:'inline-flex'}}>{icon}</span>}
      <span>{children}</span>
      {iconRight && <span style={{display:'inline-flex'}}>{iconRight}</span>}
      {kbd && <Kbd size={s.kbd} style={{
        marginLeft: 2,
        background: variant === 'primary' ? 'rgba(255,255,255,0.18)' : 'rgba(255,255,255,0.06)',
        color: variant === 'primary' ? 'rgba(255,255,255,0.9)' : 'var(--ink-3)',
        borderColor: variant === 'primary' ? 'rgba(255,255,255,0.14)' : 'var(--line)',
      }}>{kbd}</Kbd>}
    </button>
  );
}

// ── Kbd ───────────────────────────────────────────────────────
function Kbd({ children, size = 11, style }) {
  return (
    <span className="mono" style={{
      display:'inline-flex', alignItems:'center', justifyContent:'center',
      minWidth: size + 6, height: size + 6, padding:'0 5px',
      background:'var(--surface-3)', color:'var(--ink-2)',
      border:'1px solid var(--line)', borderRadius:'var(--r-1)',
      fontSize: size, fontWeight: 500, lineHeight: 1, ...style,
    }}>{children}</span>
  );
}

// ── Toggle (sleek switch) ─────────────────────────────────────
function Toggle({ value, onChange, size = 'md' }) {
  const w = size === 'sm' ? 30 : size === 'lg' ? 44 : 36;
  const h = size === 'sm' ? 18 : size === 'lg' ? 24 : 20;
  const knob = h - 4;
  return (
    <button onClick={() => onChange && onChange(!value)} style={{
      width: w, height: h, padding: 0,
      borderRadius: 999, border:'1px solid ' + (value ? 'transparent' : 'var(--line-2)'),
      background: value ? 'var(--accent)' : 'var(--surface-3)',
      position:'relative', transition:'background 0.18s ease',
      flexShrink: 0,
    }}>
      <span style={{
        position:'absolute', top: 1, left: value ? w - knob - 3 : 1,
        width: knob, height: knob, borderRadius:'50%',
        background:'#fff', transition:'left 0.18s ease',
        boxShadow:'0 1px 3px rgba(0,0,0,0.4)',
      }}/>
    </button>
  );
}

// ── Slider ────────────────────────────────────────────────────
function Slider({ value, min = 0, max = 100, step = 1, onChange, accent = 'var(--accent)', marks }) {
  const ref = React.useRef(null);
  const pct = ((value - min) / (max - min)) * 100;
  const move = (e) => {
    const r = ref.current.getBoundingClientRect();
    const p = Math.max(0, Math.min(1, (e.clientX - r.left) / r.width));
    let v = min + p * (max - min);
    v = Math.round(v / step) * step;
    onChange && onChange(v);
  };
  const down = (e) => {
    move(e);
    const mv = (ev) => move(ev);
    const up = () => { window.removeEventListener('pointermove', mv); window.removeEventListener('pointerup', up); };
    window.addEventListener('pointermove', mv); window.addEventListener('pointerup', up);
  };
  return (
    <div ref={ref} onPointerDown={down} style={{
      position:'relative', height: 22, width:'100%', cursor:'pointer', touchAction:'none',
      display:'flex', alignItems:'center',
    }}>
      <div style={{ position:'absolute', inset:'10px 0', borderRadius: 999, background:'var(--surface-3)' }}/>
      <div style={{
        position:'absolute', left: 0, top: 10, height: 2, width: `${pct}%`,
        borderRadius: 999, background: accent,
      }}/>
      {marks && marks.map((m, i) => {
        const mp = ((m - min) / (max - min)) * 100;
        return <div key={i} style={{
          position:'absolute', left: `${mp}%`, top: 6, width: 1, height: 10,
          background:'var(--line-strong)', transform:'translateX(-0.5px)',
        }}/>;
      })}
      <div style={{
        position:'absolute', left: `${pct}%`, top: '50%',
        width: 18, height: 18, transform:'translate(-50%, -50%)',
        background: '#fff', border: `2px solid ${accent}`,
        borderRadius:'50%', boxShadow:'0 2px 6px rgba(0,0,0,0.4)',
      }}/>
    </div>
  );
}

// ── Field / input ─────────────────────────────────────────────
function Field({ icon, value, placeholder, rightKbd, onChange, size = 'md', style }) {
  const sizes = { sm: 30, md: 34, lg: 40 };
  return (
    <div style={{
      display:'flex', alignItems:'center', gap: 8,
      height: sizes[size], padding:'0 10px',
      background:'var(--surface-2)', border:'1px solid var(--line)', borderRadius:'var(--r-3)',
      ...style,
    }}>
      {icon && <span style={{ color: 'var(--ink-3)', display:'inline-flex' }}>{icon}</span>}
      <input
        value={value || ''} placeholder={placeholder}
        onChange={(e) => onChange && onChange(e.target.value)}
        style={{
          flex: 1, background:'transparent', border:'none', outline:'none',
          fontFamily:'var(--font-sans)', fontSize: 13, color:'var(--ink-1)',
          minWidth: 0,
        }}
      />
      {rightKbd && <Kbd>{rightKbd}</Kbd>}
    </div>
  );
}

// ── Segmented control ─────────────────────────────────────────
function Segmented({ value, options, onChange, size = 'md' }) {
  const h = size === 'sm' ? 26 : 32;
  return (
    <div style={{
      display:'inline-flex', padding: 3, height: h,
      background:'var(--surface-3)', borderRadius:'var(--r-3)',
      border:'1px solid var(--line)',
    }}>
      {options.map((o) => {
        const v = typeof o === 'string' ? o : o.value;
        const l = typeof o === 'string' ? o : o.label;
        const active = v === value;
        return (
          <button key={v} onClick={() => onChange(v)} style={{
            height: h - 6, padding:'0 12px',
            background: active ? 'var(--surface-4)' : 'transparent',
            color: active ? 'var(--ink-1)' : 'var(--ink-3)',
            border:'none', borderRadius:'var(--r-2)',
            fontSize: 12, fontWeight: 600, letterSpacing:'-0.005em',
            display:'inline-flex', alignItems:'center', gap: 6,
          }}>{l}</button>
        );
      })}
    </div>
  );
}

// ── Stat block (display number + label) ───────────────────────
// Use for the big counters. Mono number, sans label.
function Stat({ value, unit, label, delta, deltaTone='success', sub, accent='var(--ink-1)', size = 'md' }) {
  const sizes = {
    sm: { num: 26, unit: 12, lbl: 11 },
    md: { num: 36, unit: 14, lbl: 12 },
    lg: { num: 48, unit: 16, lbl: 13 },
    xl: { num: 60, unit: 18, lbl: 13 },
  };
  const s = sizes[size];
  const deltaColor = {
    success: 'var(--success)',
    warn: 'var(--warn)',
    accent: 'var(--accent)',
    info: 'var(--info)',
    neutral: 'var(--ink-3)',
  }[deltaTone];
  return (
    <div>
      {label && <div className="caption" style={{ marginBottom: 6 }}>{label}</div>}
      <div style={{ display:'flex', alignItems:'baseline', gap: 6 }}>
        <span className="mono tnum" style={{
          fontSize: s.num, fontWeight: 600, letterSpacing:'-0.04em', color: accent, lineHeight: 0.95,
        }}>{value}</span>
        {unit && <span className="mono" style={{ fontSize: s.unit, color:'var(--ink-3)' }}>{unit}</span>}
        {delta && (
          <span style={{
            marginLeft: 6, fontSize: 12, fontWeight: 600, color: deltaColor,
            display:'inline-flex', alignItems:'center', gap: 3,
          }}>{delta}</span>
        )}
      </div>
      {sub && <div style={{ fontSize: 12, color:'var(--ink-3)', marginTop: 6 }}>{sub}</div>}
    </div>
  );
}

// ── Sparkline ─────────────────────────────────────────────────
function Sparkline({ data, width = 200, height = 48, color = 'var(--accent)', fill = true, dots = false }) {
  if (!data || !data.length) return null;
  const max = Math.max(...data), min = Math.min(...data);
  const range = max - min || 1;
  const w = width, h = height;
  const pts = data.map((v, i) => {
    const x = (i / (data.length - 1)) * w;
    const y = h - ((v - min) / range) * (h - 6) - 3;
    return [x, y];
  });
  const path = pts.map(([x,y], i) => `${i?'L':'M'}${x.toFixed(2)} ${y.toFixed(2)}`).join(' ');
  const fillPath = path + ` L ${w} ${h} L 0 ${h} Z`;
  const uid = React.useId();
  return (
    <svg width={w} height={h} viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none" style={{display:'block'}}>
      {fill && (
        <>
          <defs>
            <linearGradient id={`sp-${uid}`} x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%"   stopColor={color} stopOpacity="0.32"/>
              <stop offset="100%" stopColor={color} stopOpacity="0"/>
            </linearGradient>
          </defs>
          <path d={fillPath} fill={`url(#sp-${uid})`}/>
        </>
      )}
      <path d={path} fill="none" stroke={color} strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round"/>
      {dots && pts.map(([x,y], i) => i === pts.length - 1 && (
        <circle key={i} cx={x} cy={y} r="3" fill={color}/>
      ))}
    </svg>
  );
}

// ── Bar chart (vertical) ──────────────────────────────────────
function BarChart({ data, width = 320, height = 80, color = 'var(--accent)', gap = 3, accents }) {
  if (!data || !data.length) return null;
  const max = Math.max(...data) || 1;
  const bw = (width - gap * (data.length - 1)) / data.length;
  return (
    <svg width={width} height={height} style={{ display:'block' }}>
      {data.map((v, i) => {
        const bh = Math.max(2, (v / max) * height);
        return (
          <rect key={i} x={i * (bw + gap)} y={height - bh}
            width={bw} height={bh} rx="2"
            fill={accents ? accents[i] : color} opacity="0.85"/>
        );
      })}
    </svg>
  );
}

// ── Progress bar ──────────────────────────────────────────────
function Progress({ value = 0, color = 'var(--accent)', height = 6, track = 'var(--surface-3)' }) {
  return (
    <div style={{ width:'100%', height, background: track, borderRadius: 999, overflow:'hidden' }}>
      <div style={{ width: `${Math.max(0, Math.min(100, value * 100))}%`, height:'100%', background: color, borderRadius: 999 }}/>
    </div>
  );
}

// ── Crosshair (for floating tool screens) ─────────────────────
function Crosshair({ size = 22, color = 'var(--accent)' }) {
  return (
    <svg width={size} height={size} viewBox="0 0 22 22" style={{ display:'block' }}>
      <g stroke={color} strokeWidth="1.4" fill="none">
        <path d="M11 0 V8 M11 14 V22 M0 11 H8 M14 11 H22"/>
      </g>
      <circle cx="11" cy="11" r="2" fill={color}/>
    </svg>
  );
}

// ── Section header (used in big screens) ─────────────────────
function SectionHeader({ title, sub, right }) {
  return (
    <div style={{ display:'flex', alignItems:'flex-start', gap: 16, marginBottom: 14 }}>
      <div style={{ flex: 1 }}>
        <h3 style={{
          margin: 0, fontSize: 17, fontWeight: 600, letterSpacing:'-0.015em', color:'var(--ink-1)',
        }}>{title}</h3>
        {sub && <div style={{ fontSize: 13, color:'var(--ink-3)', marginTop: 3, lineHeight: 1.45 }}>{sub}</div>}
      </div>
      {right}
    </div>
  );
}

// ── Avatar / app icon (placeholder coloured square) ──────────
function AppIcon({ size = 22, color, letter, radius = 6 }) {
  return (
    <div style={{
      width: size, height: size, borderRadius: radius,
      background: color || 'var(--surface-3)',
      display:'inline-flex', alignItems:'center', justifyContent:'center',
      fontSize: Math.round(size * 0.5), fontWeight: 700, color:'#fff', letterSpacing:'-0.02em',
      flexShrink: 0,
    }}>{letter}</div>
  );
}

// ── Icon kit (1.4 stroke, rounded) ───────────────────────────
const I = {
  block:   (s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round"><circle cx="8" cy="8" r="6"/><path d="M3.5 3.5 L12.5 12.5"/></svg>),
  shield:  (s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round"><path d="M8 1.5L2.5 3.5v4c0 3.4 2.4 5.8 5.5 7 3.1-1.2 5.5-3.6 5.5-7v-4z"/></svg>),
  check:   (s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round"><path d="M3 8.5l3 3 7-7"/></svg>),
  x:       (s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round"><path d="M3.5 3.5 L12.5 12.5 M12.5 3.5 L3.5 12.5"/></svg>),
  plus:    (s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round"><path d="M8 2.5v11 M2.5 8h11"/></svg>),
  arrow:   (s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round"><path d="M3 8h10 M9 4l4 4-4 4"/></svg>),
  search:  (s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round"><circle cx="7" cy="7" r="4.2"/><path d="M10 10l3.5 3.5"/></svg>),
  settings:(s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round"><circle cx="8" cy="8" r="2.2"/><path d="M8 1.5v2 M8 12.5v2 M1.5 8h2 M12.5 8h2 M3.4 3.4l1.4 1.4 M11.2 11.2l1.4 1.4 M3.4 12.6l1.4-1.4 M11.2 4.8l1.4-1.4"/></svg>),
  spark:   (s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round"><path d="M8 1.5l1.4 4 4.1 1.5-4.1 1.5L8 12.5 6.6 8.5 2.5 7l4.1-1.5z"/></svg>),
  pause:   (s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="currentColor"><rect x="4" y="3" width="3" height="10" rx="1"/><rect x="9" y="3" width="3" height="10" rx="1"/></svg>),
  play:    (s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="currentColor"><path d="M4 2.5l9 5.5-9 5.5z"/></svg>),
  cpu:     (s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round"><rect x="3.5" y="3.5" width="9" height="9" rx="1.5"/><rect x="6" y="6" width="4" height="4" rx="0.5"/><path d="M6 1.5v2 M10 1.5v2 M6 12.5v2 M10 12.5v2 M1.5 6h2 M1.5 10h2 M12.5 6h2 M12.5 10h2"/></svg>),
  zap:     (s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="currentColor"><path d="M9 1.5L3 9h3.5l-1 5.5L13 7H9z"/></svg>),
  region:  (s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeDasharray="3 2" strokeLinejoin="round"><rect x="2" y="2" width="12" height="12" rx="1.5"/></svg>),
  layers:  (s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round"><path d="M8 1.5L1.5 5 8 8.5 14.5 5z"/><path d="M2 8.5L8 12l6-3.5 M2 11.5L8 15l6-3.5"/></svg>),
  list:    (s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round"><path d="M2.5 4h11 M2.5 8h11 M2.5 12h11"/></svg>),
  grid:    (s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4"><rect x="2.5" y="2.5" width="4.5" height="4.5" rx="1"/><rect x="9" y="2.5" width="4.5" height="4.5" rx="1"/><rect x="2.5" y="9" width="4.5" height="4.5" rx="1"/><rect x="9" y="9" width="4.5" height="4.5" rx="1"/></svg>),
  drag:    (s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="currentColor"><circle cx="6" cy="4" r="1"/><circle cx="10" cy="4" r="1"/><circle cx="6" cy="8" r="1"/><circle cx="10" cy="8" r="1"/><circle cx="6" cy="12" r="1"/><circle cx="10" cy="12" r="1"/></svg>),
  trash:   (s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round"><path d="M2.5 4.5h11 M6 4.5V3h4v1.5 M4 4.5l.7 9h6.6l.7-9 M6.5 7v4 M9.5 7v4"/></svg>),
  more:    (s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="currentColor"><circle cx="3" cy="8" r="1.4"/><circle cx="8" cy="8" r="1.4"/><circle cx="13" cy="8" r="1.4"/></svg>),
  filter:  (s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round"><path d="M2 3.5h12 L10 9 v4.5 L6 12 V9z"/></svg>),
  globe:   (s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4"><circle cx="8" cy="8" r="6"/><path d="M2 8h12 M8 2c2.2 2.4 2.2 9.6 0 12 M8 2c-2.2 2.4-2.2 9.6 0 12"/></svg>),
  bell:    (s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round"><path d="M4 12V7a4 4 0 0 1 8 0v5 M2.5 12h11 M6.5 14a1.5 1.5 0 0 0 3 0"/></svg>),
  external:(s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round"><path d="M6 3.5H3v9.5h9.5V10 M9.5 3.5H13V7 M7.5 8.5L13 3"/></svg>),
  history: (s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round"><path d="M2.5 4.5V8h3.5 M2.5 8a5.5 5.5 0 1 0 1.6-3.9L2.5 6 M8 5v3.5L10 10"/></svg>),
  lock:    (s=16) => (<svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round"><rect x="3" y="7" width="10" height="7" rx="1.5"/><path d="M5 7V5a3 3 0 0 1 6 0v2"/></svg>),
};

Object.assign(window, {
  Logo, Wordmark, Dot, Pill, Btn, Kbd, Toggle, Slider, Field, Segmented,
  Stat, Sparkline, BarChart, Progress, Crosshair, SectionHeader, AppIcon, I,
});
