// LiveBlocker v4 — humanized dashboard. App entry.
// Wires every screen into the design canvas; exposes a small Tweaks panel
// for accent colour, density, and the corner radius "softness".

const TWEAK_DEFAULTS = /*EDITMODE-BEGIN*/{
  "theme": "dark",
  "accent": "#ff5039",
  "density": "comfortable",
  "softness": "medium",
  "mono": "JetBrains Mono"
}/*EDITMODE-END*/;

function applyTweaks(t) {
  const r = document.documentElement.style;
  // Theme — flips token overrides defined in tokens.css
  document.documentElement.setAttribute('data-theme', t.theme || 'dark');
  r.setProperty('--accent', t.accent);

  // density → base font size + spacing offsets
  const dens = t.density === 'compact'   ? { fs: 13, sp: 0 }
            : t.density === 'spacious'   ? { fs: 15, sp: 2 }
            :                              { fs: 14, sp: 1 };
  r.setProperty('--base-fs', `${dens.fs}px`);

  // softness → radii
  const soft = t.softness === 'sharp'  ? { r3: 4,  r4: 6,  r5: 8  }
            : t.softness === 'soft'    ? { r3: 12, r4: 16, r5: 22 }
            :                            { r3: 8,  r4: 12, r5: 16 };
  r.setProperty('--r-3', `${soft.r3}px`);
  r.setProperty('--r-4', `${soft.r4}px`);
  r.setProperty('--r-5', `${soft.r5}px`);
}

function App() {
  const [t, setTweak] = useTweaks(TWEAK_DEFAULTS);
  React.useEffect(() => { applyTweaks(t); }, [t]);

  return (
    <>
      <DesignCanvas>
        {/* ── 1. Daily touchpoint: the menu-bar dashboard + HUD + identity ── */}
        <DCSection
          id="daily"
          title="Daily touchpoints"
          subtitle="The screens an average person actually sees. Big numbers, plain language, mono only where data lives."
        >
          <DCArtboard id="dashboard" label="Menu-bar dashboard · the daily quick panel" width={400} height={760}>
            <ScreenDashboard/>
          </DCArtboard>
          <DCArtboard id="hud" label="Always-on HUD · drag-anywhere status pill" width={420} height={170}>
            <ScreenHUD/>
          </DCArtboard>
          <DCArtboard id="brand" label="App identity" width={380} height={380}>
            <ScreenAppIcon/>
          </DCArtboard>
          <DCArtboard id="cross-os" label="One UI · three operating systems" width={740} height={360}>
            <ScreenCrossOS/>
          </DCArtboard>
        </DCSection>

        {/* ── 2. The main app — settings shells ── */}
        <DCSection
          id="main"
          title="The main app"
          subtitle="A calm sidebar layout with a search bar, an engine card at the bottom, and one big content area per page."
        >
          <DCArtboard id="overview" label="Overview · the set-it-and-forget-it page" width={1000} height={680}>
            <ScreenOverview/>
          </DCArtboard>
          <DCArtboard id="detector" label="Smart detection · tune the auto-blocker" width={1000} height={680}>
            <ScreenDetector/>
          </DCArtboard>
          <DCArtboard id="library" label="Region library · everything blocked, ever" width={1000} height={680}>
            <ScreenLibrary/>
          </DCArtboard>
        </DCSection>

        {/* ── 3. Onboarding ── */}
        <DCSection
          id="onboard"
          title="Onboarding"
          subtitle="Three steps. Welcome · permissions · the &lsquo;you won't really use this app&rsquo; pitch."
        >
          <DCArtboard id="ob1" label="01 · Welcome"      width={480} height={560}><ScreenOnboarding1/></DCArtboard>
          <DCArtboard id="ob2" label="02 · Permissions"  width={480} height={560}><ScreenOnboarding2/></DCArtboard>
          <DCArtboard id="ob3" label="03 · Set & forget" width={480} height={560}><ScreenOnboarding3/></DCArtboard>
        </DCSection>

        {/* ── 4. Floating marking tool ── */}
        <DCSection
          id="floating"
          title="Floating marking tool"
          subtitle="How the marker is summoned, what it shows while scanning, and the two interaction models — Smart (hover-to-snap) and Classic (drag)."
        >
          <DCArtboard id="summon"  label="Summon · always-on edge chip + one chord" width={560} height={400}><ScreenSummonHint/></DCArtboard>
          <DCArtboard id="idle"    label="Scanning · 3 candidates"                  width={560} height={400}><ScreenFloatingIdle/></DCArtboard>
          <DCArtboard id="smart"   label="Smart · hover to snap"                    width={560} height={400}><ScreenFloatingSmart/></DCArtboard>
          <DCArtboard id="classic" label="Classic · drag to mark"                   width={560} height={400}><ScreenFloatingClassic/></DCArtboard>
        </DCSection>

        {/* ── 5. In action — before / after ── */}
        <DCSection
          id="action"
          title="In action"
          subtitle="Same page, same load, same screen. One has had its ad slot painted out by LiveBlocker."
        >
          <DCArtboard id="before" label="Without LiveBlocker"  width={740} height={480}><ScreenInActionBefore/></DCArtboard>
          <DCArtboard id="after"  label="With LiveBlocker"     width={740} height={480}><ScreenInActionAfter/></DCArtboard>
        </DCSection>
      </DesignCanvas>

      <TweaksPanel title="Tweaks">
        <TweakSection label="Theme">
          <TweakRadio
            label="Mode" value={t.theme}
            options={[
              { value:'dark',  label:'Dark'  },
              { value:'light', label:'Light' },
            ]}
            onChange={(v) => setTweak('theme', v)}
          />
        </TweakSection>
        <TweakSection label="Accent">
          <TweakColor
            label="Block colour" value={t.accent}
            options={['#ff5039','#3b82f6','#a78bfa','#10b981','#f59e0b','#ec4899']}
            onChange={(v) => setTweak('accent', v)}
          />
        </TweakSection>
        <TweakSection label="Density">
          <TweakRadio
            label="Spacing" value={t.density}
            options={[
              { value:'compact',     label:'Compact' },
              { value:'comfortable', label:'Comfort' },
              { value:'spacious',    label:'Roomy'   },
            ]}
            onChange={(v) => setTweak('density', v)}
          />
        </TweakSection>
        <TweakSection label="Corner softness">
          <TweakRadio
            label="Radii" value={t.softness}
            options={[
              { value:'sharp',  label:'Sharp'  },
              { value:'medium', label:'Medium' },
              { value:'soft',   label:'Soft'   },
            ]}
            onChange={(v) => setTweak('softness', v)}
          />
        </TweakSection>
      </TweaksPanel>
    </>
  );
}

ReactDOM.createRoot(document.getElementById('root')).render(<App/>);
