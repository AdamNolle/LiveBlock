// LiveBlocker — floating marking tool screens.
// Two solutions to the same problem, presented side-by-side:
//
//   v3-SMART  — hover-to-snap. ML proposes rectangles in real time;
//               your cursor magnetically locks to the nearest candidate.
//               Tap to block. No drag.
//
//   v3-CLASSIC — drag-to-mark, refined.  Snap rails, live size readout,
//                ENTER to confirm, no dancing modes.

// Shared faux desktop (a content area with some "ads" to mark)
function FauxDesktop({ children }) {
  return (
    <div style={{position:'absolute', inset:0, background:'#0e0a18', overflow:'hidden'}}>
      <div className="lb-scan" style={{position:'absolute', inset:0, opacity:0.4}}/>
      {/* fake article columns */}
      <div style={{position:'absolute', left:24, top:20, right:24, bottom:80, display:'grid', gridTemplateColumns:'1.4fr 1fr', gap:18}}>
        {/* text column */}
        <div style={{display:'flex', flexDirection:'column', gap:6}}>
          <div style={{height:14, width:'70%', background:'rgba(255,255,255,0.55)', marginBottom:4}}/>
          <div style={{height:5, width:'94%', background:'rgba(255,255,255,0.18)'}}/>
          <div style={{height:5, width:'88%', background:'rgba(255,255,255,0.18)'}}/>
          <div style={{height:5, width:'72%', background:'rgba(255,255,255,0.18)'}}/>
          <div style={{height:5, width:'90%', background:'rgba(255,255,255,0.18)'}}/>
          <div style={{height:5, width:'80%', background:'rgba(255,255,255,0.18)'}}/>
          <div style={{height:5, width:'94%', background:'rgba(255,255,255,0.18)'}}/>
          <div style={{height:60, width:'100%', background:'rgba(255,255,255,0.12)', marginTop:8}}/>
          <div style={{height:5, width:'88%', background:'rgba(255,255,255,0.18)', marginTop:8}}/>
          <div style={{height:5, width:'76%', background:'rgba(255,255,255,0.18)'}}/>
        </div>
        {/* right rail (where the ad lives) */}
        <div style={{display:'flex', flexDirection:'column', gap:10}}>
          {children}
          <div style={{height:5, width:'70%', background:'rgba(255,255,255,0.18)', marginTop:8}}/>
          <div style={{height:5, width:'90%', background:'rgba(255,255,255,0.18)'}}/>
          <div style={{height:5, width:'62%', background:'rgba(255,255,255,0.18)'}}/>
        </div>
      </div>
    </div>
  );
}

// ── A) SMART: hover-to-snap ──────────────────────────────────
// One detected candidate is in the right rail; cursor is hovering it;
// the box is "locked" with crosshair + label. Tap to block.
function ScreenFloatingSmart() {
  return (
    <div className="lb" style={{ width:540, height:380, position:'relative', overflow:'hidden', border:'1px solid var(--lb-rule)' }}>
      <FauxDesktop>
        {/* the candidate "ad" surface */}
        <div style={{
          height:140, width:'100%', position:'relative', overflow:'hidden',
          background:'linear-gradient(160deg,#fbbf24 0%,#ff7a3a 100%)',
        }}>
          <div style={{
            position:'absolute', inset:0, display:'flex', flexDirection:'column',
            alignItems:'center', justifyContent:'center', color:'#3a1500',
            fontFamily:'system-ui, sans-serif', fontWeight:800,
          }}>
            <div style={{fontSize:20, letterSpacing:'-0.02em'}}>SHOP NOW</div>
            <div style={{fontSize:10, opacity:0.7}}>Limited time · 50% off</div>
          </div>
        </div>
      </FauxDesktop>

      {/* SNAP overlay — the locked candidate */}
      <div style={{
        position:'absolute', right: 24, top: 20, width: 'calc((100% - 24px - 24px - 18px) * (1 / 2.4))', height: 140,
        border:'1px solid var(--lb-block)', background:'rgba(255,59,31,0.16)',
        boxShadow:'0 0 0 3000px rgba(10,10,12,0.55)',
        pointerEvents:'none',
      }}>
        {/* corner ticks */}
        {[[0,0],[100,0],[0,100],[100,100]].map(([px,py],i) => (
          <div key={i} style={{
            position:'absolute', left:`${px}%`, top:`${py}%`,
            width:8, height:8, marginLeft:-4, marginTop:-4,
            background:'var(--lb-block)',
          }}/>
        ))}
        {/* label */}
        <div className="lb-mono" style={{
          position:'absolute', top:-1, left:-1, transform:'translateY(-100%)',
          background:'var(--lb-block)', color:'#0a0a0c',
          padding:'2px 7px', fontSize:10, fontWeight:700, letterSpacing:'0.08em',
          display:'inline-flex', alignItems:'center', gap:6,
        }}>
          BANNER · 96%
          <span style={{opacity:0.6}}>300×140</span>
        </div>
        {/* tap-to-block hint */}
        <div className="lb-mono" style={{
          position:'absolute', bottom:-1, right:-1, transform:'translateY(100%)',
          background:'#0a0a0c', color:'var(--lb-block)', border:'1px solid var(--lb-block)',
          padding:'2px 7px', fontSize:10, fontWeight:600, letterSpacing:'0.08em',
        }}>CLICK TO KILL ▸</div>
      </div>

      {/* cursor */}
      <div style={{position:'absolute', right:'14%', top:'34%', pointerEvents:'none'}}>
        <Crosshair color="var(--lb-block)" size={26}/>
      </div>

      {/* The floating tool itself — a single, minimal strip pinned to the bottom */}
      <div style={{position:'absolute', left:'50%', bottom:14, transform:'translateX(-50%)'}}>
        <div className="lb lb-panel" style={{
          display:'flex', alignItems:'stretch', height:36, background:'rgba(10,10,12,0.95)',
          backdropFilter:'blur(10px)',
        }}>
          <div style={{display:'flex', alignItems:'center', gap:8, padding:'0 12px'}}>
            <Dot color="var(--lb-block)" size={6} pulse/>
            <span className="lb-mono" style={{fontSize:10, color:'var(--lb-ink)', letterSpacing:'0.10em', fontWeight:600}}>
              SMART · HOVER TO SNAP
            </span>
          </div>
          <Hairline vertical/>
          {/* the three actions the user needs — no S/M/L/FS nonsense */}
          {[
            { l: 'KILL', kb: '↵', hot:true, icon: Icon.block(11) },
            { l: 'SKIP', kb: '↹', icon: Icon.arrow(11) },
            { l: 'EXIT', kb: 'ESC', icon: Icon.x(11) },
          ].map((b,i) => (
            <React.Fragment key={i}>
              <button style={{
                padding:'0 14px', background:'transparent', border:'none',
                color: b.hot ? 'var(--lb-block)' : 'var(--lb-ink-2)',
                fontFamily:'var(--lb-font-mono)', fontSize:11, fontWeight:600,
                letterSpacing:'0.06em', cursor:'pointer',
                display:'inline-flex', alignItems:'center', gap:6,
              }}>
                {b.icon}
                {b.l}
                <Kbd style={{borderColor: b.hot ? 'var(--lb-block)' : undefined}}>{b.kb}</Kbd>
              </button>
              {i < 2 && <Hairline vertical/>}
            </React.Fragment>
          ))}
        </div>
      </div>

      {/* state label */}
      <div className="lb-mono" style={{
        position:'absolute', top:10, left:12, fontSize:9, color:'rgba(255,255,255,0.4)',
        letterSpacing:'0.16em',
      }}>v3·A · SMART · 1 CANDIDATE LOCKED</div>
    </div>
  );
}

// ── B) CLASSIC: refined drag-to-mark ─────────────────────────
function ScreenFloatingClassic() {
  return (
    <div className="lb" style={{ width:540, height:380, position:'relative', overflow:'hidden', border:'1px solid var(--lb-rule)' }}>
      <FauxDesktop>
        <div style={{
          height:140, width:'100%', position:'relative', overflow:'hidden',
          background:'linear-gradient(160deg,#fbbf24 0%,#ff7a3a 100%)',
        }}>
          <div style={{
            position:'absolute', inset:0, display:'flex', flexDirection:'column',
            alignItems:'center', justifyContent:'center', color:'#3a1500',
            fontFamily:'system-ui, sans-serif', fontWeight:800,
          }}>
            <div style={{fontSize:20}}>SHOP NOW</div>
          </div>
        </div>
      </FauxDesktop>

      {/* user-drawn marquee */}
      <div style={{
        position:'absolute', left:'10%', top:48, width:'82%', height:172,
        border:'1px solid var(--lb-block)', background:'rgba(255,59,31,0.10)',
        boxShadow:'0 0 0 3000px rgba(10,10,12,0.55)',
      }}>
        {/* snap rails — guidelines */}
        <div style={{position:'absolute', left:-2000, right:-2000, top:0, height:1, background:'var(--lb-block)', opacity:0.45}}/>
        <div style={{position:'absolute', left:-2000, right:-2000, bottom:0, height:1, background:'var(--lb-block)', opacity:0.45}}/>
        <div style={{position:'absolute', top:-2000, bottom:-2000, left:0, width:1, background:'var(--lb-block)', opacity:0.45}}/>
        <div style={{position:'absolute', top:-2000, bottom:-2000, right:0, width:1, background:'var(--lb-block)', opacity:0.45}}/>
        {/* corner handles */}
        {[[0,0],[100,0],[0,100],[100,100]].map(([px,py],i) => (
          <div key={i} style={{
            position:'absolute', left:`${px}%`, top:`${py}%`,
            width:9, height:9, marginLeft:-5, marginTop:-5,
            background:'#0a0a0c', border:'1px solid var(--lb-block)',
          }}/>
        ))}
        {/* W / H readouts */}
        <div className="lb-mono" style={{
          position:'absolute', top:-1, left:'50%', transform:'translate(-50%,-100%)',
          background:'var(--lb-block)', color:'#0a0a0c',
          padding:'2px 6px', fontSize:10, fontWeight:700,
        }}>410 W</div>
        <div className="lb-mono" style={{
          position:'absolute', left:-1, top:'50%', transform:'translate(-100%,-50%) rotate(0deg)',
          background:'var(--lb-block)', color:'#0a0a0c',
          padding:'2px 6px', fontSize:10, fontWeight:700,
        }}>172 H</div>
        {/* snap hint */}
        <div className="lb-mono" style={{
          position:'absolute', bottom:-1, right:-1, transform:'translateY(100%)',
          background:'#0a0a0c', color:'var(--lb-block)', border:'1px solid var(--lb-block)',
          padding:'2px 7px', fontSize:9, fontWeight:600, letterSpacing:'0.08em',
        }}>SNAPPED · BANNER · 96%</div>
      </div>

      {/* floating tool */}
      <div style={{position:'absolute', left:'50%', bottom:14, transform:'translateX(-50%)'}}>
        <div className="lb lb-panel" style={{
          display:'flex', alignItems:'stretch', height:36, background:'rgba(10,10,12,0.95)',
          backdropFilter:'blur(10px)',
        }}>
          <div style={{display:'flex', alignItems:'center', gap:8, padding:'0 12px'}}>
            <Crosshair color="var(--lb-block)" size={12}/>
            <span className="lb-mono" style={{fontSize:10, color:'var(--lb-ink)', letterSpacing:'0.10em', fontWeight:600}}>
              CLASSIC · DRAG TO MARK
            </span>
          </div>
          <Hairline vertical/>
          {/* fine adjust nudge controls */}
          <div style={{display:'flex', alignItems:'center', gap:6, padding:'0 12px'}}>
            <span className="lb-mono" style={{fontSize:10, color:'var(--lb-ink-muted)'}}>NUDGE</span>
            {['←','→','↑','↓'].map(k => <Kbd key={k}>{k}</Kbd>)}
          </div>
          <Hairline vertical/>
          {[
            { l: 'CONFIRM', kb: '↵', hot:true, icon: Icon.check(11) },
            { l: 'CANCEL', kb: 'ESC', icon: Icon.x(11) },
          ].map((b,i) => (
            <React.Fragment key={i}>
              <button style={{
                padding:'0 14px', background:'transparent', border:'none',
                color: b.hot ? 'var(--lb-block)' : 'var(--lb-ink-2)',
                fontFamily:'var(--lb-font-mono)', fontSize:11, fontWeight:600,
                letterSpacing:'0.06em', cursor:'pointer',
                display:'inline-flex', alignItems:'center', gap:6,
              }}>
                {b.icon}{b.l}<Kbd style={{borderColor: b.hot ? 'var(--lb-block)' : undefined}}>{b.kb}</Kbd>
              </button>
              {i === 0 && <Hairline vertical/>}
            </React.Fragment>
          ))}
        </div>
      </div>

      <div className="lb-mono" style={{
        position:'absolute', top:10, left:12, fontSize:9, color:'rgba(255,255,255,0.4)',
        letterSpacing:'0.16em',
      }}>v3·B · CLASSIC · MARQUEE 410×172</div>
    </div>
  );
}

// ── C) Idle state — the floating tool BEFORE you interact ─────
function ScreenFloatingIdle() {
  return (
    <div className="lb" style={{ width:540, height:380, position:'relative', overflow:'hidden', border:'1px solid var(--lb-rule)' }}>
      <FauxDesktop>
        <div style={{height:140, background:'linear-gradient(160deg,#fbbf24 0%,#ff7a3a 100%)', display:'flex', alignItems:'center', justifyContent:'center', color:'#3a1500', fontWeight:800, fontFamily:'system-ui'}}>SHOP NOW</div>
      </FauxDesktop>

      {/* idle: scanning crosshairs over candidates, no commit yet */}
      <div style={{position:'absolute', right:'8%', top:'12%', width:'34%', height:'38%', pointerEvents:'none'}}>
        <div style={{position:'absolute', inset:0, border:'1px dashed rgba(255,59,31,0.55)'}}/>
        {[[0,0],[100,0],[0,100],[100,100]].map(([px,py],i) => (
          <div key={i} style={{position:'absolute', left:`${px}%`, top:`${py}%`, width:6, height:6, marginLeft:-3, marginTop:-3, background:'var(--lb-block)'}}/>
        ))}
        <div className="lb-mono" style={{position:'absolute', top:-1, left:-1, transform:'translateY(-100%)', background:'transparent', color:'var(--lb-block)', border:'1px solid var(--lb-block)', padding:'1px 5px', fontSize:9, fontWeight:600}}>BANNER · 96% · HOVER TO LOCK</div>
      </div>

      <div style={{position:'absolute', left:'50%', bottom:14, transform:'translateX(-50%)'}}>
        <div className="lb lb-panel" style={{
          display:'flex', alignItems:'stretch', height:36, background:'rgba(10,10,12,0.95)',
          backdropFilter:'blur(10px)',
        }}>
          <div style={{display:'flex', alignItems:'center', gap:10, padding:'0 14px'}}>
            <Dot color="var(--lb-ml)" size={6} pulse/>
            <span className="lb-mono" style={{fontSize:10, color:'var(--lb-ink)', letterSpacing:'0.10em', fontWeight:600}}>SCANNING</span>
            <span className="lb-mono" style={{fontSize:10, color:'var(--lb-ink-muted)'}}>· <span style={{color:'var(--lb-ml)'}}>3</span> candidates · <span style={{color:'var(--lb-ml)'}}>8.4</span>ms</span>
          </div>
          <Hairline vertical/>
          <div style={{display:'flex', alignItems:'center', gap:6, padding:'0 14px'}}>
            <span className="lb-mono" style={{fontSize:10, color:'var(--lb-ink-muted)'}}>MODE</span>
            <Btn size="sm" variant="ghost-bright" active>SMART</Btn>
            <Btn size="sm" variant="ghost">CLASSIC</Btn>
          </div>
          <Hairline vertical/>
          <button style={{
            padding:'0 14px', background:'transparent', border:'none', color:'var(--lb-ink-2)',
            fontFamily:'var(--lb-font-mono)', fontSize:11, fontWeight:600, letterSpacing:'0.06em', cursor:'pointer',
            display:'inline-flex', alignItems:'center', gap:6,
          }}>{Icon.x(11)} CLOSE<Kbd>ESC</Kbd></button>
        </div>
      </div>

      <div className="lb-mono" style={{position:'absolute', top:10, left:12, fontSize:9, color:'rgba(255,255,255,0.4)', letterSpacing:'0.16em'}}>v3 · IDLE · HOVER ANY CANDIDATE</div>
    </div>
  );
}

// ── D) Summon hint — the BEFORE state, showing how to call the tool ──
// The pain point: "hotkey is awkward, forgets where it is".
// New solution: an always-visible kill-pixel chip on the screen edge.
// Click it OR press one global hotkey. Always reachable.
function ScreenSummonHint() {
  return (
    <div className="lb" style={{ width:540, height:380, position:'relative', overflow:'hidden', border:'1px solid var(--lb-rule)' }}>
      <FauxDesktop>
        <div style={{height:140, background:'linear-gradient(160deg,#fbbf24 0%,#ff7a3a 100%)', display:'flex', alignItems:'center', justifyContent:'center', color:'#3a1500', fontWeight:800, fontFamily:'system-ui'}}>SHOP NOW</div>
      </FauxDesktop>

      {/* screen-edge kill chip — top right, always there */}
      <div style={{position:'absolute', top:0, right:24}}>
        <div className="lb-mono" style={{
          padding:'4px 9px', background:'#0a0a0c', color:'var(--lb-block)',
          border:'1px solid var(--lb-block)', borderTop:'none',
          fontSize:10, fontWeight:600, letterSpacing:'0.10em',
          display:'inline-flex', alignItems:'center', gap:6,
        }}>
          <Dot color="var(--lb-block)" size={5} pulse/>
          KILL · 847
        </div>
      </div>

      {/* arrow + explanation pointing at the chip */}
      <div style={{position:'absolute', top:30, right:90, width:120, textAlign:'right'}}>
        <div className="lb-mono" style={{fontSize:10, color:'var(--lb-ink)', letterSpacing:'0.06em', textTransform:'uppercase'}}>
          <span style={{color:'var(--lb-block)'}}>↗</span> Always there.<br/>
          <span style={{color:'var(--lb-ink-muted)'}}>Click to mark, or press the chord.</span>
        </div>
      </div>

      {/* hotkey card — bottom centred */}
      <div style={{position:'absolute', left:'50%', bottom:24, transform:'translateX(-50%)', textAlign:'center'}}>
        <div className="lb-mono" style={{fontSize:10, color:'var(--lb-ink-faint)', letterSpacing:'0.18em', marginBottom:8}}>OR · ONE CHORD · ANY APP · ANY OS</div>
        <div style={{display:'inline-flex', alignItems:'center', gap:6}}>
          <Kbd style={{height:30, minWidth:34, fontSize:12, fontWeight:600}}>⌘ / Ctrl</Kbd>
          <span style={{color:'var(--lb-ink-faint)'}}>+</span>
          <Kbd style={{height:30, minWidth:30, fontSize:12, fontWeight:600}}>⇧</Kbd>
          <span style={{color:'var(--lb-ink-faint)'}}>+</span>
          <Kbd style={{height:30, minWidth:30, fontSize:14, fontWeight:700, color:'var(--lb-block)', borderColor:'var(--lb-block)'}}>K</Kbd>
        </div>
        <div className="lb-mono" style={{fontSize:9, color:'var(--lb-ink-muted)', letterSpacing:'0.10em', marginTop:8}}>
          K = KILL. ONE KEY TO REMEMBER.
        </div>
      </div>

      <div className="lb-mono" style={{position:'absolute', top:10, left:12, fontSize:9, color:'rgba(255,255,255,0.4)', letterSpacing:'0.16em'}}>v3 · SUMMON · ALWAYS-VISIBLE EDGE CHIP</div>
    </div>
  );
}

Object.assign(window, { ScreenFloatingSmart, ScreenFloatingClassic, ScreenFloatingIdle, ScreenSummonHint });
