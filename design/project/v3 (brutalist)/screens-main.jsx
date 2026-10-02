// LiveBlocker — tray quick panel + main settings + region library.

// ── 4. TRAY / QUICK PANEL ───────────────────────────────────
// OS-agnostic: docks under a tray icon (Win/Linux) or menubar (macOS).
function ScreenTray() {
  const apps = [
    { app:'BROWSER · youtube.com', n:6, on:true, c:'var(--lb-block)' },
    { app:'SPOTIFY', n:2, on:true, c:'var(--lb-ok)' },
    { app:'TWITTER · x.com', n:4, on:false, c:'var(--lb-ink-muted)' },
    { app:'REDDIT', n:3, on:true, c:'var(--lb-warn)' },
  ];
  return (
    <div className="lb lb-panel" style={{ width:360, minHeight:580, display:'flex', flexDirection:'column', overflow:'hidden' }}>
      {/* hairline accent on top, hints at tray attachment */}
      <div style={{height:1, background:'var(--lb-block)'}}/>
      {/* header */}
      <div style={{padding:'14px 14px 12px', display:'flex', alignItems:'center', gap:10, borderBottom:'1px solid var(--lb-rule)'}}>
        <Wordmark size={11}/>
        <div style={{flex:1}}/>
        <Toggle value={true} onChange={()=>{}}/>
      </div>

      {/* live status block — the most important info first */}
      <div style={{padding:'18px 14px 14px', borderBottom:'1px solid var(--lb-rule)'}}>
        <div style={{display:'flex', alignItems:'baseline', gap:8, marginBottom:6}}>
          <Dot color="var(--lb-block)" size={6} pulse/>
          <span className="lb-mono" style={{fontSize:10, color:'var(--lb-block)', letterSpacing:'0.10em', fontWeight:700}}>BLOCKING</span>
          <div style={{flex:1}}/>
          <span className="lb-mono" style={{fontSize:10, color:'var(--lb-ink-faint)'}}>14:23:08</span>
        </div>
        <div style={{display:'flex', alignItems:'baseline', gap:8}}>
          <span style={{
            fontFamily:'var(--lb-font-mono)', fontSize:44, fontWeight:600,
            color:'var(--lb-ink)', letterSpacing:'-0.04em', lineHeight:0.95,
          }}>847</span>
          <span className="lb-mono" style={{fontSize:10, color:'var(--lb-ink-muted)', letterSpacing:'0.10em'}}>BLOCKED · TODAY</span>
        </div>
        <div style={{marginTop:10}}>
          <Sparkline data={[3,5,4,7,9,8,12,10,14,11,18,16,22,19,26,28,24,32,38,30,42,48]} width={332} height={28} color="var(--lb-block)"/>
        </div>
      </div>

      {/* mini metrics row */}
      <div style={{display:'grid', gridTemplateColumns:'1fr 1fr 1fr', borderBottom:'1px solid var(--lb-rule)'}}>
        {[
          { v:'14', l:'REGIONS', c:'var(--lb-ink)' },
          { v:'2.1ms', l:'FRAME', c:'var(--lb-ml)' },
          { v:'0.4%', l:'CPU', c:'var(--lb-ok)' },
        ].map((s,i)=> (
          <div key={i} style={{
            padding:'10px 12px', borderRight: i<2 ? '1px solid var(--lb-rule)' : 'none',
          }}>
            <div style={{fontFamily:'var(--lb-font-mono)', fontSize:14, fontWeight:600, color:s.c, letterSpacing:'-0.02em'}}>{s.v}</div>
            <div className="lb-mono" style={{fontSize:9, color:'var(--lb-ink-muted)', letterSpacing:'0.12em', marginTop:2}}>{s.l}</div>
          </div>
        ))}
      </div>

      {/* primary CTA — single, dominant button */}
      <div style={{padding:'14px'}}>
        <Btn full variant="hot" size="lg" icon={Icon.crosshair(13)} kbd="⌘⇧K">MARK A REGION</Btn>
      </div>

      {/* this app/space */}
      <div style={{padding:'0 14px 12px', display:'flex', alignItems:'center', gap:8}}>
        <span className="lb-label">THIS SPACE</span>
        <div style={{flex:1, height:1, background:'var(--lb-rule)'}}/>
        <span className="lb-mono" style={{fontSize:9, color:'var(--lb-ink-faint)'}}>4 APPS</span>
      </div>
      <div style={{padding:'0 6px 14px', flex:1}}>
        {apps.map((a,i)=> (
          <div key={i} style={{
            display:'flex', alignItems:'center', gap:10,
            padding:'8px 10px', background: i===0 ? 'var(--lb-panel-2)' : 'transparent',
            borderLeft: i===0 ? '2px solid var(--lb-block)' : '2px solid transparent',
          }}>
            <div style={{width:8, height:8, background:a.c}}/>
            <span style={{flex:1, fontFamily:'var(--lb-font-mono)', fontSize:11, color: a.on ? 'var(--lb-ink)' : 'var(--lb-ink-muted)', letterSpacing:'0.04em'}}>{a.app}</span>
            <span className="lb-mono" style={{fontSize:10, color:'var(--lb-ink-faint)'}}>×{a.n}</span>
            <Toggle value={a.on} onChange={()=>{}}/>
          </div>
        ))}
      </div>

      {/* footer */}
      <div style={{padding:'10px 14px', borderTop:'1px solid var(--lb-rule)', display:'flex', alignItems:'center', gap:10}}>
        <span className="lb-mono" style={{fontSize:10, color:'var(--lb-ink-muted)', letterSpacing:'0.06em', cursor:'pointer'}}>
          {Icon.settings(11)} PREFERENCES
        </span>
        <div style={{flex:1}}/>
        <Kbd>⌘⇧K</Kbd>
      </div>
    </div>
  );
}

// ── 5. MAIN SETTINGS — overview / blocking tab ──────────────
function NavRow({ icon, label, count, active }) {
  return (
    <div style={{
      display:'flex', alignItems:'center', gap:10, padding:'8px 12px',
      background: active ? 'var(--lb-panel-2)' : 'transparent',
      borderLeft: `2px solid ${active ? 'var(--lb-block)' : 'transparent'}`,
      color: active ? 'var(--lb-ink)' : 'var(--lb-ink-2)',
      fontFamily:'var(--lb-font-mono)', fontSize:11, fontWeight: active ? 600 : 500,
      letterSpacing:'0.04em', cursor:'pointer',
    }}>
      <span style={{color: active ? 'var(--lb-block)' : 'var(--lb-ink-muted)', display:'inline-flex'}}>{icon}</span>
      <span style={{flex:1}}>{label}</span>
      {count != null && <span className="lb-mono" style={{fontSize:10, color:'var(--lb-ink-faint)'}}>{count}</span>}
    </div>
  );
}

function MainSettingsShell({ children, active='OVERVIEW', subtitle }) {
  const nav = [
    { l:'OVERVIEW', i: Icon.terminal(12), count:null },
    { l:'BLOCK ENGINE', i: Icon.block(12), count:'14' },
    { l:'ML DETECTOR', i: Icon.ml(12), count:'v3' },
    { l:'REGION LIBRARY', i: Icon.region(12), count:'14' },
    { l:'PER-APP RULES', i: Icon.list(12), count:'6' },
    { l:'PERFORMANCE', i: Icon.cpu(12), count:null },
    { l:'SHORTCUTS', i: Icon.settings(12), count:null },
  ];
  return (
    <WinFrame title="LIVEBLOCKER" subtitle={subtitle} width={920} height={620}
      left={
        <div style={{display:'flex', gap:6, marginLeft:8}}>
          <Btn size="sm" variant="hot" icon={Icon.crosshair(11)}>MARK</Btn>
          <Btn size="sm" variant="ghost-bright" icon={Icon.ml(11)}>TRAIN</Btn>
        </div>
      }
    >
      <div style={{display:'flex', flex:1, minHeight:0}}>
        {/* sidebar */}
        <div style={{width:200, borderRight:'1px solid var(--lb-rule)', display:'flex', flexDirection:'column'}}>
          <div style={{padding:'12px 12px 10px'}}>
            <Field icon={Icon.search(12)} placeholder="Search…" rightKbd="⌘K"/>
          </div>
          <div style={{padding:'4px 0'}}>
            {nav.map(n => <NavRow key={n.l} icon={n.i} label={n.l} count={n.count} active={n.l === active}/>)}
          </div>
          <div style={{flex:1}}/>
          {/* engine status */}
          <div style={{borderTop:'1px solid var(--lb-rule)', padding:'12px'}}>
            <div className="lb-label" style={{marginBottom:8}}>ENGINE</div>
            <div style={{display:'flex', alignItems:'center', gap:8, marginBottom:4}}>
              <Dot color="var(--lb-ok)" size={6} pulse/>
              <span className="lb-mono" style={{fontSize:11, color:'var(--lb-ok)', letterSpacing:'0.06em'}}>ACTIVE · 60 fps</span>
            </div>
            <div className="lb-mono" style={{fontSize:10, color:'var(--lb-ink-muted)'}}>2.1 ms · 0.4% CPU · 14.2 MB</div>
            <div style={{marginTop:8}}>
              <Sparkline data={[3,2,3,2,2,3,2,2,3,2,2,4,3,2,2,3]} width={170} height={20} color="var(--lb-ok)" fill={false}/>
            </div>
          </div>
        </div>
        <div style={{flex:1, minWidth:0, overflow:'auto'}}>{children}</div>
      </div>
    </WinFrame>
  );
}

function ScreenMainSettings() {
  return (
    <MainSettingsShell subtitle="OVERVIEW">
      <div style={{padding:'24px 28px', display:'flex', flexDirection:'column', gap:18}}>
        {/* HERO */}
        <div style={{display:'flex', alignItems:'flex-start', gap:24, borderBottom:'1px solid var(--lb-rule)', paddingBottom:20}}>
          <div style={{flex:1}}>
            <span className="lb-label">SYSTEM STATUS</span>
            <div style={{
              fontFamily:'var(--lb-font-mono)', fontSize:32, fontWeight:600, marginTop:8,
              color:'var(--lb-ink)', letterSpacing:'-0.02em', lineHeight:1,
            }}>
              <span style={{color:'var(--lb-ok)'}}>●</span> Set &amp; forgotten.
            </div>
            <div style={{fontSize:12, color:'var(--lb-ink-muted)', marginTop:8, maxWidth:480}}>
              The detector is finding ads on its own. You haven't had to mark anything in <span style={{color:'var(--lb-ink-2)'}}>4 days</span>.
              Auto-confidence is <span style={{color:'var(--lb-ml)'}}>84%</span>; raise it if you want fewer false positives.
            </div>
          </div>
          <div style={{display:'flex', flexDirection:'column', alignItems:'flex-end', gap:6}}>
            <Toggle value={true} onChange={()=>{}}/>
            <span className="lb-mono" style={{fontSize:9, color:'var(--lb-ink-faint)', letterSpacing:'0.10em'}}>MASTER · UPTIME 14h 02m</span>
          </div>
        </div>

        {/* STAT GRID */}
        <div style={{display:'grid', gridTemplateColumns:'1.4fr 1fr 1fr', gap:14}}>
          <div className="lb-panel" style={{padding:18}}>
            <div style={{display:'flex', alignItems:'baseline', gap:8, marginBottom:8}}>
              <span className="lb-label">BLOCKED · 24H</span>
              <div style={{flex:1}}/>
              <span className="lb-mono" style={{fontSize:10, color:'var(--lb-ok)', letterSpacing:'0.10em'}}>+18%</span>
            </div>
            <div style={{display:'flex', alignItems:'baseline', gap:10}}>
              <span style={{fontFamily:'var(--lb-font-mono)', fontSize:52, fontWeight:600, color:'var(--lb-block)', letterSpacing:'-0.04em', lineHeight:0.9}}>847</span>
              <span className="lb-mono" style={{fontSize:11, color:'var(--lb-ink-muted)'}}>regions filled</span>
            </div>
            <div style={{marginTop:14}}>
              <Sparkline data={[12,15,11,18,22,19,28,25,32,30,38,42,36,48,55,52,61,58,72,68,82,79,91,87]} width={400} height={48} color="var(--lb-block)"/>
            </div>
            <div className="lb-mono" style={{fontSize:9, color:'var(--lb-ink-faint)', letterSpacing:'0.12em', marginTop:6, display:'flex', justifyContent:'space-between'}}>
              <span>00:00</span><span>12:00</span><span>NOW</span>
            </div>
          </div>

          <div className="lb-panel" style={{padding:18}}>
            <span className="lb-label">ACTIVE REGIONS</span>
            <div style={{display:'flex', alignItems:'baseline', gap:6, marginTop:8}}>
              <span style={{fontFamily:'var(--lb-font-mono)', fontSize:38, fontWeight:600, color:'var(--lb-ink)', lineHeight:0.95}}>14</span>
              <span className="lb-mono" style={{fontSize:11, color:'var(--lb-ink-muted)'}}>/ 256 max</span>
            </div>
            <div style={{marginTop:14}}>
              <BarMeter value={14/256} length={18} accent="var(--lb-ink-2)"/>
            </div>
            <div className="lb-mono" style={{fontSize:10, color:'var(--lb-ink-muted)', marginTop:8}}>
              across <span style={{color:'var(--lb-ink-2)'}}>6</span> apps · <span style={{color:'var(--lb-ml)'}}>11</span> auto-learned
            </div>
          </div>

          <div className="lb-panel" style={{padding:18}}>
            <span className="lb-label">FRAME BUDGET</span>
            <div style={{display:'flex', alignItems:'baseline', gap:6, marginTop:8}}>
              <span style={{fontFamily:'var(--lb-font-mono)', fontSize:38, fontWeight:600, color:'var(--lb-ml)', lineHeight:0.95}}>2.1</span>
              <span className="lb-mono" style={{fontSize:14, color:'var(--lb-ink-muted)'}}>ms</span>
            </div>
            <div style={{marginTop:14}}>
              <BarMeter value={2.1/16.6} length={18} accent="var(--lb-ml)"/>
            </div>
            <div className="lb-mono" style={{fontSize:10, color:'var(--lb-ink-muted)', marginTop:8}}>
              of 16.6 ms · <span style={{color:'var(--lb-ok)'}}>87% headroom</span>
            </div>
          </div>
        </div>

        {/* FILL TECHNIQUE */}
        <div className="lb-panel">
          <div style={{padding:'10px 14px', borderBottom:'1px solid var(--lb-rule)', display:'flex', alignItems:'center', gap:10}}>
            <span className="lb-label">FILL TECHNIQUE · how blocked regions get repainted</span>
            <div style={{flex:1}}/>
            <span className="lb-mono" style={{fontSize:10, color:'var(--lb-ink-muted)'}}>4 OPTIONS</span>
          </div>
          <div style={{display:'grid', gridTemplateColumns:'repeat(4, 1fr)'}}>
            {[
              { l:'EDGE-EXTRAPOLATE', d:'Continue surrounding pixels', on:true },
              { l:'GAUSSIAN BLUR', d:'Blur sampled neighborhood', on:false },
              { l:'AVG COLOR', d:'Median of region edge', on:false },
              { l:'OPAQUE', d:'Hard mask · solid black', on:false },
            ].map((m,i) => (
              <div key={i} style={{
                padding:'14px', borderRight: i<3 ? '1px solid var(--lb-rule)' : 'none',
                background: m.on ? 'var(--lb-panel-2)' : 'transparent', cursor:'pointer',
                position:'relative',
              }}>
                {/* preview */}
                <div style={{
                  height:48, marginBottom:10, position:'relative', overflow:'hidden',
                  border:'1px solid var(--lb-rule)',
                  background: m.l === 'OPAQUE' ? '#0a0a0c'
                    : m.l === 'AVG COLOR' ? '#3a2f54'
                    : m.l === 'GAUSSIAN BLUR' ? 'linear-gradient(135deg,#3a2f54,#5e4a82)'
                    : 'repeating-linear-gradient(45deg,#3a2f54 0 8px,#4a3e6a 8px 16px)',
                }}>
                  {m.on && (
                    <span style={{
                      position:'absolute', top:4, right:4, width:14, height:14,
                      background:'var(--lb-block)', display:'flex', alignItems:'center', justifyContent:'center',
                      color:'#0a0a0c',
                    }}>{Icon.check(10)}</span>
                  )}
                </div>
                <div style={{fontFamily:'var(--lb-font-mono)', fontSize:11, fontWeight:600, color:'var(--lb-ink)', letterSpacing:'0.04em'}}>{m.l}</div>
                <div style={{fontSize:11, color:'var(--lb-ink-muted)', marginTop:3}}>{m.d}</div>
              </div>
            ))}
          </div>
        </div>

        {/* SETTINGS LIST */}
        <div className="lb-panel">
          {[
            { l:'AUTO-BLOCK ML SUGGESTIONS', d:'Apply detector predictions above the confidence threshold', t:true },
            { l:'PAUSE IN FULLSCREEN VIDEO', d:'Skip blocking inside detected fullscreen video apps', t:true },
            { l:'EDGE CHIP ON SCREEN', d:'Show the always-visible kill chip in the corner of every display', t:true },
            { l:'ANONYMOUS TELEMETRY', d:'Frame timing only — no pixel content, ever', t:false },
          ].map((r,i) => (
            <div key={i} style={{
              display:'flex', alignItems:'center', gap:14, padding:'12px 16px',
              borderTop: i ? '1px solid var(--lb-rule)' : 'none',
            }}>
              <div style={{flex:1}}>
                <div style={{fontFamily:'var(--lb-font-mono)', fontSize:12, fontWeight:600, color:'var(--lb-ink)', letterSpacing:'0.04em'}}>{r.l}</div>
                <div style={{fontSize:11, color:'var(--lb-ink-muted)', marginTop:3}}>{r.d}</div>
              </div>
              <Toggle value={r.t} onChange={()=>{}}/>
            </div>
          ))}
        </div>
      </div>
    </MainSettingsShell>
  );
}

// ── 6. REGION LIBRARY ──────────────────────────────────────
function ScreenRegionLibrary() {
  const regions = [
    { app:'BROWSER', site:'youtube.com', name:'right-rail · sponsored', size:'300×600', hits:142, on:true, c:'var(--lb-block)' },
    { app:'BROWSER', site:'youtube.com', name:'in-feed promo cards', size:'728×90', hits:88, on:true, c:'var(--lb-block)' },
    { app:'SPOTIFY', site:'app', name:'now-playing banner', size:'300×250', hits:34, on:true, c:'var(--lb-ok)' },
    { app:'BROWSER', site:'x.com', name:'promoted tweets', size:'auto', hits:215, on:false, c:'var(--lb-ink-muted)' },
    { app:'REDDIT', site:'app', name:'sidebar ads', size:'300×600', hits:76, on:true, c:'var(--lb-warn)' },
    { app:'MAIL', site:'app', name:'newsletter footer', size:'variable', hits:12, on:true, c:'var(--lb-info)' },
    { app:'CURSOR', site:'app', name:'update toast', size:'320×80', hits:4, on:false, c:'var(--lb-ml)' },
  ];
  return (
    <MainSettingsShell active="REGION LIBRARY" subtitle="REGION LIBRARY">
      <div style={{padding:'24px 28px', display:'flex', flexDirection:'column', gap:14, height:'100%'}}>
        <div style={{display:'flex', alignItems:'center', gap:12}}>
          <div style={{flex:1}}>
            <span className="lb-label">REGION LIBRARY · ALL APPS</span>
            <div style={{fontFamily:'var(--lb-font-mono)', fontSize:24, fontWeight:600, color:'var(--lb-ink)', letterSpacing:'-0.01em', marginTop:6}}>
              14 saved <span style={{color:'var(--lb-ink-faint)'}}>·</span> 7 active <span style={{color:'var(--lb-ink-faint)'}}>·</span> 11 ML-learned
            </div>
          </div>
          <Field icon={Icon.search(12)} placeholder="Find a region…" rightKbd="/" width={220}/>
          <Btn size="md" variant="hot" icon={Icon.plus(11)}>NEW</Btn>
        </div>

        {/* table */}
        <div className="lb-panel" style={{flex:1, display:'flex', flexDirection:'column', minHeight:0}}>
          <div style={{
            display:'grid', gridTemplateColumns:'20px 1.8fr 1.1fr 90px 70px 70px 24px',
            gap:12, padding:'8px 14px', borderBottom:'1px solid var(--lb-rule)',
            fontFamily:'var(--lb-font-mono)', fontSize:9, color:'var(--lb-ink-muted)',
            letterSpacing:'0.10em', textTransform:'uppercase',
          }}>
            <span/><span>REGION</span><span>APP · CONTEXT</span><span>SIZE</span><span>BLOCKS</span><span>ACTIVE</span><span/>
          </div>
          <div style={{flex:1, overflow:'auto'}}>
            {regions.map((r,i) => (
              <div key={i} style={{
                display:'grid', gridTemplateColumns:'20px 1.8fr 1.1fr 90px 70px 70px 24px',
                gap:12, alignItems:'center', padding:'9px 14px',
                borderTop: i ? '1px solid var(--lb-rule)' : 'none',
                background: i===0 ? 'var(--lb-panel-2)' : 'transparent',
                borderLeft: i===0 ? '2px solid var(--lb-block)' : '2px solid transparent',
              }}>
                <span style={{color:'var(--lb-ink-faint)', display:'inline-flex'}}>{Icon.drag(12)}</span>
                <div style={{display:'flex', alignItems:'center', gap:10}}>
                  {/* preview thumb */}
                  <div style={{width:32, height:20, background:'#0a0a0c', border:'1px solid var(--lb-rule)', position:'relative'}}>
                    <div style={{position:'absolute', inset:3, border:'1px dashed var(--lb-block)'}}/>
                  </div>
                  <span style={{fontFamily:'var(--lb-font-mono)', fontSize:12, color:'var(--lb-ink)', letterSpacing:'0.02em'}}>{r.name}</span>
                </div>
                <div style={{display:'flex', alignItems:'center', gap:8}}>
                  <div style={{width:8, height:8, background:r.c}}/>
                  <span style={{fontFamily:'var(--lb-font-mono)', fontSize:11, color:'var(--lb-ink-2)', letterSpacing:'0.04em'}}>{r.app}</span>
                  <span className="lb-mono" style={{fontSize:10, color:'var(--lb-ink-faint)'}}>· {r.site}</span>
                </div>
                <span className="lb-mono" style={{fontSize:11, color:'var(--lb-ink-muted)'}}>{r.size}</span>
                <span className="lb-mono" style={{fontSize:12, color:'var(--lb-block)', fontWeight:600}}>{r.hits}</span>
                <Toggle value={r.on} onChange={()=>{}}/>
                <button style={{background:'transparent', border:'none', color:'var(--lb-ink-faint)', cursor:'pointer'}}>{Icon.trash(12)}</button>
              </div>
            ))}
          </div>
        </div>
      </div>
    </MainSettingsShell>
  );
}

Object.assign(window, { ScreenTray, ScreenMainSettings, ScreenRegionLibrary, MainSettingsShell });
