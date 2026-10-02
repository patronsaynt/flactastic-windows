import { motion } from "motion/react";
import { AArrowDown, AArrowUp, Folder, FolderOpen, Speaker, X } from "lucide-react";
import { useEffect, useState } from "react";
import { useLibrary } from "../../app/library";
import { useDebug, useDownloads } from "../../app/downloads";
import { useSyncSession } from "../../app/sync";
import { SyncContent } from "../sync/SyncContent";
import { openSyncWindow } from "../sync/SyncWindow";
import { setSetting, useSettingsStore } from "../../app/settings";
import { api, on, type OutputStatus } from "../../lib/api";
import { platform } from "../../lib/native";
import { kilohertzString } from "../../lib/format";
import {
  GroupDivider,
  MenuPicker,
  PickerRow,
  PillButton,
  RowLabel,
  SettingsGroup,
  Slider,
  ToggleRow,
  type PickerOption,
} from "../../components/settings/Primitives";
import { chooseLibraryFolder } from "../home/HomeView";
import { openOnboardingPreview } from "../onboarding/OnboardingView";
import wordmark from "../../assets/Wordmark.png";
import "./SettingsView.css";

type Tab = "General" | "Audio" | "Connections" | "Devices" | "Appearance" | "Visualizer" | "Debug";
const tabs: Tab[] = ["General", "Audio", "Connections", "Devices", "Appearance", "Visualizer", "Debug"];

/** Reads a settings key with a fallback; writes through `setSetting`. */
function useKey<T>(key: string, fallback: T): [T, (v: T) => void] {
  const v = useSettingsStore((s) => s.raw[key]) as T | undefined;
  return [v ?? fallback, (nv: T) => setSetting(key, nv)];
}

/** `SettingsView`: 700×560 sheet, wordmark header, centred tab bar. */
export function SettingsView({ onClose }: { onClose: () => void }) {
  const [tab, setTab] = useState<Tab>("General");
  return (
    <div className="settings">
      <div className="settings__header">
        <div className="wordmark" style={{ height: 16, maskImage: `url(${wordmark})`, WebkitMaskImage: `url(${wordmark})` }} />
        <button className="settings__close" onClick={onClose}>
          <X size={14} strokeWidth={2} />
        </button>
      </div>
      <div className="settings__divider" />
      <div className="settings__tabs">
        {tabs.map((t) => (
          <button key={t} className={"settings__tab" + (t === tab ? " is-active" : "")} onClick={() => setTab(t)}>
            {t === tab && <motion.span layoutId="settings-tab" className="settings__tab-bg" transition={{ duration: 0.2, ease: "easeInOut" }} />}
            <span className="settings__tab-label">{t}</span>
          </button>
        ))}
      </div>
      <div className="settings__divider" />
      <div className="settings__content">
        {tab === "General" && <GeneralPane />}
        {tab === "Audio" && <AudioPane />}
        {tab === "Connections" && <ConnectionsPane />}
        {tab === "Devices" && <DevicesPane />}
        {tab === "Appearance" && <AppearancePane />}
        {tab === "Visualizer" && <VisualizerPane />}
        {tab === "Debug" && <DebugPane />}
      </div>
    </div>
  );
}

// MARK: - General

function GeneralPane() {
  const root = useLibrary((s) => s.root);
  const [showDownload, setShowDownload] = useKey("flactastic.showDownloadTab", false);
  const [menuBar, setMenuBar] = useKey("flactastic.showMenuBarPlayer", true);
  const [artistImages, setArtistImages] = useKey("flactastic.autoFetchArtistImages", true);
  const [vpn, setVpn] = useKey("flactastic.showVpnNotice", true);
  const debugMode = useDebug((s) => s.lucidaDebugEnabled);
  return (
    <div className="pane">
      <SettingsGroup title="Library">
        <div className="settings-row">
          {root ? <Folder size={14} fill="currentColor" className="dim" /> : <FolderOpen size={14} className="dim" />}
          <span className={"library-path" + (root ? "" : " is-empty")} title={root ?? undefined}>
            {root ?? "No folder selected"}
          </span>
          <PillButton onClick={() => void chooseLibraryFolder()}>Choose Folder…</PillButton>
        </div>
      </SettingsGroup>
      <SettingsGroup title="Navigation">
        <ToggleRow label="Show Download Tab" subtitle="Show the Download tab in the top navigation bar." on={showDownload} onChange={setShowDownload} />
      </SettingsGroup>
      <SettingsGroup title="Playback">
        <ToggleRow
          label={platform === "windows" ? "Tray Mini-Player" : "Tray Mini-Player"}
          subtitle="Show a compact player in the system tray."
          on={menuBar}
          onChange={setMenuBar}
        />
        <GroupDivider />
        <ToggleRow
          label="Auto-Fetch Artist Images"
          subtitle="Automatically download artist artwork from Deezer when missing."
          on={artistImages}
          onChange={setArtistImages}
        />
      </SettingsGroup>
      <SettingsGroup title="Statistics">
        <CountedPlayThreshold />
      </SettingsGroup>
      {debugMode && (
        <SettingsGroup title="Debug">
          <ToggleRow
            label="Show VPN Advisory"
            subtitle="Show a reminder to use a VPN when opening the Downloads tab."
            on={vpn}
            onChange={setVpn}
          />
        </SettingsGroup>
      )}
    </div>
  );
}

function CountedPlayThreshold() {
  const [fraction, setFraction] = useKey("flactastic.countedPlayFraction", 0.9);
  const percent = Math.round(fraction * 100);
  const [text, setText] = useState(String(percent));
  useEffect(() => setText(String(percent)), [percent]);
  const commit = () => {
    const digits = text.replace(/\D/g, "");
    const v = digits ? parseInt(digits, 10) : percent;
    const c = Math.min(100, Math.max(0, v));
    setFraction(c / 100);
    setText(String(c));
  };
  return (
    <div className="settings-row" style={{ flexDirection: "column", alignItems: "stretch", gap: 8 }}>
      <div style={{ display: "flex", alignItems: "flex-start", gap: 24 }}>
        <RowLabel
          label="Counted Play Threshold"
          subtitle="Percentage of a track that must play straight through before it counts as a single play. Higher values are stricter; scrubbing or skipping never counts."
        />
        <div className="percent-field">
          <input
            className="text-field tabular"
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && commit()}
            onBlur={commit}
          />
          <span>%</span>
        </div>
      </div>
      <Slider value={fraction} step={0.01} onChange={setFraction} />
    </div>
  );
}

// MARK: - Audio

function AudioPane() {
  const [status, setStatus] = useState<OutputStatus | null>(null);
  useEffect(() => {
    void api.outputStatus().then((s) => s && setStatus(s));
    return on<OutputStatus>("output://status", setStatus);
  }, []);
  if (!status) return <div className="pane dim">Reading audio devices…</div>;

  const defaultDevice = status.devices.find((d) => d.id === status.defaultDeviceId);
  const effective = status.devices.find((d) => d.id === status.effectiveDeviceId);
  const systemDefaultLabel =
    defaultDevice && defaultDevice.id !== "default" ? `System Default (${defaultDevice.name})` : defaultDevice?.name ?? "System Default";

  const deviceOptions: PickerOption<string>[] = [
    { value: "", label: systemDefaultLabel, divider: true },
    ...status.devices.filter((d) => d.id !== "default").map((d) => ({ value: d.id, label: d.name })),
  ];
  if (status.isSelectedDeviceMissing && status.selectedDeviceId) {
    deviceOptions.push({ value: status.selectedDeviceId, label: "Disconnected Device" });
  }

  const rates = [...status.availableSampleRates];
  if (status.selectedSampleRate != null && !rates.includes(status.selectedSampleRate)) rates.push(status.selectedSampleRate);
  rates.sort((a, b) => a - b);
  const depths = [...status.availableBitDepths];
  if (status.selectedBitDepth != null && !depths.includes(status.selectedBitDepth)) depths.push(status.selectedBitDepth);
  depths.sort((a, b) => a - b);

  const deviceDefault = (cur: string | null) => (cur ? `Device Default (currently ${cur})` : "Device Default");
  const parts: string[] = [];
  const shownRate = status.exclusive ? status.streamSampleRate : status.currentSampleRate;
  if (shownRate != null) parts.push(kilohertzString(shownRate));
  if (status.currentBitDepth != null) parts.push(`${status.currentBitDepth}-bit`);
  const name = effective?.name ?? "No output device";
  const nowOutputting = parts.length ? `${parts.join(" · ")} → ${name}` : name;

  return (
    <div className="pane">
      <SettingsGroup title="Output Device">
        <PickerRow
          label="Device"
          subtitle={
            status.isSelectedDeviceMissing
              ? "The selected device is disconnected. Using the system default until it's reconnected."
              : undefined
          }
        >
          <MenuPicker value={status.selectedDeviceId ?? ""} options={deviceOptions} onChange={(v) => void api.selectOutputDevice(v || null)} />
        </PickerRow>
      </SettingsGroup>

      <SettingsGroup title="Format">
        <PickerRow
          label="Sample Rate"
          subtitle="Every track is resampled to this rate so playback stays gapless. Changing output settings briefly restarts the current track at the same position."
        >
          <MenuPicker
            value={status.selectedSampleRate ?? 0}
            options={[
              { value: 0, label: deviceDefault(status.currentSampleRate != null ? kilohertzString(status.currentSampleRate) : null), divider: true },
              ...rates.map((r) => ({ value: r, label: kilohertzString(r) })),
            ]}
            onChange={(v) => void api.selectOutputSampleRate(v === 0 ? null : v)}
          />
        </PickerRow>
        <GroupDivider />
        <PickerRow label="Bit Depth">
          <MenuPicker
            value={status.selectedBitDepth ?? 0}
            disabled={status.availableBitDepths.length === 0}
            options={[
              { value: 0, label: deviceDefault(status.currentBitDepth != null ? `${status.currentBitDepth}-bit` : null), divider: true },
              ...depths.map((b) => ({ value: b, label: `${b}-bit` })),
            ]}
            onChange={(v) => void api.selectOutputBitDepth(v === 0 ? null : v)}
          />
        </PickerRow>
        {platform === "windows" && (
          <>
            <GroupDivider />
            <ToggleRow
              label="Exclusive Mode"
              subtitle="Take sole control of the device while FLACtastic plays: the sample rate and bit depth above go straight to the DAC, and other apps are silent until you pause."
              on={status.exclusive}
              onChange={(v) => void api.setExclusiveOutput(v)}
            />
          </>
        )}
      </SettingsGroup>

      <SettingsGroup title="Now Outputting">
        <div className="settings-row">
          <Speaker size={14} className="dim" />
          <span className="tabular" style={{ color: "var(--text-secondary)", flex: 1 }}>
            {nowOutputting}
          </span>
        </div>
      </SettingsGroup>
    </div>
  );
}

// MARK: - Connections

function ConnectionsPane() {
  const [discord, setDiscord] = useKey("flactastic.discordRichPresenceEnabled", true);
  const [liked, setLiked] = useKey("flactastic.showSpotifyLikedSongs", true);
  const connection = useDownloads((s) => s.spotify.connection);
  return (
    <div className="pane">
      <SettingsGroup title="Social">
        <ToggleRow label="Discord Rich Presence" subtitle="Show the currently playing track in your Discord status." on={discord} onChange={setDiscord} />
      </SettingsGroup>
      <SettingsGroup title="Spotify Account">
        <div className="settings-row" style={{ flexDirection: "column", alignItems: "stretch", gap: 12 }}>
          <div className="settings-row__subtitle">
            Connect your Spotify account to browse and download your own playlists, including private ones, in full.
          </div>
          <div className="spotify-account">
            {connection.kind === "disconnected" && (
              <PillButton primary onClick={() => void api.spotifyConnect().catch(() => {})}>
                Connect Spotify
              </PillButton>
            )}
            {connection.kind === "connecting" && (
              <>
                <span className="dl-spinner is-small" />
                <span className="spotify-account__status">Connecting…</span>
                <span className="spotify-account__spacer" />
                <PillButton muted onClick={() => void api.spotifyCancelConnect()}>
                  Cancel
                </PillButton>
              </>
            )}
            {connection.kind === "connected" && (
              <>
                <span className="spotify-account__dot" />
                <span className="spotify-account__status">
                  Connected as <b>{connection.displayName}</b>
                </span>
                <span className="spotify-account__spacer" />
                <PillButton onClick={() => void api.spotifyDisconnect()}>Disconnect</PillButton>
              </>
            )}
          </div>
        </div>
        <ToggleRow
          label="Show Liked Songs"
          subtitle="Show a Liked Songs entry alongside your playlists on the Playlists download screen."
          on={liked}
          onChange={setLiked}
        />
      </SettingsGroup>
    </div>
  );
}

// MARK: - Devices (library sync)

function DevicesPane() {
  useSyncSession();
  return (
    <div className="pane">
      <SyncContent />
      <div className="sync-devices-footer">
        <span className="sync-caption">Devices are only discoverable while this tab or the Sync window is open.</span>
        <PillButton onClick={openSyncWindow}>Open Sync Window</PillButton>
      </div>
    </div>
  );
}

// MARK: - Appearance

function AppearancePane() {
  const [light, setLight] = useKey("flactastic.useLightMode", false);
  const [group, setGroup] = useKey("flactastic.groupByArtist", false);
  const [rounded, setRounded] = useKey("flactastic.roundedArtwork", true);
  const [shadow, setShadow] = useKey("flactastic.showArtworkShadow", true);
  const [fade, setFade] = useKey("flactastic.fadeAnimationsEnabled", true);
  const [dir, setDir] = useKey("flactastic.fadeAnimationDirection", "up");
  const [scale, setScale] = useKey("flactastic.uiScale", 1);
  return (
    <div className="pane">
      <SettingsGroup title="Theme">
        <ToggleRow label="Light Mode" subtitle="Switch to a light background throughout the app." on={light} onChange={setLight} />
      </SettingsGroup>
      <SettingsGroup title="Library View">
        <ToggleRow label="Group Albums by Artist" subtitle="In grid view, cluster albums under their artist." on={group} onChange={setGroup} />
      </SettingsGroup>
      <SettingsGroup title="Artwork">
        <ToggleRow label="Rounded Album Art" on={rounded} onChange={setRounded} />
        <GroupDivider />
        <ToggleRow label="Drop Shadow" on={shadow} onChange={setShadow} />
      </SettingsGroup>
      <SettingsGroup title="Animations">
        <ToggleRow label="Fade Animations" on={fade} onChange={setFade} />
        <GroupDivider />
        <div className="settings-row" style={{ opacity: fade ? 1 : 0.4 }}>
          <RowLabel label="Fade Direction" dim={!fade} />
          <MenuPicker
            value={dir}
            disabled={!fade}
            options={[
              { value: "up", label: "Upward" },
              { value: "leftToRight", label: "Left to Right" },
              { value: "rightToLeft", label: "Right to Left" },
            ]}
            onChange={setDir}
          />
        </div>
      </SettingsGroup>
      <SettingsGroup title="Interface">
        <div className="settings-row" style={{ flexDirection: "column", alignItems: "stretch", gap: 8 }}>
          <div style={{ display: "flex", alignItems: "center" }}>
            <RowLabel label="UI Scale" subtitle="Scales text and controls throughout the app." />
            <span className="dim tabular" style={{ fontSize: "var(--font-caption)" }}>
              {Math.round(scale * 100)}%
            </span>
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <AArrowDown size={15} className="dim" />
            <Slider value={scale} min={0.9} max={1.35} step={0.05} onChange={setScale} />
            <AArrowUp size={15} className="dim" />
            <PillButton small onClick={() => setScale(1)} disabled={Math.abs(scale - 1) < 0.001}>
              Reset
            </PillButton>
          </div>
        </div>
      </SettingsGroup>
    </div>
  );
}

// MARK: - Visualizer

function VisualizerPane() {
  const [lookup, setLookup] = useKey("flactastic.lyricsLookupEnabled", true);
  const [save, setSave] = useKey("flactastic.saveLyricsToFiles", true);
  return (
    <div className="pane">
      <SettingsGroup title="Lyrics">
        <ToggleRow
          label="Fetch Lyrics from lrclib.net"
          subtitle="Enables the Lyrics visualizer mode. Lookups are cached on disk; disable to stay fully offline."
          on={lookup}
          onChange={setLookup}
        />
        <GroupDivider />
        <ToggleRow
          label="Save Lyrics to Audio Files"
          subtitle="Embed fetched lyrics into the LYRICS tag on each file (Vorbis, ID3v2 USLT, MP4). Other players will pick them up automatically."
          on={save}
          onChange={setSave}
          enabled={lookup}
        />
      </SettingsGroup>
    </div>
  );
}

// MARK: - Debug

function DebugPane() {
  const lucidaDebug = useDebug((s) => s.lucidaDebugEnabled);
  return (
    <div className="pane">
      <SettingsGroup title="Developer Tools">
        <ToggleRow
          label="Debug Lucida"
          subtitle="Open the Lucida bridge inspector — live phase, web view, and navigation/bridge log."
          on={lucidaDebug}
          onChange={(v) => {
            useDebug.setState({ lucidaDebugEnabled: v });
            if (v) void api.lucidaWarmUp();
          }}
        />
        <GroupDivider />
        <div className="settings-row is-top">
          <RowLabel
            label="Debug Onboarding"
            subtitle="Replay the first-run onboarding sequence in its own window, against the app's live settings and library."
          />
          <PillButton onClick={openOnboardingPreview}>Preview</PillButton>
        </div>
      </SettingsGroup>
    </div>
  );
}
