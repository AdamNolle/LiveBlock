// LiveBlocker — brand + onboarding + HUD screens (brutalist terminal).

// ── 1. APP ICON CARD ──────────────────────────────────────────
function ScreenAppIcon() {
  return (
    <div className="lb lb-panel" style={{
      width: 360, height: 360, background:'var(--lb-bg)',
      display:'flex', flexDirection:'column', position:'relative',
    }}>
      <div className="lb-scan" style={{position:'absolute', inset:0, pointerEvents:'none', opacity:0.6}}/>
      <div style={{padding:'10px 12px', display:'flex', alignItems:'center', gap:8, borderBottom:'1px solid var(--lb-rule)'}}>
        <span className="lb-label">APP.IDENTITY</span>
        <div style={{flex:1}}/>
        <span className="lb-mono" style={{fontSize:10, color:'var(--lb-ink-faint)'}}>v3.0.1</span>
      </div>
      <div style={{flex:1, display:'flex', flexDirection:'column', alignItems:'center', justifyContent:'center', gap:22, position:'relative'}}>
        <Logo size={148}/>
        <div style={{display:'flex', flexDirection:'column', alignItems:'center', gap:6}}>
          <Wordmark size={18} withLogo={false}/>
          <span className="lb-mono" style={{fontSize:10, color:'var(--lb-ink-muted)', letterSpacing:'0.18em'}}>
            ON-DEVICE · CROSS-PLATFORM
          </span>
        </div>
      </div>
      <div style={{padding:'8px 12px', display:'flex', alignItems:'center', gap:10, borderTop:'1px solid var(--lb-rule)', fontSize:10, color:'var(--lb-ink-faint)'}}>
        <Dot color="var(--lb-ok)" size={6}/>
        <span>MACOS · WINDOWS · LINUX</span>
        <div style={{flex:1}}/>
        <span>14.2 MB</span>
      </div>
    </div>
  );
}

// ── 2. HUD — corner status pill ───────────────────────────────
function ScreenHUD() {
  return (
    <div className="lb" style={{
      width: 380, height: 140, position:'relative',
      display:'flex', alignItems:'center', justifyContent:'center', overflow:'hidden',
      border:'1px solid var(--lb-rule)',
    }}>
      <div style={{position:'absolute', inset:0, background:'linear-gradient(135deg,#1c1530 0%, #321e3a 100%)'}}/>
      <div className="lb-scan" style={{position:'absolute', inset:0, opacity:0.5}}/>
      <div className="lb lb-panel" style={{
        display:'flex', alignItems:'stretch', height:34, position:'relative',
        background:'rgba(10,10,12,0.92)', backdropFilter:'blur(8px)',
      }}>
        <div style={{display:'flex', alignItems:'center', gap:8, padding:'0 12px'}}>
          <Dot color="var(--lb-block)" size={7} pulse/>
          <span className="lb-mono" style={{fontSize:11, color:'var(--lb-ink)', letterSpacing:'0.08em', fontWeight:600}}>
            BLOCKING
          </span>
        </div>
        <Hairline vertical/>
        <div style={{display:'flex', alignItems:'center', gap:10, padding:'0 12px', fontSize:10, color:'var(--lb-ink-muted)'}}>
          <span><span style={{color:'var(--lb-ink-2)'}}>14</span> regions</span>
          <span style={{color:'var(--lb-ink-dim)'}}>│</span>
          <span><span style={{color:'var(--lb-ml)'}}>2.1</span>ms</span>
          <span style={{color:'var(--lb-ink-dim)'}}>│</span>
          <span><span style={{color:'var(--lb-ok)'}}>847</span> today</span>
        </div>
        <Hairline vertical/>
        <button style={{
          padding:'0 12px', background:'transparent', border:'none', color:'var(--lb-ink-2)',
          fontFamily:'var(--lb-font-mono)', fontSize:11, letterSpacing:'0.06em', cursor:'pointer',
          display:'inline-flex', alignItems:'center', gap:6,
        }}>{Icon.pause(11)} PAUSE</button>
      </div>
      <div style={{position:'absolute', bottom:8, left:0, right:0, textAlign:'center'}}>
        <span className="lb-mono" style={{fontSize:9, color:'rgba(255,255,255,0.4)', letterSpacing:'0.16em'}}>
          ↑ DRAGGABLE · LIVES AT YOUR SCREEN'S EDGE
        </span>
      </div>
    </div>
  );
}

// ── 3. ONBOARDING ─────────────────────────────────────────────
function OnboardingShell({ step, total=3, children, ctaLabel, ctaIcon, back }) {
  return (
    <WinFrame title="LIVEBLOCKER" subtitle={`SETUP ${String(step).padStart(2,'0')}/${String(total).padStart(2,'0')}`} width={460} height={540}>
      <div style={{flex:1, padding:'24px 26px 0', display:'flex', flexDirection:'column', minHeight:0, position:'relative'}}>
        {children}
      </div>
      <div style={{
        padding:'12px 18px', borderTop:'1px solid var(--lb-rule)',
        display:'flex', alignItems:'center', gap:10, background:'var(--lb-panel)',
      }}>
        <div style={{display:'flex', gap:5}}>
          {Array.from({length:total}).map((_,i)=> {
            const done = i+1 < step, active = i+1 === step;
            return <span key={i} style={{
              width: active ? 28 : 12, height: 3,
              background: done || active ? 'var(--lb-block)' : 'var(--lb-ink-dim)',
            }}/>;
          })}
        </div>
        <span className="lb-mono" style={{fontSize:10, color:'var(--lb-ink-muted)', letterSpacing:'0.08em'}}>
          {String(step).padStart(2,'0')}/{String(total).padStart(2,'0')}
        </span>
        <div style={{flex:1}}/>
        {back && <Btn size="sm" variant="ghost">{back}</Btn>}
        <Btn size="md" variant="hot" icon={ctaIcon}>{ctaLabel}</Btn>
      </div>
    </WinFrame>
  );
}

function ScreenOnboarding1() {
  return (
    <OnboardingShell step={1} ctaIcon={Icon.arrow(11)} ctaLabel="BEGIN">
      <div className="lb-scan" style={{position:'absolute', inset:0, opacity:0.4, pointerEvents:'none'}}/>
      <div style={{position:'relative', display:'flex', flexDirection:'column', height:'100%', gap:18}}>
        <div style={{display:'flex', alignItems:'center', gap:10}}>
          <Dot color="var(--lb-ok)" size={6} pulse/>
          <span className="lb-mono" style={{fontSize:10, color:'var(--lb-ok)', letterSpacing:'0.16em'}}>READY</span>
          <Hairline color="var(--lb-rule)" style={{flex:1}}/>
          <span className="lb-mono" style={{fontSize:10, color:'var(--lb-ink-faint)', letterSpacing:'0.12em'}}>WELCOME</span>
        </div>

        <Logo size={56}/>

        <div>
          <div style={{
            fontFamily:'var(--lb-font-mono)', fontSize:28, fontWeight:600,
            color:'var(--lb-ink)', lineHeight:1.05, letterSpacing:'-0.02em',
          }}>
            BLOCK WHAT<br/>YOUR SCREEN<br/>
            <span style={{color:'var(--lb-block)'}}>SHOULDN'T</span> SHOW.
          </div>
          <div style={{fontSize:12, color:'var(--lb-ink-muted)', marginTop:14, lineHeight:1.55, maxWidth:360}}>
            Reads your display frames on-device. Replaces marked regions with
            edge-extrapolated fill before they hit your eyes.
          </div>
        </div>

        <div style={{flex:1}}/>

        <div className="lb-panel-inset" style={{
          padding:'8px 10px', fontFamily:'var(--lb-font-mono)', fontSize:11,
          color:'var(--lb-ink-2)', marginBottom:14,
        }}>
          <span style={{color:'var(--lb-ok)'}}>$</span> liveblocker --init
          <span className="lb-caret" style={{color:'var(--lb-block)', marginLeft:6}}>▌</span>
        </div>
      </div>
    </OnboardingShell>
  );
}

function ScreenOnboarding2() {
  const perms = [
    { l:'SCREEN CAPTURE', d:'Required to read display frames', i: Icon.cpu(14), granted:true, scope:'macOS · Win · Linux' },
    { l:'ACCESSIBILITY', d:'Optional — lets blocks snap to UI elements', i: Icon.shield(14), granted:false, scope:'Optional' },
  ];
  return (
    <OnboardingShell step={2} back="BACK" ctaIcon={Icon.arrow(11)} ctaLabel="CONTINUE">
      <div style={{display:'flex', flexDirection:'column', gap:14, height:'100%'}}>
        <div>
          <span className="lb-label">SECTION 02 — PERMISSIONS</span>
          <div style={{
            fontFamily:'var(--lb-font-mono)', fontSize:22, fontWeight:600, marginTop:8,
            color:'var(--lb-ink)', letterSpacing:'-0.01em',
          }}>Two quick grants.</div>
          <div style={{fontSize:12, color:'var(--lb-ink-muted)', marginTop:4}}>
            Both stay on this device. Nothing leaves.
          </div>
        </div>

        <div style={{display:'flex', flexDirection:'column', gap:8}}>
          {perms.map((p,i)=> (
            <div key={i} className="lb-panel" style={{
              padding:'12px 14px', display:'flex', alignItems:'center', gap:14,
              borderLeft: `2px solid ${p.granted ? 'var(--lb-ok)' : 'var(--lb-block)'}`,
            }}>
              <div style={{
                width:32, height:32, border:'1px solid var(--lb-rule-2)',
                display:'flex', alignItems:'center', justifyContent:'center',
                color: p.granted ? 'var(--lb-ok)' : 'var(--lb-ink-2)',
                background: p.granted ? 'rgba(74,222,128,0.06)' : 'transparent',
              }}>{p.i}</div>
              <div style={{flex:1, minWidth:0}}>
                <div style={{display:'flex', alignItems:'baseline', gap:8, flexWrap:'wrap'}}>
                  <span style={{fontFamily:'var(--lb-font-mono)', fontSize:12, fontWeight:600, color:'var(--lb-ink)', letterSpacing:'0.04em'}}>{p.l}</span>
                  <span className="lb-mono" style={{fontSize:9, color:'var(--lb-ink-faint)', letterSpacing:'0.08em'}}>[{p.scope}]</span>
                </div>
                <div style={{fontSize:11, color:'var(--lb-ink-muted)', marginTop:3}}>{p.d}</div>
              </div>
              {p.granted
                ? <span className="lb-mono" style={{fontSize:10, fontWeight:600, color:'var(--lb-ok)', letterSpacing:'0.10em', display:'inline-flex', alignItems:'center', gap:5}}>
                    {Icon.check(11)} GRANTED
                  </span>
                : <Btn size="sm" variant="ghost">ALLOW</Btn>}
            </div>
          ))}
        </div>

        <div style={{flex:1}}/>

        <div className="lb" style={{
          padding:'10px 12px', border:'1px dashed var(--lb-rule-2)',
          display:'flex', gap:10, alignItems:'flex-start', marginBottom:14,
        }}>
          <span style={{color:'var(--lb-ml)', flexShrink:0, marginTop:1}}>{Icon.shield(14)}</span>
          <div style={{fontSize:10, color:'var(--lb-ink-muted)', lineHeight:1.55}}>
            Frames are processed in a sandboxed GPU pipeline and discarded after compositing.
            <span style={{color:'var(--lb-ink-2)'}}> No screenshots, no telemetry of pixel content. Ever.</span>
          </div>
        </div>
      </div>
    </OnboardingShell>
  );
}

function ScreenOnboarding3() {
  return (
    <OnboardingShell step={3} back="BACK" ctaIcon={Icon.check(11)} ctaLabel="I'M IN">
      <div style={{display:'flex', flexDirection:'column', gap:14, height:'100%'}}>
        <div>
          <span className="lb-label">SECTION 03 — SET &amp; FORGET</span>
          <div style={{
            fontFamily:'var(--lb-font-mono)', fontSize:22, fontWeight:600, marginTop:8,
            color:'var(--lb-ink)', letterSpacing:'-0.01em',
          }}>You won't drag rectangles.</div>
          <div style={{fontSize:12, color:'var(--lb-ink-muted)', marginTop:4}}>
            Auto-detection finds ads, popups and chat widgets. Confirm with one click — or never look at it again.
          </div>
        </div>

        <div className="lb-panel-inset" style={{position:'relative', flex:1, overflow:'hidden'}}>
          <div style={{position:'absolute', inset:0, background:'linear-gradient(160deg,#16111e 0%,#241734 100%)'}}/>
          <div className="lb-scan" style={{position:'absolute', inset:0, opacity:0.4}}/>
          {[18,34,26,40,22,28].map((w, i) => (
            <div key={i} style={{
              position:'absolute', left:14, top: 16 + i*14, width:`${w}%`, height:4,
              background:'rgba(255,255,255,0.16)',
            }}/>
          ))}
          {[
            { x:58, y:12, w:34, h:42, label:'BANNER · 94%', primary:true },
            { x:14, y:62, w:26, h:24, label:'POPUP · 71%', primary:false },
          ].map((d,i) => (
            <div key={i} style={{
              position:'absolute', left:`${d.x}%`, top:`${d.y}%`, width:`${d.w}%`, height:`${d.h}%`,
              border:`1px ${d.primary ? 'solid' : 'dashed'} ${d.primary ? 'var(--lb-block)' : 'var(--lb-warn)'}`,
              background: d.primary ? 'rgba(255,59,31,0.10)' : 'transparent',
            }}>
              <div className="lb-mono" style={{
                position:'absolute', top:-15, left:-1,
                background: d.primary ? 'var(--lb-block)' : 'var(--lb-warn)',
                color:'#0a0a0c', padding:'1px 5px', fontSize:9, fontWeight:700, letterSpacing:'0.06em',
              }}>{d.label}</div>
            </div>
          ))}
          {/* cursor hovering the banner */}
          <div style={{position:'absolute', left:'72%', top:'30%', color:'var(--lb-block)'}}>
            <Crosshair color="var(--lb-block)" size={20}/>
          </div>
        </div>

        <div style={{
          display:'flex', alignItems:'center', gap:8, padding:'8px 10px',
          border:'1px solid var(--lb-rule)', background:'var(--lb-panel-2)',
          marginBottom: 14,
        }}>
          <Kbd>⌥</Kbd>
          <span className="lb-mono" style={{fontSize:10, color:'var(--lb-ink-muted)', letterSpacing:'0.06em'}}>
            HOLD TO MARK MANUALLY ANYTIME — RARELY NEEDED
          </span>
        </div>
      </div>
    </OnboardingShell>
  );
}

Object.assign(window, { ScreenAppIcon, ScreenHUD, ScreenOnboarding1, ScreenOnboarding2, ScreenOnboarding3 });
