// LiveBlocker — brutalist terminal primitives.
// Cross-platform window chrome, mono type, hairline rules, no shadows.
// Components are tiny and composable. All export to window at end.

// ── Logo ──────────────────────────────────────────────────────
// A 12x12 pixel grid with corner brackets and a red record dot.
// Now framed by a thin square, no rounding. Brutalist.
function Logo({ size = 64, accent = '#ff3b1f' }) {
  const palette = [
    '#ff3b1f','#fb923c','#fbbf24','#4ade80','#22d3ee','#60a5fa',
    '#a78bfa','#f472b6','#facc15','#34d399','#67e8f9','#c084fc',
  ];
  const uid = React.useId();
  let seed = 73;
  const rnd = () => { seed = (seed*9301 + 49297) % 233280; return seed/233280; };
  const cells = [];
  for (let y=0; y<12; y++) for (let x=0; x<12; x++) cells.push({x,y,c: palette[Math.floor(rnd()*palette.length)]});
  return (
    <svg viewBox="0 0 120 120" width={size} height={size} style={{display:'block'}}>
      <defs><clipPath id={`lc-${uid}`}><rect x="0" y="0" width="120" height="120"/></clipPath></defs>
      <g clipPath={`url(#lc-${uid})`}>
        <rect width="120" height="120" fill="#0a0a0c"/>
        {cells.map((c,i)=> <rect key={i} x={c.x*10} y={c.y*10} width="10" height="10" fill={c.c} opacity={0.85}/>)}
        <g fill="none" stroke="#0a0a0c" strokeWidth="9" strokeLinecap="square" strokeLinejoin="miter">
          <path d="M14 36 V14 H36"/>
          <path d="M84 14 H106 V36"/>
          <path d="M106 84 V106 H84"/>
          <path d="M36 106 H14 V84"/>
        </g>
        <rect x="78" y="20" width="22" height="22" fill="#0a0a0c"/>
        <rect x="82" y="24" width="14" height="14" fill={accent}/>
      </g>
      <rect x="0.5" y="0.5" width="119" height="119" fill="none" stroke="rgba(255,255,255,0.16)"/>
    </svg>
  );
}

// ── Wordmark ──────────────────────────────────────────────────
function Wordmark({ size = 13, withLogo = true }) {
  return (
    <div style={{display:'flex', alignItems:'center', gap:8}}>
      {withLogo && <Logo size={size + 5}/>}
      <span style={{
        fontFamily:'var(--lb-font-mono)', fontSize:size, fontWeight:600,
        letterSpacing:'0.04em', color:'var(--lb-ink)', textTransform:'uppercase',
      }}>
        LIVE<span style={{color:'var(--lb-block)'}}>BLOCK</span>ER
      </span>
    </div>
  );
}

// ── Hairline ──────────────────────────────────────────────────
function Hairline({ vertical = false, length, color = 'var(--lb-rule)', style = {} }) {
  const base = vertical
    ? { width:1, height: length || '100%', background: color }
    : { height:1, width: length || '100%', background: color };
  return <div style={{...base, ...style}}/>;
}

// ── Cross-platform window frame ───────────────────────────────
// Top bar with traffic-light-free chrome: only [×] in the corner and a
// centred title in mono. Works on all three OSes. Optional left/right slots.
function WinFrame({ title, subtitle, children, width, height, left, right, accent, style={} }) {
  return (
    <div className="lb lb-panel" style={{
      width, height, position:'relative', overflow:'hidden',
      display:'flex', flexDirection:'column',
      background:'var(--lb-bg)', color:'var(--lb-ink)',
      ...style,
    }}>
      {/* tiny ribbon hint of accent on the very top edge */}
      {accent !== false && (
        <div style={{height:1, background:`linear-gradient(90deg, transparent, var(--lb-block) 30%, var(--lb-block) 70%, transparent)`, opacity:0.7}}/>
      )}
      <div style={{
        height:34, padding:'0 12px', display:'flex', alignItems:'center', gap:10,
        borderBottom:'1px solid var(--lb-rule)', background:'var(--lb-panel)',
        flexShrink:0,
      }}>
        <div style={{display:'flex', gap:4}}>
          {[0,1,2].map(i => <span key={i} style={{width:8,height:8, border:'1px solid var(--lb-rule-3)'}}/>)}
        </div>
        {left}
        <div style={{flex:1, display:'flex', justifyContent:'center', alignItems:'center', gap:8, fontSize:11, color:'var(--lb-ink-muted)', letterSpacing:'0.04em'}}>
          <span>{title}</span>
          {subtitle && <><span style={{color:'var(--lb-ink-dim)'}}>·</span><span style={{color:'var(--lb-ink-faint)'}}>{subtitle}</span></>}
        </div>
        {right}
        <div style={{display:'flex', gap:6, color:'var(--lb-ink-muted)', fontSize:13, lineHeight:1}}>
          <span>_</span><span>□</span><span style={{color:'var(--lb-block)'}}>×</span>
        </div>
      </div>
      <div style={{flex:1, minHeight:0, display:'flex', flexDirection:'column'}}>{children}</div>
    </div>
  );
}

// ── Status dot ────────────────────────────────────────────────
function Dot({ color = 'var(--lb-ok)', size = 8, pulse = false }) {
  return (
    <span className={pulse ? 'lb-pulse' : ''} style={{
      display:'inline-block', width:size, height:size, background:color, borderRadius:0,
      boxShadow:`0 0 0 2px ${color}22`,
    }}/>
  );
}

// ── Button: brutalist square / pill — depending on variant ────
// Variants: 'ghost' (outline), 'solid' (filled), 'hot' (block accent)
function Btn({ children, icon, variant='ghost', size='md', active=false, kbd, onClick, style={}, hot }) {
  const padY = size === 'sm' ? 5 : size === 'lg' ? 10 : 7;
  const padX = size === 'sm' ? 10 : size === 'lg' ? 18 : 14;
  const fs   = size === 'sm' ? 11 : size === 'lg' ? 13 : 12;
  let bg, color, border;
  if (variant === 'hot' || hot) {
    bg = 'var(--lb-block)'; color = '#0a0a0c'; border = '1px solid var(--lb-block)';
  } else if (variant === 'solid') {
    bg = 'var(--lb-ink)'; color = '#0a0a0c'; border = '1px solid var(--lb-ink)';
  } else if (active) {
    bg = 'var(--lb-panel-3)'; color = 'var(--lb-ink)'; border = '1px solid var(--lb-rule-3)';
  } else {
    bg = 'transparent'; color = 'var(--lb-ink-2)'; border = '1px solid var(--lb-rule-2)';
  }
  return (
    <button onClick={onClick} className="lb" style={{
      display:'inline-flex', alignItems:'center', gap:7,
      padding:`${padY}px ${padX}px`, fontSize:fs, fontWeight:500,
      letterSpacing:'0.04em', textTransform:'uppercase',
      background:bg, color, border, borderRadius:0, cursor:'pointer',
      lineHeight:1, ...style,
    }}>
      {icon && <span style={{display:'inline-flex'}}>{icon}</span>}
      {children}
      {kbd && <span style={{
        marginLeft:6, padding:'1px 5px', border:'1px solid currentColor',
        opacity:0.55, fontSize:9, letterSpacing:'0.06em',
      }}>{kbd}</span>}
    </button>
  );
}

// ── Toggle: brutalist [ ON ] / [ off ] ────────────────────────
function Toggle({ value, onChange, on='ON', off='OFF' }) {
  return (
    <button onClick={() => onChange(!value)} className="lb" style={{
      display:'inline-flex', alignItems:'center', gap:0, padding:0,
      background:'transparent', border:'1px solid var(--lb-rule-2)',
      borderRadius:0, cursor:'pointer', fontSize:10, letterSpacing:'0.08em',
    }}>
      <span style={{
        padding:'4px 9px', background: value ? 'var(--lb-ok)' : 'transparent',
        color: value ? '#0a0a0c' : 'var(--lb-ink-faint)', fontWeight:600,
      }}>{on}</span>
      <span style={{
        padding:'4px 9px', background: value ? 'transparent' : 'var(--lb-panel-3)',
        color: value ? 'var(--lb-ink-faint)' : 'var(--lb-ink-2)', fontWeight:600,
        borderLeft:'1px solid var(--lb-rule-2)',
      }}>{off}</span>
    </button>
  );
}

Object.assign(window, { Logo, Wordmark, Hairline, WinFrame, Dot, Btn, Toggle });
