// LiveBlocker v3 — brutalist terminal. Cross-platform.
// Wires every screen into a design canvas, with a Tweaks panel for
// accent color, density, and chrome variant.

const TWEAK_DEFAULTS = /*EDITMODE-BEGIN*/{
  "accent": "#ff3b1f",
  "density": "regular",
  "scanlines": true,
  "showMode": "all"
}/*EDITMODE-END*/;

function applyTweaks(t) {
  const r = document.documentElement.style;
  r.setProperty('--lb-block', t.accent);
  const fs = t.density === 'compact' ? 12 : t.density === 'spacious' ? 14 : 13;
  r.setProperty('--lb-base-fs', `${fs}px`);
  // scanlines toggle on body
  document.body.classList.toggle('lb-scan', !!t.scanlines);
}

function App() {
  const [t, setTweak] = useTweaks(TWEAK_DEFAULTS);
  React.useEffect(() => { applyTweaks(t); }, [t]);

  return (
    <>
      <DesignCanvas>
        <DCSection id="cross-os" title="Cross-platform proof" subtitle="The same app, identical at home on macOS, Windows, and Linux. No platform-specific UI.">
          <DCArtboard id="os-row" label="One UI · three operating systems" width={720} height={340}>
            <ScreenCrossOS />
          </DCArtboard>
          <DCArtboard id="brand" label="App identity" width={360} height={360}>
            <ScreenAppIcon />
          </DCArtboard>
          <DCArtboard id="hud" label="Mini HUD · always-on edge chip" width={380} height={140}>
            <ScreenHUD />
          </DCArtboard>
        </DCSection>

        <DCSection id="onboarding" title="Onboarding" subtitle="Three-step setup. Welcome · permissions · set-and-forget pitch.">
          <DCArtboard id="ob1" label="01 · Welcome" width={460} height={540}>
            <ScreenOnboarding1 />
          </DCArtboard>
          <DCArtboard id="ob2" label="02 · Permissions" width={460} height={540}>
            <ScreenOnboarding2 />
          </DCArtboard>
          <DCArtboard id="ob3" label="03 · Set & forget" width={460} height={540}>
            <ScreenOnboarding3 />
          </DCArtboard>
        </DCSection>

        <DCSection id="floating" title="Floating marking tool — the painful part, rebuilt"
          subtitle="Two answers to one problem: SMART (hover-to-snap, no drag) and CLASSIC (refined drag-rectangle). Plus a new always-on edge chip so the tool is never lost.">
          <DCArtboard id="summon" label="Summon — always-visible edge chip + one global hotkey" width={540} height={380}>
            <ScreenSummonHint />
          </DCArtboard>
          <DCArtboard id="idle" label="Idle — scanning, before you do anything" width={540} height={380}>
            <ScreenFloatingIdle />
          </DCArtboard>
          <DCArtboard id="smart" label="v3·A · SMART · hover-to-snap" width={540} height={380}>
            <ScreenFloatingSmart />
          </DCArtboard>
          <DCArtboard id="classic" label="v3·B · CLASSIC · refined drag" width={540} height={380}>
            <ScreenFloatingClassic />
          </DCArtboard>
        </DCSection>

        <DCSection id="tray" title="Tray / quick panel"
          subtitle="One click from the system tray (Windows/Linux) or menu bar (macOS).">
          <DCArtboard id="tray-card" label="Tray dropdown" width={360} height={620}>
            <ScreenTray />
          </DCArtboard>
        </DCSection>

        <DCSection id="settings" title="Main settings window"
          subtitle="Set-and-forget overview. The whole story on one page.">
          <DCArtboard id="overview" label="Overview · the set-it-and-forget-it page" width={920} height={620}>
            <ScreenMainSettings />
          </DCArtboard>
        </DCSection>

        <DCSection id="ml" title="ML detector"
          subtitle="Where you tune the auto-detector. Higher threshold = fewer false positives.">
          <DCArtboard id="ml-tune" label="Detector tuning · live preview" width={920} height={620}>
            <ScreenMLDetector />
          </DCArtboard>
        </DCSection>

        <DCSection id="library" title="Region library"
          subtitle="Everything you (and the ML) have ever marked.">
          <DCArtboard id="lib" label="Library · 14 saved · 7 active · 11 ML-learned" width={920} height={620}>
            <ScreenRegionLibrary />
          </DCArtboard>
        </DCSection>

        <DCSection id="action" title="In action — before / after"
          subtitle="Same page; same load; same screen. One has had its ad slot edge-extrapolated into the surrounding pixels.">
          <DCArtboard id="before" label="Without LiveBlocker" width={720} height={460}>
            <ScreenInActionBefore />
          </DCArtboard>
          <DCArtboard id="after" label="With LiveBlocker · live" width={720} height={460}>
            <ScreenInActionAfter />
          </DCArtboard>
        </DCSection>
      </DesignCanvas>

      <TweaksPanel title="Tweaks">
        <TweakSection label="Accent">
          <TweakColor label="Kill color" value={t.accent}
            options={['#ff3b1f','#22d3ee','#a78bfa','#fbbf24','#4ade80']}
            onChange={(v) => setTweak('accent', v)} />
        </TweakSection>
        <TweakSection label="Density">
          <TweakRadio label="Spacing" value={t.density}
            options={['compact','regular','spacious']}
            onChange={(v) => setTweak('density', v)} />
        </TweakSection>
        <TweakSection label="CRT vibe">
          <TweakToggle label="Scanlines on HUDs" value={t.scanlines}
            onChange={(v) => setTweak('scanlines', v)} />
        </TweakSection>
      </TweaksPanel>
    </>
  );
}

ReactDOM.createRoot(document.getElementById('root')).render(<App />);
