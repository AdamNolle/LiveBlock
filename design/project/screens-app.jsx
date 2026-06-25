// LiveBlocker v4 — main app screens.
// All the screens an average user actually interacts with:
//   · Dashboard (menu-bar / tray quick panel — the daily touchpoint)
//   · Overview (main settings page)
//   · Region Library (everything you and the auto-detector have blocked)
//   · Smart Detection (where you tune the auto-detector)

/* ────────────────────────────────────────────────────────────
   Shared bits
   ──────────────────────────────────────────────────────────── */

// Window frame — a calm, minimal "macOS-ish" chrome that reads
// as a desktop app on any OS.
function AppWindow({ width = 980, height = 660, title, children, style }) {
  return (
    <div className="lb" style={{
      width, height, background:'var(--bg)',
      border:'1px solid var(--line-2)', borderRadius:'var(--r-5)', overflow:'hidden',
      display:'flex', flexDirection:'column',
      boxShadow:'0 30px 60px rgba(0,0,0,0.55), 0 0 0 1px var(--line)',
      ...style,
    }}>
      <div style={{
        height: 38, display:'flex', alignItems:'center', gap: 12, padding:'0 14px',
        background:'var(--surface)', borderBottom:'1px solid var(--line)', flexShrink: 0,
      }}>
        <div style={{ display:'flex', gap: 7 }}>
          {['#ff5f57','#febc2e','#28c840'].map((c,i) => (
            <span key={i} style={{ width: 11, height: 11, borderRadius:'50%', background: c, opacity: 0.92 }}/>
          ))}
        </div>
        <div style={{ flex: 1, textAlign:'center', fontSize: 12, color:'var(--ink-3)', fontWeight: 500 }}>
          {title}
        </div>
        <div style={{ width: 50 }}/>
      </div>
      <div style={{ flex: 1, minHeight: 0, display:'flex', flexDirection:'column' }}>{children}</div>
    </div>
  );
}

// Sidebar nav row
function NavRow({ icon, label, count, active, tone = 'neutral' }) {
  const dotColor = { neutral:'var(--ink-4)', accent:'var(--accent)', ml:'var(--ml)', success:'var(--success)' }[tone];
  return (
    <div style={{
      display:'flex', alignItems:'center', gap: 10, padding:'8px 12px',
      background: active ? 'var(--surface-2)' : 'transparent',
      borderRadius:'var(--r-2)',
      color: active ? 'var(--ink-1)' : 'var(--ink-2)',
      fontSize: 13, fontWeight: active ? 600 : 500, letterSpacing:'-0.005em',
      cursor:'pointer',
    }}>
      <span style={{ color: active ? dotColor : 'var(--ink-4)', display:'inline-flex' }}>{icon}</span>
      <span style={{ flex: 1 }}>{label}</span>
      {count != null && (
        <span className="mono" style={{
          fontSize: 11, color: active ? 'var(--ink-3)' : 'var(--ink-4)',
          background: active ? 'var(--surface-3)' : 'transparent',
          padding:'1px 6px', borderRadius:'var(--r-1)', minWidth: 18, textAlign:'center',
        }}>{count}</span>
      )}
    </div>
  );
}

function MainShell({ children, active = 'overview', title = 'Overview' }) {
  const nav = [
    { id: 'overview',  l: 'Overview',         i: I.grid(15)   },
    { id: 'library',   l: 'Region library',   i: I.region(15), c: 14 },
    { id: 'detector',  l: 'Smart detection',  i: I.spark(15),  c: 'on', tone:'ml' },
    { id: 'apps',      l: 'Apps & sites',     i: I.layers(15), c: 6 },
    { id: 'history',   l: 'Activity',         i: I.history(15) },
    { id: 'shortcuts', l: 'Shortcuts',        i: I.cpu(15)     },
    { id: 'privacy',   l: 'Privacy',          i: I.lock(15)    },
  ];
  return (
    <AppWindow title="LiveBlocker" width={980} height={660}>
      <div style={{ display:'flex', flex: 1, minHeight: 0 }}>
        {/* Sidebar */}
        <div style={{
          width: 230, background:'var(--surface)', borderRight:'1px solid var(--line)',
          display:'flex', flexDirection:'column', padding: 14, gap: 12,
        }}>
          <div style={{ display:'flex', alignItems:'center', gap: 8, padding:'2px 4px' }}>
            <Logo size={26}/>
            <span style={{ fontWeight: 700, fontSize: 15, letterSpacing:'-0.018em' }}>LiveBlocker</span>
          </div>
          <Field size="sm" icon={I.search(13)} placeholder="Search settings" rightKbd="⌘K" />
          <div style={{ display:'flex', flexDirection:'column', gap: 1 }}>
            {nav.map(n => (
              <NavRow key={n.id} icon={n.i} label={n.l} count={n.c}
                tone={n.tone || (n.id === active ? 'accent' : 'neutral')}
                active={n.id === active}/>
            ))}
          </div>
          <div style={{ flex: 1 }}/>
          {/* Live engine card */}
          <div className="card-inset" style={{ padding: 12 }}>
            <div style={{ display:'flex', alignItems:'center', gap: 8, marginBottom: 8 }}>
              <Dot color="var(--success)" size={7} pulse/>
              <span style={{ fontSize: 12, fontWeight: 600, color:'var(--ink-1)' }}>Engine active</span>
            </div>
            <div style={{ fontSize: 11, color:'var(--ink-3)', lineHeight: 1.5 }}>
              Running at 60 fps · 2.1 ms / frame
            </div>
            <div style={{ marginTop: 8 }}>
              <Sparkline data={[2.1,2.0,2.2,2.1,2.0,2.3,2.1,2.0,2.1,2.2,2.0,2.1,2.0,2.2,2.1,2.0]}
                width={196} height={26} color="var(--success)" fill={true}/>
            </div>
          </div>
        </div>

        {/* Main */}
        <div style={{ flex: 1, minWidth: 0, display:'flex', flexDirection:'column' }}>
          {/* Top bar */}
          <div style={{
            height: 56, padding:'0 22px', display:'flex', alignItems:'center', gap: 12,
            borderBottom:'1px solid var(--line)', flexShrink: 0,
          }}>
            <h2 style={{ margin: 0, fontSize: 18, fontWeight: 600, letterSpacing:'-0.02em' }}>{title}</h2>
            <Pill tone="success" dot="pulse" size="sm">On</Pill>
            <div style={{ flex: 1 }}/>
            <Btn size="sm" variant="ghost" icon={I.bell(14)}/>
            <Btn size="sm" variant="primary" icon={I.plus(13)} kbd="⌘⇧K">Mark region</Btn>
          </div>
          <div style={{ flex: 1, minHeight: 0, overflow:'auto' }}>{children}</div>
        </div>
      </div>
    </AppWindow>
  );
}

/* ────────────────────────────────────────────────────────────
   1. DASHBOARD — the menu bar / tray quick panel
   The screen most people see most days.
   ──────────────────────────────────────────────────────────── */

function ScreenDashboard() {
  const apps = [
    { name:'YouTube',        host:'youtube.com',   color:'#ff0033', letter:'Y', blocked: 142, today: true,  active: true  },
    { name:'X (Twitter)',    host:'x.com',         color:'#1d9bf0', letter:'X', blocked: 215, today: true,  active: false },
    { name:'Reddit',         host:'reddit.com',    color:'#ff4500', letter:'R', blocked:  76, today: true,  active: true  },
    { name:'Spotify',        host:'desktop app',   color:'#1db954', letter:'S', blocked:  34, today: true,  active: true  },
    { name:'Mail',           host:'desktop app',   color:'#a78bfa', letter:'M', blocked:  12, today: false, active: true  },
  ];

  const hourly = [3,5,4,7,9,8,12,10,14,11,18,16,22,19,26,28,24,32,38,30,42,48,52,47];

  return (
    <div className="lb" style={{
      width: 380, background:'var(--surface)',
      border:'1px solid var(--line-2)', borderRadius:'var(--r-5)',
      boxShadow:'0 24px 60px rgba(0,0,0,0.55), 0 0 0 1px var(--line)',
      overflow:'hidden', display:'flex', flexDirection:'column',
    }}>
      {/* ── Header ── */}
      <div style={{ padding:'14px 16px', borderBottom:'1px solid var(--line)',
        display:'flex', alignItems:'center', gap: 10 }}>
        <Logo size={24}/>
        <span style={{ fontWeight: 700, fontSize: 14, letterSpacing:'-0.015em' }}>LiveBlocker</span>
        <Pill tone="success" dot="pulse" size="sm">Active</Pill>
        <div style={{ flex: 1 }}/>
        <button style={{
          width: 28, height: 28, padding: 0, border:'none', background:'transparent',
          color:'var(--ink-3)', borderRadius:'var(--r-2)',
        }}>{I.settings(15)}</button>
      </div>

      {/* ── Hero — today's count ── */}
      <div style={{ padding:'18px 16px 16px' }}>
        <div className="caption" style={{ marginBottom: 4 }}>Blocked today</div>
        <div style={{ display:'flex', alignItems:'baseline', gap: 10 }}>
          <span className="mono tnum" style={{
            fontSize: 56, fontWeight: 600, letterSpacing:'-0.04em', color:'var(--ink-1)', lineHeight: 0.92,
          }}>847</span>
          <Pill tone="success" size="sm">↑ 18% vs yesterday</Pill>
        </div>
        <div style={{ marginTop: 14 }}>
          <Sparkline data={hourly} width={348} height={56} color="var(--accent)" fill={true} dots={true}/>
          <div style={{ display:'flex', justifyContent:'space-between', marginTop: 6,
            fontSize: 10, color:'var(--ink-4)' }}>
            <span>12am</span><span>6am</span><span>noon</span><span>6pm</span><span>now</span>
          </div>
        </div>
      </div>

      {/* ── Three quick stats ── */}
      <div style={{ padding:'0 16px 16px', display:'grid', gridTemplateColumns:'repeat(3, 1fr)', gap: 8 }}>
        {[
          { lbl:'Time saved',   val:'1h 24m', tone:'var(--success)' },
          { lbl:'Data skipped', val:'42 MB',  tone:'var(--info)'    },
          { lbl:'Frame cost',   val:'2.1 ms', tone:'var(--ml)'      },
        ].map((s) => (
          <div key={s.lbl} className="card-inset" style={{ padding:'10px 11px' }}>
            <div style={{ fontSize: 11, color:'var(--ink-3)', marginBottom: 3, lineHeight: 1.2 }}>{s.lbl}</div>
            <div className="mono tnum" style={{ fontSize: 16, fontWeight: 600, color: s.tone, letterSpacing:'-0.02em' }}>{s.val}</div>
          </div>
        ))}
      </div>

      {/* ── Mark CTA ── */}
      <div style={{ padding:'0 16px 14px' }}>
        <Btn variant="primary" size="lg" full icon={I.plus(15)} kbd="⌘⇧K">Mark a new region</Btn>
      </div>

      {/* ── Apps & sites ── */}
      <div style={{ padding:'4px 16px 8px', display:'flex', alignItems:'center', gap: 8 }}>
        <span style={{ fontSize: 12, fontWeight: 600, color:'var(--ink-2)' }}>Where it's working</span>
        <div style={{ flex: 1, height: 1, background:'var(--line)' }}/>
        <span className="mono" style={{ fontSize: 11, color:'var(--ink-4)' }}>{apps.length} apps</span>
      </div>
      <div style={{ padding:'2px 8px 8px' }}>
        {apps.map((a, i) => (
          <div key={a.name} style={{
            display:'flex', alignItems:'center', gap: 11,
            padding:'10px 8px', borderRadius:'var(--r-2)',
            background: i === 0 ? 'var(--surface-2)' : 'transparent',
          }}>
            <AppIcon size={26} color={a.color} letter={a.letter} radius={7}/>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontSize: 13, fontWeight: 600, color:'var(--ink-1)', letterSpacing:'-0.005em' }}>{a.name}</div>
              <div style={{ fontSize: 11, color:'var(--ink-3)', marginTop: 1 }}>
                <span className="mono tnum">{a.blocked}</span> blocked · {a.host}
              </div>
            </div>
            <Toggle size="sm" value={a.active} onChange={()=>{}}/>
          </div>
        ))}
      </div>

      {/* ── Footer ── */}
      <div style={{
        marginTop:'auto', padding:'10px 14px',
        borderTop:'1px solid var(--line)', background:'var(--surface)',
        display:'flex', alignItems:'center', gap: 10,
      }}>
        <button style={{
          height: 28, padding:'0 10px', background:'transparent', border:'none',
          color:'var(--ink-3)', fontSize: 12, fontWeight: 500,
          display:'inline-flex', alignItems:'center', gap: 7, borderRadius:'var(--r-2)',
        }}>{I.settings(13)} Open settings</button>
        <div style={{ flex: 1 }}/>
        <Kbd size={10}>⌘</Kbd><Kbd size={10}>⇧</Kbd><Kbd size={10}>K</Kbd>
      </div>
    </div>
  );
}

/* ────────────────────────────────────────────────────────────
   2. OVERVIEW — the main "set it and forget it" page
   ──────────────────────────────────────────────────────────── */

function ScreenOverview() {
  return (
    <MainShell active="overview" title="Overview">
      <div style={{ padding:'22px 22px 28px', display:'flex', flexDirection:'column', gap: 22 }}>
        {/* ── Hero ── */}
        <div style={{
          display:'grid', gridTemplateColumns:'1.5fr 1fr', gap: 14, alignItems:'stretch',
        }}>
          <div className="card" style={{ padding:'22px 24px', position:'relative', overflow:'hidden' }}>
            <div style={{
              position:'absolute', top: -60, right: -60, width: 220, height: 220,
              background:'radial-gradient(closest-side, var(--accent-soft), transparent 75%)',
            }}/>
            <div style={{ position:'relative' }}>
              <div style={{ display:'flex', alignItems:'center', gap: 8, marginBottom: 12 }}>
                <Dot color="var(--success)" size={8} pulse/>
                <span style={{ fontSize: 12, fontWeight: 600, color:'var(--success)' }}>You're protected</span>
                <span style={{ fontSize: 12, color:'var(--ink-4)' }}>· uptime 14h 02m</span>
              </div>
              <h1 style={{
                margin: 0, fontSize: 36, fontWeight: 600, letterSpacing:'-0.025em',
                color:'var(--ink-1)', lineHeight: 1.05, maxWidth: 480,
              }}>
                Set, and forgotten.<br/>
                <span style={{ color:'var(--ink-3)' }}>The detector has been catching ads on its own for 4 days.</span>
              </h1>
              <div style={{ marginTop: 18, display:'flex', alignItems:'center', gap: 10 }}>
                <Btn variant="primary" icon={I.plus(14)} kbd="⌘⇧K">Mark region</Btn>
                <Btn variant="outline" icon={I.spark(14)}>Train detector</Btn>
                <div style={{ flex: 1 }}/>
                <span style={{ fontSize: 12, color:'var(--ink-3)' }}>Master switch</span>
                <Toggle value={true} onChange={()=>{}}/>
              </div>
            </div>
          </div>

          {/* Today live counter — to the right */}
          <div className="card" style={{ padding:'22px 24px', display:'flex', flexDirection:'column' }}>
            <div className="caption">Blocked today</div>
            <div style={{ display:'flex', alignItems:'baseline', gap: 8, marginTop: 6 }}>
              <span className="mono tnum" style={{
                fontSize: 58, fontWeight: 600, letterSpacing:'-0.04em', color:'var(--accent)', lineHeight: 0.92,
              }}>847</span>
              <Pill tone="success" size="sm">↑ 18%</Pill>
            </div>
            <div style={{ fontSize: 12, color:'var(--ink-3)', marginTop: 10 }}>
              <span className="mono tnum">+38</span> in the last hour ·
              <span className="mono tnum"> 1h 24m</span> reading time recovered
            </div>
            <div style={{ flex: 1, minHeight: 8 }}/>
            <Sparkline data={[12,15,11,18,22,19,28,25,32,30,38,42,36,48,55,52,61,58,72,68,82,79,91,87]}
              width={300} height={50} color="var(--accent)" fill={true}/>
            <div style={{ display:'flex', justifyContent:'space-between', fontSize: 10, color:'var(--ink-4)', marginTop: 4 }}>
              <span>12am</span><span>noon</span><span>now</span>
            </div>
          </div>
        </div>

        {/* ── KPI strip ── */}
        <div style={{ display:'grid', gridTemplateColumns:'repeat(4, 1fr)', gap: 12 }}>
          {[
            { lbl:'Active regions', val:'14',   sub:'of 256 max', cap: 14/256, color:'var(--ink-1)', accent:'var(--ink-2)' },
            { lbl:'Detector confidence', val:'84', unit:'%', sub:'auto-blocking on',   color:'var(--ml)',     accent:'var(--ml)' },
            { lbl:'Frame cost', val:'2.1', unit:'ms', sub:'87% headroom @60fps', cap: 2.1/16.6, color:'var(--success)', accent:'var(--success)' },
            { lbl:'CPU', val:'0.4', unit:'%', sub:'14.2 MB memory', cap: 0.04, color:'var(--info)', accent:'var(--info)' },
          ].map((k) => (
            <div key={k.lbl} className="card-2" style={{ padding:'14px 16px' }}>
              <div className="caption">{k.lbl}</div>
              <div style={{ display:'flex', alignItems:'baseline', gap: 4, marginTop: 8 }}>
                <span className="mono tnum" style={{
                  fontSize: 30, fontWeight: 600, letterSpacing:'-0.03em', color: k.color, lineHeight: 0.95,
                }}>{k.val}</span>
                {k.unit && <span className="mono" style={{ fontSize: 13, color:'var(--ink-3)' }}>{k.unit}</span>}
              </div>
              {k.cap != null && <div style={{ marginTop: 10 }}><Progress value={k.cap} color={k.accent}/></div>}
              <div style={{ fontSize: 11, color:'var(--ink-3)', marginTop: 8 }}>{k.sub}</div>
            </div>
          ))}
        </div>

        {/* ── Fill technique ── */}
        <div>
          <SectionHeader
            title="How blocked regions are repainted"
            sub="When LiveBlocker hides a region, it needs to put something there. Pick the look that's least distracting."
            right={<span className="mono" style={{ fontSize: 12, color:'var(--ink-4)' }}>4 styles</span>}
          />
          <div className="card" style={{ display:'grid', gridTemplateColumns:'repeat(4, 1fr)', padding: 4, gap: 4 }}>
            {[
              { l:'Smart fill',   d:'Continue the surrounding pixels', on:true,  bg:'repeating-linear-gradient(135deg, #4a3e6a 0 12px, #5e4a82 12px 24px)' },
              { l:'Blur',         d:'Frosted blur of the neighbours',  on:false, bg:'linear-gradient(135deg, #3a2f54, #6a5392)' },
              { l:'Average color',d:'Mean colour of the edge',         on:false, bg:'#4a3a68' },
              { l:'Solid black',  d:'Just hide it',                    on:false, bg:'#0a0a0c' },
            ].map((m) => (
              <div key={m.l} style={{
                padding: 14, borderRadius:'var(--r-3)', cursor:'pointer',
                background: m.on ? 'var(--surface-2)' : 'transparent',
                border: `1px solid ${m.on ? 'var(--accent)' : 'transparent'}`,
                position:'relative', transition:'background 0.12s ease',
              }}>
                <div style={{
                  height: 60, borderRadius:'var(--r-2)', marginBottom: 12, position:'relative',
                  background: m.bg, overflow:'hidden',
                  border:'1px solid var(--line)',
                }}>
                  {m.on && (
                    <span style={{
                      position:'absolute', top: 6, right: 6, width: 18, height: 18,
                      borderRadius:'50%', background:'var(--accent)',
                      display:'inline-flex', alignItems:'center', justifyContent:'center', color:'#fff',
                    }}>{I.check(11)}</span>
                  )}
                </div>
                <div style={{ fontSize: 13, fontWeight: 600, color:'var(--ink-1)', letterSpacing:'-0.005em' }}>{m.l}</div>
                <div style={{ fontSize: 11, color:'var(--ink-3)', marginTop: 3, lineHeight: 1.4 }}>{m.d}</div>
              </div>
            ))}
          </div>
        </div>

        {/* ── Toggles ── */}
        <div>
          <SectionHeader title="Behaviour" sub="Small choices that change how LiveBlocker fits into your day."/>
          <div className="card" style={{ padding: 4 }}>
            {[
              { l:'Auto-block what the detector finds', d:'Apply suggestions above your confidence threshold.', t:true,  icon: I.spark(15), tone:'var(--ml)' },
              { l:'Pause inside fullscreen video',      d:'Skip blocking when a video player goes fullscreen.',  t:true,  icon: I.play(15),  tone:'var(--info)' },
              { l:'Always-on screen-edge chip',         d:'Keeps a tiny live counter pinned to the top-right of every display.', t:true, icon: I.bell(15), tone:'var(--accent)' },
              { l:'Anonymous performance telemetry',    d:'Frame timing only. Never any pixel content. Ever.',   t:false, icon: I.shield(15), tone:'var(--success)' },
            ].map((r, i) => (
              <div key={r.l} style={{
                display:'flex', alignItems:'center', gap: 14, padding:'12px 14px',
                borderRadius:'var(--r-2)',
                borderTop: i ? '1px solid var(--line)' : 'none',
              }}>
                <div style={{
                  width: 32, height: 32, borderRadius:'var(--r-2)',
                  display:'inline-flex', alignItems:'center', justifyContent:'center',
                  color: r.tone, background:`${r.tone}1f`,
                }}>{r.icon}</div>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontSize: 13, fontWeight: 600, color:'var(--ink-1)', letterSpacing:'-0.005em' }}>{r.l}</div>
                  <div style={{ fontSize: 12, color:'var(--ink-3)', marginTop: 2 }}>{r.d}</div>
                </div>
                <Toggle value={r.t} onChange={()=>{}}/>
              </div>
            ))}
          </div>
        </div>
      </div>
    </MainShell>
  );
}

/* ────────────────────────────────────────────────────────────
   3. REGION LIBRARY
   ──────────────────────────────────────────────────────────── */

function ScreenLibrary() {
  const regions = [
    { name:'Right-rail sponsored',  app:'YouTube',     host:'youtube.com', size:'300×600', hits: 142, on:true,  source:'manual', color:'#ff0033', letter:'Y' },
    { name:'In-feed promo cards',   app:'YouTube',     host:'youtube.com', size:'728×90',  hits:  88, on:true,  source:'manual', color:'#ff0033', letter:'Y' },
    { name:'Now-playing banner',    app:'Spotify',     host:'desktop',     size:'300×250', hits:  34, on:true,  source:'manual', color:'#1db954', letter:'S' },
    { name:'Promoted tweets',       app:'X',           host:'x.com',       size:'auto',    hits: 215, on:false, source:'manual', color:'#1d9bf0', letter:'X' },
    { name:'Sidebar ads',           app:'Reddit',      host:'reddit.com',  size:'300×600', hits:  76, on:true,  source:'auto',   color:'#ff4500', letter:'R' },
    { name:'Newsletter footer',     app:'Mail',        host:'desktop',     size:'variable',hits:  12, on:true,  source:'auto',   color:'#a78bfa', letter:'M' },
    { name:'Update toast',          app:'Cursor',      host:'desktop',     size:'320×80',  hits:   4, on:false, source:'auto',   color:'#22d3ee', letter:'C' },
    { name:'Cookie banner',         app:'Browser',     host:'all sites',   size:'variable',hits: 188, on:true,  source:'auto',   color:'#7a7e8b', letter:'B' },
    { name:'Chat widget',           app:'Browser',     host:'multiple',    size:'380×580', hits:  23, on:false, source:'auto',   color:'#7a7e8b', letter:'B' },
  ];

  return (
    <MainShell active="library" title="Region library">
      <div style={{ padding:'22px 22px 28px', display:'flex', flexDirection:'column', gap: 18, minHeight:'100%' }}>
        {/* Filters & search */}
        <div style={{ display:'flex', alignItems:'flex-start', gap: 14, flexWrap:'wrap' }}>
          <div style={{ flex: 1, minWidth: 0 }}>
            <Stat value="14" label="Saved regions"
              sub={<><span className="mono tnum">7</span> active · <span className="mono tnum" style={{color:'var(--ml)'}}>11</span> learned by the detector · <span className="mono tnum">847</span> blocks today</>}
              size="lg"/>
          </div>
          <div style={{ display:'flex', alignItems:'center', gap: 10 }}>
            <Field icon={I.search(14)} placeholder="Find a region…" rightKbd="/" style={{ width: 240 }}/>
            <Segmented value="all" onChange={()=>{}} options={[
              { value:'all',    label:'All' },
              { value:'manual', label:'Manual' },
              { value:'auto',   label:'Learned' },
            ]}/>
            <Btn variant="primary" icon={I.plus(13)}>New</Btn>
          </div>
        </div>

        {/* Table */}
        <div className="card" style={{ display:'flex', flexDirection:'column', overflow:'hidden', flex: 1, minHeight: 0 }}>
          {/* Head */}
          <div style={{
            display:'grid', gridTemplateColumns:'40px 1.7fr 1.2fr 0.6fr 110px 90px 60px 30px',
            gap: 12, padding:'10px 16px', borderBottom:'1px solid var(--line)',
            background:'var(--surface-2)', fontSize: 11, color:'var(--ink-3)', fontWeight: 600,
          }}>
            <span/><span>Region</span><span>App · context</span><span>Source</span>
            <span style={{ textAlign:'right' }}>Size</span>
            <span style={{ textAlign:'right' }}>Blocks · 7d</span>
            <span>On</span><span/>
          </div>
          {/* Rows */}
          <div style={{ flex: 1, overflow:'auto' }}>
            {regions.map((r, i) => (
              <div key={i} style={{
                display:'grid', gridTemplateColumns:'40px 1.7fr 1.2fr 0.6fr 110px 90px 60px 30px',
                gap: 12, padding:'12px 16px', alignItems:'center',
                borderTop: i ? '1px solid var(--line)' : 'none',
                background: i === 0 ? 'var(--surface-2)' : 'transparent',
              }}>
                {/* preview */}
                <div style={{
                  width: 36, height: 22, borderRadius:'var(--r-1)',
                  background:'var(--bg)', border:'1px solid var(--line)', position:'relative',
                  display:'flex', alignItems:'center', justifyContent:'center',
                }}>
                  <div style={{
                    position:'absolute', inset: 3, border:`1px dashed ${r.source==='auto' ? 'var(--ml)' : 'var(--accent)'}`,
                    borderRadius: 1,
                  }}/>
                </div>
                {/* name */}
                <div style={{ minWidth: 0 }}>
                  <div style={{
                    fontSize: 13, fontWeight: 600, color:'var(--ink-1)',
                    whiteSpace:'nowrap', overflow:'hidden', textOverflow:'ellipsis',
                    letterSpacing:'-0.005em',
                  }}>{r.name}</div>
                  <div style={{ fontSize: 11, color:'var(--ink-4)', marginTop: 1 }}>region · {r.size}</div>
                </div>
                {/* app */}
                <div style={{ display:'flex', alignItems:'center', gap: 8, minWidth: 0 }}>
                  <AppIcon size={22} color={r.color} letter={r.letter}/>
                  <div style={{ minWidth: 0 }}>
                    <div style={{ fontSize: 13, color:'var(--ink-2)', whiteSpace:'nowrap', overflow:'hidden', textOverflow:'ellipsis' }}>{r.app}</div>
                    <div style={{ fontSize: 11, color:'var(--ink-4)', whiteSpace:'nowrap' }}>{r.host}</div>
                  </div>
                </div>
                {/* source */}
                <div>
                  {r.source === 'auto'
                    ? <Pill tone="ml" size="sm" icon={I.spark(11)}>Learned</Pill>
                    : <Pill tone="ghost" size="sm">Manual</Pill>}
                </div>
                {/* size */}
                <span className="mono tnum" style={{ fontSize: 12, color:'var(--ink-3)', textAlign:'right' }}>{r.size}</span>
                {/* hits */}
                <span className="mono tnum" style={{
                  fontSize: 14, fontWeight: 600, color: r.on ? 'var(--accent)' : 'var(--ink-4)',
                  textAlign:'right', letterSpacing:'-0.01em',
                }}>{r.hits}</span>
                {/* toggle */}
                <Toggle value={r.on} onChange={()=>{}} size="sm"/>
                {/* menu */}
                <button style={{
                  width: 24, height: 24, padding: 0, border:'none', background:'transparent',
                  color:'var(--ink-4)', borderRadius:'var(--r-1)', display:'inline-flex', alignItems:'center', justifyContent:'center',
                }}>{I.more(14)}</button>
              </div>
            ))}
          </div>
          {/* Foot */}
          <div style={{
            padding:'10px 16px', borderTop:'1px solid var(--line)',
            display:'flex', alignItems:'center', gap: 10, fontSize: 12, color:'var(--ink-3)',
          }}>
            <span>Showing <span className="mono tnum" style={{ color:'var(--ink-1)' }}>{regions.length}</span> of <span className="mono tnum" style={{ color:'var(--ink-1)' }}>14</span></span>
            <div style={{ flex: 1 }}/>
            <Btn size="sm" variant="ghost" icon={I.external(13)}>Export…</Btn>
          </div>
        </div>
      </div>
    </MainShell>
  );
}

/* ────────────────────────────────────────────────────────────
   4. SMART DETECTION (renamed from "ML detector")
   ──────────────────────────────────────────────────────────── */

function ScreenDetector() {
  const cats = [
    { l:'Banner ads',          n: 412, on:true,  tone:'accent' },
    { l:'Video pre-rolls',     n:  87, on:true,  tone:'accent' },
    { l:'Sponsored cards',     n: 234, on:true,  tone:'accent' },
    { l:'Newsletter pop-ups',  n:  56, on:true,  tone:'warn'   },
    { l:'Cookie banners',      n: 188, on:false, tone:'neutral'},
    { l:'Chat widgets',        n:  23, on:false, tone:'neutral'},
    { l:'Engagement nudges',   n:  41, on:false, tone:'neutral'},
  ];
  const detections = [
    { x:  8, y:  10, w: 34, h: 22, label:'Banner',     conf: 0.94, c:'var(--accent)', dashed:false },
    { x: 60, y:  40, w: 32, h: 44, label:'Sponsored',  conf: 0.87, c:'var(--accent)', dashed:false },
    { x: 10, y:  64, w: 26, h: 26, label:'Pop-up',     conf: 0.62, c:'var(--warn)',   dashed:true  },
  ];

  return (
    <MainShell active="detector" title="Smart detection">
      <div style={{ padding:'22px 22px 28px', display:'grid', gridTemplateColumns:'1.4fr 1fr', gap: 16, minHeight:'100%' }}>
        {/* ── Top status (spans both columns) ── */}
        <div style={{ gridColumn:'1 / -1', display:'flex', alignItems:'flex-end', gap: 18 }}>
          <div style={{ flex: 1 }}>
            <div className="caption" style={{ marginBottom: 8 }}>
              On-device model · LiveBlocker ViT v3 · 14.2 MB
            </div>
            <div style={{ display:'flex', alignItems:'baseline', gap: 10 }}>
              <span className="mono tnum" style={{ fontSize: 44, fontWeight: 600, letterSpacing:'-0.035em', color:'var(--ml)', lineHeight: 0.95 }}>84%</span>
              <span style={{ fontSize: 16, color:'var(--ink-2)', fontWeight: 500 }}>confidence threshold</span>
              <Pill tone="success" size="sm" dot>Auto-block on</Pill>
            </div>
            <div style={{ fontSize: 13, color:'var(--ink-3)', marginTop: 8, maxWidth: 540 }}>
              The detector only auto-blocks when it's at least this sure. Raise it for fewer false positives, lower it to catch more.
            </div>
          </div>
          <Btn variant="outline" icon={I.spark(14)}>Train on selection</Btn>
          <Btn variant="primary"  icon={I.arrow(14)}>Review 3 borderline</Btn>
        </div>

        {/* ── Threshold slider ── */}
        <div className="card" style={{ gridColumn:'1 / -1', padding:'16px 20px' }}>
          <div style={{ display:'flex', alignItems:'center', gap: 14, marginBottom: 10 }}>
            <span style={{ fontSize: 12, fontWeight: 600, color:'var(--ink-2)' }}>Confidence threshold</span>
            <div style={{ flex: 1 }}/>
            <span style={{ fontSize: 11, color:'var(--ink-3)' }}><span style={{ color:'var(--accent)' }}>← more aggressive</span> · <span style={{ color:'var(--ml)' }}>fewer mistakes →</span></span>
          </div>
          <Slider value={84} min={50} max={99} onChange={()=>{}} accent="var(--ml)" marks={[50,60,70,80,90,99]}/>
          <div style={{ display:'flex', justifyContent:'space-between', fontSize: 10, color:'var(--ink-4)', marginTop: 6 }}>
            <span>50%</span><span>60%</span><span>70%</span><span>80%</span><span>90%</span><span>99%</span>
          </div>
        </div>

        {/* ── Live preview ── */}
        <div className="card" style={{ display:'flex', flexDirection:'column', overflow:'hidden', minHeight: 320 }}>
          <div style={{
            display:'flex', alignItems:'center', gap: 10, padding:'10px 14px',
            borderBottom:'1px solid var(--line)',
          }}>
            <Dot color="var(--accent)" size={7} pulse/>
            <span style={{ fontSize: 12, fontWeight: 600 }}>Live preview</span>
            <span style={{ fontSize: 11, color:'var(--ink-4)' }}>What the detector sees right now</span>
            <div style={{ flex: 1 }}/>
            <span className="mono" style={{ fontSize: 11, color:'var(--ink-3)' }}>3 found · 8.4 ms</span>
          </div>
          <div className="scan" style={{
            flex: 1, position:'relative', overflow:'hidden',
            background:'linear-gradient(160deg,#1c1530 0%, #2a1f44 100%)',
          }}>
            {/* faux content lines */}
            {[14,32,22,38,26,30,18,28,34,22].map((w,i) => (
              <div key={i} style={{
                position:'absolute', left:'8%', top:`${8 + i*8}%`, width:`${w}%`, height: 3,
                background:'rgba(255,255,255,0.12)', borderRadius: 1,
              }}/>
            ))}
            {detections.map((d, i) => (
              <div key={i} style={{
                position:'absolute', left:`${d.x}%`, top:`${d.y}%`, width:`${d.w}%`, height:`${d.h}%`,
                border:`1.5px ${d.dashed ? 'dashed' : 'solid'} ${d.c}`,
                background: d.dashed ? 'transparent' : `${d.c.replace('var(--', 'rgba(').replace(')', ', 0.12)')}`,
                borderRadius: 4,
              }}>
                <div style={{
                  position:'absolute', top: -22, left: -1,
                  background: d.c, color:'#fff',
                  padding:'2px 7px', borderRadius:'var(--r-1)',
                  fontSize: 10, fontWeight: 700, letterSpacing:'-0.005em',
                }}>{d.label} · {Math.round(d.conf*100)}%</div>
              </div>
            ))}
          </div>
        </div>

        {/* ── Categories ── */}
        <div className="card" style={{ display:'flex', flexDirection:'column', overflow:'hidden', minHeight: 320 }}>
          <div style={{
            padding:'10px 14px', borderBottom:'1px solid var(--line)',
            display:'flex', alignItems:'center', gap: 10,
          }}>
            <span style={{ fontSize: 12, fontWeight: 600 }}>What to catch</span>
            <Pill tone="ghost" size="sm">7 categories</Pill>
            <div style={{ flex: 1 }}/>
            <Pill tone="warn" size="sm">3 need review</Pill>
          </div>
          <div style={{ flex: 1, overflow:'auto' }}>
            {cats.map((c, i) => (
              <div key={c.l} style={{
                display:'flex', alignItems:'center', gap: 12, padding:'11px 14px',
                borderTop: i ? '1px solid var(--line)' : 'none',
              }}>
                <div style={{ width: 3, height: 26, borderRadius: 2,
                  background: c.on ? (c.tone === 'warn' ? 'var(--warn)' : 'var(--accent)') : 'var(--ink-5)' }}/>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontSize: 13, fontWeight: 600, color:'var(--ink-1)' }}>{c.l}</div>
                  <div style={{ fontSize: 11, color:'var(--ink-3)', marginTop: 2 }}>
                    <span className="mono tnum">{c.n.toLocaleString()}</span> caught in the last 30 days
                  </div>
                </div>
                <div style={{ width: 72 }}><Progress value={Math.min(1, c.n / 500)} color={c.on ? (c.tone==='warn' ? 'var(--warn)' : 'var(--accent)') : 'var(--ink-5)'}/></div>
                <Toggle value={c.on} onChange={()=>{}} size="sm"/>
              </div>
            ))}
          </div>
        </div>
      </div>
    </MainShell>
  );
}

Object.assign(window, { ScreenDashboard, ScreenOverview, ScreenLibrary, ScreenDetector, MainShell, AppWindow });
