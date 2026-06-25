// LiveBlocker — additional brutalist primitives:
// Slider, Field, Kbd, Bar meter, Crosshair, Sparkline, Icon kit, Tab strip.

// ── Slider ────────────────────────────────────────────────────
function Slider({ value, min=0, max=100, onChange, accent='var(--lb-block)', step=1, width='100%' }) {
  const ref = React.useRef(null);
  const pct = ((value - min) / (max - min)) * 100;
  const handle = (e) => {
    const r = ref.current.getBoundingClientRect();
    const p = Math.max(0, Math.min(1, (e.clientX - r.left) / r.width));
    let v = min + p * (max - min);
    v = Math.round(v / step) * step;
    onChange(v);
  };
  const onDown = (e) => {
    handle(e);
    const m = (ev) => handle(ev);
    const u = () => { window.removeEventListener('pointermove', m); window.removeEventListener('pointerup', u); };
    window.addEventListener('pointermove', m); window.addEventListener('pointerup', u);
  };
  return (
    <div ref={ref} onPointerDown={onDown} style={{
      position:'relative', height:18, width, cursor:'pointer', touchAction:'none',
      display:'flex', alignItems:'center',
    }}>
      <div style={{position:'absolute', left:0, right:0, height:1, background:'var(--lb-rule-2)'}}/>
      {/* tick marks */}
      <div style={{position:'absolute', left:0, right:0, top:0, bottom:0, display:'flex', justifyContent:'space-between'}}>
        {Array.from({length:21}).map((_,i)=> (
          <div key={i} style={{width:1, height: i%5===0 ? 10 : 5, background:'var(--lb-rule-2)', alignSelf:'center'}}/>
        ))}
      </div>
      <div style={{position:'absolute', left:0, height:1, width:`${pct}%`, background:accent, top:'50%', transform:'translateY(-50%)'}}/>
      <div style={{
        position:'absolute', left:`calc(${pct}% - 4px)`, top:'50%', transform:'translateY(-50%)',
        width:8, height:14, background:accent, border:'1px solid var(--lb-ink)',
      }}/>
    </div>
  );
}

// ── Kbd ───────────────────────────────────────────────────────
function Kbd({ children, style = {} }) {
  return (
    <span className="lb-mono" style={{
      display:'inline-flex', alignItems:'center', justifyContent:'center',
      minWidth:18, height:18, padding:'0 5px',
      border:'1px solid var(--lb-rule-2)', background:'var(--lb-panel-2)',
      color:'var(--lb-ink-2)', fontSize:10, fontWeight:500, letterSpacing:'0.02em',
      lineHeight:1, ...style,
    }}>{children}</span>
  );
}

// ── Field ─────────────────────────────────────────────────────
function Field({ icon, value, placeholder, rightKbd, width='100%', style={} }) {
  return (
    <div className="lb" style={{
      display:'flex', alignItems:'center', gap:8, padding:'6px 10px',
      border:'1px solid var(--lb-rule-2)', background:'var(--lb-panel-2)',
      width, fontSize:12, color:'var(--lb-ink-2)', ...style,
    }}>
      {icon && <span style={{color:'var(--lb-ink-muted)', display:'inline-flex'}}>{icon}</span>}
      <span style={{flex:1, color: value ? 'var(--lb-ink)' : 'var(--lb-ink-faint)'}}>
        {value || placeholder}
      </span>
      {rightKbd && <Kbd>{rightKbd}</Kbd>}
    </div>
  );
}

// ── Bar meter (ASCII block bars) ──────────────────────────────
// length=12, fill 0..1
function BarMeter({ value=0.5, length=14, accent='var(--lb-block)', dim='var(--lb-ink-dim)' }) {
  const filled = Math.round(value * length);
  const bars = [];
  for (let i=0; i<length; i++) bars.push(i < filled);
  return (
    <span className="lb-mono lb-bars" style={{ fontSize: 12, letterSpacing:'-0.02em' }}>
      {bars.map((b,i) => (
        <span key={i} style={{ color: b ? accent : dim }}>█</span>
      ))}
    </span>
  );
}

// ── Crosshair — used as cursor companion in floating tool ────
function Crosshair({ size=22, color='var(--lb-block)', style={} }) {
  return (
    <svg width={size} height={size} viewBox="0 0 22 22" style={{display:'block', ...style}}>
      <g stroke={color} strokeWidth="1" fill="none">
        <path d="M11 0 V8 M11 14 V22 M0 11 H8 M14 11 H22"/>
      </g>
      <rect x="9" y="9" width="4" height="4" fill={color}/>
    </svg>
  );
}

// ── Sparkline ─────────────────────────────────────────────────
function Sparkline({ data, width=160, height=32, color='var(--lb-block)', fill=true }) {
  if (!data || !data.length) return null;
  const max = Math.max(...data), min = Math.min(...data);
  const range = max - min || 1;
  const pts = data.map((v, i) => {
    const x = (i / (data.length - 1)) * width;
    const y = height - ((v - min) / range) * (height - 4) - 2;
    return [x, y];
  });
  const path = pts.map(([x,y], i) => `${i?'L':'M'}${x.toFixed(1)} ${y.toFixed(1)}`).join(' ');
  const fillPath = path + ` L ${width} ${height} L 0 ${height} Z`;
  return (
    <svg width={width} height={height} style={{display:'block'}} preserveAspectRatio="none" viewBox={`0 0 ${width} ${height}`}>
      {fill && <path d={fillPath} fill={color} opacity="0.10"/>}
      <path d={path} fill="none" stroke={color} strokeWidth="1"/>
      {pts.map(([x,y], i) => i === pts.length - 1 && (
        <rect key={i} x={x-2} y={y-2} width="4" height="4" fill={color}/>
      ))}
    </svg>
  );
}

// ── Tabs ──────────────────────────────────────────────────────
function Tabs({ items, value, onChange, style={} }) {
  return (
    <div className="lb" style={{display:'flex', gap:0, borderBottom:'1px solid var(--lb-rule)', ...style}}>
      {items.map((it) => {
        const active = it.id === value;
        return (
          <button key={it.id} onClick={() => onChange && onChange(it.id)} style={{
            padding:'8px 14px', background:'transparent', border:'none',
            borderBottom: active ? '2px solid var(--lb-block)' : '2px solid transparent',
            marginBottom:-1, color: active ? 'var(--lb-ink)' : 'var(--lb-ink-muted)',
            fontFamily:'var(--lb-font-mono)', fontSize:11, fontWeight: active ? 600 : 500,
            letterSpacing:'0.06em', textTransform:'uppercase', cursor:'pointer',
            display:'inline-flex', alignItems:'center', gap:7,
          }}>
            {it.icon}
            {it.label}
            {it.badge != null && (
              <span className="lb-mono" style={{
                fontSize:9, padding:'1px 4px',
                background: active ? 'var(--lb-block)' : 'var(--lb-panel-3)',
                color: active ? '#0a0a0c' : 'var(--lb-ink-muted)',
              }}>{it.badge}</span>
            )}
          </button>
        );
      })}
    </div>
  );
}

// ── Icon kit (1px line) ───────────────────────────────────────
const Icon = {
  block: (s=14) => (<svg width={s} height={s} viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1"><rect x="1.5" y="1.5" width="11" height="11"/><path d="M2.5 2.5 L11.5 11.5"/></svg>),
  crosshair: (s=14) => (<svg width={s} height={s} viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1"><circle cx="7" cy="7" r="3.5"/><path d="M7 1V4 M7 10V13 M1 7H4 M10 7H13"/></svg>),
  ml: (s=14) => (<svg width={s} height={s} viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1"><rect x="1.5" y="1.5" width="3" height="3"/><rect x="9.5" y="1.5" width="3" height="3"/><rect x="1.5" y="9.5" width="3" height="3"/><rect x="9.5" y="9.5" width="3" height="3"/><circle cx="7" cy="7" r="1.5"/><path d="M3 5V9 M11 5V9 M5 3H9 M5 11H9 M4.5 4.5L6 6 M9.5 4.5L8 6 M4.5 9.5L6 8 M9.5 9.5L8 8"/></svg>),
  shield: (s=14) => (<svg width={s} height={s} viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1"><path d="M7 1L2 3v4c0 3 2.2 5 5 6 2.8-1 5-3 5-6V3z"/></svg>),
  zap: (s=14) => (<svg width={s} height={s} viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1" strokeLinejoin="miter"><path d="M8 1L3 7h3l-1 6 5-6H7z"/></svg>),
  cpu: (s=14) => (<svg width={s} height={s} viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1"><rect x="3" y="3" width="8" height="8"/><rect x="5" y="5" width="4" height="4"/><path d="M5 1v2 M9 1v2 M5 11v2 M9 11v2 M1 5h2 M1 9h2 M11 5h2 M11 9h2"/></svg>),
  search: (s=14) => (<svg width={s} height={s} viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1"><circle cx="6" cy="6" r="3.5"/><path d="M8.5 8.5L12 12"/></svg>),
  plus: (s=14) => (<svg width={s} height={s} viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.2"><path d="M7 2v10 M2 7h10"/></svg>),
  arrow: (s=14) => (<svg width={s} height={s} viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1"><path d="M2 7h10 M8 3l4 4-4 4"/></svg>),
  arrowDown: (s=14) => (<svg width={s} height={s} viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1"><path d="M7 2v10 M3 8l4 4 4-4"/></svg>),
  check: (s=14) => (<svg width={s} height={s} viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="square"><path d="M2 7l3.5 3.5L12 4"/></svg>),
  x: (s=14) => (<svg width={s} height={s} viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1"><path d="M3 3l8 8 M11 3l-8 8"/></svg>),
  pause: (s=14) => (<svg width={s} height={s} viewBox="0 0 14 14" fill="currentColor"><rect x="3" y="3" width="2.5" height="8"/><rect x="8.5" y="3" width="2.5" height="8"/></svg>),
  play: (s=14) => (<svg width={s} height={s} viewBox="0 0 14 14" fill="currentColor"><path d="M3 2l9 5-9 5z"/></svg>),
  drag: (s=14) => (<svg width={s} height={s} viewBox="0 0 14 14" fill="currentColor"><rect x="4" y="3" width="2" height="2"/><rect x="8" y="3" width="2" height="2"/><rect x="4" y="6" width="2" height="2"/><rect x="8" y="6" width="2" height="2"/><rect x="4" y="9" width="2" height="2"/><rect x="8" y="9" width="2" height="2"/></svg>),
  trash: (s=14) => (<svg width={s} height={s} viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1"><path d="M2 4h10 M5 4V2.5h4V4 M3.5 4l.5 8h6l.5-8"/></svg>),
  settings: (s=14) => (<svg width={s} height={s} viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1"><circle cx="7" cy="7" r="2"/><path d="M7 1v2 M7 11v2 M1 7h2 M11 7h2 M3 3l1.5 1.5 M11 11l-1.5-1.5 M3 11l1.5-1.5 M11 3l-1.5 1.5"/></svg>),
  region: (s=14) => (<svg width={s} height={s} viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1" strokeDasharray="2 1.5"><rect x="2" y="2" width="10" height="10"/></svg>),
  list: (s=14) => (<svg width={s} height={s} viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1"><path d="M2 3h10 M2 7h10 M2 11h10"/></svg>),
  star: (s=14) => (<svg width={s} height={s} viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1"><path d="M7 1l1.8 4 4.2.4-3.2 2.9 1 4.2L7 10.3 3.2 12.5l1-4.2L1 5.4l4.2-.4z"/></svg>),
  globe: (s=14) => (<svg width={s} height={s} viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1"><circle cx="7" cy="7" r="5.5"/><path d="M1.5 7h11 M7 1.5c2 2 2 9 0 11 M7 1.5c-2 2-2 9 0 11"/></svg>),
  terminal: (s=14) => (<svg width={s} height={s} viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1"><rect x="1.5" y="2.5" width="11" height="9"/><path d="M3.5 5l2 1.5-2 1.5 M7 8.5h3"/></svg>),
};

Object.assign(window, { Slider, Kbd, Field, BarMeter, Crosshair, Sparkline, Tabs, Icon });
