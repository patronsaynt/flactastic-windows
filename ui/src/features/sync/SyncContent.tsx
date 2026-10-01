import { AlertTriangle, ChevronRight, Info, Laptop, Monitor, MoreHorizontal, Smartphone, Tablet, X } from "lucide-react";
import { useMemo, useState } from "react";
import { byteCount, lastSyncedDescription, PickState, useSync, type Mark } from "../../app/sync";
import { PillToggle } from "../../components/chrome/Chrome";
import { openMenuAt } from "../../components/menu/ContextMenu";
import { confirmDialog } from "../../components/sheet/ConfirmDialog";
import { FLSheet, Modal } from "../../components/sheet/Sheet";
import { GroupDivider, PillButton, RowLabel, SettingsGroup, Switch } from "../../components/settings/Primitives";
import { api, type SyncDeviceKind, type SyncPeerRow, type SyncPhase, type SyncState, type SyncSummary } from "../../lib/api";
import "./Sync.css";

/** Same word the Mac uses for "this device". */
const THIS_DEVICE = "this computer";

/**
 * `SyncContent`: notices, devices, direction, pairing and progress — shared
 * by the Sync sheet and Settings ▸ Devices.
 */
export function SyncContent() {
  const state = useSync((s) => s.state);
  const [pairingTarget, setPairingTarget] = useState<SyncPeerRow | null>(null);
  if (!state) return <div className="sync-loading">Starting sync…</div>;
  const approval = state.phase.kind === "awaitingApproval" ? state.phase : null;
  return (
    <div className="sync">
      {!state.hasLibrary && <Notice message="Open a music folder before syncing." />}
      {state.listenerError && <Notice message={state.listenerError} />}
      {state.errorMessage && <Notice message={state.errorMessage} isError onDismiss={() => void api.syncDismissError()} />}
      <Devices state={state} onPair={setPairingTarget} />
      <DirectionGroup state={state} />
      <PairingGroup state={state} />
      {state.phase.kind !== "idle" && state.phase.kind !== "awaitingApproval" && <StatusGroup phase={state.phase} />}

      <Modal open={pairingTarget != null} onClose={() => setPairingTarget(null)}>
        {pairingTarget && (
          <PairingCodeEntrySheet
            peer={pairingTarget}
            onSubmit={(code) => {
              void api.syncPair(pairingTarget.deviceID, code);
              setPairingTarget(null);
            }}
            onCancel={() => setPairingTarget(null)}
          />
        )}
      </Modal>
      <Modal open={approval != null} onClose={() => void api.syncDecline()}>
        {approval && <SyncPlanSheet phase={approval} key={approval.planHash} />}
      </Modal>
    </div>
  );
}

function Notice({ message, isError, onDismiss }: { message: string; isError?: boolean; onDismiss?: () => void }) {
  return (
    <div className="sync-notice">
      {isError ? <AlertTriangle size={14} className="sync-notice__icon is-error" /> : <Info size={14} className="sync-notice__icon" />}
      <span className="sync-notice__text">{message}</span>
      {onDismiss && (
        <button className="sync-notice__close" onClick={onDismiss}>
          <X size={12} strokeWidth={2.4} />
        </button>
      )}
    </div>
  );
}

// MARK: - Devices

function KindIcon({ kind }: { kind: SyncDeviceKind }) {
  const Icon = kind === "mac" ? Laptop : kind === "iPhone" ? Smartphone : kind === "iPad" ? Tablet : Monitor;
  return <Icon size={18} strokeWidth={1.6} className="sync-peer__icon" />;
}

function subtitle(row: SyncPeerRow): string {
  if (!row.isCompatible) return "Needs a matching version of FLACtastic";
  if (!row.isPaired) return row.isPairingOpen ? "Showing a pairing code" : "Not paired yet";
  return lastSyncedDescription(row.lastSyncedAt);
}

function Devices({ state, onPair }: { state: SyncState; onPair: (r: SyncPeerRow) => void }) {
  const online = state.rows;
  const offline = state.offlinePeers;
  return (
    <SettingsGroup title="Devices">
      {online.length === 0 && offline.length === 0 && (
        <div className="settings-row">
          {state.isBrowsing && <span className="dl-spinner is-small" />}
          <span className="sync-caption">Looking for other devices running FLACtastic on this network…</span>
        </div>
      )}
      {online.map((row, i) => (
        <div key={row.deviceID}>
          {i > 0 && <GroupDivider />}
          <PeerRow
            row={row}
            subtitle={subtitle(row)}
            isOnline
            isBusy={state.activePeerID === row.deviceID || state.pairingPeerID === row.deviceID}
            isEnabled={row.isCompatible && state.hasLibrary}
            primaryTitle={row.isPaired ? "Sync" : "Pair…"}
            onPrimary={() => (row.isPaired ? void api.syncStart(row.deviceID) : onPair(row))}
          />
        </div>
      ))}
      {offline.map((row, i) => (
        <div key={row.deviceID}>
          {(i > 0 || online.length > 0) && <GroupDivider />}
          <PeerRow
            row={row}
            subtitle={"Not on this network · " + lastSyncedDescription(row.lastSyncedAt)}
            isOnline={false}
            isBusy={false}
            isEnabled={false}
            primaryTitle="Sync"
            onPrimary={() => {}}
          />
        </div>
      ))}
    </SettingsGroup>
  );
}

function PeerRow(p: {
  row: SyncPeerRow;
  subtitle: string;
  isOnline: boolean;
  isBusy: boolean;
  isEnabled: boolean;
  primaryTitle: string;
  onPrimary: () => void;
}) {
  const { row } = p;
  const forget = (e: React.MouseEvent) => {
    openMenuAt(
      [
        {
          kind: "button",
          title: "Forget This Device",
          destructive: true,
          action: () =>
            confirmDialog({
              title: `Forget ${row.displayName}?`,
              message: `This computer will no longer accept syncs from ${row.displayName}, and you'll need to pair again to sync with it. Your music isn't affected.`,
              buttons: [{ title: "Forget", destructive: true, action: () => void api.syncForget(row.deviceID) }],
            }),
        },
      ],
      e.clientX,
      e.clientY,
    );
  };
  return (
    <div className="settings-row sync-peer">
      <KindIcon kind={row.kind} />
      <div className="sync-peer__text">
        <div className="sync-peer__name">
          <span>{row.displayName}</span>
          {row.isPaired && <span className={"sync-peer__dot" + (p.isOnline ? " is-online" : "")} />}
        </div>
        <div className="sync-caption">{p.subtitle}</div>
      </div>
      {p.isBusy ? (
        <span className="dl-spinner is-small" />
      ) : (
        p.isOnline && (
          <PillButton primary={row.isPaired} onClick={p.onPrimary} disabled={!p.isEnabled}>
            {p.primaryTitle}
          </PillButton>
        )
      )}
      {row.isPaired && (
        <button className="sync-peer__more" onClick={forget} title="More">
          <MoreHorizontal size={15} />
        </button>
      )}
    </div>
  );
}

// MARK: - Direction

function DirectionGroup({ state }: { state: SyncState }) {
  return (
    <SettingsGroup title="Direction">
      <div className="settings-row sync-direction">
        <PillToggle
          selection={state.direction}
          segments={[
            { value: "push", title: "Send to the other device" },
            { value: "pull", title: "Get from the other device" },
          ]}
          onChange={(d) => void api.syncSetDirection(d)}
        />
        <div className="sync-caption">
          {state.direction === "push"
            ? "Files here that the other device is missing will be copied to it."
            : `Files on the other device that are missing here will be copied to ${THIS_DEVICE}.`}
        </div>
      </div>
    </SettingsGroup>
  );
}

// MARK: - Pairing

function PairingGroup({ state }: { state: SyncState }) {
  const code = state.pairingCode;
  return (
    <SettingsGroup title="Pair this computer">
      {code ? (
        <div className="settings-row sync-code">
          <div className="sync-code__digits">{`${code.slice(0, code.length / 2)} ${code.slice(code.length / 2)}`}</div>
          <div className="sync-caption">Type this code on your other device. It works once, and expires shortly.</div>
          <PillButton onClick={() => void api.syncClosePairingCode()}>Stop</PillButton>
        </div>
      ) : state.lockoutSeconds > 0 ? (
        <div className="settings-row">
          <span className="sync-caption is-secondary">
            Too many failed pairing attempts. Try again in {state.lockoutSeconds} seconds.
          </span>
        </div>
      ) : (
        <div className="settings-row is-top">
          <RowLabel label="Pairing code" subtitle="Shows an eight-digit code for another device to enter." />
          <PillButton onClick={() => void api.syncOpenPairingCode()} disabled={!state.isAdvertising}>
            Show Code
          </PillButton>
        </div>
      )}
    </SettingsGroup>
  );
}

// MARK: - Status

function summaryText(s: SyncSummary): string {
  if (s.tracksTransferred === 0 && s.playlistsTransferred === 0) return "Already up to date.";
  const parts: string[] = [];
  if (s.tracksTransferred > 0) parts.push(`${s.tracksTransferred} track${s.tracksTransferred === 1 ? "" : "s"}`);
  if (s.playlistsTransferred > 0) parts.push(`${s.playlistsTransferred} playlist${s.playlistsTransferred === 1 ? "" : "s"}`);
  let text = "Synced " + parts.join(" and ") + " · " + byteCount(s.bytesTransferred);
  if (s.failures.length) text += ` · ${s.failures.length} failed`;
  return text;
}

function StatusGroup({ phase }: { phase: SyncPhase }) {
  const bar = (f: number) => (
    <div className="sync-bar">
      <div className="sync-bar__fill" style={{ width: `${Math.min(1, Math.max(0, f)) * 100}%` }} />
    </div>
  );
  const cancel = <PillButton onClick={() => void api.syncCancel()}>Cancel</PillButton>;
  let body = null;
  if (phase.kind === "preparing") {
    body = (
      <>
        {bar(phase.fraction)}
        <div className="sync-status__row">
          <span className="sync-caption is-secondary">Checking your library…</span>
          {cancel}
        </div>
      </>
    );
  } else if (phase.kind === "transferring") {
    const p = phase.progress;
    const fraction = p.totalBytes > 0 ? p.bytesTransferred / p.totalBytes : p.totalFiles === 0 ? 1 : 0;
    body = (
      <>
        {bar(fraction)}
        <div className="sync-status__row">
          <div>
            <div className="sync-caption is-secondary">
              {p.completedFiles} of {p.totalFiles} files
            </div>
            {p.currentFileName && <div className="sync-caption sync-ellipsis">{p.currentFileName}</div>}
          </div>
          {cancel}
        </div>
      </>
    );
  } else if (phase.kind === "finished") {
    body = <span className="sync-caption is-secondary">{summaryText(phase.summary)}</span>;
  } else if (phase.kind === "failed") {
    body = <span className="sync-caption is-secondary sync-clamp">{phase.message}</span>;
  }
  return (
    <SettingsGroup title="Status">
      <div className="settings-row sync-status">{body}</div>
    </SettingsGroup>
  );
}

// MARK: - Pairing code entry

function normalizeTyped(raw: string) {
  return raw.replace(/[^0-9]/g, "");
}

function PairingCodeEntrySheet({ peer, onSubmit, onCancel }: { peer: SyncPeerRow; onSubmit: (code: string) => void; onCancel: () => void }) {
  const [code, setCode] = useState("");
  const normalized = normalizeTyped(code);
  const valid = normalized.length === 8;
  return (
    <FLSheet
      title={`Pair with ${peer.displayName}`}
      width={440}
      height={340}
      onClose={onCancel}
      footer={
        <div className="sync-footer">
          <PillButton onClick={onCancel}>Cancel</PillButton>
          <PillButton primary onClick={() => onSubmit(normalized)} disabled={!valid}>
            Pair
          </PillButton>
        </div>
      }
    >
      <div className="sync-pair">
        <div className="sync-caption">
          On {peer.displayName}, open Sync (or Settings ▸ Devices) and select Show Code, then type the eight digits here.
        </div>
        <input
          className="sync-pair__input"
          value={code}
          placeholder="00000000"
          autoFocus
          inputMode="numeric"
          spellCheck={false}
          onChange={(e) => setCode(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && valid && onSubmit(normalized)}
        />
        {!peer.isPairingOpen && <div className="sync-caption">{peer.displayName} isn't showing a code right now.</div>}
      </div>
    </FLSheet>
  );
}

// MARK: - Plan confirmation

function Checkbox({ mark }: { mark: Mark }) {
  return <span className={"sync-check is-" + mark}>{mark === "all" ? "✓" : mark === "some" ? "–" : ""}</span>;
}

function SyncPlanSheet({ phase }: { phase: Extract<SyncPhase, { kind: "awaitingApproval" }> }) {
  const { plan, pickList, overwriteCount } = phase;
  const [picks, setPicks] = useState(() => new PickState(pickList));
  const [openArtists, setOpenArtists] = useState<Set<string>>(new Set());
  const [openAlbums, setOpenAlbums] = useState<Set<string>>(new Set());
  const isPush = plan.direction === "push";
  const target = isPush ? "the other device" : THIS_DEVICE;
  const source = isPush ? THIS_DEVICE : "the other device";
  const isEmpty = plan.newTracks.length + plan.trackConflicts.length + plan.newPlaylists.length + plan.playlistConflicts.length === 0;

  const summary = useMemo(() => {
    if (isEmpty) return "Both devices already match. Nothing will change.";
    if (picks.isEmptySelection) return `Nothing is selected. Tick what you want copied to ${target}.`;
    const parts: string[] = [];
    const t = picks.selectedTrackCount;
    const p = picks.selectedPlaylistCount;
    if (t > 0) parts.push(`${t} track${t === 1 ? "" : "s"}`);
    if (p > 0) parts.push(`${p} playlist${p === 1 ? "" : "s"}`);
    let text = `${parts.join(" and ")} will be copied to ${target}`;
    if (picks.selectedBytes > 0) text += ` (${byteCount(picks.selectedBytes)})`;
    return text + ".";
  }, [picks, isEmpty, target]);

  const flip = (set: Set<string>, id: string) => {
    const n = new Set(set);
    if (n.has(id)) n.delete(id);
    else n.add(id);
    return n;
  };
  const countDetail = (ids: string[], bytes: number) => {
    const included = picks.includedCount(ids);
    const tracks = `${ids.length} track${ids.length === 1 ? "" : "s"}`;
    return included === ids.length ? `${tracks} · ${byteCount(bytes)}` : `${included} of ${tracks}`;
  };
  const confirmTitle = picks.isEverything
    ? overwriteCount > 0
      ? "Replace and Sync"
      : "Sync"
    : picks.selectedTrackCount > 0
      ? `Sync ${picks.selectedTrackCount} Track${picks.selectedTrackCount === 1 ? "" : "s"}`
      : "Sync Playlists";

  const row = (o: {
    key: string;
    indent: number;
    mark: Mark;
    title: string;
    detail: string;
    note?: string | null;
    expanded?: boolean;
    onToggle: () => void;
    onExpand?: () => void;
  }) => (
    <div key={o.key} className="sync-plan-row" style={{ paddingLeft: 12 + o.indent * 22 }}>
      {o.onExpand ? (
        <button className={"sync-plan-row__chevron" + (o.expanded ? " is-open" : "")} onClick={o.onExpand}>
          <ChevronRight size={11} strokeWidth={2.6} />
        </button>
      ) : (
        <span className="sync-plan-row__chevron" />
      )}
      <button className="sync-plan-row__main" onClick={o.onToggle}>
        <Checkbox mark={o.mark} />
        <span className="sync-plan-row__text">
          <span className={"sync-plan-row__title" + (o.mark === "none" ? " is-off" : "")}>{o.title}</span>
          {o.detail && <span className="sync-caption sync-ellipsis">{o.detail}</span>}
        </span>
        {o.note && <span className="sync-plan-row__note">{o.note}</span>}
      </button>
    </div>
  );

  return (
    <FLSheet
      title={overwriteCount > 0 ? "Review before syncing" : "Choose what to sync"}
      width={600}
      height={660}
      onClose={() => void api.syncDecline()}
      footer={
        <div className="sync-footer">
          <PillButton onClick={() => void api.syncDecline()}>Cancel</PillButton>
          <PillButton primary disabled={picks.isEmptySelection && !isEmpty} onClick={() => void api.syncApprove(picks.selection)}>
            {confirmTitle}
          </PillButton>
        </div>
      }
    >
      <div className="sync-plan">
        <div className="sync-plan__summary">{summary}</div>
        <div className="sync-plan__everything">
          <RowLabel label="Entire library" subtitle={`Everything on ${source} that ${target} is missing or has differently.`} />
          <Switch on={picks.isEverything} onChange={(v) => setPicks(picks.setEverything(v))} />
        </div>
        {overwriteCount > 0 && (
          <div className="sync-caption">
            Items marked “Replaces” already exist on {target} with different contents. The version from {source} will replace
            them.
          </div>
        )}
        {pickList.artists.length > 0 && (
          <div>
            <div className="sync-eyebrow">Artists, albums and tracks</div>
            <div className="sync-plan__list">
              {pickList.artists.flatMap((artist) => [
                row({
                  key: artist.id,
                  indent: 0,
                  mark: picks.mark(artist.trackIDs),
                  title: artist.name,
                  detail: countDetail(artist.trackIDs, artist.bytes),
                  expanded: openArtists.has(artist.id),
                  onToggle: () => setPicks(picks.toggle(artist.trackIDs)),
                  onExpand: () => setOpenArtists(flip(openArtists, artist.id)),
                }),
                ...(openArtists.has(artist.id)
                  ? artist.albums.flatMap((album) => [
                      row({
                        key: album.id,
                        indent: 1,
                        mark: picks.mark(album.trackIDs),
                        title: album.title,
                        detail: countDetail(album.trackIDs, album.bytes),
                        expanded: openAlbums.has(album.id),
                        onToggle: () => setPicks(picks.toggle(album.trackIDs)),
                        onExpand: () => setOpenAlbums(flip(openAlbums, album.id)),
                      }),
                      ...(openAlbums.has(album.id)
                        ? album.tracks.map((t) =>
                            row({
                              key: t.entry.trackID,
                              indent: 2,
                              mark: picks.mark([t.entry.trackID]),
                              title: t.entry.title,
                              detail: [t.creditedArtist, byteCount(t.entry.fileSize)].filter(Boolean).join(" · "),
                              note: t.replaces ? `Replaces · ${t.replaces}` : null,
                              onToggle: () => setPicks(picks.toggle([t.entry.trackID])),
                            }),
                          )
                        : []),
                    ])
                  : []),
              ])}
            </div>
          </div>
        )}
        {pickList.playlists.length > 0 && (
          <div>
            <div className="sync-eyebrow">Playlists</div>
            <div className="sync-plan__list">
              {pickList.playlists.map((p) =>
                row({
                  key: p.entry.id,
                  indent: 0,
                  mark: picks.isPlaylistIncluded(p.entry.id) ? "all" : "none",
                  title: p.entry.name,
                  detail: `${p.entry.entryCount} tracks`,
                  note: p.replacesExisting ? "Replaces · contents differ" : null,
                  onToggle: () => setPicks(picks.togglePlaylist(p.entry.id)),
                }),
              )}
            </div>
            <div className="sync-caption" style={{ marginTop: 8 }}>
              A playlist brings its list of tracks, not the tracks themselves — untick an album and its songs show as missing in
              any playlist that uses them.
            </div>
          </div>
        )}
      </div>
    </FLSheet>
  );
}
