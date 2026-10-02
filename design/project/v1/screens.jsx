// LiveBlocker — all screens.
// Each screen is a self-contained React component sized for a design_canvas
// artboard. They consume the global primitives (Logo, Surface, Pill, Toggle,
// Slider, Field, Icon, LavenderBar, PixelTile, Wordmark) declared on window
// from primitives.jsx.

// ─────────────────────────────────────────────────────────────
// 1. APP ICON CARD
// ─────────────────────────────────────────────────────────────
function ScreenAppIcon() {
  return (
    <div className="lb" style={{ width: 360, height: 360, background: 'var(--lb-bg)', display: 'flex', alignItems: 'center', justifyContent: 'center', borderRadius: 28 }}>
      <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 24 }}>
        <div style={{ filter: 'drop-shadow(12px 12px 24px var(--lb-shadow-dark)) drop-shadow(-8px -8px 18px var(--lb-shadow-light))' }}>
          <Logo size={156} radius={36} />
        </div>
        <div style={{ textAlign: 'center' }}>
          <div className="lb-display" style={{ fontSize: 28, color: 'var(--lb-ink)' }}>
            Live<span style={{ color: 'var(--lb-block)' }}>Block</span>er
          </div>
          <div style={{ fontSize: 12, color: 'var(--lb-ink-muted)', marginTop: 4, letterSpacing: '0.04em' }}>
            ON-DEVICE · MANUAL AD BLOCKER
          </div>
        </div>
      </div>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────
// 2. MENU BAR DROPDOWN — small, sits in macOS top bar
// ─────────────────────────────────────────────────────────────
function ScreenMenuBar() {
  return (
    <div className="lb" style={{ width: 360, padding: 24, background: '#3a3550', borderRadius: 28, position: 'relative' }}>
      {/* faux desktop wallpaper hint above the panel */}
      <div style={{
        position: 'absolute', inset: 0, borderRadius: 28,
        background: 'radial-gradient(120% 80% at 30% 0%, #6a4d8e 0%, #2c2042 60%, #1a1230 100%)',
      }} />
      <div style={{ position: 'relative' }}>
        <Surface radius={22} padding={18} style={{ background: 'var(--lb-bg)' }}>
          {/* head */}
          <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 14 }}>
            <Logo size={32} radius={8} />
            <div style={{ flex: 1 }}>
              <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--lb-ink)' }}>LiveBlocker</div>
              <div style={{ fontSize: 11, color: 'var(--lb-ink-muted)', display: 'flex', alignItems: 'center', gap: 5, marginTop: 1 }}>
                <Dot color="var(--lb-success)" size={6} /> Active · 14 regions
              </div>
            </div>
            <Toggle value={true} onChange={() => {}} accent="var(--lb-success)" />
          </div>

          {/* primary actions */}
          <div style={{ display: 'flex', gap: 8, marginBottom: 14 }}>
            <Pill variant="block" size="md" icon={Icon.marker(14)} style={{ flex: 1, justifyContent: 'center' }}>Block</Pill>
            <Pill variant="train" size="md" icon={Icon.ml(14)} style={{ flex: 1, justifyContent: 'center' }}>Train</Pill>
          </div>

          {/* live stats — inset wells */}
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: 8, marginBottom: 14 }}>
            {[
              { v: '847', l: 'blocked today', c: 'var(--lb-block)' },
              { v: '14', l: 'active regions', c: 'var(--lb-train)' },
              { v: '2.1', l: 'ms/frame', c: 'var(--lb-detect)' },
            ].map((s) => (
              <div key={s.l} className="lb-inset" style={{ padding: '10px 8px', borderRadius: 14, textAlign: 'center' }}>
                <div className="lb-mono" style={{ fontSize: 18, fontWeight: 600, color: s.c, lineHeight: 1 }}>{s.v}</div>
                <div style={{ fontSize: 9, color: 'var(--lb-ink-muted)', marginTop: 4, letterSpacing: '0.04em', textTransform: 'uppercase' }}>{s.l}</div>
              </div>
            ))}
          </div>

          {/* per-app row */}
          <div style={{ fontSize: 10, fontWeight: 600, color: 'var(--lb-ink-muted)', letterSpacing: '0.06em', textTransform: 'uppercase', marginBottom: 8 }}>This space</div>
          <div className="lb-inset" style={{ borderRadius: 14, padding: 4 }}>
            {[
              { app: 'Safari · youtube.com', n: 6, on: true },
              { app: 'Spotify', n: 2, on: true },
              { app: 'Twitter / X', n: 4, on: false },
            ].map((r, i) => (
              <div key={r.app} style={{
                display: 'flex', alignItems: 'center', gap: 10, padding: '8px 10px',
                borderRadius: 10, background: i === 0 ? 'rgba(255,255,255,0.5)' : 'transparent',
              }}>
                <div style={{ width: 16, height: 16, borderRadius: 4, background: ['#3da6ff','#1ec860','#000'][i] }} />
                <div style={{ flex: 1, fontSize: 12, fontWeight: 500, color: 'var(--lb-ink)' }}>{r.app}</div>
                <span className="lb-mono" style={{ fontSize: 11, color: 'var(--lb-ink-muted)' }}>{r.n}</span>
                <Toggle value={r.on} onChange={() => {}} accent="var(--lb-block)" />
              </div>
            ))}
          </div>

          {/* footer */}
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginTop: 14 }}>
            <Pill variant="default" size="sm" icon={Icon.settings(13)}>Preferences</Pill>
            <div style={{ flex: 1 }} />
            <span className="lb-mono" style={{ fontSize: 10, color: 'var(--lb-ink-faint)' }}>⌘⇧B</span>
          </div>
        </Surface>
      </div>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────
// 3. FLOATING MARKING TOOL — three states
// ─────────────────────────────────────────────────────────────
function FloatingTool({ state = 'idle' }) {
  // state: 'idle' (compact), 'sizing' (drag handles), 'fullscreen' (whole-screen mode)
  return (
    <div className="lb" style={{ width: 540, height: 380, background: 'transparent', position: 'relative', borderRadius: 22, overflow: 'hidden' }}>
      {/* faux desktop background */}
      <div style={{ position: 'absolute', inset: 0, background: 'linear-gradient(135deg,#1f1830 0%, #432a5a 100%)' }} />
      <div style={{ position: 'absolute', inset: 0, opacity: 0.25 }}><PixelTile cellsX={36} cellsY={26} size={16} opacity={0.28} /></div>

      {/* faux ad rectangle (the thing we're masking) */}
      {state !== 'fullscreen' && (
        <div style={{
          position: 'absolute', left: 60, top: 80, width: 220, height: 130, borderRadius: 8,
          background: 'linear-gradient(180deg,#ffe27a 0%,#ff8a3d 100%)',
          boxShadow: '0 8px 24px rgba(0,0,0,0.4)',
          display: 'flex', alignItems: 'center', justifyContent: 'center',
          fontFamily: 'ui-sans-serif', fontWeight: 700, fontSize: 22, color: '#3a1500',
        }}>SHOP NOW →</div>
      )}

      {/* fullscreen mode: pixel-extrapolated fill across the whole canvas */}
      {state === 'fullscreen' && (
        <div style={{ position: 'absolute', inset: 0 }}>
          <div style={{ position: 'absolute', inset: 0, background: 'linear-gradient(135deg,#372550 0%,#1a1230 100%)' }} />
          <div style={{ position: 'absolute', inset: '-4px', backdropFilter: 'blur(2px)' }} />
        </div>
      )}

      {/* the marking rectangle with handles */}
      {state === 'sizing' && (
        <div style={{
          position: 'absolute', left: 50, top: 70, width: 240, height: 150,
          border: '1.5px dashed #ff4a2e', borderRadius: 6,
          boxShadow: '0 0 0 2000px rgba(20,12,40,0.55)',
        }}>
          {/* corner handles */}
          {[
            { t: -6, l: -6 }, { t: -6, r: -6 }, { b: -6, l: -6 }, { b: -6, r: -6 },
          ].map((p, i) => (
            <div key={i} style={{
              position: 'absolute',
              top: p.t, left: p.l, right: p.r, bottom: p.b,
              width: 12, height: 12, borderRadius: '50%',
              background: '#fff', border: '2px solid #ff4a2e',
              boxShadow: '0 1px 3px rgba(0,0,0,0.35)',
            }} />
          ))}
          {/* size label */}
          <div className="lb-mono" style={{
            position: 'absolute', top: -28, left: 0,
            background: '#1a1230', color: '#ff8a3d',
            padding: '3px 8px', borderRadius: 4, fontSize: 11, fontWeight: 600,
          }}>240 × 150 px</div>
        </div>
      )}

      {/* the floating tool itself */}
      <div style={{
        position: 'absolute', bottom: 24, left: '50%', transform: 'translateX(-50%)',
      }}>
        <Surface radius={28} padding={10} style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <Logo size={32} radius={8} />
          <div style={{ width: 1, height: 28, background: 'rgba(0,0,0,0.07)' }} />

          {/* size segmented */}
          <div className="lb-inset" style={{ padding: 3, borderRadius: 999, display: 'flex', gap: 2 }}>
            {[
              { id: 'sm', l: 'S', w: 'S' },
              { id: 'md', l: 'M', w: 'M' },
              { id: 'lg', l: 'L', w: 'L' },
              { id: 'fs', l: '⤢', w: 'fs' },
            ].map((opt) => {
              const active = (state === 'fullscreen' && opt.id === 'fs') ||
                             (state !== 'fullscreen' && opt.id === 'md');
              return (
                <div key={opt.id} style={{
                  width: 28, height: 28, borderRadius: 999,
                  display: 'flex', alignItems: 'center', justifyContent: 'center',
                  background: active ? 'var(--lb-block)' : 'transparent',
                  color: active ? '#fff' : 'var(--lb-ink-2)',
                  fontSize: opt.id === 'fs' ? 14 : 12, fontWeight: 700,
                  boxShadow: active ? '0 2px 6px rgba(255,74,46,0.4)' : 'none',
                  cursor: 'pointer',
                }}>{opt.l}</div>
              );
            })}
          </div>

          <div style={{ width: 1, height: 28, background: 'rgba(0,0,0,0.07)' }} />

          {/* the big block button */}
          <Pill variant="block" size="md" icon={Icon.marker(14)}>
            {state === 'fullscreen' ? 'Block screen' : state === 'sizing' ? 'Confirm block' : 'Drag to mark'}
          </Pill>

          {/* divider + cancel */}
          <div style={{ width: 1, height: 28, background: 'rgba(0,0,0,0.07)' }} />
          <button style={{
            width: 32, height: 32, border: 'none', borderRadius: '50%', cursor: 'pointer',
            background: 'var(--lb-bg)',
            boxShadow: '3px 3px 6px var(--lb-shadow-dark), -3px -3px 6px var(--lb-shadow-light)',
            color: 'var(--lb-ink-muted)', fontSize: 16, fontWeight: 600,
          }}>×</button>
        </Surface>

        {/* hint */}
        <div style={{ marginTop: 10, textAlign: 'center', color: 'rgba(255,255,255,0.65)', fontSize: 11, letterSpacing: '0.04em' }}>
          {state === 'sizing' && 'DRAG HANDLES TO RESIZE  ·  ⏎ TO CONFIRM  ·  ESC TO CANCEL'}
          {state === 'fullscreen' && 'WHOLE DISPLAY WILL BE BLOCKED  ·  RELEASE TO CONFIRM'}
          {state === 'idle' && 'CLICK & DRAG ANY REGION  ·  ⌘⇧B HIDES THIS TOOL'}
        </div>
      </div>
    </div>
  );
}

const ScreenFloatingIdle   = () => <FloatingTool state="idle" />;
const ScreenFloatingSizing = () => <FloatingTool state="sizing" />;
const ScreenFloatingFS     = () => <FloatingTool state="fullscreen" />;

// ─────────────────────────────────────────────────────────────
// 4. MAIN SETTINGS WINDOW — sidebar + content (matches sketch chrome)
// ─────────────────────────────────────────────────────────────
function ScreenMainSettings() {
  const navItems = [
    { l: 'Blocking', i: Icon.block(14), on: true },
    { l: 'Region library', i: Icon.marker(14) },
    { l: 'ML detector', i: Icon.ml(14) },
    { l: 'Per-app rules', i: Icon.shield(14) },
    { l: 'Performance', i: Icon.cpu(14) },
    { l: 'Shortcuts', i: Icon.settings(14) },
  ];
  return (
    <div className="lb" style={{ width: 920, height: 620, borderRadius: 28, overflow: 'hidden', background: 'var(--lb-bg)', boxShadow: '0 24px 60px rgba(20,12,40,0.35), 0 0 0 0.5px rgba(0,0,0,0.18)' }}>
      <LavenderBar leftPills={(
        <>
          <Pill variant="block" size="sm" icon={Icon.marker(13)}>Block</Pill>
          <Pill variant="train" size="sm" icon={Icon.ml(13)}>Train</Pill>
        </>
      )} />
      <div style={{ display: 'flex', height: 'calc(100% - 56px)' }}>
        {/* sidebar */}
        <div style={{ width: 220, padding: '20px 14px', display: 'flex', flexDirection: 'column', gap: 6 }}>
          <Wordmark size={16} />
          <div className="lb-inset" style={{ borderRadius: 14, padding: 6, marginTop: 18 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '6px 8px' }}>
              {Icon.search(12)}
              <span style={{ fontSize: 12, color: 'var(--lb-ink-faint)' }}>Search…</span>
              <span style={{ flex: 1 }} />
              <span className="lb-mono" style={{ fontSize: 10, color: 'var(--lb-ink-faint)' }}>⌘K</span>
            </div>
          </div>

          <div style={{ marginTop: 8, display: 'flex', flexDirection: 'column', gap: 2 }}>
            {navItems.map((n) => (
              <div key={n.l} className={n.on ? 'lb-pressed' : ''} style={{
                display: 'flex', alignItems: 'center', gap: 10,
                padding: '9px 12px', borderRadius: 12, fontSize: 13, fontWeight: n.on ? 600 : 500,
                color: n.on ? 'var(--lb-block)' : 'var(--lb-ink-2)',
                background: n.on ? 'var(--lb-bg)' : 'transparent',
                boxShadow: n.on ? 'inset 3px 3px 6px var(--lb-shadow-dark), inset -3px -3px 6px var(--lb-shadow-light)' : 'none',
                cursor: 'pointer',
              }}>
                <span style={{ color: n.on ? 'var(--lb-block)' : 'var(--lb-ink-muted)' }}>{n.i}</span>
                {n.l}
              </div>
            ))}
          </div>
          <div style={{ flex: 1 }} />
          {/* status block */}
          <Surface kind="inset" radius={14} padding={12} style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            <div style={{ fontSize: 10, color: 'var(--lb-ink-muted)', letterSpacing: '0.06em', textTransform: 'uppercase' }}>Capture</div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
              <Dot color="var(--lb-success)" size={7} pulse />
              <span style={{ fontSize: 12, color: 'var(--lb-ink)' }}>ScreenCaptureKit · 60 fps</span>
            </div>
            <div className="lb-mono" style={{ fontSize: 10, color: 'var(--lb-ink-muted)' }}>2.1 ms/frame · 0.4% CPU</div>
          </Surface>
        </div>

        {/* main */}
        <div style={{ flex: 1, padding: '24px 28px', overflow: 'auto', display: 'flex', flexDirection: 'column', gap: 20 }}>
          {/* hero */}
          <div style={{ display: 'flex', alignItems: 'flex-start', gap: 16 }}>
            <div style={{ flex: 1 }}>
              <div className="lb-display" style={{ fontSize: 28, color: 'var(--lb-ink)' }}>Blocking</div>
              <div style={{ fontSize: 13, color: 'var(--lb-ink-muted)', marginTop: 4 }}>
                Replace marked regions with edge-extrapolated fill before each frame is composited to your display.
              </div>
            </div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--lb-success)' }}>ACTIVE</span>
              <Toggle value={true} onChange={() => {}} accent="var(--lb-success)" />
            </div>
          </div>

          {/* big stats grid */}
          <div style={{ display: 'grid', gridTemplateColumns: '1.4fr 1fr 1fr', gap: 14 }}>
            <Surface radius={20} padding={20} style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
              <div style={{ fontSize: 11, color: 'var(--lb-ink-muted)', letterSpacing: '0.06em', textTransform: 'uppercase' }}>Blocked today</div>
              <div style={{ display: 'flex', alignItems: 'baseline', gap: 8 }}>
                <span className="lb-display" style={{ fontSize: 56, color: 'var(--lb-block)', lineHeight: 0.9 }}>847</span>
                <span style={{ fontSize: 13, color: 'var(--lb-ink-muted)' }}>regions filled</span>
              </div>
              {/* sparkline */}
              <svg viewBox="0 0 200 40" width="100%" height="40" preserveAspectRatio="none">
                <defs>
                  <linearGradient id="sparkA" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0" stopColor="#ff4a2e" stopOpacity="0.4" />
                    <stop offset="1" stopColor="#ff4a2e" stopOpacity="0" />
                  </linearGradient>
                </defs>
                <path d="M0 30 L20 24 L40 28 L60 18 L80 22 L100 12 L120 16 L140 8 L160 14 L180 6 L200 10 L200 40 L0 40 Z" fill="url(#sparkA)" />
                <path d="M0 30 L20 24 L40 28 L60 18 L80 22 L100 12 L120 16 L140 8 L160 14 L180 6 L200 10" fill="none" stroke="#ff4a2e" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
            </Surface>

            <Surface radius={20} padding={20} style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
              <div style={{ fontSize: 11, color: 'var(--lb-ink-muted)', letterSpacing: '0.06em', textTransform: 'uppercase' }}>Active regions</div>
              <span className="lb-display" style={{ fontSize: 44, color: 'var(--lb-train)', lineHeight: 1 }}>14</span>
              <div style={{ fontSize: 11, color: 'var(--lb-ink-muted)' }}>across 6 apps</div>
            </Surface>

            <Surface radius={20} padding={20} style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
              <div style={{ fontSize: 11, color: 'var(--lb-ink-muted)', letterSpacing: '0.06em', textTransform: 'uppercase' }}>Frame budget</div>
              <span className="lb-display lb-mono" style={{ fontSize: 38, color: 'var(--lb-detect)', lineHeight: 1 }}>2.1<span style={{ fontSize: 18, color: 'var(--lb-ink-muted)' }}>ms</span></span>
              {/* tiny bar */}
              <div className="lb-inset" style={{ height: 8, borderRadius: 999, padding: 0 }}>
                <div style={{ width: '12%', height: '100%', borderRadius: 999, background: 'var(--lb-detect)' }} />
              </div>
              <div style={{ fontSize: 10, color: 'var(--lb-ink-muted)' }}>of 16.6 ms · 60 fps headroom</div>
            </Surface>
          </div>

          {/* fill mode selector */}
          <Surface radius={20} padding={20}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 14 }}>
              <div>
                <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--lb-ink)' }}>Fill technique</div>
                <div style={{ fontSize: 12, color: 'var(--lb-ink-muted)', marginTop: 2 }}>How blocked regions are repainted</div>
              </div>
              <Pill size="sm" variant="ghost" icon={Icon.arrow(12)}>See examples</Pill>
            </div>
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4,1fr)', gap: 10 }}>
              {[
                { l: 'Edge extrapolate', d: 'Continue surrounding pixels', on: true },
                { l: 'Solid blur', d: 'Blur a sample of nearby area' },
                { l: 'Average color', d: 'Median of region edge' },
                { l: 'Black', d: 'Hard mask' },
              ].map((m) => (
                <div key={m.l} className={m.on ? 'lb-pressed' : 'lb-raised-sm'} style={{
                  borderRadius: 14, padding: 12,
                  border: m.on ? '1.5px solid var(--lb-block)' : '0.5px solid transparent',
                  cursor: 'pointer',
                }}>
                  <div style={{ height: 56, borderRadius: 8, marginBottom: 10, position: 'relative', overflow: 'hidden',
                    background: m.l === 'Edge extrapolate' ? 'linear-gradient(135deg,#3a2a55,#7c5cff,#3a2a55)' :
                                m.l === 'Solid blur' ? 'linear-gradient(135deg,#5a4a7a,#3a2a55)' :
                                m.l === 'Average color' ? '#4d3a6e' : '#000' }}>
                    {m.on && (
                      <div style={{ position: 'absolute', top: 6, right: 6, width: 18, height: 18, borderRadius: '50%', background: 'var(--lb-block)', color: '#fff', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                        {Icon.check(11)}
                      </div>
                    )}
                  </div>
                  <div style={{ fontSize: 12, fontWeight: 600, color: 'var(--lb-ink)' }}>{m.l}</div>
                  <div style={{ fontSize: 11, color: 'var(--lb-ink-muted)', marginTop: 2 }}>{m.d}</div>
                </div>
              ))}
            </div>
          </Surface>

          {/* settings rows */}
          <Surface radius={20} padding={6}>
            {[
              { l: 'Auto-block detector suggestions', d: 'Apply CoreML detections above 84% confidence', t: true },
              { l: 'Pause when game / fullscreen video active', d: 'Skip blocking inside SCStream-detected fullscreen apps', t: true },
              { l: 'Show floating tool on hotkey', d: '⌘⇧B summons the marker bar', t: true },
              { l: 'Send anonymous telemetry', d: 'Frame timing only — no captured pixels ever leave the device', t: false },
            ].map((r, i) => (
              <div key={r.l} style={{ display: 'flex', alignItems: 'center', gap: 14, padding: '14px 16px', borderTop: i ? '1px solid rgba(0,0,0,0.05)' : 'none' }}>
                <div style={{ flex: 1 }}>
                  <div style={{ fontSize: 13, fontWeight: 500, color: 'var(--lb-ink)' }}>{r.l}</div>
                  <div style={{ fontSize: 11, color: 'var(--lb-ink-muted)', marginTop: 2 }}>{r.d}</div>
                </div>
                <Toggle value={r.t} onChange={() => {}} accent="var(--lb-block)" />
              </div>
            ))}
          </Surface>
        </div>
      </div>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────
// 5. REGION LIBRARY
// ─────────────────────────────────────────────────────────────
function ScreenRegionLibrary() {
  const regions = [
    { app: 'Safari', site: 'youtube.com', name: 'Right rail · sponsored', size: '300×600', hits: 142, on: true, color: '#ff0000' },
    { app: 'Safari', site: 'youtube.com', name: 'In-feed promo cards', size: '728×90', hits: 88, on: true, color: '#ff0000' },
    { app: 'Spotify', site: 'app', name: 'Now Playing banner', size: '300×250', hits: 34, on: true, color: '#1ec860' },
    { app: 'Twitter', site: 'x.com', name: 'Promoted tweets', size: 'auto', hits: 215, on: false, color: '#000' },
    { app: 'Reddit', site: 'reddit.com', name: 'Sidebar ads', size: '300×600', hits: 76, on: true, color: '#ff4500' },
    { app: 'Mail', site: 'app', name: 'Newsletter footer ad', size: 'variable', hits: 12, on: true, color: '#0a84ff' },
    { app: 'Cursor', site: 'app', name: 'Update toast', size: '320×80', hits: 4, on: false, color: '#7c5cff' },
  ];
  return (
    <div className="lb" style={{ width: 920, height: 620, borderRadius: 28, overflow: 'hidden', background: 'var(--lb-bg)', boxShadow: '0 24px 60px rgba(20,12,40,0.35)' }}>
      <LavenderBar leftPills={(
        <>
          <Pill variant="block" size="sm" icon={Icon.marker(13)}>Block</Pill>
          <Pill variant="train" size="sm" icon={Icon.ml(13)}>Train</Pill>
        </>
      )} />
      <div style={{ display: 'flex', height: 'calc(100% - 56px)' }}>
        <div style={{ width: 220, padding: 20, display: 'flex', flexDirection: 'column', gap: 16 }}>
          <Wordmark size={16} />
          <Surface kind="inset" radius={14} padding={12} style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            <div style={{ fontSize: 10, color: 'var(--lb-ink-muted)', letterSpacing: '0.06em', textTransform: 'uppercase' }}>Filter by app</div>
            {['All apps · 7','Safari · 2','Spotify · 1','Twitter · 1','Reddit · 1','Mail · 1','Cursor · 1'].map((a, i) => (
              <div key={a} style={{ fontSize: 12, padding: '5px 8px', borderRadius: 8, color: 'var(--lb-ink-2)', fontWeight: i === 0 ? 600 : 400, background: i === 0 ? 'rgba(255,255,255,0.6)' : 'transparent' }}>{a}</div>
            ))}
          </Surface>
        </div>

        <div style={{ flex: 1, padding: '24px 28px', display: 'flex', flexDirection: 'column', gap: 16 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
            <div style={{ flex: 1 }}>
              <div className="lb-display" style={{ fontSize: 26, color: 'var(--lb-ink)' }}>Region library</div>
              <div style={{ fontSize: 12, color: 'var(--lb-ink-muted)' }}>14 saved regions · 7 active</div>
            </div>
            <Field icon={Icon.search(13)} placeholder="Find a region…" style={{ width: 220 }} />
            <Pill variant="block" size="md" icon={Icon.plus(14)}>New region</Pill>
          </div>

          {/* table head */}
          <div style={{ display: 'grid', gridTemplateColumns: '20px 1.6fr 1.2fr 90px 80px 70px 50px', gap: 12, padding: '0 16px', fontSize: 10, color: 'var(--lb-ink-muted)', letterSpacing: '0.06em', textTransform: 'uppercase' }}>
            <span /><span>Region</span><span>App / context</span><span>Size</span><span>Blocks</span><span>Active</span><span /></div>

          <Surface radius={20} padding={6} style={{ display: 'flex', flexDirection: 'column', gap: 4, flex: 1 }}>
            {regions.map((r, i) => (
              <div key={i} style={{
                display: 'grid', gridTemplateColumns: '20px 1.6fr 1.2fr 90px 80px 70px 50px',
                gap: 12, alignItems: 'center', padding: '10px 16px',
                borderRadius: 12, background: i === 0 ? 'rgba(255,255,255,0.55)' : 'transparent',
              }}>
                <span style={{ color: 'var(--lb-ink-faint)' }}>{Icon.drag(12)}</span>
                <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                  {/* preview swatch */}
                  <div style={{ width: 36, height: 24, borderRadius: 6, background: '#1a1230', position: 'relative', overflow: 'hidden', border: '0.5px solid rgba(0,0,0,0.1)' }}>
                    <div style={{ position: 'absolute', inset: 4, borderRadius: 2, border: '1px dashed #ff4a2e' }} />
                  </div>
                  <div style={{ fontSize: 13, fontWeight: 500, color: 'var(--lb-ink)' }}>{r.name}</div>
                </div>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                  <div style={{ width: 14, height: 14, borderRadius: 4, background: r.color, flexShrink: 0 }} />
                  <span style={{ fontSize: 12, color: 'var(--lb-ink-2)' }}>{r.app}</span>
                  <span style={{ fontSize: 11, color: 'var(--lb-ink-muted)' }}>· {r.site}</span>
                </div>
                <span className="lb-mono" style={{ fontSize: 11, color: 'var(--lb-ink-muted)' }}>{r.size}</span>
                <span className="lb-mono" style={{ fontSize: 12, fontWeight: 600, color: 'var(--lb-block)' }}>{r.hits}</span>
                <Toggle value={r.on} onChange={() => {}} accent="var(--lb-block)" />
                <button style={{ width: 26, height: 26, borderRadius: 8, border: 'none', background: 'transparent', color: 'var(--lb-ink-faint)', cursor: 'pointer' }}>{Icon.trash(12)}</button>
              </div>
            ))}
          </Surface>
        </div>
      </div>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────
// 6. ML DETECTOR TUNING
// ─────────────────────────────────────────────────────────────
function ScreenMLDetector() {
  const cats = [
    { l: 'Banner ads', n: 412, on: true },
    { l: 'Video pre-roll', n: 87, on: true },
    { l: 'Sponsored cards', n: 234, on: true },
    { l: 'Newsletter popups', n: 56, on: true },
    { l: 'Cookie banners', n: 188, on: false },
    { l: 'Chat widgets', n: 23, on: false },
    { l: 'Engagement nudges', n: 41, on: false },
  ];
  // synthetic detections on a fake screen
  const dets = [
    { x: 8, y: 14, w: 38, h: 18, label: 'banner', conf: 0.94, on: true },
    { x: 62, y: 42, w: 28, h: 32, label: 'sponsored', conf: 0.87, on: true },
    { x: 14, y: 64, w: 26, h: 14, label: 'popup', conf: 0.62, on: false },
  ];
  return (
    <div className="lb" style={{ width: 920, height: 620, borderRadius: 28, overflow: 'hidden', background: 'var(--lb-bg)', boxShadow: '0 24px 60px rgba(20,12,40,0.35)' }}>
      <LavenderBar leftPills={<><Pill variant="block" size="sm" icon={Icon.marker(13)}>Block</Pill><Pill variant="train" size="sm" icon={Icon.ml(13)}>Train</Pill></>} />
      <div style={{ display: 'flex', height: 'calc(100% - 56px)' }}>
        <div style={{ width: 220, padding: 20 }}><Wordmark size={16} /></div>
        <div style={{ flex: 1, padding: '24px 28px', display: 'grid', gridTemplateColumns: '1.3fr 1fr', gap: 18, gridTemplateRows: 'auto 1fr', minHeight: 0 }}>
          {/* header full-width */}
          <div style={{ gridColumn: '1 / -1', display: 'flex', alignItems: 'center', gap: 12 }}>
            <div style={{ flex: 1 }}>
              <div className="lb-display" style={{ fontSize: 26, color: 'var(--lb-ink)' }}>ML detector</div>
              <div style={{ fontSize: 12, color: 'var(--lb-ink-muted)' }}>On-device CoreML model · LiveBlocker-vit-v3 · 14.2 MB</div>
            </div>
            <Pill variant="train" size="md" icon={Icon.ml(13)}>Train on selection</Pill>
          </div>

          {/* live preview */}
          <Surface radius={20} padding={16} style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
            <div style={{ fontSize: 11, color: 'var(--lb-ink-muted)', letterSpacing: '0.06em', textTransform: 'uppercase' }}>Live preview · ScreenCaptureKit</div>
            <div style={{ position: 'relative', borderRadius: 12, overflow: 'hidden', background: '#1a1230', aspectRatio: '16/10' }}>
              <div style={{ position: 'absolute', inset: 0, background: 'linear-gradient(135deg,#2a1f44 0%,#4a2d6e 100%)' }} />
              <div style={{ position: 'absolute', inset: 0 }}><PixelTile cellsX={48} cellsY={30} size={10} opacity={0.18} /></div>
              {/* detections */}
              {dets.map((d, i) => (
                <div key={i} style={{
                  position: 'absolute', left: `${d.x}%`, top: `${d.y}%`, width: `${d.w}%`, height: `${d.h}%`,
                  border: `1.5px ${d.on ? 'solid' : 'dashed'} ${d.conf > 0.8 ? '#ff4a2e' : '#ffd23a'}`,
                  borderRadius: 4,
                  background: d.on ? 'rgba(255,74,46,0.16)' : 'rgba(255,210,58,0.08)',
                }}>
                  <div className="lb-mono" style={{
                    position: 'absolute', top: -22, left: -1,
                    background: d.conf > 0.8 ? '#ff4a2e' : '#ffd23a', color: d.conf > 0.8 ? '#fff' : '#3a1500',
                    padding: '2px 6px', borderRadius: 3, fontSize: 10, fontWeight: 700, letterSpacing: '0.04em', textTransform: 'uppercase',
                  }}>{d.label} · {Math.round(d.conf * 100)}%</div>
                </div>
              ))}
              {/* HUD overlay */}
              <div style={{ position: 'absolute', left: 10, bottom: 10, display: 'flex', gap: 6, alignItems: 'center', background: 'rgba(0,0,0,0.55)', backdropFilter: 'blur(8px)', borderRadius: 999, padding: '5px 10px' }}>
                <Dot color="#ff4a2e" size={6} pulse />
                <span className="lb-mono" style={{ fontSize: 10, color: '#fff', letterSpacing: '0.04em' }}>3 DETECTIONS · 8.4 ms</span>
              </div>
            </div>
            {/* confidence threshold */}
            <div>
              <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 12, color: 'var(--lb-ink)', fontWeight: 500, marginBottom: 8 }}>
                <span>Auto-block threshold</span>
                <span className="lb-mono" style={{ color: 'var(--lb-block)' }}>84%</span>
              </div>
              <Slider value={84} min={50} max={99} onChange={() => {}} />
              <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 10, color: 'var(--lb-ink-muted)', marginTop: 6 }}>
                <span>more aggressive</span><span>fewer false positives</span>
              </div>
            </div>
          </Surface>

          {/* categories */}
          <Surface radius={20} padding={16} style={{ display: 'flex', flexDirection: 'column', gap: 10, minHeight: 0 }}>
            <div style={{ fontSize: 11, color: 'var(--lb-ink-muted)', letterSpacing: '0.06em', textTransform: 'uppercase' }}>Categories</div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 4, overflow: 'auto' }}>
              {cats.map((c) => (
                <div key={c.l} style={{
                  display: 'flex', alignItems: 'center', gap: 12, padding: '10px 12px', borderRadius: 12,
                  background: c.on ? 'rgba(255,255,255,0.55)' : 'transparent',
                }}>
                  <div style={{ width: 6, height: 28, borderRadius: 3, background: c.on ? 'var(--lb-block)' : 'var(--lb-ink-faint)' }} />
                  <div style={{ flex: 1 }}>
                    <div style={{ fontSize: 13, fontWeight: 500, color: 'var(--lb-ink)' }}>{c.l}</div>
                    <div className="lb-mono" style={{ fontSize: 10, color: 'var(--lb-ink-muted)', marginTop: 1 }}>{c.n.toLocaleString()} caught · 30d</div>
                  </div>
                  <Toggle value={c.on} onChange={() => {}} accent="var(--lb-block)" />
                </div>
              ))}
            </div>
            {/* feedback row */}
            <div className="lb-inset" style={{ borderRadius: 12, padding: 12, marginTop: 'auto' }}>
              <div style={{ fontSize: 11, color: 'var(--lb-ink-muted)', letterSpacing: '0.06em', textTransform: 'uppercase', marginBottom: 6 }}>Review queue</div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                <div className="lb-mono" style={{ fontSize: 22, fontWeight: 700, color: 'var(--lb-warn)' }}>3</div>
                <div style={{ flex: 1, fontSize: 12, color: 'var(--lb-ink-2)', lineHeight: 1.3 }}>borderline detections waiting for your call</div>
                <Pill variant="ghost" size="sm" icon={Icon.arrow(12)}>Review</Pill>
              </div>
            </div>
          </Surface>
        </div>
      </div>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────
// 7. MINI HUD — small live status while blocking is active
// ─────────────────────────────────────────────────────────────
function ScreenHUD() {
  return (
    <div className="lb" style={{ width: 380, height: 140, padding: 24, background: 'transparent', display: 'flex', alignItems: 'center', justifyContent: 'center', borderRadius: 28, position: 'relative', overflow: 'hidden' }}>
      <div style={{ position: 'absolute', inset: 0, background: 'linear-gradient(135deg,#2c2042 0%,#1a1230 100%)' }} />
      <Surface radius={24} padding={14} style={{ display: 'flex', alignItems: 'center', gap: 14, position: 'relative' }}>
        <div style={{ position: 'relative' }}>
          <Logo size={42} radius={11} />
          <div style={{ position: 'absolute', top: -3, right: -3, width: 14, height: 14, borderRadius: '50%', background: '#ff4a2e', border: '2px solid var(--lb-bg)', boxShadow: '0 0 0 3px rgba(255,74,46,0.25)' }} />
        </div>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
            <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--lb-block)', letterSpacing: '0.04em', textTransform: 'uppercase' }}>Blocking</span>
            <span className="lb-mono" style={{ fontSize: 11, color: 'var(--lb-ink-muted)' }}>· 2.1 ms</span>
          </div>
          <div className="lb-display" style={{ fontSize: 22, color: 'var(--lb-ink)', lineHeight: 1 }}>14 regions live</div>
          <div style={{ fontSize: 11, color: 'var(--lb-ink-muted)' }}>847 fills today · ⌘⇧B to mark</div>
        </div>
        <div style={{ flex: 1 }} />
        <button style={{ width: 38, height: 38, border: 'none', borderRadius: '50%', background: 'var(--lb-bg)', boxShadow: '3px 3px 7px var(--lb-shadow-dark), -3px -3px 7px var(--lb-shadow-light)', color: 'var(--lb-ink-2)', cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>{Icon.pause(14)}</button>
      </Surface>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────
// 8. ONBOARDING (3 steps in one card row)
// ─────────────────────────────────────────────────────────────
function OnboardingStep({ step }) {
  // step 1 welcome, 2 permissions, 3 first-block
  const total = 3;
  return (
    <div className="lb" style={{ width: 460, height: 540, borderRadius: 28, background: 'var(--lb-bg)', overflow: 'hidden', boxShadow: '0 24px 60px rgba(20,12,40,0.30)', display: 'flex', flexDirection: 'column' }}>
      {/* tiny title bar */}
      <div style={{ height: 36, padding: '0 14px', display: 'flex', alignItems: 'center', background: 'linear-gradient(180deg,#cdcce8,#b9b8d8)' }}>
        <TrafficLights />
      </div>
      <div style={{ flex: 1, padding: '28px 32px', display: 'flex', flexDirection: 'column' }}>
        {step === 1 && (
          <>
            <div style={{ display: 'flex', justifyContent: 'center', marginTop: 24 }}>
              <div style={{ filter: 'drop-shadow(8px 8px 16px var(--lb-shadow-dark)) drop-shadow(-6px -6px 12px var(--lb-shadow-light))' }}>
                <Logo size={120} radius={28} />
              </div>
            </div>
            <div className="lb-display" style={{ fontSize: 36, color: 'var(--lb-ink)', textAlign: 'center', marginTop: 28, lineHeight: 1 }}>
              Block what your<br />screen shouldn't show.
            </div>
            <div style={{ fontSize: 13, color: 'var(--lb-ink-muted)', textAlign: 'center', marginTop: 14, lineHeight: 1.5, maxWidth: 360, alignSelf: 'center' }}>
              LiveBlocker reads display frames with ScreenCaptureKit and replaces marked regions with edge-extrapolated fill — before they hit your eyes.
            </div>
          </>
        )}
        {step === 2 && (
          <>
            <div className="lb-display" style={{ fontSize: 26, color: 'var(--lb-ink)', marginTop: 8 }}>Two quick permissions</div>
            <div style={{ fontSize: 12, color: 'var(--lb-ink-muted)', marginTop: 4 }}>Both stay on your Mac. Nothing leaves the device.</div>
            <div style={{ marginTop: 20, display: 'flex', flexDirection: 'column', gap: 10 }}>
              {[
                { l: 'Screen Recording', d: 'Required by ScreenCaptureKit', on: true, i: Icon.cpu(16) },
                { l: 'Accessibility (optional)', d: 'Lets blocks snap to UI elements', on: false, i: Icon.shield(16) },
              ].map((p) => (
                <Surface key={p.l} kind={p.on ? 'pressed' : 'sm'} radius={16} padding={14} style={{ display: 'flex', alignItems: 'center', gap: 14, border: p.on ? '1.5px solid var(--lb-success)' : '0.5px solid transparent' }}>
                  <div style={{ width: 38, height: 38, borderRadius: 10, display: 'flex', alignItems: 'center', justifyContent: 'center', color: p.on ? 'var(--lb-success)' : 'var(--lb-ink-muted)', background: p.on ? 'rgba(43,191,108,0.12)' : 'rgba(0,0,0,0.04)' }}>{p.i}</div>
                  <div style={{ flex: 1 }}>
                    <div style={{ fontSize: 13, fontWeight: 600 }}>{p.l}</div>
                    <div style={{ fontSize: 11, color: 'var(--lb-ink-muted)', marginTop: 2 }}>{p.d}</div>
                  </div>
                  {p.on
                    ? <span style={{ fontSize: 11, fontWeight: 600, color: 'var(--lb-success)', display: 'inline-flex', alignItems: 'center', gap: 4 }}>{Icon.check(11)} Granted</span>
                    : <Pill size="sm">Allow</Pill>}
                </Surface>
              ))}
            </div>
            <Surface kind="inset" radius={14} padding={12} style={{ marginTop: 20, display: 'flex', gap: 10, alignItems: 'flex-start' }}>
              <div style={{ color: 'var(--lb-detect)', flexShrink: 0, marginTop: 1 }}>{Icon.shield(14)}</div>
              <div style={{ fontSize: 11, color: 'var(--lb-ink-muted)', lineHeight: 1.4 }}>
                Frames are processed in a sandboxed Metal pipeline and discarded after compositing. No screenshots, no telemetry of pixel content, ever.
              </div>
            </Surface>
          </>
        )}
        {step === 3 && (
          <>
            <div className="lb-display" style={{ fontSize: 26, color: 'var(--lb-ink)' }}>Make your first block</div>
            <div style={{ fontSize: 12, color: 'var(--lb-ink-muted)', marginTop: 4 }}>Press the hotkey, drag any rectangle, done.</div>
            <div className="lb-inset" style={{ marginTop: 20, borderRadius: 16, height: 240, position: 'relative', overflow: 'hidden' }}>
              <div style={{ position: 'absolute', inset: 0, background: 'linear-gradient(135deg,#2a1f44 0%,#4a2d6e 100%)' }} />
              <div style={{ position: 'absolute', inset: 0, opacity: 0.3 }}><PixelTile cellsX={26} cellsY={16} size={14} opacity={0.4} /></div>
              <div style={{ position: 'absolute', left: '18%', top: '24%', width: '64%', height: '52%', border: '1.5px dashed #ff4a2e', borderRadius: 6, background: 'rgba(255,74,46,0.10)' }}>
                <div className="lb-mono" style={{ position: 'absolute', top: -22, left: 0, background: '#ff4a2e', color: '#fff', padding: '2px 6px', borderRadius: 3, fontSize: 10, fontWeight: 700 }}>240 × 96</div>
                {[
                  { t: -5, l: -5 }, { t: -5, r: -5 }, { b: -5, l: -5 }, { b: -5, r: -5 },
                ].map((p, i) => (
                  <div key={i} style={{ position: 'absolute', top: p.t, left: p.l, right: p.r, bottom: p.b, width: 10, height: 10, borderRadius: '50%', background: '#fff', border: '2px solid #ff4a2e' }} />
                ))}
              </div>
            </div>
            <div style={{ marginTop: 18, display: 'flex', alignItems: 'center', gap: 8, justifyContent: 'center' }}>
              <span className="lb-mono" style={{ fontSize: 11, color: 'var(--lb-ink-muted)' }}>Press</span>
              {['⌘','⇧','B'].map((k) => (
                <Surface key={k} kind="sm" radius={8} padding={0} style={{ minWidth: 28, height: 28, display: 'flex', alignItems: 'center', justifyContent: 'center', fontSize: 12, fontWeight: 600 }}>{k}</Surface>
              ))}
              <span className="lb-mono" style={{ fontSize: 11, color: 'var(--lb-ink-muted)' }}>anywhere on your Mac</span>
            </div>
          </>
        )}
        <div style={{ flex: 1 }} />
        {/* footer: dots + cta */}
        <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginTop: 20 }}>
          <div style={{ display: 'flex', gap: 6 }}>
            {[0,1,2].map((i) => (
              <div key={i} style={{ width: i === step - 1 ? 22 : 7, height: 7, borderRadius: 999, background: i === step - 1 ? 'var(--lb-block)' : 'var(--lb-ink-faint)', transition: 'width .25s' }} />
            ))}
          </div>
          <div style={{ flex: 1 }} />
          {step > 1 && <Pill variant="ghost" size="sm">Back</Pill>}
          <Pill variant="block" size="md" icon={step === total ? Icon.check(13) : Icon.arrow(13)}>
            {step === 1 ? 'Get started' : step === 2 ? 'Continue' : 'Try it now'}
          </Pill>
        </div>
      </div>
    </div>
  );
}
const ScreenOnboarding1 = () => <OnboardingStep step={1} />;
const ScreenOnboarding2 = () => <OnboardingStep step={2} />;
const ScreenOnboarding3 = () => <OnboardingStep step={3} />;

// ─────────────────────────────────────────────────────────────
// 9. IN-ACTION — fake desktop showing before/after
// ─────────────────────────────────────────────────────────────
function ScreenInAction({ mode = 'after' }) {
  const W = 720, H = 460;
  return (
    <div className="lb" style={{ width: W, height: H, borderRadius: 22, overflow: 'hidden', position: 'relative', boxShadow: '0 24px 60px rgba(20,12,40,0.35)' }}>
      {/* desktop wallpaper */}
      <div style={{ position: 'absolute', inset: 0, background: 'linear-gradient(160deg,#1a1230 0%,#3a2055 50%,#5e2860 100%)' }} />

      {/* fake browser window */}
      <div style={{ position: 'absolute', left: 30, top: 26, right: 30, bottom: 60, borderRadius: 14, overflow: 'hidden', background: '#fff', boxShadow: '0 16px 40px rgba(0,0,0,0.4)' }}>
        <div style={{ height: 34, display: 'flex', alignItems: 'center', gap: 8, padding: '0 14px', background: '#f1f1f4', borderBottom: '0.5px solid rgba(0,0,0,0.08)' }}>
          <TrafficLights />
          <div style={{ flex: 1 }} />
          <div style={{ background: '#fff', borderRadius: 6, padding: '3px 14px', fontSize: 10, color: '#666', fontFamily: 'ui-sans-serif' }}>news.example.com</div>
          <div style={{ flex: 1 }} />
        </div>
        {/* page content */}
        <div style={{ position: 'relative', height: 'calc(100% - 34px)', padding: 18, fontFamily: 'ui-sans-serif' }}>
          <div style={{ fontSize: 22, fontWeight: 700, color: '#1a1230', marginBottom: 6 }}>Top stories today</div>
          <div style={{ fontSize: 11, color: '#888', marginBottom: 14 }}>NEWS · 6 min read</div>
          {/* article columns */}
          <div style={{ display: 'grid', gridTemplateColumns: '1.6fr 1fr', gap: 18 }}>
            <div>
              {[60, 95, 80, 92, 70, 98, 82, 76].map((w, i) => (
                <div key={i} style={{ height: 7, width: `${w}%`, background: '#dcd6e6', borderRadius: 3, marginBottom: 7 }} />
              ))}
              <div style={{ height: 96, marginTop: 10, borderRadius: 6, background: 'linear-gradient(135deg,#dcd6e6,#b8b0c8)' }} />
              {[88, 70, 92].map((w, i) => (
                <div key={i} style={{ height: 7, width: `${w}%`, background: '#dcd6e6', borderRadius: 3, marginTop: 8 }} />
              ))}
            </div>
            {/* right rail (the ad slot) */}
            <div>
              {/* ad slot */}
              <div style={{ position: 'relative', height: 240, borderRadius: 6, overflow: 'hidden' }}>
                {mode === 'before' && (
                  <div style={{ position: 'absolute', inset: 0, background: 'linear-gradient(160deg,#ffd23a 0%,#ff4a2e 100%)', display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', color: '#3a1500' }}>
                    <div style={{ fontWeight: 700, fontSize: 26, letterSpacing: '-0.02em' }}>BUY NOW</div>
                    <div style={{ fontSize: 11, marginTop: 6, opacity: 0.85 }}>Limited offer · Click here</div>
                    <div style={{ marginTop: 14, padding: '6px 16px', background: '#1a1230', color: '#fff', borderRadius: 999, fontSize: 11, fontWeight: 600 }}>SHOP →</div>
                  </div>
                )}
                {mode === 'after' && (
                  // edge-extrapolated fill: continue surrounding pixels (same as page bg)
                  <div style={{ position: 'absolute', inset: 0, background: '#fff', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                    {/* subtle pixel texture to imply extrapolation */}
                    <div style={{ position: 'absolute', inset: 0, opacity: 0.04 }}><PixelTile cellsX={30} cellsY={50} size={4} opacity={0.5} palette={['#dcd6e6','#b8b0c8','#fff','#fff','#fff']} /></div>
                    <div style={{ position: 'relative', display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 6, color: '#a0a0b0' }}>
                      <Logo size={28} radius={7} />
                      <span className="lb-mono" style={{ fontSize: 9, letterSpacing: '0.06em' }}>BLOCKED · region #03</span>
                    </div>
                  </div>
                )}
                {/* live overlay marker (only in 'after') */}
                {mode === 'after' && (
                  <div style={{ position: 'absolute', inset: 0, border: '1.5px dashed rgba(255,74,46,0.45)', borderRadius: 6, pointerEvents: 'none' }} />
                )}
              </div>
              {/* small list */}
              <div style={{ marginTop: 14 }}>
                <div style={{ fontSize: 9, color: '#888', letterSpacing: '0.06em', textTransform: 'uppercase', marginBottom: 6 }}>Related</div>
                {[80, 70, 90].map((w, i) => (
                  <div key={i} style={{ height: 6, width: `${w}%`, background: '#dcd6e6', borderRadius: 3, marginBottom: 5 }} />
                ))}
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* dock-like LiveBlocker HUD */}
      <div style={{ position: 'absolute', left: '50%', bottom: 14, transform: 'translateX(-50%)' }}>
        <Surface radius={22} padding={8} style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <Logo size={26} radius={7} />
          <span style={{ fontSize: 11, fontWeight: 600, color: 'var(--lb-block)' }}>BLOCKING</span>
          <span className="lb-mono" style={{ fontSize: 10, color: 'var(--lb-ink-muted)' }}>· 2.1 ms · 3 fills</span>
          <div style={{ width: 1, height: 18, background: 'rgba(0,0,0,0.07)' }} />
          <Pill variant="block" size="sm" icon={Icon.marker(12)}>Mark</Pill>
        </Surface>
      </div>

      {/* mode label */}
      <div className="lb-mono" style={{ position: 'absolute', top: 14, left: '50%', transform: 'translateX(-50%)', background: 'rgba(0,0,0,0.45)', color: '#fff', padding: '4px 10px', borderRadius: 999, fontSize: 10, fontWeight: 700, letterSpacing: '0.08em', backdropFilter: 'blur(6px)' }}>
        {mode === 'before' ? 'WITHOUT LIVEBLOCKER' : 'WITH LIVEBLOCKER · LIVE'}
      </div>
    </div>
  );
}
const ScreenInActionBefore = () => <ScreenInAction mode="before" />;
const ScreenInActionAfter = () => <ScreenInAction mode="after" />;

Object.assign(window, {
  ScreenAppIcon, ScreenMenuBar,
  ScreenFloatingIdle, ScreenFloatingSizing, ScreenFloatingFS,
  ScreenMainSettings, ScreenRegionLibrary, ScreenMLDetector,
  ScreenHUD,
  ScreenOnboarding1, ScreenOnboarding2, ScreenOnboarding3,
  ScreenInActionBefore, ScreenInActionAfter,
});
