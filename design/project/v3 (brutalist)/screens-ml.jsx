// LiveBlocker — ML detector + in-action before/after + cross-OS chrome card.

// ── 7. ML DETECTOR TUNING ──────────────────────────────────
function ScreenMLDetector() {
  const cats = [
    { l:'BANNER ADS',          n:412, on:true },
    { l:'VIDEO PRE-ROLL',      n:87,  on:true },
    { l:'SPONSORED CARDS',     n:234, on:true },
    { l:'NEWSLETTER POPUPS',   n:56,  on:true },
    { l:'COOKIE BANNERS',      n:188, on:false },
    { l:'CHAT WIDGETS',        n:23,  on:false },
    { l:'ENGAGEMENT NUDGES',   n:41,  on:false },
  ];
  return (
    <MainSettingsShell active="ML DETECTOR" subtitle="ML DETECTOR">
      <div style={{padding:'22px 26px', display:'grid', gridTemplateColumns:'1.3fr 1fr', gap:16, gridTemplateRows:'auto auto 1fr', minHeight:0, height:'100%'}}>
        {/* HEADER */}
        <div style={{gridColumn:'1 / -1', display:'flex', alignItems:'center', gap:12}}>
          <div style={{flex:1}}>
            <span className="lb-label">DETECTOR · liveblocker-vit-v3 · 14.2 MB · ON-DEVICE</span>
            <div style={{fontFamily:'var(--lb-font-mono)', fontSize:24, fontWeight:600, color:'var(--lb-ink)', letterSpacing:'-0.01em', marginTop:6}}>
              Confidence <span style={{color:'var(--lb-ml)'}}>84%</span> <span style={{color:'var(--lb-ink-faint)'}}>·</span> auto-blocking <span style={{color:'var(--lb-ok)'}}>ON</span>
            </div>
          </div>
          <Btn size="md" variant="ghost-bright" icon={Icon.ml(11)}>TRAIN ON SELECTION</Btn>
        </div>

        {/* CONFIDENCE / THRESHOLD ROW */}
        <div className="lb-panel" style={{gridColumn:'1 / -1', padding:'14px 18px'}}>
          <div style={{display:'flex', alignItems:'center', gap:14, marginBottom:10}}>
            <span className="lb-label">AUTO-BLOCK THRESHOLD</span>
            <div style={{flex:1}}/>
            <span className="lb-mono" style={{fontSize:11, color:'var(--lb-ink-muted)'}}>
              <span style={{color:'var(--lb-block)'}}>↑</span> more aggressive
              <span style={{color:'var(--lb-ink-faint)', margin:'0 8px'}}>·</span>
              <span style={{color:'var(--lb-ml)'}}>↓</span> fewer false positives
            </span>
            <span className="lb-mono" style={{fontSize:18, fontWeight:600, color:'var(--lb-ml)', letterSpacing:'-0.02em'}}>84<span style={{fontSize:11, color:'var(--lb-ink-muted)'}}>%</span></span>
          </div>
          <Slider value={84} min={50} max={99} onChange={()=>{}} accent="var(--lb-ml)"/>
          <div className="lb-mono" style={{display:'flex', justifyContent:'space-between', fontSize:9, color:'var(--lb-ink-faint)', letterSpacing:'0.10em', marginTop:6}}>
            <span>50%</span><span>60%</span><span>70%</span><span>80%</span><span>90%</span><span>99%</span>
          </div>
        </div>

        {/* LIVE PREVIEW */}
        <div className="lb-panel" style={{minHeight:0, display:'flex', flexDirection:'column'}}>
          <div style={{padding:'8px 12px', borderBottom:'1px solid var(--lb-rule)', display:'flex', alignItems:'center', gap:8}}>
            <Dot color="var(--lb-block)" size={6} pulse/>
            <span className="lb-label">LIVE PREVIEW · CAPTURE STREAM</span>
            <div style={{flex:1}}/>
            <span className="lb-mono" style={{fontSize:10, color:'var(--lb-ink-muted)'}}>3 DET · 8.4 MS</span>
          </div>
          <div style={{flex:1, position:'relative', overflow:'hidden', background:'#0e0a18'}}>
            <div className="lb-scan" style={{position:'absolute', inset:0, opacity:0.5}}/>
            {/* faux content */}
            {[14,32,22,38,26,30,18].map((w,i) => (
              <div key={i} style={{position:'absolute', left:'8%', top:`${8 + i*9}%`, width:`${w}%`, height:3, background:'rgba(255,255,255,0.16)'}}/>
            ))}
            {/* detection boxes */}
            {[
              { x:6, y:12, w:34, h:14, label:'BANNER', conf:0.94, c:'var(--lb-block)', dashed:false },
              { x:62, y:38, w:32, h:38, label:'SPONSORED', conf:0.87, c:'var(--lb-block)', dashed:false },
              { x:10, y:66, w:28, h:20, label:'POPUP', conf:0.62, c:'var(--lb-warn)', dashed:true },
            ].map((d,i) => (
              <div key={i} style={{
                position:'absolute', left:`${d.x}%`, top:`${d.y}%`, width:`${d.w}%`, height:`${d.h}%`,
                border:`1px ${d.dashed ? 'dashed' : 'solid'} ${d.c}`,
                background: d.dashed ? 'transparent' : `${d.c}1f`,
              }}>
                {/* corner ticks */}
                {[[0,0],[100,0],[0,100],[100,100]].map(([px,py],j) => (
                  <div key={j} style={{
                    position:'absolute', left:`${px}%`, top:`${py}%`, width:5, height:5, marginLeft:-3, marginTop:-3, background:d.c,
                  }}/>
                ))}
                <div className="lb-mono" style={{
                  position:'absolute', top:-1, left:-1, transform:'translateY(-100%)',
                  background: d.c, color:'#0a0a0c',
                  padding:'1px 5px', fontSize:9, fontWeight:700, letterSpacing:'0.08em',
                }}>{d.label} · {Math.round(d.conf*100)}%</div>
              </div>
            ))}
          </div>
        </div>

        {/* CATEGORIES */}
        <div className="lb-panel" style={{minHeight:0, display:'flex', flexDirection:'column'}}>
          <div style={{padding:'8px 12px', borderBottom:'1px solid var(--lb-rule)', display:'flex', alignItems:'center'}}>
            <span className="lb-label">CATEGORIES · 7</span>
            <div style={{flex:1}}/>
            <span className="lb-mono" style={{fontSize:10, color:'var(--lb-warn)'}}>3 REVIEW</span>
          </div>
          <div style={{flex:1, overflow:'auto'}}>
            {cats.map((c,i) => (
              <div key={c.l} style={{
                display:'flex', alignItems:'center', gap:10, padding:'9px 12px',
                borderTop: i ? '1px solid var(--lb-rule)' : 'none',
              }}>
                <div style={{width:3, height:24, background: c.on ? 'var(--lb-block)' : 'var(--lb-ink-faint)'}}/>
                <div style={{flex:1, minWidth:0}}>
                  <div style={{fontFamily:'var(--lb-font-mono)', fontSize:11, fontWeight:600, color:'var(--lb-ink)', letterSpacing:'0.04em'}}>{c.l}</div>
                  <div className="lb-mono" style={{fontSize:10, color:'var(--lb-ink-muted)', marginTop:2}}>
                    {c.n.toLocaleString()} caught · 30d
                  </div>
                </div>
                <div style={{width:60}}>
                  <BarMeter value={c.n / 500} length={8} accent={c.on ? 'var(--lb-block)' : 'var(--lb-ink-faint)'}/>
                </div>
                <Toggle value={c.on} onChange={()=>{}}/>
              </div>
            ))}
          </div>
          <div style={{padding:'10px 12px', borderTop:'1px solid var(--lb-rule)', display:'flex', alignItems:'center', gap:10}}>
            <span style={{fontFamily:'var(--lb-font-mono)', fontSize:22, fontWeight:600, color:'var(--lb-warn)', lineHeight:1}}>3</span>
            <span className="lb-mono" style={{fontSize:11, color:'var(--lb-ink-2)', flex:1}}>borderline detections waiting</span>
            <Btn size="sm" variant="ghost" icon={Icon.arrow(11)}>REVIEW</Btn>
          </div>
        </div>
      </div>
    </MainSettingsShell>
  );
}

// ── 8. IN-ACTION before/after ───────────────────────────────
function ScreenInAction({ mode='after' }) {
  return (
    <div className="lb" style={{ width:720, height:460, position:'relative', overflow:'hidden', border:'1px solid var(--lb-rule)' }}>
      <div style={{position:'absolute', inset:0, background:'linear-gradient(160deg,#0e0a18 0%, #1a1230 50%, #2a1240 100%)'}}/>
      <div className="lb-scan" style={{position:'absolute', inset:0, opacity:0.4}}/>

      {/* fake content window */}
      <div style={{position:'absolute', left:32, top:30, right:32, bottom:62, border:'1px solid var(--lb-rule)', background:'var(--lb-panel)', display:'flex', flexDirection:'column', overflow:'hidden'}}>
        <div style={{height:28, borderBottom:'1px solid var(--lb-rule)', display:'flex', alignItems:'center', gap:8, padding:'0 10px'}}>
          <div style={{display:'flex', gap:4}}>{[0,1,2].map(i => <span key={i} style={{width:6, height:6, border:'1px solid var(--lb-rule-3)'}}/>)}</div>
          <div style={{flex:1, textAlign:'center'}}>
            <span className="lb-mono" style={{fontSize:10, color:'var(--lb-ink-muted)', letterSpacing:'0.06em'}}>news.example.com / article-04</span>
          </div>
        </div>
        <div style={{flex:1, padding:18, display:'grid', gridTemplateColumns:'1.6fr 1fr', gap:18, fontFamily:'var(--lb-font-mono)'}}>
          {/* article */}
          <div>
            <div style={{fontSize:18, fontWeight:600, color:'var(--lb-ink)', marginBottom:8, letterSpacing:'-0.01em'}}>Top stories today</div>
            <div className="lb-mono" style={{fontSize:9, color:'var(--lb-ink-muted)', letterSpacing:'0.10em', marginBottom:12}}>NEWS · 6 MIN READ</div>
            {[88,72,94,80,90,68,86,76,92,70,84].map((w,i) => (
              <div key={i} style={{height:5, width:`${w}%`, background:'var(--lb-rule-2)', marginBottom:6}}/>
            ))}
            <div style={{height:60, background:'linear-gradient(135deg,var(--lb-rule),var(--lb-rule-2))', marginTop:8, marginBottom:10}}/>
            {[82,68,90].map((w,i) => (
              <div key={i} style={{height:5, width:`${w}%`, background:'var(--lb-rule-2)', marginBottom:6}}/>
            ))}
          </div>
          {/* right rail */}
          <div>
            <div style={{height:240, position:'relative', border:'1px solid var(--lb-rule)', overflow:'hidden'}}>
              {mode === 'before' && (
                <div style={{position:'absolute', inset:0, background:'linear-gradient(160deg,#fbbf24 0%,#ff3b1f 100%)', display:'flex', flexDirection:'column', alignItems:'center', justifyContent:'center', color:'#3a1500', fontFamily:'system-ui'}}>
                  <div style={{fontSize:24, fontWeight:800, letterSpacing:'-0.02em'}}>BUY NOW</div>
                  <div style={{fontSize:10, marginTop:6, opacity:0.85}}>Limited offer · Click here</div>
                  <div style={{marginTop:14, padding:'5px 14px', background:'#0a0a0c', color:'#fff', fontSize:10, fontWeight:600, letterSpacing:'0.08em'}}>SHOP →</div>
                </div>
              )}
              {mode === 'after' && (
                <div style={{position:'absolute', inset:0, background:'var(--lb-panel)', display:'flex', flexDirection:'column', alignItems:'center', justifyContent:'center', gap:6, color:'var(--lb-ink-muted)'}}>
                  <div style={{width:18, height:18, border:'1px solid var(--lb-rule-2)', display:'flex', alignItems:'center', justifyContent:'center'}}>
                    {Icon.block(10)}
                  </div>
                  <span className="lb-mono" style={{fontSize:9, color:'var(--lb-ink-muted)', letterSpacing:'0.14em'}}>BLOCKED · REGION #03</span>
                </div>
              )}
              {mode === 'after' && (
                <div style={{position:'absolute', inset:0, border:'1px dashed rgba(255,59,31,0.45)', pointerEvents:'none'}}>
                  {[[0,0],[100,0],[0,100],[100,100]].map(([px,py],i) => (
                    <div key={i} style={{position:'absolute', left:`${px}%`, top:`${py}%`, width:5, height:5, marginLeft:-3, marginTop:-3, background:'var(--lb-block)'}}/>
                  ))}
                </div>
              )}
            </div>
            <div style={{marginTop:12}}>
              <div className="lb-mono" style={{fontSize:9, color:'var(--lb-ink-muted)', letterSpacing:'0.10em', marginBottom:6}}>RELATED</div>
              {[80,68,88].map((w,i) => <div key={i} style={{height:5, width:`${w}%`, background:'var(--lb-rule-2)', marginBottom:5}}/>)}
            </div>
          </div>
        </div>
      </div>

      {/* HUD chip */}
      <div style={{position:'absolute', left:'50%', bottom:18, transform:'translateX(-50%)'}}>
        <div className="lb lb-panel" style={{
          display:'flex', alignItems:'stretch', height:30, background:'rgba(10,10,12,0.95)',
        }}>
          <div style={{display:'flex', alignItems:'center', gap:8, padding:'0 12px'}}>
            <Dot color={mode === 'after' ? 'var(--lb-block)' : 'var(--lb-ink-muted)'} size={6} pulse={mode === 'after'}/>
            <span className="lb-mono" style={{fontSize:10, color: mode === 'after' ? 'var(--lb-ink)' : 'var(--lb-ink-muted)', letterSpacing:'0.10em', fontWeight:600}}>
              {mode === 'after' ? 'BLOCKING' : 'PAUSED'}
            </span>
          </div>
          <Hairline vertical/>
          <div style={{padding:'0 12px', display:'flex', alignItems:'center', gap:8}}>
            <span className="lb-mono" style={{fontSize:10, color:'var(--lb-ink-muted)'}}>
              <span style={{color:'var(--lb-ml)'}}>2.1</span>ms · <span style={{color:'var(--lb-ok)'}}>3</span> fills
            </span>
          </div>
        </div>
      </div>

      {/* label */}
      <div className="lb-mono" style={{
        position:'absolute', top:10, left:'50%', transform:'translateX(-50%)',
        padding:'3px 10px', border:`1px solid ${mode==='after' ? 'var(--lb-ok)' : 'var(--lb-warn)'}`,
        color: mode==='after' ? 'var(--lb-ok)' : 'var(--lb-warn)',
        fontSize:9, fontWeight:600, letterSpacing:'0.14em',
      }}>
        {mode === 'after' ? 'AFTER · LIVEBLOCKER ENGAGED' : 'BEFORE · WITHOUT LIVEBLOCKER'}
      </div>
    </div>
  );
}
const ScreenInActionBefore = () => <ScreenInAction mode="before"/>;
const ScreenInActionAfter  = () => <ScreenInAction mode="after"/>;

// ── 9. CROSS-OS CHROME — one design across mac/win/linux ────
function OSPlatform({ os }) {
  // Tiny content card showing the floating tool on the host OS's wallpaper.
  const wall = os === 'mac'
    ? 'linear-gradient(160deg,#1c2952 0%,#3a2a78 50%,#7a3d8e 100%)'
    : os === 'win'
    ? 'linear-gradient(160deg,#0b3d6b 0%,#1e6fbf 50%,#3a8ad6 100%)'
    : 'linear-gradient(160deg,#2a1e0e 0%,#5e3a1a 50%,#8a5e2e 100%)';
  const label = os === 'mac' ? 'macOS' : os === 'win' ? 'Windows' : 'Linux';
  return (
    <div className="lb" style={{ flex:1, display:'flex', flexDirection:'column', border:'1px solid var(--lb-rule)', overflow:'hidden' }}>
      <div style={{padding:'6px 10px', borderBottom:'1px solid var(--lb-rule)', display:'flex', alignItems:'center', gap:8, background:'var(--lb-panel)'}}>
        <Dot color="var(--lb-block)" size={5}/>
        <span className="lb-mono" style={{fontSize:10, color:'var(--lb-ink-2)', letterSpacing:'0.10em', fontWeight:600}}>{label.toUpperCase()}</span>
        <div style={{flex:1}}/>
        <span className="lb-mono" style={{fontSize:9, color:'var(--lb-ink-faint)'}}>IDENTICAL UI</span>
      </div>
      <div style={{position:'relative', height:180, overflow:'hidden'}}>
        <div style={{position:'absolute', inset:0, background:wall}}/>
        <div className="lb-scan" style={{position:'absolute', inset:0, opacity:0.5}}/>
        {/* edge chip — top right, always */}
        <div style={{position:'absolute', top:0, right:18}}>
          <div className="lb-mono" style={{
            padding:'3px 8px', background:'#0a0a0c', color:'var(--lb-block)',
            border:'1px solid var(--lb-block)', borderTop:'none',
            fontSize:9, fontWeight:600, letterSpacing:'0.10em',
            display:'inline-flex', alignItems:'center', gap:5,
          }}>
            <Dot color="var(--lb-block)" size={4} pulse/>KILL · 847
          </div>
        </div>
        {/* floating tool */}
        <div style={{position:'absolute', left:'50%', bottom:14, transform:'translateX(-50%)'}}>
          <div className="lb lb-panel" style={{display:'flex', height:26, background:'rgba(10,10,12,0.95)', backdropFilter:'blur(8px)'}}>
            <div style={{display:'flex', alignItems:'center', gap:6, padding:'0 9px'}}>
              <Dot color="var(--lb-block)" size={5} pulse/>
              <span className="lb-mono" style={{fontSize:9, color:'var(--lb-ink)', letterSpacing:'0.10em', fontWeight:600}}>SMART</span>
            </div>
            <Hairline vertical/>
            <div style={{padding:'0 9px', display:'flex', alignItems:'center', gap:6}}>
              <span className="lb-mono" style={{fontSize:9, color:'var(--lb-ink-muted)'}}><span style={{color:'var(--lb-ml)'}}>3</span> CANDIDATES</span>
            </div>
            <Hairline vertical/>
            <div style={{padding:'0 9px', display:'flex', alignItems:'center', gap:5, color:'var(--lb-block)'}}>
              <span className="lb-mono" style={{fontSize:9, fontWeight:700, letterSpacing:'0.10em'}}>KILL</span>
              <span style={{fontSize:9, padding:'0 4px', border:'1px solid var(--lb-block)'}}>↵</span>
            </div>
          </div>
        </div>
      </div>
      <div style={{padding:'8px 10px', borderTop:'1px solid var(--lb-rule)', fontSize:10, color:'var(--lb-ink-muted)', display:'flex', alignItems:'center', gap:8}}>
        <span className="lb-mono">HOTKEY</span>
        <Kbd>{os === 'mac' ? '⌘⇧K' : 'Ctrl+⇧+K'}</Kbd>
        <div style={{flex:1}}/>
        <span className="lb-mono" style={{fontSize:9, color:'var(--lb-ink-faint)'}}>{os === 'mac' ? 'ScreenCaptureKit' : os === 'win' ? 'Windows.Graphics' : 'PipeWire'}</span>
      </div>
    </div>
  );
}

function ScreenCrossOS() {
  return (
    <div className="lb lb-panel" style={{ width:720, height:340, padding:18, display:'flex', flexDirection:'column', gap:14 }}>
      <div>
        <span className="lb-label">CROSS-PLATFORM · SAME APP · NATIVE FEEL</span>
        <div style={{
          fontFamily:'var(--lb-font-mono)', fontSize:18, fontWeight:600, color:'var(--lb-ink)',
          letterSpacing:'-0.01em', marginTop:6,
        }}>
          One UI. Three operating systems. Zero compromise.
        </div>
      </div>
      <div style={{display:'flex', gap:14, flex:1, minHeight:0}}>
        <OSPlatform os="mac"/>
        <OSPlatform os="win"/>
        <OSPlatform os="linux"/>
      </div>
    </div>
  );
}

Object.assign(window, { ScreenMLDetector, ScreenInActionBefore, ScreenInActionAfter, ScreenCrossOS });
