// LiveBlocker — entry app. Wires every screen into a design_canvas with
// a Tweaks panel for neumorphism intensity + density. Reads tokens.css
// custom properties and rewrites them on the :root from tweak values.

const TWEAK_DEFAULTS = /*EDITMODE-BEGIN*/{
  "intensity": 5,
  "density": "cozy",
  "accent": "#ff4a2e"
}/*EDITMODE-END*/;

// Map intensity 1..10 → shadow offset & blur on :root.
function applyTweaks(t) {
  const r = document.documentElement.style;
  // intensity: 1 = barely-there, 10 = deep
  const offset = 2 + (t.intensity - 1) * 0.9;   // 2..10.1 px
  const blur = 8 + (t.intensity - 1) * 2.4;     // 8..29.6 px
  r.setProperty('--lb-shadow-offset', `${offset.toFixed(1)}px`);
  r.setProperty('--lb-shadow-blur', `${blur.toFixed(1)}px`);
  // density doesn't affect the inner screens (they are sized for canvas
  // artboards) but does scale the Wordmark/typography slightly via root font.
  const dens = t.density === 'compact' ? 13 : t.density === 'spacious' ? 16 : 14.5;
  r.setProperty('--lb-base-fs', `${dens}px`);
  // accent
  r.setProperty('--lb-block', t.accent);
}

function App() {
  const [t, setTweak] = useTweaks(TWEAK_DEFAULTS);
  React.useEffect(() => { applyTweaks(t); }, [t]);

  return (
    <>
      <DesignCanvas>
        <DCSection id="brand" title="Brand" subtitle="Logo, wordmark, identity">
          <DCArtboard id="logo" label="App icon" width={360} height={360}>
            <ScreenAppIcon />
          </DCArtboard>
          <DCArtboard id="hud" label="Mini HUD · while blocking" width={380} height={140}>
            <ScreenHUD />
          </DCArtboard>
        </DCSection>

        <DCSection id="onboarding" title="Onboarding" subtitle="First-run · welcome → permissions → first block">
          <DCArtboard id="ob1" label="01 · Welcome" width={460} height={540}>
            <ScreenOnboarding1 />
          </DCArtboard>
          <DCArtboard id="ob2" label="02 · Permissions" width={460} height={540}>
            <ScreenOnboarding2 />
          </DCArtboard>
          <DCArtboard id="ob3" label="03 · Make your first block" width={460} height={540}>
            <ScreenOnboarding3 />
          </DCArtboard>
        </DCSection>

        <DCSection id="menubar" title="Menu bar" subtitle="The always-on quick panel that hangs off the macOS menu bar">
          <DCArtboard id="mb" label="Menu bar dropdown" width={360} height={580}>
            <ScreenMenuBar />
          </DCArtboard>
        </DCSection>

        <DCSection id="floating" title="Floating marking tool" subtitle="Resizable bar — Small / Medium / Large / Fullscreen — for marking regions">
          <DCArtboard id="ft1" label="Idle · awaiting drag" width={540} height={380}>
            <ScreenFloatingIdle />
          </DCArtboard>
          <DCArtboard id="ft2" label="Sizing · drag handles" width={540} height={380}>
            <ScreenFloatingSizing />
          </DCArtboard>
          <DCArtboard id="ft3" label="Fullscreen · whole display" width={540} height={380}>
            <ScreenFloatingFS />
          </DCArtboard>
        </DCSection>

        <DCSection id="settings" title="Main window" subtitle="Preferences · Region library · ML detector">
          <DCArtboard id="set" label="Blocking · overview" width={920} height={620}>
            <ScreenMainSettings />
          </DCArtboard>
          <DCArtboard id="reg" label="Region library" width={920} height={620}>
            <ScreenRegionLibrary />
          </DCArtboard>
          <DCArtboard id="ml" label="ML detector tuning" width={920} height={620}>
            <ScreenMLDetector />
          </DCArtboard>
        </DCSection>

        <DCSection id="action" title="In action" subtitle="Before / after — the edge-extrapolated fill replacing an ad slot in real time">
          <DCArtboard id="bef" label="Without LiveBlocker" width={720} height={460}>
            <ScreenInActionBefore />
          </DCArtboard>
          <DCArtboard id="aft" label="With LiveBlocker · live" width={720} height={460}>
            <ScreenInActionAfter />
          </DCArtboard>
        </DCSection>
      </DesignCanvas>

      <TweaksPanel title="Tweaks">
        <TweakSection label="Neumorphism">
          <TweakSlider label="Intensity" value={t.intensity} min={1} max={10} step={1}
            onChange={(v) => setTweak('intensity', v)} />
        </TweakSection>
        <TweakSection label="Density">
          <TweakRadio label="Spacing" value={t.density}
            options={['compact','cozy','spacious']}
            onChange={(v) => setTweak('density', v)} />
        </TweakSection>
        <TweakSection label="Accent">
          <TweakColor label="Block color" value={t.accent}
            options={['#ff4a2e','#ff2e6e','#7c5cff','#0a84ff','#1ec860']}
            onChange={(v) => setTweak('accent', v)} />
        </TweakSection>
      </TweaksPanel>
    </>
  );
}

ReactDOM.createRoot(document.getElementById('root')).render(<App />);
