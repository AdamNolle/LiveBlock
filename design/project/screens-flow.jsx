// LiveBlocker v4 — supporting screens.
// Onboarding · Floating marking tool · HUD · Cross-OS proof · In action

/* ────────────────────────────────────────────────────────────
   HUD — the tiny always-on status chip
   ──────────────────────────────────────────────────────────── */

function ScreenHUD() {
  return (
    <div className="lb" style={{
      width: 400, height: 160, position:'relative', overflow:'hidden',
      borderRadius:'var(--r-4)', background:'linear-gradient(135deg, #1c1530 0%, #321e3a 100%)',
    }}>
      <div className="scan" style={{ position:'absolute', inset: 0 }}/>
      <div style={{ position:'absolute', inset: 0, display:'flex', alignItems:'center', justifyContent:'center' }}>
        <div style={{
          display:'flex', alignItems:'stretch', height: 36,
          background:'rgba(15,16,22,0.92)', backdropFilter:'blur(20px)',
          border:'1px solid rgba(255,255,255,0.10)', borderRadius:'var(--r-pill)',
          padding:'0 4px', boxShadow:'0 10px 30px rgba(0,0,0,0.4)',
        }}>
          <div style={{ display:'flex', alignItems:'center', gap: 8, padding:'0 12px' }}>
            <Dot color="var(--accent)" size={7} pulse/>
            <span style={{ fontSize: 12, fontWeight: 600, color:'var(--ink-1)' }}>Blocking</span>
          </div>
          <div style={{ width: 1, background:'var(--line)', margin:'7px 0' }}/>
          <div style={{ display:'flex', alignItems:'center', gap: 8, padding:'0 12px', fontSize: 11, color:'var(--ink-3)' }}>
            <span><span className="mono tnum" style={{ color:'var(--ink-1)' }}>14</span> regions</span>
            <span style={{ color:'var(--ink-5)' }}>·</span>
            <span><span className="mono tnum" style={{ color:'var(--ml)' }}>2.1</span> ms</span>
            <span style={{ color:'var(--ink-5)' }}>·</span>
            <span><span className="mono tnum" style={{ color:'var(--accent)' }}>847</span> today</span>
          </div>
          <div style={{ width: 1, background:'var(--line)', margin:'7px 0' }}/>
          <button style={{
            display:'inline-flex', alignItems:'center', gap: 6, padding:'0 12px',
            background:'transparent', border:'none', color:'var(--ink-2)', fontSize: 12, fontWeight: 500,
          }}>{I.pause(13)} Pause</button>
        </div>
      </div>
      <div style={{
        position:'absolute', bottom: 12, left: 0, right: 0, textAlign:'center',
        fontSize: 11, color:'rgba(255,255,255,0.45)',
      }}>Drag anywhere · lives at the edge of your screen</div>
    </div>
  );
}

/* ────────────────────────────────────────────────────────────
   APP IDENTITY card — the wordmark + logo presentation
   ──────────────────────────────────────────────────────────── */

function ScreenAppIcon() {
  return (
    <div className="lb card" style={{
      width: 360, height: 360, padding: 24, position:'relative', overflow:'hidden',
      display:'flex', flexDirection:'column',
    }}>
      <div style={{
        position:'absolute', top: -80, right: -80, width: 280, height: 280,
        background:'radial-gradient(closest-side, var(--accent-soft), transparent 70%)',
      }}/>
      <div style={{ position:'relative', display:'flex', alignItems:'center', gap: 8 }}>
        <Pill tone="ghost" size="sm">v3.0.1</Pill>
        <div style={{ flex: 1 }}/>
        <Pill tone="success" size="sm" dot>Available</Pill>
      </div>
      <div style={{ flex: 1, display:'flex', flexDirection:'column', alignItems:'center', justifyContent:'center', gap: 22, position:'relative' }}>
        <Logo size={108}/>
        <div style={{ textAlign:'center' }}>
          <Wordmark size={22} showLogo={false}/>
          <div style={{ fontSize: 12, color:'var(--ink-3)', marginTop: 8, letterSpacing:'-0.005em' }}>
            On-device · macOS · Windows · Linux
          </div>
        </div>
      </div>
      <div style={{
        position:'relative', padding:'10px 0 0', borderTop:'1px solid var(--line)',
        display:'flex', alignItems:'center', gap: 10, fontSize: 12, color:'var(--ink-3)',
      }}>
        <span className="mono tnum">14.2 MB</span>
        <span style={{ color:'var(--ink-5)' }}>·</span>
        <span>0% telemetry</span>
        <div style={{ flex: 1 }}/>
        <Btn size="sm" variant="ghost" iconRight={I.external(12)}>Open site</Btn>
      </div>
    </div>
  );
}

/* ────────────────────────────────────────────────────────────
   ONBOARDING — 3 steps, kinder copy
   ──────────────────────────────────────────────────────────── */

function OnboardShell({ step, total = 3, children, primaryLabel, primaryIcon, secondaryLabel }) {
  return (
    <div className="lb card" style={{
      width: 460, height: 540, overflow:'hidden',
      display:'flex', flexDirection:'column',
      boxShadow:'0 24px 60px rgba(0,0,0,0.5), 0 0 0 1px var(--line)',
    }}>
      <div style={{
        height: 44, padding:'0 16px', borderBottom:'1px solid var(--line)',
        display:'flex', alignItems:'center', gap: 10, background:'var(--surface)',
      }}>
        <Logo size={20}/>
        <span style={{ fontSize: 13, fontWeight: 600, letterSpacing:'-0.015em' }}>LiveBlocker</span>
        <div style={{ flex: 1 }}/>
        <span className="mono" style={{ fontSize: 11, color:'var(--ink-4)' }}>
          Step {step} of {total}
        </span>
      </div>
      <div style={{ flex: 1, padding:'26px 28px', display:'flex', flexDirection:'column', minHeight: 0 }}>
        {children}
      </div>
      <div style={{
        padding:'14px 18px', borderTop:'1px solid var(--line)',
        display:'flex', alignItems:'center', gap: 10, background:'var(--surface)',
      }}>
        <div style={{ display:'flex', gap: 6 }}>
          {Array.from({ length: total }).map((_, i) => {
            const done = i + 1 < step, active = i + 1 === step;
            return <span key={i} style={{
              width: active ? 24 : 6, height: 6, borderRadius: 99,
              background: done ? 'var(--accent)' : active ? 'var(--accent)' : 'var(--surface-3)',
              transition:'width 0.18s ease',
            }}/>;
          })}
        </div>
        <div style={{ flex: 1 }}/>
        {secondaryLabel && <Btn variant="ghost">{secondaryLabel}</Btn>}
        <Btn variant="primary" icon={primaryIcon} iconRight={I.arrow(13)}>{primaryLabel}</Btn>
      </div>
    </div>
  );
}

function ScreenOnboarding1() {
  return (
    <OnboardShell step={1} primaryLabel="Get started">
      <div style={{ marginBottom: 22 }}>
        <Pill tone="success" size="sm" dot>Ready</Pill>
      </div>
      <Logo size={76}/>
      <h1 style={{
        margin:'24px 0 0', fontSize: 30, fontWeight: 700, lineHeight: 1.08,
        letterSpacing:'-0.025em', color:'var(--ink-1)',
      }}>
        Take the ads<br/>
        <span style={{ color:'var(--accent)' }}>out of your screen.</span>
      </h1>
      <p style={{
        fontSize: 14, color:'var(--ink-3)', marginTop: 16, lineHeight: 1.55,
        maxWidth: 380, letterSpacing:'-0.005em',
      }}>
        LiveBlocker reads what's on your display, finds the bits you didn't ask for,
        and paints over them — locally, on your machine, before they reach your eyes.
      </p>
      <div style={{ flex: 1 }}/>
      <div className="card-inset" style={{ padding: 14, display:'flex', alignItems:'center', gap: 12 }}>
        <div style={{
          width: 32, height: 32, borderRadius:'var(--r-2)',
          background:'var(--ml-soft)', color:'var(--ml)',
          display:'inline-flex', alignItems:'center', justifyContent:'center',
        }}>{I.shield(16)}</div>
        <div style={{ flex: 1 }}>
          <div style={{ fontSize: 13, fontWeight: 600 }}>Nothing leaves this device</div>
          <div style={{ fontSize: 12, color:'var(--ink-3)', marginTop: 2 }}>
            No pixel content, ever. Just frame timing if you opt in.
          </div>
        </div>
      </div>
    </OnboardShell>
  );
}

function ScreenOnboarding2() {
  const perms = [
    { l:'Screen recording', d:'So LiveBlocker can read each frame and decide what to fill.', granted:true,  required:true,  i: I.cpu(15) },
    { l:'Accessibility',    d:'Optional — lets blocks snap to real UI elements instead of pixels.', granted:false, required:false, i: I.shield(15) },
  ];
  return (
    <OnboardShell step={2} secondaryLabel="Back" primaryLabel="Continue">
      <div className="caption">Step 2 — permissions</div>
      <h2 style={{ margin:'8px 0 6px', fontSize: 24, fontWeight: 600, letterSpacing:'-0.02em' }}>
        Two quick grants.
      </h2>
      <p style={{ fontSize: 13, color:'var(--ink-3)', margin: 0, lineHeight: 1.5 }}>
        Both stay on this device. You can revoke either anytime from System Settings.
      </p>

      <div style={{ marginTop: 18, display:'flex', flexDirection:'column', gap: 10 }}>
        {perms.map((p) => (
          <div key={p.l} className="card-2" style={{
            padding: 14, display:'flex', alignItems:'center', gap: 14,
            borderLeft: `3px solid ${p.granted ? 'var(--success)' : 'var(--accent)'}`,
          }}>
            <div style={{
              width: 38, height: 38, borderRadius:'var(--r-3)',
              background: p.granted ? 'var(--success-soft)' : 'var(--surface-3)',
              color: p.granted ? 'var(--success)' : 'var(--ink-2)',
              display:'inline-flex', alignItems:'center', justifyContent:'center', flexShrink: 0,
            }}>{p.i}</div>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ display:'flex', alignItems:'center', gap: 8 }}>
                <span style={{ fontSize: 14, fontWeight: 600 }}>{p.l}</span>
                <Pill tone="ghost" size="sm">{p.required ? 'Required' : 'Optional'}</Pill>
              </div>
              <div style={{ fontSize: 12, color:'var(--ink-3)', marginTop: 3, lineHeight: 1.4 }}>{p.d}</div>
            </div>
            {p.granted
              ? <Pill tone="success" size="sm" icon={I.check(11)}>Granted</Pill>
              : <Btn size="sm" variant="outline">Allow</Btn>}
          </div>
        ))}
      </div>

      <div style={{ flex: 1 }}/>

      <div style={{
        padding: 12, borderRadius:'var(--r-3)', border:'1px dashed var(--line-2)',
        display:'flex', alignItems:'flex-start', gap: 10,
      }}>
        <span style={{ color:'var(--ml)', flexShrink: 0 }}>{I.lock(15)}</span>
        <div style={{ fontSize: 12, color:'var(--ink-3)', lineHeight: 1.5 }}>
          Frames are processed inside a sandboxed GPU pipeline and discarded the moment they've been composited.
          <span style={{ color:'var(--ink-2)' }}> Nothing is saved.</span>
        </div>
      </div>
    </OnboardShell>
  );
}

function ScreenOnboarding3() {
  return (
    <OnboardShell step={3} secondaryLabel="Back" primaryLabel="I'm in">
      <div className="caption">Step 3 — set & forget</div>
      <h2 style={{ margin:'8px 0 6px', fontSize: 24, fontWeight: 600, letterSpacing:'-0.02em' }}>
        You won't be marking rectangles.
      </h2>
      <p style={{ fontSize: 13, color:'var(--ink-3)', margin: 0, lineHeight: 1.5 }}>
        After a couple of corrections, the detector takes over. Most days you'll never open this app.
      </p>

      <div style={{
        flex: 1, marginTop: 14, position:'relative', overflow:'hidden',
        borderRadius:'var(--r-3)', border:'1px solid var(--line)',
        background:'linear-gradient(160deg, #14111e 0%, #221735 100%)',
      }}>
        <div className="scan" style={{ position:'absolute', inset: 0 }}/>
        {[14,32,22,38,26,30,18,28].map((w, i) => (
          <div key={i} style={{
            position:'absolute', left:'8%', top:`${10 + i*9}%`, width:`${w}%`, height: 3,
            background:'rgba(255,255,255,0.18)', borderRadius: 1,
          }}/>
        ))}
        {[
          { x:58, y:12, w:34, h:40, label:'Banner · 94%',  primary:true  },
          { x:14, y:62, w:26, h:26, label:'Pop-up · 71%',  primary:false },
        ].map((d, i) => (
          <div key={i} style={{
            position:'absolute', left:`${d.x}%`, top:`${d.y}%`, width:`${d.w}%`, height:`${d.h}%`,
            border:`1.5px ${d.primary ? 'solid' : 'dashed'} ${d.primary ? 'var(--accent)' : 'var(--warn)'}`,
            background: d.primary ? 'var(--accent-soft)' : 'transparent',
            borderRadius: 4,
          }}>
            <div style={{
              position:'absolute', top: -22, left: -1,
              background: d.primary ? 'var(--accent)' : 'var(--warn)', color:'#fff',
              padding:'2px 7px', borderRadius:'var(--r-1)',
              fontSize: 10, fontWeight: 700, letterSpacing:'-0.005em',
            }}>{d.label}</div>
          </div>
        ))}
        <div style={{ position:'absolute', left:'68%', top:'24%' }}>
          <Crosshair size={22} color="var(--accent)"/>
        </div>
      </div>

      <div style={{
        marginTop: 14, padding:'10px 12px',
        background:'var(--surface-2)', border:'1px solid var(--line)', borderRadius:'var(--r-3)',
        display:'flex', alignItems:'center', gap: 10, fontSize: 12, color:'var(--ink-3)',
      }}>
        <Kbd>⌘</Kbd><Kbd>⇧</Kbd><Kbd>K</Kbd>
        <span>Press anytime to mark something manually. You rarely will.</span>
      </div>
    </OnboardShell>
  );
}

/* ────────────────────────────────────────────────────────────
   FLOATING TOOL — Summon, Idle, Smart, Classic
   ──────────────────────────────────────────────────────────── */

function FauxDesktop({ children }) {
  return (
    <div style={{ position:'absolute', inset: 0, background:'#16121f', overflow:'hidden' }}>
      <div className="scan" style={{ position:'absolute', inset: 0 }}/>
      <div style={{
        position:'absolute', left: 24, top: 22, right: 24, bottom: 76,
        display:'grid', gridTemplateColumns:'1.4fr 1fr', gap: 18,
      }}>
        <div style={{ display:'flex', flexDirection:'column', gap: 8 }}>
          <div style={{ height: 16, width:'66%', background:'rgba(255,255,255,0.6)',  borderRadius: 2 }}/>
          {[94,88,72,90,80,94,82].map((w,i) => (
            <div key={i} style={{ height: 5, width:`${w}%`, background:'rgba(255,255,255,0.18)', borderRadius: 1 }}/>
          ))}
          <div style={{ height: 70, background:'rgba(255,255,255,0.12)', borderRadius: 4, marginTop: 8 }}/>
          {[88,76,92].map((w,i) => (
            <div key={i} style={{ height: 5, width:`${w}%`, background:'rgba(255,255,255,0.18)', borderRadius: 1, marginTop: 6 }}/>
          ))}
        </div>
        <div style={{ display:'flex', flexDirection:'column', gap: 10 }}>
          {children}
          <div style={{ height: 5, width:'70%', background:'rgba(255,255,255,0.18)', borderRadius: 1, marginTop: 8 }}/>
          <div style={{ height: 5, width:'90%', background:'rgba(255,255,255,0.18)', borderRadius: 1 }}/>
          <div style={{ height: 5, width:'60%', background:'rgba(255,255,255,0.18)', borderRadius: 1 }}/>
        </div>
      </div>
    </div>
  );
}

function FloatingFrame({ children, label }) {
  return (
    <div className="lb" style={{
      width: 540, height: 380, position:'relative', overflow:'hidden',
      borderRadius:'var(--r-4)', border:'1px solid var(--line)',
    }}>
      {children}
      <div style={{
        position:'absolute', top: 12, left: 14, fontSize: 11, fontWeight: 500,
        color:'rgba(255,255,255,0.45)', letterSpacing:'-0.005em',
      }}>{label}</div>
    </div>
  );
}

function FloatingToolbar({ children, label, dotColor = 'var(--accent)' }) {
  return (
    <div style={{ position:'absolute', left:'50%', bottom: 18, transform:'translateX(-50%)' }}>
      <div style={{
        display:'flex', alignItems:'stretch', height: 40,
        background:'rgba(14,15,21,0.95)', backdropFilter:'blur(20px)',
        border:'1px solid rgba(255,255,255,0.10)', borderRadius:'var(--r-4)',
        padding:'0 4px', boxShadow:'0 16px 40px rgba(0,0,0,0.55)',
      }}>
        <div style={{ display:'flex', alignItems:'center', gap: 8, padding:'0 12px' }}>
          <Dot color={dotColor} size={7} pulse/>
          <span style={{ fontSize: 12, fontWeight: 600, letterSpacing:'-0.005em', color:'var(--ink-1)' }}>{label}</span>
        </div>
        {children}
      </div>
    </div>
  );
}

function TbBtn({ icon, kbd, children, accent, last }) {
  return (
    <>
      <div style={{ width: 1, background:'var(--line)', margin:'7px 0' }}/>
      <button style={{
        display:'inline-flex', alignItems:'center', gap: 6, padding:'0 12px',
        background:'transparent', border:'none',
        color: accent ? 'var(--accent)' : 'var(--ink-2)',
        fontSize: 12, fontWeight: 600, letterSpacing:'-0.005em',
      }}>
        {icon}{children}
        {kbd && <Kbd size={10} style={accent ? { borderColor:'var(--accent)', color:'var(--accent)' } : {}}>{kbd}</Kbd>}
      </button>
    </>
  );
}

// (A) Summon — how you call the tool. Pinned edge chip + hotkey.
function ScreenSummonHint() {
  return (
    <FloatingFrame label="Summoning the marker">
      <FauxDesktop>
        <div style={{ height: 140, background:'linear-gradient(160deg, #fbbf24, #ff7a3a)',
          display:'flex', alignItems:'center', justifyContent:'center',
          color:'#3a1500', fontWeight: 800, fontSize: 22, borderRadius: 4 }}>SHOP NOW</div>
      </FauxDesktop>

      {/* Edge chip — top right */}
      <div style={{ position:'absolute', top: 0, right: 30 }}>
        <div style={{
          padding:'5px 11px', background:'rgba(14,15,21,0.95)', color:'var(--accent)',
          border:'1px solid var(--accent)', borderTop:'none',
          borderBottomLeftRadius:'var(--r-3)', borderBottomRightRadius:'var(--r-3)',
          fontSize: 11, fontWeight: 600, letterSpacing:'-0.005em',
          display:'inline-flex', alignItems:'center', gap: 7, boxShadow:'0 6px 16px rgba(0,0,0,0.4)',
        }}>
          <Dot color="var(--accent)" size={6} pulse/>
          <span className="mono tnum">847</span> blocked
        </div>
      </div>
      <div style={{ position:'absolute', top: 36, right: 130, width: 130, textAlign:'right',
        fontSize: 11, color:'var(--ink-2)', lineHeight: 1.5 }}>
        <span style={{ color:'var(--accent)' }}>↗</span> Always there.<br/>
        <span style={{ color:'var(--ink-4)' }}>Click to mark, or press the chord.</span>
      </div>

      {/* Hotkey card */}
      <div style={{ position:'absolute', left:'50%', bottom: 32, transform:'translateX(-50%)', textAlign:'center' }}>
        <div style={{ fontSize: 11, color:'var(--ink-4)', marginBottom: 10, letterSpacing:'-0.005em' }}>
          or press, in any app, any OS
        </div>
        <div style={{ display:'inline-flex', alignItems:'center', gap: 6 }}>
          <Kbd size={14} style={{ height: 32, minWidth: 38, fontSize: 13 }}>⌘ / Ctrl</Kbd>
          <span style={{ color:'var(--ink-4)' }}>+</span>
          <Kbd size={14} style={{ height: 32, minWidth: 32, fontSize: 13 }}>⇧</Kbd>
          <span style={{ color:'var(--ink-4)' }}>+</span>
          <Kbd size={16} style={{ height: 32, minWidth: 32, fontSize: 15, color:'var(--accent)', borderColor:'var(--accent)', background:'var(--accent-soft)', fontWeight: 700 }}>K</Kbd>
        </div>
      </div>
    </FloatingFrame>
  );
}

// (B) Idle — scanning. Three candidates, none locked yet.
function ScreenFloatingIdle() {
  return (
    <FloatingFrame label="Idle — hover any candidate">
      <FauxDesktop>
        <div style={{ height: 140, background:'linear-gradient(160deg, #fbbf24, #ff7a3a)',
          display:'flex', alignItems:'center', justifyContent:'center',
          color:'#3a1500', fontWeight: 800, fontSize: 22, borderRadius: 4 }}>SHOP NOW</div>
      </FauxDesktop>
      <div style={{ position:'absolute', right:'8%', top:'14%', width:'34%', height:'36%', pointerEvents:'none' }}>
        <div style={{ position:'absolute', inset: 0, border:'1.5px dashed var(--accent)', borderRadius: 4 }}/>
        <div style={{
          position:'absolute', top: -22, left: -1,
          background:'transparent', color:'var(--accent)',
          padding:'2px 7px', border:'1px solid var(--accent)', borderRadius:'var(--r-1)',
          fontSize: 10, fontWeight: 700, letterSpacing:'-0.005em',
        }}>Banner · 96% · hover to lock</div>
      </div>

      <FloatingToolbar label="Scanning" dotColor="var(--ml)">
        <div style={{ width: 1, background:'var(--line)', margin:'7px 0' }}/>
        <div style={{ padding:'0 12px', display:'flex', alignItems:'center', gap: 10, fontSize: 11, color:'var(--ink-3)' }}>
          <span><span className="mono tnum" style={{ color:'var(--ml)' }}>3</span> candidates</span>
          <span style={{ color:'var(--ink-5)' }}>·</span>
          <span><span className="mono tnum" style={{ color:'var(--ml)' }}>8.4</span> ms</span>
        </div>
        <div style={{ width: 1, background:'var(--line)', margin:'7px 0' }}/>
        <div style={{ display:'flex', alignItems:'center', gap: 6, padding:'4px 12px' }}>
          <span style={{ fontSize: 11, color:'var(--ink-4)' }}>Mode</span>
          <Segmented value="smart" onChange={()=>{}} size="sm"
            options={[{value:'smart',label:'Smart'}, {value:'classic',label:'Classic'}]}/>
        </div>
        <TbBtn icon={I.x(12)} kbd="esc">Close</TbBtn>
      </FloatingToolbar>
    </FloatingFrame>
  );
}

// (C) Smart — hover-to-snap; the ad is locked. Tap to block.
function ScreenFloatingSmart() {
  return (
    <FloatingFrame label="Smart — hover to snap">
      <FauxDesktop>
        <div style={{
          height: 140, background:'linear-gradient(160deg, #fbbf24, #ff7a3a)',
          display:'flex', flexDirection:'column', alignItems:'center', justifyContent:'center',
          color:'#3a1500', fontWeight: 800, borderRadius: 4,
        }}>
          <div style={{ fontSize: 22 }}>SHOP NOW</div>
          <div style={{ fontSize: 11, opacity: 0.75, marginTop: 2 }}>Limited time · 50% off</div>
        </div>
      </FauxDesktop>

      {/* dim overlay */}
      <div style={{ position:'absolute', inset: 0, background:'rgba(8,9,14,0.4)', pointerEvents:'none' }}/>

      {/* locked snap rect */}
      <div style={{
        position:'absolute', right: 26, top: 22, width:'34.5%', height: 140,
        border:'2px solid var(--accent)', background:'var(--accent-soft)',
        borderRadius: 4, pointerEvents:'none',
        boxShadow:'0 0 0 9999px rgba(8,9,14,0.55)',
      }}>
        {[[0,0],[100,0],[0,100],[100,100]].map(([px,py], i) => (
          <div key={i} style={{
            position:'absolute', left:`${px}%`, top:`${py}%`,
            width: 10, height: 10, marginLeft: -5, marginTop: -5,
            background:'var(--accent)', border:'2px solid #fff', borderRadius: 2,
          }}/>
        ))}
        <div style={{
          position:'absolute', top: -28, left: -2,
          background:'var(--accent)', color:'#fff',
          padding:'3px 9px', borderRadius:'var(--r-2)',
          fontSize: 11, fontWeight: 700, letterSpacing:'-0.005em',
          display:'inline-flex', alignItems:'center', gap: 8,
        }}>Banner · 96% <span style={{ opacity: 0.7, fontWeight: 500 }}>300×140</span></div>
        <div style={{
          position:'absolute', bottom: -32, right: -2,
          background:'rgba(14,15,21,0.95)', color:'var(--accent)',
          padding:'4px 9px', borderRadius:'var(--r-2)',
          border:'1px solid var(--accent)',
          fontSize: 11, fontWeight: 600, letterSpacing:'-0.005em',
          display:'inline-flex', alignItems:'center', gap: 6,
        }}>Click to block →</div>
      </div>

      <div style={{ position:'absolute', right:'16%', top:'36%', pointerEvents:'none' }}>
        <Crosshair size={26} color="var(--accent)"/>
      </div>

      <FloatingToolbar label="Smart">
        <TbBtn icon={I.block(13)} kbd="↵" accent>Block</TbBtn>
        <TbBtn icon={I.arrow(13)} kbd="↹">Skip</TbBtn>
        <TbBtn icon={I.x(13)}     kbd="esc">Done</TbBtn>
      </FloatingToolbar>
    </FloatingFrame>
  );
}

// (D) Classic — drag-to-mark
function ScreenFloatingClassic() {
  return (
    <FloatingFrame label="Classic — drag to mark">
      <FauxDesktop>
        <div style={{
          height: 140, background:'linear-gradient(160deg, #fbbf24, #ff7a3a)',
          display:'flex', alignItems:'center', justifyContent:'center',
          color:'#3a1500', fontWeight: 800, fontSize: 22, borderRadius: 4,
        }}>SHOP NOW</div>
      </FauxDesktop>

      <div style={{
        position:'absolute', left:'10%', top: 48, width:'82%', height: 180,
        border:'2px solid var(--accent)', background:'var(--accent-soft)',
        borderRadius: 4,
        boxShadow:'0 0 0 9999px rgba(8,9,14,0.55)',
      }}>
        {/* snap rails */}
        <div style={{ position:'absolute', left: -2000, right: -2000, top: 0,    height: 1, background:'var(--accent)', opacity: 0.45 }}/>
        <div style={{ position:'absolute', left: -2000, right: -2000, bottom: 0, height: 1, background:'var(--accent)', opacity: 0.45 }}/>
        <div style={{ position:'absolute', top: -2000, bottom: -2000, left: 0,   width: 1,  background:'var(--accent)', opacity: 0.45 }}/>
        <div style={{ position:'absolute', top: -2000, bottom: -2000, right: 0,  width: 1,  background:'var(--accent)', opacity: 0.45 }}/>
        {[[0,0],[100,0],[0,100],[100,100]].map(([px,py], i) => (
          <div key={i} style={{
            position:'absolute', left:`${px}%`, top:`${py}%`,
            width: 10, height: 10, marginLeft: -5, marginTop: -5,
            background:'#fff', border:'2px solid var(--accent)', borderRadius: 2,
          }}/>
        ))}
        <div style={{
          position:'absolute', top: -1, left:'50%', transform:'translate(-50%, -100%)',
          background:'var(--accent)', color:'#fff',
          padding:'3px 8px', borderRadius:'var(--r-2)',
          fontSize: 11, fontWeight: 700, letterSpacing:'-0.005em',
        }}><span className="mono tnum">410</span> × <span className="mono tnum">172</span></div>
        <div style={{
          position:'absolute', bottom: -28, right: -2,
          background:'rgba(14,15,21,0.95)', color:'var(--accent)',
          padding:'3px 9px', borderRadius:'var(--r-2)',
          border:'1px solid var(--accent)',
          fontSize: 11, fontWeight: 600,
        }}>Snapped · Banner · 96%</div>
      </div>

      <FloatingToolbar label="Classic">
        <div style={{ width: 1, background:'var(--line)', margin:'7px 0' }}/>
        <div style={{ padding:'0 12px', display:'flex', alignItems:'center', gap: 6 }}>
          <span style={{ fontSize: 11, color:'var(--ink-4)' }}>Nudge</span>
          <Kbd size={10}>←</Kbd><Kbd size={10}>→</Kbd><Kbd size={10}>↑</Kbd><Kbd size={10}>↓</Kbd>
        </div>
        <TbBtn icon={I.check(13)} kbd="↵" accent>Confirm</TbBtn>
        <TbBtn icon={I.x(13)}     kbd="esc">Cancel</TbBtn>
      </FloatingToolbar>
    </FloatingFrame>
  );
}

/* ────────────────────────────────────────────────────────────
   IN ACTION — before / after
   ──────────────────────────────────────────────────────────── */

function ScreenInAction({ mode = 'after' }) {
  return (
    <div className="lb" style={{
      width: 720, height: 460, position:'relative', overflow:'hidden',
      borderRadius:'var(--r-4)', border:'1px solid var(--line)',
      background:'linear-gradient(160deg, #14111e 0%, #2a1840 100%)',
    }}>
      <div className="scan" style={{ position:'absolute', inset: 0 }}/>

      {/* faux browser */}
      <div style={{
        position:'absolute', left: 28, top: 26, right: 28, bottom: 70,
        background:'#fafafa', borderRadius:'var(--r-3)', overflow:'hidden',
        boxShadow:'0 16px 40px rgba(0,0,0,0.4)',
        display:'flex', flexDirection:'column',
      }}>
        {/* address bar */}
        <div style={{
          height: 32, background:'#ececec', display:'flex', alignItems:'center', gap: 8, padding:'0 12px',
          borderBottom:'1px solid #ddd',
        }}>
          <div style={{ display:'flex', gap: 5 }}>
            {['#ff5f57','#febc2e','#28c840'].map((c,i) => <span key={i} style={{ width: 9, height: 9, borderRadius:'50%', background: c }}/>)}
          </div>
          <div style={{ flex: 1, height: 18, background:'#fff', borderRadius: 99, padding:'0 10px',
            display:'flex', alignItems:'center', fontSize: 10, color:'#888' }}>
            news.example.com / article-04
          </div>
        </div>

        {/* content */}
        <div style={{ flex: 1, padding: 20, display:'grid', gridTemplateColumns:'1.6fr 1fr', gap: 20 }}>
          <div>
            <div style={{ fontSize: 19, fontWeight: 700, color:'#0e0e14', letterSpacing:'-0.015em' }}>Top stories today</div>
            <div style={{ fontSize: 10, color:'#888', marginTop: 4, fontWeight: 500, textTransform:'uppercase', letterSpacing:'0.06em' }}>News · 6 min read</div>
            <div style={{ marginTop: 14, display:'flex', flexDirection:'column', gap: 6 }}>
              {[92,88,76,90,84,72,90,80,86,76,90].map((w,i) => (
                <div key={i} style={{ height: 5, width:`${w}%`, background:'#d8d8d8', borderRadius: 1 }}/>
              ))}
            </div>
            <div style={{ height: 80, background:'#dcdcdc', borderRadius: 4, marginTop: 12 }}/>
            <div style={{ marginTop: 10, display:'flex', flexDirection:'column', gap: 6 }}>
              {[80,86,72].map((w,i) => <div key={i} style={{ height: 5, width:`${w}%`, background:'#d8d8d8', borderRadius: 1 }}/>)}
            </div>
          </div>
          <div>
            {/* ad slot */}
            <div style={{
              position:'relative', height: 240,
              borderRadius: 4, overflow:'hidden',
              background: mode === 'before' ? 'linear-gradient(160deg, #fbbf24 0%, #ff3b1f 100%)' : '#f0f0f4',
              border: mode === 'after' ? '1px dashed rgba(255,80,57,0.45)' : '1px solid #e6e6ea',
            }}>
              {mode === 'before' && (
                <div style={{
                  position:'absolute', inset: 0, display:'flex', flexDirection:'column',
                  alignItems:'center', justifyContent:'center', color:'#3a1500',
                  fontFamily:'system-ui', fontWeight: 800,
                }}>
                  <div style={{ fontSize: 28, letterSpacing:'-0.02em' }}>BUY NOW</div>
                  <div style={{ fontSize: 11, opacity: 0.85, marginTop: 4 }}>Limited offer · Click here</div>
                  <div style={{ marginTop: 14, padding:'6px 14px', background:'#0a0a0c', color:'#fff',
                    fontSize: 11, fontWeight: 700, letterSpacing:'0.04em', borderRadius: 3 }}>SHOP →</div>
                </div>
              )}
              {mode === 'after' && (
                <div style={{ position:'absolute', inset: 0, display:'flex', flexDirection:'column',
                  alignItems:'center', justifyContent:'center', gap: 8, color:'#7a7e8b' }}>
                  <div style={{
                    width: 28, height: 28, borderRadius:'50%', background:'rgba(255,80,57,0.08)',
                    display:'flex', alignItems:'center', justifyContent:'center', color:'var(--accent)',
                  }}>{I.block(14)}</div>
                  <span style={{ fontSize: 11, fontWeight: 600, color:'#9a9eaa' }}>Region blocked</span>
                </div>
              )}
              {mode === 'after' && [[0,0],[100,0],[0,100],[100,100]].map(([px,py], i) => (
                <div key={i} style={{
                  position:'absolute', left:`${px}%`, top:`${py}%`,
                  width: 6, height: 6, marginLeft: -3, marginTop: -3, background:'var(--accent)', borderRadius: 1,
                }}/>
              ))}
            </div>
            <div style={{ marginTop: 14, fontSize: 10, color:'#888', fontWeight: 600,
              textTransform:'uppercase', letterSpacing:'0.06em', marginBottom: 6 }}>Related</div>
            <div style={{ display:'flex', flexDirection:'column', gap: 6 }}>
              {[80,68,88].map((w,i) => <div key={i} style={{ height: 5, width:`${w}%`, background:'#d8d8d8', borderRadius: 1 }}/>)}
            </div>
          </div>
        </div>
      </div>

      {/* HUD chip */}
      <div style={{ position:'absolute', left:'50%', bottom: 22, transform:'translateX(-50%)' }}>
        <div style={{
          display:'flex', alignItems:'stretch', height: 32,
          background:'rgba(14,15,21,0.95)', backdropFilter:'blur(20px)',
          border:'1px solid rgba(255,255,255,0.10)', borderRadius:'var(--r-pill)',
          padding:'0 4px',
        }}>
          <div style={{ display:'flex', alignItems:'center', gap: 7, padding:'0 11px' }}>
            <Dot color={mode === 'after' ? 'var(--accent)' : 'var(--ink-4)'} size={6} pulse={mode === 'after'}/>
            <span style={{ fontSize: 11, fontWeight: 600, color: mode === 'after' ? 'var(--ink-1)' : 'var(--ink-3)' }}>
              {mode === 'after' ? 'Blocking' : 'Paused'}
            </span>
          </div>
          <div style={{ width: 1, background:'var(--line)', margin:'7px 0' }}/>
          <div style={{ padding:'0 11px', display:'flex', alignItems:'center', gap: 8, fontSize: 11, color:'var(--ink-3)' }}>
            <span><span className="mono tnum" style={{ color:'var(--ml)' }}>2.1</span> ms</span>
            <span style={{ color:'var(--ink-5)' }}>·</span>
            <span><span className="mono tnum" style={{ color:'var(--accent)' }}>3</span> fills</span>
          </div>
        </div>
      </div>

      {/* label */}
      <div style={{
        position:'absolute', top: 12, left:'50%', transform:'translateX(-50%)',
        padding:'4px 10px', borderRadius:'var(--r-pill)',
        background: mode === 'after' ? 'var(--success-soft)' : 'var(--warn-soft)',
        color: mode === 'after' ? 'var(--success)' : 'var(--warn)',
        fontSize: 11, fontWeight: 700, letterSpacing:'-0.005em',
        border:`1px solid ${mode === 'after' ? 'rgba(74,222,128,0.32)' : 'rgba(251,191,36,0.32)'}`,
      }}>
        {mode === 'after' ? 'After · LiveBlocker engaged' : 'Before · without LiveBlocker'}
      </div>
    </div>
  );
}
const ScreenInActionBefore = () => <ScreenInAction mode="before"/>;
const ScreenInActionAfter  = () => <ScreenInAction mode="after"/>;

/* ────────────────────────────────────────────────────────────
   CROSS-OS — same app, three operating systems
   ──────────────────────────────────────────────────────────── */

function OSPanel({ os }) {
  const wall = os === 'mac'
    ? 'linear-gradient(160deg, #1c2952 0%, #3a2a78 50%, #7a3d8e 100%)'
    : os === 'win'
    ? 'linear-gradient(160deg, #0b3d6b 0%, #1e6fbf 50%, #3a8ad6 100%)'
    : 'linear-gradient(160deg, #2a1e0e 0%, #5e3a1a 50%, #8a5e2e 100%)';
  const label  = os === 'mac' ? 'macOS' : os === 'win' ? 'Windows' : 'Linux';
  const tech   = os === 'mac' ? 'ScreenCaptureKit' : os === 'win' ? 'Graphics.Capture' : 'PipeWire';
  const hot    = os === 'mac' ? '⌘⇧K' : 'Ctrl+⇧+K';
  return (
    <div className="lb" style={{
      flex: 1, borderRadius:'var(--r-3)', overflow:'hidden',
      border:'1px solid var(--line)', display:'flex', flexDirection:'column',
    }}>
      <div style={{
        padding:'8px 12px', borderBottom:'1px solid var(--line)',
        display:'flex', alignItems:'center', gap: 8, background:'var(--surface)',
      }}>
        <Dot color="var(--accent)" size={6}/>
        <span style={{ fontSize: 12, fontWeight: 600, letterSpacing:'-0.005em' }}>{label}</span>
        <div style={{ flex: 1 }}/>
        <span style={{ fontSize: 11, color:'var(--ink-4)' }}>identical UI</span>
      </div>
      <div style={{ position:'relative', height: 174, overflow:'hidden' }}>
        <div style={{ position:'absolute', inset: 0, background: wall }}/>
        <div className="scan" style={{ position:'absolute', inset: 0 }}/>
        {/* edge chip */}
        <div style={{ position:'absolute', top: 0, right: 16 }}>
          <div style={{
            padding:'4px 9px', background:'rgba(14,15,21,0.95)', color:'var(--accent)',
            border:'1px solid var(--accent)', borderTop:'none',
            borderBottomLeftRadius:'var(--r-2)', borderBottomRightRadius:'var(--r-2)',
            fontSize: 10, fontWeight: 600,
            display:'inline-flex', alignItems:'center', gap: 5,
          }}>
            <Dot color="var(--accent)" size={5} pulse/>
            <span className="mono tnum">847</span>
          </div>
        </div>
        {/* floating tool */}
        <div style={{ position:'absolute', left:'50%', bottom: 14, transform:'translateX(-50%)' }}>
          <div style={{
            display:'flex', alignItems:'stretch', height: 28,
            background:'rgba(14,15,21,0.95)', backdropFilter:'blur(20px)',
            border:'1px solid rgba(255,255,255,0.10)', borderRadius:'var(--r-pill)',
            padding:'0 3px',
          }}>
            <div style={{ display:'flex', alignItems:'center', gap: 6, padding:'0 9px' }}>
              <Dot color="var(--accent)" size={5} pulse/>
              <span style={{ fontSize: 10, fontWeight: 600, color:'var(--ink-1)' }}>Smart</span>
            </div>
            <div style={{ width: 1, background:'var(--line)', margin:'6px 0' }}/>
            <div style={{ padding:'0 9px', display:'flex', alignItems:'center', gap: 5, fontSize: 10, color:'var(--ink-3)' }}>
              <span className="mono tnum" style={{ color:'var(--ml)' }}>3</span> candidates
            </div>
            <div style={{ width: 1, background:'var(--line)', margin:'6px 0' }}/>
            <div style={{ padding:'0 9px', display:'flex', alignItems:'center', gap: 4, color:'var(--accent)' }}>
              <span style={{ fontSize: 10, fontWeight: 700 }}>Block</span>
              <Kbd size={9} style={{ borderColor:'var(--accent)', color:'var(--accent)' }}>↵</Kbd>
            </div>
          </div>
        </div>
      </div>
      <div style={{ padding:'8px 12px', borderTop:'1px solid var(--line)', fontSize: 11,
        color:'var(--ink-3)', display:'flex', alignItems:'center', gap: 8 }}>
        <span>Hotkey</span><Kbd size={10}>{hot}</Kbd>
        <div style={{ flex: 1 }}/>
        <span className="mono" style={{ fontSize: 10, color:'var(--ink-4)' }}>{tech}</span>
      </div>
    </div>
  );
}

function ScreenCrossOS() {
  return (
    <div className="lb card" style={{
      width: 720, height: 340, padding: 20, display:'flex', flexDirection:'column', gap: 14,
    }}>
      <div>
        <div className="caption">Cross-platform · same app · native feel</div>
        <h3 style={{ margin:'6px 0 0', fontSize: 18, fontWeight: 600, letterSpacing:'-0.02em' }}>
          One design, three operating systems.
        </h3>
      </div>
      <div style={{ display:'flex', gap: 12, flex: 1, minHeight: 0 }}>
        <OSPanel os="mac"/>
        <OSPanel os="win"/>
        <OSPanel os="linux"/>
      </div>
    </div>
  );
}

Object.assign(window, {
  ScreenHUD, ScreenAppIcon,
  ScreenOnboarding1, ScreenOnboarding2, ScreenOnboarding3,
  ScreenSummonHint, ScreenFloatingIdle, ScreenFloatingSmart, ScreenFloatingClassic,
  ScreenInActionBefore, ScreenInActionAfter, ScreenCrossOS,
});
