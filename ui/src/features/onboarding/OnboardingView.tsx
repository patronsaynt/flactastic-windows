import { AnimatePresence, motion } from "motion/react";
import { Check, CheckCircle2, ChevronLeft, FolderPlus, ListMusic } from "lucide-react";
import { useState, type ReactNode } from "react";
import { setSetting, useSetting, useSettingsStore } from "../../app/settings";
import { api } from "../../lib/api";
import wordmark from "../../assets/Wordmark.png";
import { RiseFadeIn } from "../../components/RiseFadeIn";
import { PillButton } from "../../components/settings/Primitives";
import { WindowControls } from "../../components/shell/WindowControls";
import { alertDialog } from "../../components/sheet/ConfirmDialog";
import { chooseLibraryFolder } from "../home/HomeView";
import "./Onboarding.css";

const STEPS = ["welcome", "appearance", "library", "spotify", "done"] as const;
type Step = (typeof STEPS)[number];

/**
 * `OnboardingView`: welcome, appearance, library folder, optional Spotify,
 * then a summary. Replaces the whole window until finished.
 */
export function OnboardingView() {
  const [step, setStep] = useState<Step>("welcome");
  const [dir, setDir] = useState(1);
  const i = STEPS.indexOf(step);
  const advance = () => {
    setDir(1);
    setStep(STEPS[Math.min(i + 1, STEPS.length - 1)]);
  };
  const back = () => {
    setDir(-1);
    setStep(STEPS[Math.max(i - 1, 0)]);
  };
  const finish = () => setSetting("flactastic.hasCompletedOnboarding", true);

  return (
    <div className="onboarding">
      <div className="onboarding__titlebar" data-tauri-drag-region>
        <WindowControls />
      </div>
      <div className="onboarding__stage">
        <div className="onboarding-card">
          {step !== "welcome" && (
            <button className="detail-back-button onboarding__back" onClick={back} title="Back">
              <ChevronLeft size={15} strokeWidth={2.6} />
            </button>
          )}
          <div className="onboarding__wordmark" style={{ maskImage: `url(${wordmark})`, WebkitMaskImage: `url(${wordmark})` }} />
          <AnimatePresence mode="popLayout" initial={false} custom={dir}>
            <motion.div
              key={step}
              custom={dir}
              variants={{
                enter: (d: number) => ({ x: d * 60, opacity: 0 }),
                center: { x: 0, opacity: 1 },
                exit: (d: number) => ({ x: d * -60, opacity: 0 }),
              }}
              initial="enter"
              animate="center"
              exit="exit"
              transition={{ duration: 0.28, ease: "easeInOut" }}
            >
              {step === "welcome" && <WelcomePage onNext={advance} />}
              {step === "appearance" && <AppearancePage onNext={advance} />}
              {step === "library" && <LibraryPage onNext={advance} />}
              {step === "spotify" && <SpotifyPage onNext={advance} />}
              {step === "done" && <DonePage onFinish={finish} />}
            </motion.div>
          </AnimatePresence>
        </div>
      </div>
    </div>
  );
}

/** `OnboardingStepHeader`: "STEP N OF 3 · BADGE", title, subtitle. */
function StepHeader({ step, badge, title, subtitle }: { step: number; badge?: string; title: string; subtitle: string }) {
  return (
    <div className="step-header">
      <div className="step-header__eyebrow">
        STEP {step} OF 3{badge ? ` · ${badge.toUpperCase()}` : ""}
      </div>
      <div className="step-header__title">{title}</div>
      <div className="step-header__subtitle">{subtitle}</div>
    </div>
  );
}

function Page({ gap, pad, children }: { gap: string; pad: string; children: ReactNode }) {
  return (
    <div className="onboarding-page" style={{ gap, padding: `${pad} 0` }}>
      {children}
    </div>
  );
}

function WelcomePage({ onNext }: { onNext: () => void }) {
  return (
    <Page gap="var(--space-lg)" pad="var(--space-xl)">
      <RiseFadeIn delay={0}>
        <div className="step-header__title">Welcome to FLACtastic</div>
      </RiseFadeIn>
      <RiseFadeIn delay={0.06}>
        <div className="onboarding__body" style={{ maxWidth: 380 }}>
          Let's get your lossless library set up. It only takes a moment.
        </div>
      </RiseFadeIn>
      <RiseFadeIn delay={0.12} style={{ paddingTop: "var(--space-sm)" }}>
        <PillButton primary onClick={onNext}>
          Get Started
        </PillButton>
      </RiseFadeIn>
    </Page>
  );
}

function AppearancePage({ onNext }: { onNext: () => void }) {
  const light = useSetting("flactastic.useLightMode");
  return (
    <Page gap="var(--space-xl)" pad="var(--space-sm)">
      <RiseFadeIn delay={0}>
        <StepHeader step={1} title="Choose your look" subtitle="You can change this later in Settings." />
      </RiseFadeIn>
      <RiseFadeIn delay={0.08} className="appearance-tiles">
        <AppearanceTile title="Dark" isLight={false} selected={!light} onTap={() => setSetting("flactastic.useLightMode", false)} />
        <AppearanceTile title="Light" isLight selected={light} onTap={() => setSetting("flactastic.useLightMode", true)} />
      </RiseFadeIn>
      <RiseFadeIn delay={0.14}>
        <PillButton primary onClick={onNext}>
          Continue
        </PillButton>
      </RiseFadeIn>
    </Page>
  );
}

function AppearanceTile({ title, isLight, selected, onTap }: { title: string; isLight: boolean; selected: boolean; onTap: () => void }) {
  return (
    <button className={"appearance-tile" + (selected ? " is-selected" : "")} onClick={onTap}>
      <span className={"appearance-tile__swatch" + (isLight ? " is-light" : "")}>
        <span />
        <span />
      </span>
      <span className="appearance-tile__row">
        <span className="appearance-tile__title">{title}</span>
        <span className={"selection-mark" + (selected ? " is-on" : "")}>{selected && <Check size={9} strokeWidth={4} />}</span>
      </span>
    </button>
  );
}

function LibraryPage({ onNext }: { onNext: () => void }) {
  const path = useSettingsStore((s) => (s.raw["flactastic.lastRootPath"] as string | undefined) ?? null);

  const createEmpty = async () => {
    try {
      const dir = await api.createDefaultMusicFolder();
      if (dir) await api.openLibrary(dir);
    } catch (e) {
      alertDialog("Couldn't Create Folder", String(e));
    }
  };

  return (
    <Page gap="var(--space-lg)" pad="var(--space-sm)">
      <RiseFadeIn delay={0}>
        <StepHeader
          step={2}
          title="Point us to your library"
          subtitle="Choose the folder where your FLAC files live. We'll watch it for changes."
        />
      </RiseFadeIn>
      <RiseFadeIn delay={0.06} style={{ alignSelf: "stretch" }}>
        {path ? (
          <div className="linked-card">
            <span className="linked-card__badge">
              <CheckCircle2 size={16} fill="currentColor" color="var(--surface)" />
            </span>
            <span className="linked-card__text">
              <span className="linked-card__title">Library linked</span>
              <span className="linked-card__path" title={path}>
                {path}
              </span>
            </span>
            <button className="linked-card__change" onClick={() => void chooseLibraryFolder()}>
              Change
            </button>
          </div>
        ) : (
          <button className="folder-dropzone" onClick={() => void chooseLibraryFolder()}>
            <FolderPlus size={26} strokeWidth={1.5} />
            <span className="folder-dropzone__title">Choose Folder…</span>
            <span className="folder-dropzone__hint">or drag a folder here</span>
          </button>
        )}
      </RiseFadeIn>
      {!path && (
        <RiseFadeIn delay={0.1}>
          <button className="onboarding__link" onClick={() => void createEmpty()}>
            Create an empty folder for me
          </button>
        </RiseFadeIn>
      )}
      <RiseFadeIn delay={0.14}>
        <span style={{ opacity: path ? 1 : 0.4, display: "inline-block" }}>
          <PillButton primary onClick={onNext} disabled={!path}>
            Continue
          </PillButton>
        </span>
      </RiseFadeIn>
    </Page>
  );
}

/**
 * Spotify connect is part of the Download/Connections work that hasn't
 * landed on this platform yet, so the row stays "Not connected" and the
 * connect button is disabled; Skip continues as on the Mac.
 */
function SpotifyPage({ onNext }: { onNext: () => void }) {
  return (
    <Page gap="var(--space-lg)" pad="var(--space-sm)">
      <RiseFadeIn delay={0}>
        <StepHeader
          step={3}
          badge="Optional"
          title="Bring in your playlists"
          subtitle="Connect Spotify to match your playlists against your lossless library. You can skip this and add it anytime."
        />
      </RiseFadeIn>
      <RiseFadeIn delay={0.06} style={{ alignSelf: "stretch" }}>
        <div className="service-row">
          <span className="service-row__icon">
            <ListMusic size={16} />
          </span>
          <span className="linked-card__text">
            <span className="linked-card__title">Spotify</span>
            <span className="onboarding__caption">Not connected</span>
          </span>
        </div>
      </RiseFadeIn>
      <RiseFadeIn delay={0.1} className="onboarding__actions">
        <button className="onboarding__skip" onClick={onNext}>
          Skip for now
        </button>
        <PillButton primary onClick={() => {}} disabled>
          Connect Spotify
        </PillButton>
      </RiseFadeIn>
    </Page>
  );
}

function DonePage({ onFinish }: { onFinish: () => void }) {
  const light = useSetting("flactastic.useLightMode");
  const path = useSettingsStore((s) => (s.raw["flactastic.lastRootPath"] as string | undefined) ?? null);
  const short = path ? (path.split(/[\\/]/).filter(Boolean).pop() ?? path) : "—";
  return (
    <Page gap="var(--space-lg)" pad="var(--space-sm)">
      <RiseFadeIn delay={0}>
        <span className="done-badge">
          <Check size={22} strokeWidth={3.5} />
        </span>
      </RiseFadeIn>
      <RiseFadeIn delay={0.04}>
        <div className="step-header__title">You're all set</div>
      </RiseFadeIn>
      <RiseFadeIn delay={0.08}>
        <div className="onboarding__body" style={{ maxWidth: 360 }}>
          Your library is linked and ready. Time to hear it properly.
        </div>
      </RiseFadeIn>
      <RiseFadeIn delay={0.12} className="summary">
        <SummaryRow label="Appearance" value={light ? "Light" : "Dark"} />
        <SummaryRow label="Library" value={short} />
        <SummaryRow label="Spotify" value="Not connected" />
      </RiseFadeIn>
      <RiseFadeIn delay={0.16} style={{ paddingTop: "var(--space-xs)" }}>
        <PillButton primary onClick={onFinish}>
          Enter FLACtastic
        </PillButton>
      </RiseFadeIn>
    </Page>
  );
}

function SummaryRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="summary__row">
      <span className="summary__label">{label}</span>
      <span className="summary__value">{value}</span>
    </div>
  );
}
